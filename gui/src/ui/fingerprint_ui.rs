//! Fingerprint management UI functionality.

use crate::core::{fprintd, util, FingerprintContext};
use crate::ui::app::AppContext;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    pango, Align, Box as GtkBox, Button, Image, Justification, Label, Orientation, Overlay,
};
use log::info;

use std::collections::HashSet;
use std::sync::mpsc::{self, TryRecvError};

/// Perform initial fingerprint scan and enable switches if fingerprints found.
pub fn perform_initial_fingerprint_scan(ctx: &AppContext) {
    info!("Starting background fingerprint enrollment check");

    let (tx, rx) = mpsc::channel::<bool>();
    let ctx_clone = ctx.clone();

    glib::idle_add_local(move || match rx.try_recv() {
        Ok(has_any) => {
            if has_any {
                info!("Enrollment check complete: fingerprints found, enabling switches");
            } else {
                info!("Enrollment check complete: no fingerprints found, switches remain disabled");
            }
            ctx_clone
                .fingerprint_ctx
                .set_pam_switches_sensitive(has_any);
            glib::ControlFlow::Break
        }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
    });

    let rt = ctx.fingerprint_ctx.rt.clone();
    rt.spawn(async move {
        info!("Starting system fingerprint device detection and enrollment scan");
        let enrolled = crate::fingerprints::scan_enrolled_fingerprints().await;
        let has_any = !enrolled.is_empty();

        if has_any {
            info!(
                "System ready: {} enrolled fingerprint(s) detected",
                enrolled.len()
            );
            info!("PAM authentication switches will be enabled");
        } else {
            info!("No enrolled fingerprints found on initial scan");
            info!("PAM authentication switches will remain disabled until enrollment");
            info!("Click 'Enroll' to add your first fingerprint");
        }

        let _ = tx.send(has_any);
    });

    refresh_fingerprint_display(ctx.fingerprint_ctx.clone());
}

