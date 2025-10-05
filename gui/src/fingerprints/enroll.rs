//! Fingerprint enrollment functionality.

use crate::context::FingerprintContext;
use crate::fprintd;
use gtk4::glib;

use log::{error, info, warn};
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

        let client = match connect_to_fprintd().await {
            Ok(client) => client,
            Err(e) => {
                let _ = tx.send(EnrollmentEvent::SetText(format!(
                    "Failed to connect to system bus: {}",
                    e
                )));
                return;
            }
        };

        let device = match get_fingerprint_device(&client).await {
            Ok(device) => device,
            Err(e) => {
                let _ = tx.send(EnrollmentEvent::SetText(e));
                return;
            }
        };

        if let Err(e) = claim_device(&device).await {
            let _ = tx.send(EnrollmentEvent::SetText(format!(
                "Could not claim device: {}",
                e
            )));
            return;
        }

        setup_enrollment_listener(&device, &tx).await;
        start_enrollment_process(&device, &finger_key).await;
    });
}

/// Connect to fprintd system bus.
async fn connect_to_fprintd() -> Result<fprintd::Client, Box<dyn std::error::Error>> {
    info!("Connecting to fprintd system bus for enrollment");
    match fprintd::Client::system().await {
        Ok(client) => {
            info!("Successfully connected to fprintd for enrollment");
            Ok(client)
        }
        Err(e) => {
            error!(
                "Failed to connect to fprintd system bus during enrollment: {}",
                e
            );
            Err(Box::new(e))
        }
    }
}

/// Get first available fingerprint device.
async fn get_fingerprint_device(client: &fprintd::Client) -> Result<fprintd::Device, String> {
    info!("Looking for available fingerprint device for enrollment");
    match fprintd::first_device(client).await {
        Ok(Some(device)) => {
            info!("Found fingerprint device, ready for enrollment");
            Ok(device)
        }
        Ok(None) => {
            warn!("No fingerprint devices available for enrollment");
            warn!("Please connect a fingerprint reader and try again");
            Err(format!(
                "<span foreground='{}'>No fingerprint devices available.</span>",
                COLOR_WARNING
            ))
        }
        Err(e) => {
            error!("Failed to enumerate devices during enrollment: {}", e);
            Err(format!("Failed to enumerate device: {}", e))
        }
    }
}

/// Claim fingerprint device for exclusive access.
async fn claim_device(device: &fprintd::Device) -> Result<(), Box<dyn std::error::Error>> {
    info!("Claiming fingerprint device for enrollment (current user)");
    match device.claim("").await {
        Ok(_) => {
            info!("Successfully claimed device for enrollment");
            Ok(())
        }
        Err(e) => {
            error!("Failed to claim device for enrollment: {}", e);
            Err(Box::new(e))
        }
    }
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

/// Start actual enrollment process.
async fn start_enrollment_process(device: &fprintd::Device, finger_key: &str) {
    info!("Starting enrollment process for finger: '{}'", finger_key);
    if let Err(e) = device.enroll_start(finger_key).await {
        error!("Failed to start enrollment for '{}': {}", finger_key, e);
        let _ = device.enroll_stop().await;
        let _ = device.release().await;
    } else {
        info!("Enrollment started successfully, waiting for finger scans...");
    }
}

/// Clean up enrollment device.
fn cleanup_enrollment_device(device: fprintd::Device) {
    tokio::spawn(async move {
        if let Err(e) = device.enroll_stop().await {
            warn!("Failed to stop enrollment: {}", e);
        }
        if let Err(e) = device.release().await {
            warn!("Failed to release device after enrollment: {}", e);
        } else {
            info!("Successfully cleaned up after enrollment");
        }
    });
}
