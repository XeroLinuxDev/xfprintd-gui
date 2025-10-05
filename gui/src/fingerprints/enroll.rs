//! Fingerprint enrollment functionality.

use crate::config;
use crate::core::context::FingerprintContext;
use crate::core::device_manager::{DeviceError, DeviceManager};
use crate::core::fprintd;
use gtk4::glib;

use log::{info, warn};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Events sent during enrollment process.
#[derive(Clone)]
pub enum EnrollmentEvent {
    SetText(String),
    EnrollCompleted,
}

/// Holds the DeviceManager during enrollment to keep device claimed.
type SharedDeviceManager = Arc<Mutex<Option<DeviceManager>>>;

/// Start fingerprint enrollment process for specified finger.
pub fn start_enrollment(finger_key: String, ctx: FingerprintContext) {
    let (tx, rx) = mpsc::channel::<EnrollmentEvent>();

    setup_ui_listener(rx, ctx.clone());
    // We don't yet know required stages (varies by device), so we show a generic Step 1 message.
    let _ = tx.send(EnrollmentEvent::SetText(format!(
        "<b><span foreground='{}'>🔍 Scan 1</span> - Place your finger firmly on the scanner…</b>",
        config::colors().progress
    )));
    spawn_enrollment_task(finger_key, tx, ctx);
}

/// Set up UI listener for enrollment status updates.
fn setup_ui_listener(rx: mpsc::Receiver<EnrollmentEvent>, ctx: FingerprintContext) {
    let lbl = ctx.ui.labels.action.clone();
    let ctx_for_refresh = ctx.clone();

    glib::idle_add_local(move || {
        loop {
            match rx.try_recv() {
                Ok(EnrollmentEvent::SetText(text)) => {
                    lbl.set_use_markup(true);
                    lbl.set_markup(&text);
                }
                Ok(EnrollmentEvent::EnrollCompleted) => {
                    crate::ui::fingerprint_ui::refresh_fingerprint_display(ctx_for_refresh.clone());
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
        glib::ControlFlow::Continue
    });
}

/// Spawn async enrollment task.
fn spawn_enrollment_task(
    finger_key: String,
    tx: mpsc::Sender<EnrollmentEvent>,
    ctx: FingerprintContext,
) {
    ctx.rt.spawn(async move {
        info!(
            "Starting fingerprint enrollment process for finger: {}",
            finger_key
        );

        // Create shared storage for DeviceManager to keep device claimed during enrollment
        let device_manager: SharedDeviceManager = Arc::new(Mutex::new(None));
        let device_manager_for_cleanup = device_manager.clone();

        let result = DeviceManager::enroll_finger(finger_key.clone(), |device| {
            setup_enrollment_listener_sync(device, &tx, device_manager_for_cleanup)
        })
        .await;

        match result {
            Ok(manager) => {
                // Store the DeviceManager to keep the device claimed
                *device_manager.lock().await = Some(manager);
            }
            Err(e) => {
                let error_msg = match e {
                    DeviceError::NoDeviceAvailable => {
                        format!(
                            "<span foreground='{}'>No fingerprint devices available.</span>",
                            config::colors().warning
                        )
                    }
                    _ => format!("Failed to start enrollment: {}", e),
                };
                let _ = tx.send(EnrollmentEvent::SetText(error_msg));
            }
        }
    });
}

/// Set up enrollment status listener (synchronous wrapper for DeviceManager).
fn setup_enrollment_listener_sync(
    device: &fprintd::Device,
    tx: &mpsc::Sender<EnrollmentEvent>,
    device_manager: SharedDeviceManager,
) -> Result<(), DeviceError> {
    let device_clone = device.clone();
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        setup_enrollment_listener(&device_clone, &tx_clone, device_manager).await;
    });

    Ok(())
}

/// Set up enrollment status listener.
async fn setup_enrollment_listener(
    device: &fprintd::Device,
    tx: &mpsc::Sender<EnrollmentEvent>,
    device_manager: SharedDeviceManager,
) {
    let device_for_listener = device.clone();
    let device_for_cleanup = device.clone();
    let tx_status = tx.clone();

    tokio::spawn(async move {
        info!("Setting up enrollment status listener for real-time feedback");
        // Track progressive successful stages (we only show how many good scans were captured so far).
        let mut stage_count: usize = 0usize;

        let _ = device_for_listener
            .listen_enroll_status(move |evt| {
                info!(
                    "Enrollment status update: result='{}', done={}",
                    evt.result, evt.done
                );

                let mut _message: Option<String> = None;

                match evt.result.as_str() {
                    "enroll-stage-passed" => {
                        stage_count += 1;
                        _message = Some(format!(
                            "<span foreground='{}'><b>✅ Scan {} captured.</b> Lift your finger, then place it again…",
                            config::colors().progress,
                            stage_count
                        ));
                    }
                    "enroll-remove-and-retry" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>⚠️  Retry scan {}.</b> Lift your finger completely, reposition (centered & flat), then place again…",
                            config::colors().warning,
                            stage_count + 1
                        ));
                    }
                    "enroll-swipe-too-short" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>👆 Swipe too short.</b> Try a longer, smoother swipe (still on scan {}).",
                            config::colors().warning,
                            stage_count + 1
                        ));
                    }
                    "enroll-finger-not-centered" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>🎯 Not centered.</b> Re‑place finger centered & flat (scan {}).",
                            config::colors().warning,
                            stage_count + 1
                        ));
                    }
                    "enroll-duplicate" => {
                        _message = Some(
                            format!(
                                "<span foreground='{}'><b>🔄 Already enrolled!</b> Choose a different finger.</span>",
                                config::colors().warning
                            )
                        );
                    }
                    "enroll-data-full" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>📊 Processing captured data…</b> ({} scans so far)</span>",
                            config::colors().process,
                            stage_count
                        ));
                    }
                    "enroll-failed" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>❌ Enrollment failed.</b> Please try again.</span>",
                            config::colors().error
                        ));
                    }
                    "enroll-completed" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>🎉 Enrollment complete!</b> Captured {} quality scans.</span>",
                            config::colors().success,
                            stage_count
                        ));
                    }
                    other => {
                        // Fallback / unknown statuses
                        _message = Some(format!(
                            "<span foreground='{}'><b>📊 Status:</b> {} (scan {})</span>",
                            config::colors().neutral,
                            other,
                            stage_count.max(1)
                        ));
                    }
                }

                if let Some(text) = _message {
                    let _ = tx_status.send(EnrollmentEvent::SetText(text));
                }

                if evt.result == "enroll-completed" {
                    info!(
                        "Fingerprint enrollment completed successfully after {} stages",
                        stage_count
                    );
                    let _ = tx_status.send(EnrollmentEvent::EnrollCompleted);
                }

                if evt.done {
                    info!("Enrollment process finished, cleaning up device");
                    let device_clone = device_for_cleanup.clone();
                    let manager_clone = device_manager.clone();
                    tokio::spawn(async move {
                        cleanup_enrollment_device(device_clone, manager_clone).await;
                    });
                }
            })
            .await;
    });
}

/// Clean up enrollment device.
async fn cleanup_enrollment_device(device: fprintd::Device, device_manager: SharedDeviceManager) {
    if let Err(e) = device.enroll_stop().await {
        warn!("Failed to stop enrollment: {}", e);
    }

    // Release the DeviceManager to properly clean up the device
    *device_manager.lock().await = None;
    info!("Enrollment cleanup completed");
}
