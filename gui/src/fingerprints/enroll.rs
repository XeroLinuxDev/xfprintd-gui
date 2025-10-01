//! Fingerprint enrollment functionality.

use crate::fprintd;
use gtk4::glib;

use gtk4::{FlowBox, Label, Stack, Switch};
use log::{error, info, warn};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use tokio::runtime::Runtime;

/// UI context for enrollment operations.
#[derive(Clone)]
pub struct EnrollmentContext {
    pub rt: Arc<Runtime>,
    pub flow: FlowBox,
    pub stack: Stack,
    pub sw_login: Switch,
    pub sw_term: Switch,
    pub sw_prompt: Switch,
    pub selected_finger: Rc<RefCell<Option<String>>>,
    pub finger_label: Label,
    pub action_label: Label,
}

/// Events sent during enrollment process.
#[derive(Clone)]
pub enum EnrollmentEvent {
    SetText(String),
    EnrollCompleted,
}

/// Start fingerprint enrollment process for specified finger.
pub fn start_enrollment(finger_key: String, ctx: EnrollmentContext) {
    let (tx, rx) = mpsc::channel::<EnrollmentEvent>();

    setup_ui_listener(rx, ctx.clone());
    send_initial_message(&tx);
    spawn_enrollment_task(finger_key, tx, ctx);
}

/// Set up UI listener for enrollment status updates.
fn setup_ui_listener(rx: mpsc::Receiver<EnrollmentEvent>, ctx: EnrollmentContext) {
    let lbl = ctx.action_label.clone();
    let ctx_for_refresh = ctx.clone();

    glib::idle_add_local(move || {
        loop {
            match rx.try_recv() {
                Ok(EnrollmentEvent::SetText(text)) => {
                    lbl.set_use_markup(true);
                    lbl.set_markup(&text);
                }
                Ok(EnrollmentEvent::EnrollCompleted) => {
                    refresh_fingerprint_ui(ctx_for_refresh.clone());
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
        glib::ControlFlow::Continue
    });
}

/// Send initial enrollment message to UI.
fn send_initial_message(tx: &mpsc::Sender<EnrollmentEvent>) {
    let _ = tx.send(EnrollmentEvent::SetText(
        "<b>🔍 Place your finger firmly on the scanner…</b>".to_string(),
    ));
}

/// Spawn async enrollment task.
fn spawn_enrollment_task(
    finger_key: String,
    tx: mpsc::Sender<EnrollmentEvent>,
    ctx: EnrollmentContext,
) {
    ctx.rt.spawn(async move {
        info!(
            "Starting fingerprint enrollment process for finger: {}",
            finger_key
        );

        let client = match connect_to_fprintd().await {
            Ok(client) => client,
            Err(e) => {
                send_error_message(&tx, &format!("Failed to connect to system bus: {}", e));
                return;
            }
        };

        let device = match get_fingerprint_device(&client).await {
            Ok(device) => device,
            Err(e) => {
                send_error_message(&tx, &e);
                return;
            }
        };

        if let Err(e) = claim_device(&device).await {
            send_error_message(&tx, &format!("Could not claim device: {}", e));
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
            Err("<span color='orange'>No fingerprint devices available.</span>".to_string())
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
        let _ = device_for_listener
            .listen_enroll_status(move |evt| {
                info!(
                    "Enrollment status update: result='{}', done={}",
                    evt.result, evt.done
                );
                let text = process_enrollment_status(&evt.result);
                let _ = tx_status.send(EnrollmentEvent::SetText(text));

                if evt.result == "enroll-completed" {
                    info!("Fingerprint enrollment completed successfully!");
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

/// Process enrollment status and return appropriate UI message.
fn process_enrollment_status(result: &str) -> String {
    match result {
        "enroll-completed" => {
            "<span color='green'><b>🎉 Well done!</b> Your fingerprint was saved successfully.</span>".to_string()
        }
        "enroll-stage-passed" => {
            info!("Enrollment stage passed, continuing...");
            "<span color='blue'><b>✅ Good scan!</b> Lift your finger, then place it again...</span>".to_string()
        }
        "enroll-remove-and-retry" => {
            warn!("Enrollment stage failed, user needs to retry");
            "<span color='orange'><b>⚠️  Lift your finger</b> and place it down again...</span>".to_string()
        }
        "enroll-data-full" => {
            info!("Enrollment data buffer full, processing...");
            "<span color='blue'><b>📊 Processing...</b> Keep finger steady</span>".to_string()
        }
        "enroll-swipe-too-short" => {
            warn!("Finger swipe too short");
            "<span color='orange'><b>👆 Swipe too short</b> - try a longer swipe</span>".to_string()
        }
        "enroll-finger-not-centered" => {
            warn!("Finger not centered properly");
            "<span color='orange'><b>🎯 Center your finger</b> and try again</span>".to_string()
        }
        "enroll-duplicate" => {
            warn!("Duplicate fingerprint detected");
            "<span color='orange'><b>🔄 Already enrolled!</b> This fingerprint is already saved, use a different finger.</span>".to_string()
        }
        "enroll-failed" => {
            error!("Fingerprint enrollment failed");
            "<span color='red'><b>❌ Enrollment failed!</b> Sorry, your fingerprint could not be saved.</span>".to_string()
        }
        other => {
            info!("Enrollment status: '{}' (in progress)", other);
            format!("<span color='gray'><b>📊 Status:</b> {}</span>", other)
        }
    }
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

/// Send error message to UI.
fn send_error_message(tx: &mpsc::Sender<EnrollmentEvent>, message: &str) {
    let _ = tx.send(EnrollmentEvent::SetText(message.to_string()));
}

/// Refresh fingerprint UI after enrollment completion.
fn refresh_fingerprint_ui(ctx: EnrollmentContext) {
    crate::ui::refresh_fingerprint_display(
        ctx.rt,
        ctx.flow,
        ctx.stack,
        ctx.selected_finger,
        ctx.finger_label,
        ctx.action_label,
        ctx.sw_login,
        ctx.sw_term,
        ctx.sw_prompt,
    );
}
