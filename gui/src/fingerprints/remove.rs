//! Fingerprint removal functionality.

use crate::fprintd;
use gtk4::glib;

use gtk4::{
    prelude::*, ApplicationWindow, Button, CheckButton, FlowBox, Label, Stack, Switch, Window,
};
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
    pub button_add: Button,
    pub button_delete: Button,
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

    // Check toggles state before async operation
    let any_toggle_active =
        ctx.sw_login.is_active() || ctx.sw_term.is_active() || ctx.sw_prompt.is_active();

    // Only proceed with check if toggles are active
    if !any_toggle_active {
        proceed_with_removal(finger_key, ctx);
        return;
    }

    // Check if this would be the last fingerprint
    let rt_clone = ctx.rt.clone();
    let finger_key_clone = finger_key.clone();

    let (tx, rx) = mpsc::channel::<bool>();

    let ctx_for_check = ctx.clone();
    let finger_key_for_check = finger_key.clone();
    glib::idle_add_local(move || match rx.try_recv() {
        Ok(is_last_fingerprint) => {
            if is_last_fingerprint {
                show_lockout_warning_dialog(finger_key_for_check.clone(), ctx_for_check.clone());
            } else {
                proceed_with_removal(finger_key_for_check.clone(), ctx_for_check.clone());
            }
            glib::ControlFlow::Break
        }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
    });

    rt_clone.spawn(async move {
        let enrolled = crate::fingerprints::scan_enrolled_fingerprints().await;
        let is_last_fingerprint = enrolled.len() == 1 && enrolled.contains(&finger_key_clone);

        let _ = tx.send(is_last_fingerprint);
    });
}

/// Show lockout warning dialog when attempting to remove last fingerprint with toggles enabled.
fn show_lockout_warning_dialog(finger_key: String, ctx: RemovalContext) {
    info!("Showing lockout warning dialog - last fingerprint with active auth toggles");

    let builder =
        gtk4::Builder::from_resource("/xyz/xerolinux/xfprintd_gui/ui/lockout_warning_dialog.ui");
    let dialog: Window = builder
        .object("lockout_warning_window")
        .expect("Failed to get lockout_warning_window");

    // Get parent window for modal behavior
    if let Some(toplevel) = ctx.stack.root() {
        if let Some(app_window) = toplevel.downcast_ref::<ApplicationWindow>() {
            dialog.set_transient_for(Some(app_window));
        }
    }

    let cancel_button: Button = builder
        .object("cancel_button")
        .expect("Failed to get cancel_button");
    let proceed_button: Button = builder
        .object("proceed_button")
        .expect("Failed to get proceed_button");
    let confirmation_check: CheckButton = builder
        .object("confirmation_check")
        .expect("Failed to get confirmation_check");

    // Enable/disable proceed button based on checkbox state
    let proceed_button_clone = proceed_button.clone();
    confirmation_check.connect_toggled(move |check| {
        proceed_button_clone.set_sensitive(check.is_active());
    });

    let dialog_clone = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        info!("User cancelled deletion to avoid lockout");
        dialog_clone.close();
    });

    let dialog_clone = dialog.clone();
    proceed_button.connect_clicked(move |_| {
        info!("User chose to proceed with deletion despite lockout warning");
        dialog_clone.close();
        proceed_with_removal(finger_key.clone(), ctx.clone());
    });

    dialog.present();
}

/// Proceed with the actual removal process.
fn proceed_with_removal(finger_key: String, ctx: RemovalContext) {
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
        ctx.button_add,
        ctx.button_delete,
        ctx.sw_login,
        ctx.sw_term,
        ctx.sw_prompt,
    );
}
