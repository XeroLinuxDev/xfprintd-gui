//! Fingerprint removal functionality.

use crate::core::context::FingerprintContext;
use crate::core::device_manager::{DeviceError, DeviceManager};

use gtk4::glib;

use gtk4::{prelude::*, ApplicationWindow, Button, CheckButton, Window};
use log::info;
use std::sync::mpsc::{self, TryRecvError};

/// Events sent during removal process.
#[derive(Clone)]
pub enum RemovalEvent {
    Success,
    Error(String),
}

/// Start fingerprint removal process for specified finger.
pub fn start_removal(finger_key: String, ctx: FingerprintContext) {
    info!("User clicked 'Delete' button for finger: '{}'", finger_key);

    // Check toggles state before async operation
    let any_toggle_active = ctx.has_active_pam_switches();

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
fn show_lockout_warning_dialog(finger_key: String, ctx: FingerprintContext) {
    info!("Showing lockout warning dialog - last fingerprint with active auth toggles");

    let builder =
        gtk4::Builder::from_resource("/xyz/xerolinux/xfprintd_gui/ui/lockout_warning_dialog.ui");
    let dialog: Window = builder
        .object("lockout_warning_window")
        .expect("Failed to get lockout_warning_window");

    // Get parent window for modal behavior
    if let Some(toplevel) = ctx.ui.stack.root() {
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
fn proceed_with_removal(finger_key: String, ctx: FingerprintContext) {
    info!("Starting fingerprint deletion process");

    ctx.ui
        .labels
        .action
        .set_label("Deleting enrolled fingerprint...");
    let (tx, rx) = mpsc::channel::<RemovalEvent>();

    setup_removal_ui_listener(rx, ctx.clone());
    spawn_removal_task(finger_key, tx, ctx);
}

/// Set up UI listener for removal status updates.
fn setup_removal_ui_listener(rx: mpsc::Receiver<RemovalEvent>, ctx: FingerprintContext) {
    let action_label = ctx.ui.labels.action.clone();
    let _rt = ctx.rt.clone();

    glib::idle_add_local(move || match rx.try_recv() {
        Ok(RemovalEvent::Success) => {
            action_label.set_use_markup(true);
            action_label.set_markup("<span color='orange'>Fingerprint deleted.</span>");
            crate::ui::fingerprint_ui::refresh_fingerprint_display(ctx.clone());
            glib::ControlFlow::Break
        }
        Ok(RemovalEvent::Error(msg)) => {
            action_label.set_use_markup(true);
            action_label.set_markup(&msg);
            crate::ui::fingerprint_ui::refresh_fingerprint_display(ctx.clone());
            glib::ControlFlow::Break
        }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
    });
}

/// Spawn async removal task.
fn spawn_removal_task(finger_key: String, tx: mpsc::Sender<RemovalEvent>, ctx: FingerprintContext) {
    ctx.rt.spawn(async move {
        info!("Starting fingerprint deletion process for '{}'", finger_key);

        let result = DeviceManager::delete_finger(finger_key.clone()).await;

        match result {
            Ok(()) => {
                info!("Fingerprint deletion completed successfully");
                let _ = tx.send(RemovalEvent::Success);
            }
            Err(e) => {
                let error_msg = match e {
                    DeviceError::NoDeviceAvailable => {
                        "<span color='orange'>No fingerprint devices available.</span>".to_string()
                    }
                    _ => format!("<span color='red'><b>Delete failed</b>: {}</span>", e),
                };
                let _ = tx.send(RemovalEvent::Error(error_msg));
            }
        }
    });
}
