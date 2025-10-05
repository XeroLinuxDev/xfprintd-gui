//! Navigation buttons and dialogs functionality.

use crate::pam::helper::is_sddm_enabled;
use crate::ui::app::{extract_widget, AppContext};
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Builder, Button, Window};
use log::info;

/// Set up navigation buttons and dialogs.
pub fn setup_navigation_and_dialogs(
    ctx: &AppContext,
    builder: &Builder,
    window: &ApplicationWindow,
) {
    setup_navigation_buttons(ctx, builder);
    setup_info_button(window, builder);
    setup_sddm_login_hint(window, builder);
}

/// Set up navigation buttons.
fn setup_navigation_buttons(ctx: &AppContext, builder: &Builder) {
    let manage_btn: Button = extract_widget(builder, "manage_btn");
    let back_btn: Button = extract_widget(builder, "back_btn");
    let button_back: Button = extract_widget(builder, "button_back");

    {
        let stack = ctx.fingerprint_ctx.ui.stack.clone();
        manage_btn.connect_clicked(move |_| {
            info!("User clicked 'Manage' button - navigating to management page");
            stack.set_visible_child_name("manage");
        });
    }

    {
        let stack = ctx.fingerprint_ctx.ui.stack.clone();
        back_btn.connect_clicked(move |_| {
            info!("User clicked 'Back' button - returning to main page");
            stack.set_visible_child_name("main");
        });
    }

    {
        let stack = ctx.fingerprint_ctx.ui.stack.clone();
        button_back.connect_clicked(move |_| {
            info!("User clicked 'Back' button - returning to management page");
            stack.set_visible_child_name("manage");
        });
    }
}

/// Set up info button to show about dialog.
fn setup_info_button(window: &ApplicationWindow, builder: &Builder) {
    let info_btn: Button = extract_widget(builder, "info_btn");

    let window_clone = window.clone();
    info_btn.connect_clicked(move |_| {
        info!("User clicked 'About' button - showing info dialog");
        show_info_dialog(&window_clone);
    });
}

/// Set up SDDM login hint button if SDDM is detected.
fn setup_sddm_login_hint(window: &ApplicationWindow, builder: &Builder) {
    let login_info_btn: Button = extract_widget(builder, "login_info_btn");

    if is_sddm_enabled() {
        info!("SDDM detected - showing login info hint button");
        login_info_btn.set_visible(true);
        // Show popup with detailed instructions when clicked
        let parent = window.clone();
        login_info_btn.connect_clicked(move |_| {
            show_sddm_hint(&parent);
        });
    } else {
        info!("SDDM not detected - hiding login info hint button");
        login_info_btn.set_visible(false);
    }
}

/// Show the info dialog with credits and donation links.
fn show_info_dialog(main_window: &ApplicationWindow) {
    let builder = Builder::from_resource("/xyz/xerolinux/xfprintd_gui/ui/info_dialog.ui");

    let info_window: gtk4::Window = builder
        .object("info_window")
        .expect("Failed to get info_window");

    let close_button: Button = builder
        .object("close_button")
        .expect("Failed to get close_button");

    info_window.set_transient_for(Some(main_window));

    let info_window_clone = info_window.clone();
    close_button.connect_clicked(move |_| {
        info_window_clone.close();
    });

    info_window.show();
}

/// Show SDDM-specific fingerprint hint dialog.
fn show_sddm_hint(parent: &ApplicationWindow) {
    info!("Displaying SDDM fingerprint hint dialog");
    let builder = Builder::from_resource("/xyz/xerolinux/xfprintd_gui/ui/sddm_hint_dialog.ui");

    let window: Window = extract_widget(&builder, "sddm_hint_window");
    let close_button: Button = extract_widget(&builder, "sddm_hint_close_button");

    window.set_transient_for(Some(parent));

    let window_clone = window.clone();
    close_button.connect_clicked(move |_| {
        window_clone.close();
    });

    window.show();
}
