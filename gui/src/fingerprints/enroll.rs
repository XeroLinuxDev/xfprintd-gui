//! Fingerprint enrollment functionality.

use crate::context::FingerprintContext;
use crate::device_manager::{DeviceError, DeviceManager};
use crate::fprintd;
use gtk4::glib;

use log::{info, warn};
use std::sync::mpsc::{self, TryRecvError};

const COLOR_PROGRESS: &str = "#a277ff"; // Purple (progress/successful scan)
const COLOR_WARNING: &str = "#ff6ac1"; // Pink (retry / adjustment)
const COLOR_PROCESS: &str = "#5ea2ff"; // Blue (processing / neutral status)
const COLOR_COMPLETE: &str = "#a277ff"; // Reuse purple for completion
const COLOR_FAIL: &str = "#ff4d6d"; // Accent failure
const COLOR_NEUTRAL: &str = "#8a8f98"; // Neutral / fallback

/// Events sent during enrollment process.
#[derive(Clone)]
pub enum EnrollmentEvent {
    SetText(String),
    EnrollCompleted,
}

/// Start fingerprint enrollment process for specified finger.
pub fn start_enrollment(finger_key: String, ctx: FingerprintContext) {
    let (tx, rx) = mpsc::channel::<EnrollmentEvent>();

    setup_ui_listener(rx, ctx.clone());
    // We don't yet know required stages (varies by device), so we show a generic Step 1 message.
    let _ = tx.send(EnrollmentEvent::SetText(format!(
        "<b><span foreground='{}'>🔍 Scan 1</span> - Place your finger firmly on the scanner…</b>",
        COLOR_PROGRESS
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
                    crate::ui::refresh_fingerprint_display(ctx_for_refresh.clone());
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

        let result = DeviceManager::enroll_finger(finger_key.clone(), |device| {
            setup_enrollment_listener_sync(device, &tx)
        })
        .await;

        if let Err(e) = result {
            let error_msg = match e {
                DeviceError::NoDeviceAvailable => {
                    format!(
                        "<span foreground='{}'>No fingerprint devices available.</span>",
                        COLOR_WARNING
                    )
                }
                _ => format!("Failed to start enrollment: {}", e),
            };
            let _ = tx.send(EnrollmentEvent::SetText(error_msg));
        }
    });
}

/// Set up enrollment status listener (synchronous wrapper for DeviceManager).
fn setup_enrollment_listener_sync(
    device: &fprintd::Device,
    tx: &mpsc::Sender<EnrollmentEvent>,
) -> Result<(), DeviceError> {
    let device_clone = device.clone();
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        setup_enrollment_listener(&device_clone, &tx_clone).await;
    });

    Ok(())
}

/// Set up enrollment status listener.
async fn setup_enrollment_listener(device: &fprintd::Device, tx: &mpsc::Sender<EnrollmentEvent>) {
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
                            COLOR_PROGRESS,
                            stage_count
                        ));
                    }
                    "enroll-remove-and-retry" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>⚠️  Retry scan {}.</b> Lift your finger completely, reposition (centered & flat), then place again…",
                            COLOR_WARNING,
                            stage_count + 1
                        ));
                    }
                    "enroll-swipe-too-short" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>👆 Swipe too short.</b> Try a longer, smoother swipe (still on scan {}).",
                            COLOR_WARNING,
                            stage_count + 1
                        ));
                    }
                    "enroll-finger-not-centered" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>🎯 Not centered.</b> Re‑place finger centered & flat (scan {}).",
                            COLOR_WARNING,
                            stage_count + 1
                        ));
                    }
                    "enroll-duplicate" => {
                        _message = Some(
                            format!(
                                "<span foreground='{}'><b>🔄 Already enrolled!</b> Choose a different finger.</span>",
                                COLOR_WARNING
                            )
                        );
                    }
                    "enroll-data-full" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>📊 Processing captured data…</b> ({} scans so far)</span>",
                            COLOR_PROCESS,
                            stage_count
                        ));
                    }
                    "enroll-failed" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>❌ Enrollment failed.</b> Please try again.</span>",
                            COLOR_FAIL
                        ));
                    }
                    "enroll-completed" => {
                        _message = Some(format!(
                            "<span foreground='{}'><b>🎉 Enrollment complete!</b> Captured {} quality scans.</span>",
                            COLOR_COMPLETE,
                            stage_count
                        ));
                    }
                    other => {
                        // Fallback / unknown statuses
                        _message = Some(format!(
                            "<span foreground='{}'><b>📊 Status:</b> {} (scan {})</span>",
                            COLOR_NEUTRAL,
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
                    cleanup_enrollment_device(device_for_cleanup.clone());
                }
            })
            .await;
    });
}

/// Clean up enrollment device.
fn cleanup_enrollment_device(device: fprintd::Device) {
    tokio::spawn(async move {
        if let Err(e) = device.enroll_stop().await {
            warn!("Failed to stop enrollment: {}", e);
        }
        // Device release is now handled by DeviceManager's Drop implementation
        info!("Enrollment cleanup completed");
    });
}
