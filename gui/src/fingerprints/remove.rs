//! Fingerprint removal functionality.

use crate::fprintd;
use gtk4::glib;

use gtk4::{FlowBox, Label, Stack, Switch};
use log::{error, info, warn};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use tokio::runtime::Runtime;

/// UI context for removal operations.
#[derive(Clone)]
pub struct RemovalContext {
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

/// Events sent during removal process.
#[derive(Clone)]
pub enum RemovalEvent {
    Success,
    Error(String),
}

/// Start fingerprint removal process for specified finger.
pub fn start_removal(finger_key: String, ctx: RemovalContext) {
    info!("User clicked 'Delete' button for finger: '{}'", finger_key);
    info!("Starting fingerprint deletion process");

    set_deletion_status(&ctx.action_label);
    let (tx, rx) = mpsc::channel::<RemovalEvent>();

    setup_removal_ui_listener(rx, ctx.clone());
    spawn_removal_task(finger_key, tx, ctx);
}

/// Set initial deletion status message.
fn set_deletion_status(action_label: &Label) {
    action_label.set_label("Deleting enrolled fingerprint...");
}

/// Set up UI listener for removal status updates.
fn setup_removal_ui_listener(rx: mpsc::Receiver<RemovalEvent>, ctx: RemovalContext) {
    let action_label = ctx.action_label.clone();
    let _rt = ctx.rt.clone();

    glib::idle_add_local(move || match rx.try_recv() {
        Ok(RemovalEvent::Success) => {
            action_label.set_use_markup(true);
            action_label.set_markup("<span color='orange'>Fingerprint deleted.</span>");
            refresh_fingerprint_ui_after_removal(ctx.clone());
            glib::ControlFlow::Break
        }
        Ok(RemovalEvent::Error(msg)) => {
            action_label.set_use_markup(true);
            action_label.set_markup(&msg);
            refresh_fingerprint_ui_after_removal(ctx.clone());
            glib::ControlFlow::Break
        }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
    });
}

/// Spawn async removal task.
fn spawn_removal_task(finger_key: String, tx: mpsc::Sender<RemovalEvent>, ctx: RemovalContext) {
    ctx.rt.spawn(async move {
        info!(
            "Connecting to fprintd system bus for deletion of '{}'",
            finger_key
        );

        let client = match connect_to_fprintd_for_removal().await {
            Ok(client) => client,
            Err(e) => {
                send_removal_error(
                    &tx,
                    &format!("<span color='red'><b>Bus connect failed</b>: {}</span>", e),
                );
                return;
            }
        };

        let device = match get_fingerprint_device_for_removal(&client).await {
            Ok(device) => device,
            Err(e) => {
                send_removal_error(&tx, &e);
                return;
            }
        };

        if let Err(e) = claim_device_for_removal(&device).await {
            warn!("⚠️  Failed to claim device for deletion: {}", e);
        }

        perform_fingerprint_deletion(&device, &finger_key, &tx).await;
        release_device_after_removal(&device).await;

        info!("Fingerprint deletion completed successfully");
        let _ = tx.send(RemovalEvent::Success);
    });
}

/// Connect to fprintd system bus for removal operations.
async fn connect_to_fprintd_for_removal() -> Result<fprintd::Client, Box<dyn std::error::Error>> {
    match fprintd::Client::system().await {
        Ok(client) => {
            info!("Successfully connected to fprintd for deletion");
            Ok(client)
        }
        Err(e) => {
            error!("Failed to connect to system bus for deletion: {}", e);
            Err(Box::new(e))
        }
    }
}

/// Get fingerprint device for removal operations.
async fn get_fingerprint_device_for_removal(
    client: &fprintd::Client,
) -> Result<fprintd::Device, String> {
    info!("Searching for fingerprint device to perform deletion");
    match fprintd::first_device(client).await {
        Ok(Some(device)) => {
            info!("Found fingerprint device for deletion");
            Ok(device)
        }
        Ok(None) => {
            warn!("No fingerprint devices available for deletion");
            warn!("Please ensure fingerprint reader is connected");
            Err("<span color='orange'>No fingerprint devices available.</span>".to_string())
        }
        Err(e) => {
            error!("Failed to enumerate devices for deletion: {}", e);
            Err(format!(
                "<span color='red'><b>Failed</b> to enumerate devices: {}</span>",
                e
            ))
        }
    }
}

/// Claim device for removal operations.
async fn claim_device_for_removal(
    device: &fprintd::Device,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Claiming device for deletion operation");
    match device.claim("").await {
        Ok(_) => {
            info!("Successfully claimed device for deletion");
            Ok(())
        }
        Err(e) => {
            warn!("Failed to claim device for deletion: {}", e);
            Err(Box::new(e))
        }
    }
}

/// Perform actual fingerprint deletion.
async fn perform_fingerprint_deletion(
    device: &fprintd::Device,
    finger_key: &str,
    tx: &mpsc::Sender<RemovalEvent>,
) {
    info!("Executing deletion of enrolled finger: '{}'", finger_key);
    if let Err(e) = device.delete_enrolled_finger(finger_key).await {
        error!("Failed to delete enrolled finger '{}': {}", finger_key, e);
        send_removal_error(
            tx,
            &format!("<span color='red'><b>Delete failed</b>: {}</span>", e),
        );
    } else {
        info!("Successfully deleted fingerprint '{}'", finger_key);
    }
}

/// Release device after removal operations.
async fn release_device_after_removal(device: &fprintd::Device) {
    info!("Releasing device after deletion");
    if let Err(e) = device.release().await {
        warn!("Failed to release device after deletion: {}", e);
    } else {
        info!("Successfully released device after deletion");
    }
}

/// Send removal error message.
fn send_removal_error(tx: &mpsc::Sender<RemovalEvent>, message: &str) {
    let _ = tx.send(RemovalEvent::Error(message.to_string()));
}

/// Refresh fingerprint UI after removal completion.
fn refresh_fingerprint_ui_after_removal(ctx: RemovalContext) {
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