/// Refresh fingerprint display with current enrollment status.
pub fn refresh_fingerprint_display(ctx: FingerprintContext) {
    let (tx, rx) = mpsc::channel::<HashSet<String>>();

    {
        let ctx_clone = ctx.clone();

        glib::idle_add_local(move || match rx.try_recv() {
            Ok(enrolled) => {
                ctx_clone.set_enrolled(enrolled);
                update_fingerprint_ui(&ctx_clone);
                glib::ControlFlow::Break
            }
            Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    ctx.rt.spawn(async move {
        let enrolled = crate::fingerprints::scan_enrolled_fingerprints().await;
        let _ = tx.send(enrolled);
    });
}

/// Update fingerprint UI elements with enrollment data.
fn update_fingerprint_ui(ctx: &FingerprintContext) {
    let enrolled = ctx.get_enrolled();
    let has_any = !enrolled.is_empty();
    info!(
        "Updating finger selection UI with {} enrolled fingerprints",
        enrolled.len()
    );

    if has_any {
        info!("Enabling PAM authentication switches (fingerprints available)");
        info!("- Login switch: enabled");
        info!("- Sudo switch: enabled");
        info!("- Polkit switch: enabled");
    } else {
        info!("Disabling PAM authentication switches (no fingerprints enrolled)");
        info!("User must enroll fingerprints before enabling authentication");
    }

    ctx.set_pam_switches_sensitive(has_any);

    // Update button states based on selected finger and enrollment status
    update_button_states(ctx);

    while let Some(child) = ctx.ui.flow.first_child() {
        ctx.ui.flow.remove(&child);
    }

    create_finger_sections(ctx);

    info!("Finger selection UI updated successfully with hand separation");
}

/// Create finger button sections for left and right hands.
fn create_finger_sections(ctx: &FingerprintContext) {
    let left_fingers = &fprintd::FINGERS[0..5];
    let right_fingers = &fprintd::FINGERS[5..10];

    let right_hand_container = create_hand_section("Right Hand", right_fingers, ctx);
    ctx.ui.flow.append(&right_hand_container);

    let left_hand_container = create_hand_section("Left Hand", left_fingers, ctx);
    ctx.ui.flow.append(&left_hand_container);
}

/// Create hand section (left or right) with finger buttons.
fn create_hand_section(title: &str, fingers: &[&str], ctx: &FingerprintContext) -> GtkBox {
    let hand_container = GtkBox::new(Orientation::Vertical, 10);
    hand_container.set_halign(Align::Center);

    let title_label = Label::new(Some(title));
    title_label.set_css_classes(&["hand-title"]);
    hand_container.append(&title_label);

    let finger_grid = GtkBox::new(Orientation::Horizontal, 8);
    finger_grid.set_halign(Align::Center);
    finger_grid.set_homogeneous(true);

    for finger in fingers {
        let finger_box = create_finger_button(finger, ctx);
        finger_grid.append(&finger_box);
    }

    hand_container.append(&finger_grid);
    hand_container
}

/// Create finger button widget.
fn create_finger_button(finger: &str, ctx: &FingerprintContext) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 5);
    container.set_halign(Align::Center);
    container.set_size_request(120, 120);

    let button = Button::new();
    button.set_size_request(90, 90);

    let is_enrolled = ctx.is_finger_enrolled(finger);

    // Base fingerprint icon with optional enrollment badge overlay
    let overlay = Overlay::new();
    let base_image = Image::from_icon_name("fingerprint-symbolic");
    base_image.set_pixel_size(64);
    overlay.set_child(Some(&base_image));

    if is_enrolled {
        let badge = Image::from_icon_name("checkmark");
        badge.set_pixel_size(32);
        badge.set_halign(Align::End);
        badge.set_valign(Align::End);
        overlay.add_overlay(&badge);
    }

    button.set_child(Some(&overlay));

    if is_enrolled {
        button.add_css_class("finger-enrolled");
    } else {
        button.add_css_class("finger-unenrolled");
    }

    let finger_key = finger.to_string();
    let ctx_clone = ctx.clone();

    button.connect_clicked(move |_| {
        ctx_clone.set_selected_finger(Some(finger_key.clone()));
        ctx_clone
            .ui
            .labels
            .finger
            .set_label(&util::display_finger_name(&finger_key));
        ctx_clone.ui.labels.action.set_use_markup(false);
        ctx_clone
            .ui
            .labels
            .action
            .set_label("Select an action below.");
        ctx_clone.ui.stack.set_visible_child_name("finger");
        info!("User selected finger: '{}'", finger_key);

        // Update button states when finger is selected
        let is_enrolled = ctx_clone.is_finger_enrolled(&finger_key);
        ctx_clone.update_button_states(is_enrolled);
    });

    let display_name = util::display_finger_name(finger);
    let short_name = util::create_short_finger_name(&display_name);

    let label = Label::new(Some(&short_name));
    label.set_css_classes(&["finger-label"]);
    label.set_wrap(true);
    label.set_wrap_mode(pango::WrapMode::Word);
    label.set_justify(Justification::Center);
    label.set_size_request(90, -1);

    container.append(&button);
    container.append(&label);
    container
}

/// Update button states based on selected finger and enrollment status
fn update_button_states(ctx: &FingerprintContext) {
    if let Some(ref finger_key) = ctx.get_selected_finger() {
        let is_enrolled = ctx.is_finger_enrolled(finger_key);

        ctx.update_button_states(is_enrolled);

        info!(
            "Updated button states for finger '{}': Add={}, Remove={}",
            finger_key, !is_enrolled, is_enrolled
        );
    } else {
        // No finger selected, disable both buttons
        ctx.ui.buttons.add.set_sensitive(false);
        ctx.ui.buttons.delete.set_sensitive(false);
        info!("No finger selected, both buttons disabled");
    }
}
