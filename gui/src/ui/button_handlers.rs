//! Button click handlers functionality.

use crate::core::FingerprintContext;
use crate::fingerprints::{enroll, remove};
use crate::ui::app::AppContext;
use gtk4::prelude::*;
use gtk4::Button;
use log::info;

/// Set up all button handlers.
pub fn setup_button_handlers(ctx: &AppContext) {
    setup_enroll_button(&ctx.fingerprint_ctx.ui.buttons.add, &ctx.fingerprint_ctx);
    setup_delete_button(&ctx.fingerprint_ctx.ui.buttons.delete, &ctx.fingerprint_ctx);
}

/// Set up enrollment button.
fn setup_enroll_button(button_add: &Button, ctx: &FingerprintContext) {
    let ctx_clone = ctx.clone();
    button_add.connect_clicked(move |_| {
        if let Some(key) = ctx_clone.get_selected_finger() {
            info!("User clicked 'Add' button for finger: '{}'", key);
            info!("Initiating fingerprint enrollment process");

            enroll::start_enrollment(key, ctx_clone.clone());
        }
    });
}

/// Set up delete button.
fn setup_delete_button(button_delete: &Button, ctx: &FingerprintContext) {
    let ctx_clone = ctx.clone();
    button_delete.connect_clicked(move |_| {
        if let Some(key) = ctx_clone.get_selected_finger() {
            remove::start_removal(key, ctx_clone.clone());
        }
    });
}
