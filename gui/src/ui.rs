//! User Interface handling functionality.

use crate::context::FingerprintContext;
use crate::fingerprints::{enroll, remove};
use crate::pam_helper::{get_login_path, is_sddm_enabled, PamHelper, POLKIT_PATH, SUDO_PATH};
use crate::util;
use crate::{fprintd, system};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    gio, pango, Align, Application, ApplicationWindow, Box as GtkBox, Builder, Button, CssProvider,
    FlowBox, Image, Justification, Label, Orientation, Overlay, Stack, Switch, Window,
};
use log::{error, info, warn};

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Main application context with UI elements and runtime.
#[derive(Clone)]
pub struct AppContext {
    pub fingerprint_ctx: FingerprintContext,
}

/// Initialize and set up main application UI.
pub fn setup_application_ui(app: &Application) {
    info!("Initializing application components");

    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime"),
    );
    info!("Tokio async runtime initialized");

    setup_resources_and_theme();

    // Create single builder for all UI components
    let builder = Builder::from_resource("/xyz/xerolinux/xfprintd_gui/ui/main.ui");
    let window = create_main_window(app, &builder);

    window.show();

    system::check_distribution_support(&window);

    info!("Performing system environment checks");
    system::check_fprintd_service();
    system::check_helper_tool();
    system::check_pkexec_availability();

    let ctx = setup_ui_components(&window, rt, &builder);
    setup_pam_switches(&ctx);
    // Configure the SDDM-specific login info button (only visible when SDDM is enabled)
    let login_info_btn: Button = builder
        .object("login_info_btn")
        .expect("Failed to get login_info_btn");
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
    setup_navigation_buttons(&ctx, &builder);
    setup_info_button(&window, &builder);
    setup_enroll_button(&ctx.fingerprint_ctx.ui.buttons.add, &ctx.fingerprint_ctx);
    setup_delete_button(&ctx.fingerprint_ctx.ui.buttons.delete, &ctx.fingerprint_ctx);

    perform_initial_fingerprint_scan(&ctx);

    info!("Setting initial view to main page");
    ctx.fingerprint_ctx.ui.stack.set_visible_child_name("main");
    info!("XFPrintD GUI application startup complete");
}

/// Set up resources and theme.
fn setup_resources_and_theme() {
    gio::resources_register_include!("xyz.xerolinux.xfprintd_gui.gresource")
        .expect("Failed to register gresources");

    if let Some(display) = gtk4::gdk::Display::default() {
        info!("Setting up UI theme and styling");
        let theme = gtk4::IconTheme::for_display(&display);
        theme.add_resource_path("/xyz/xerolinux/xfprintd_gui/icons");

        let css_provider = CssProvider::new();
        css_provider.load_from_resource("/xyz/xerolinux/xfprintd_gui/css/style.css");
        gtk4::style_context_add_provider_for_display(
            &display,
            &css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        info!("UI theme and styling loaded successfully");
    } else {
        warn!("No default display found - UI theming may not work properly");
    }
}

/// Creates the main application window
fn create_main_window(app: &Application, builder: &Builder) -> ApplicationWindow {
    let window: ApplicationWindow = builder
        .object("app_window")
        .expect("Failed to get app_window");

    window.set_application(Some(app));
    info!("Setting window icon to fingerprint");
    window.set_icon_name(Some("xfprintd-gui"));

    window
}

/// Set up UI components and return application context.
fn setup_ui_components(
    _window: &ApplicationWindow,
    rt: Arc<Runtime>,
    builder: &Builder,
) -> AppContext {
    let stack: Stack = builder.object("stack").expect("Failed to get stack");
    let fingers_flow: FlowBox = builder
        .object("fingers_flow")
        .expect("Failed to get fingers_flow");
    let selected_finger = Rc::new(RefCell::new(None));
    let finger_label: Label = builder
        .object("finger_label")
        .expect("Failed to get finger_label");
    let action_label: Label = builder
        .object("action_label")
        .expect("Failed to get action_label");
    let button_add: Button = builder
        .object("button_add")
        .expect("Failed to get button_add");
    let button_delete: Button = builder
        .object("button_delete")
        .expect("Failed to get button_delete");
    let sw_login: Switch = builder.object("sw_login").expect("Failed to get sw_login");
    let sw_term: Switch = builder.object("sw_term").expect("Failed to get sw_term");
    let sw_prompt: Switch = builder
        .object("sw_prompt")
        .expect("Failed to get sw_prompt");

    info!("All UI components successfully initialized from Glade builder");

    // Assemble UI components using builder pattern
    let switches = crate::context::PamSwitches::new(sw_login, sw_term, sw_prompt);
    let labels = crate::context::FingerprintLabels::new(finger_label, action_label);
    let buttons = crate::context::FingerprintButtons::new(button_add, button_delete);
    let ui = crate::context::UiComponents::new(fingers_flow, stack, switches, labels, buttons);

    let fingerprint_ctx = FingerprintContext::new(rt, ui, selected_finger);

    AppContext { fingerprint_ctx }
}

/// Set up PAM authentication switches.
fn setup_pam_switches(ctx: &AppContext) {
    info!("Checking current PAM configurations for switches initialization");

    let (login_configured, sudo_configured, polkit_configured) =
        PamHelper::check_all_configurations();

    info!(
        "PAM Login Authentication: {}",
        if login_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "PAM Sudo Authentication: {}",
        if sudo_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "PAM Polkit Authentication: {}",
        if polkit_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );

    ctx.fingerprint_ctx
        .ui
        .switches
        .login
        .set_active(login_configured);
    ctx.fingerprint_ctx
        .ui
        .switches
        .term
        .set_active(sudo_configured);
    ctx.fingerprint_ctx
        .ui
        .switches
        .prompt
        .set_active(polkit_configured);

    info!("Temporarily disabling PAM switches until fingerprint enrollment check");
    ctx.fingerprint_ctx.set_pam_switches_sensitive(false);

    setup_pam_switch_handlers(ctx);
}

/// Set up PAM switch event handlers.
fn setup_pam_switch_handlers(ctx: &AppContext) {
    ctx.fingerprint_ctx
        .ui
        .switches
        .login
        .connect_state_set(move |_switch, state| {
            if state {
                info!("User enabled login fingerprint authentication switch");
            } else {
                info!("User disabled login fingerprint authentication switch");
            }

            let login_path = get_login_path();
            let res = if state {
                PamHelper::apply_configuration(login_path)
            } else {
                PamHelper::remove_configuration(login_path)
            };

            match res {
                Ok(()) => {
                    if state {
                        info!("Successfully enabled fingerprint authentication for login");
                    } else {
                        info!("Successfully disabled fingerprint authentication for login");
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to {} fingerprint authentication for login: {}",
                        if state { "enable" } else { "disable" },
                        e
                    );
                    return gtk4::glib::Propagation::Stop;
                }
            }
            gtk4::glib::Propagation::Proceed
        });

    // Sudo switch handler
    ctx.fingerprint_ctx
        .ui
        .switches
        .term
        .connect_state_set(move |_switch, state| {
            if state {
                info!("User enabled sudo fingerprint authentication switch");
            } else {
                info!("User disabled sudo fingerprint authentication switch");
            }

            let res = if state {
                PamHelper::apply_configuration(SUDO_PATH)
            } else {
                PamHelper::remove_configuration(SUDO_PATH)
            };

            match res {
                Ok(()) => {
                    if state {
                        info!("Successfully enabled fingerprint authentication for sudo");
                    } else {
                        info!("Successfully disabled fingerprint authentication for sudo");
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to {} fingerprint authentication for sudo: {}",
                        if state { "enable" } else { "disable" },
                        e
                    );
                    return gtk4::glib::Propagation::Stop;
                }
            }
            gtk4::glib::Propagation::Proceed
        });

    // Polkit switch handler
    ctx.fingerprint_ctx
        .ui
        .switches
        .prompt
        .connect_state_set(move |_switch, state| {
            if state {
                info!("User enabled polkit fingerprint authentication switch");
            } else {
                info!("User disabled polkit fingerprint authentication switch");
            }

            let res = if state {
                PamHelper::apply_configuration(POLKIT_PATH)
            } else {
                PamHelper::remove_configuration(POLKIT_PATH)
            };

            match res {
                Ok(()) => {
                    if state {
                        info!("Successfully enabled fingerprint authentication for polkit");
                    } else {
                        info!("Successfully disabled fingerprint authentication for polkit");
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to {} fingerprint authentication for polkit: {}",
                        if state { "enable" } else { "disable" },
                        e
                    );
                    return gtk4::glib::Propagation::Stop;
                }
            }
            gtk4::glib::Propagation::Proceed
        });
}

/// Set up navigation buttons.
fn setup_navigation_buttons(ctx: &AppContext, builder: &Builder) {
    let manage_btn: Button = builder
        .object("manage_btn")
        .expect("Failed to get manage_btn");
    let back_btn: Button = builder.object("back_btn").expect("Failed to get back_btn");
    let button_back: Button = builder
        .object("button_back")
        .expect("Failed to get button_back");

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
    let info_btn: Button = builder.object("info_btn").expect("Failed to get info_btn");

    let window_clone = window.clone();
    info_btn.connect_clicked(move |_| {
        info!("User clicked 'About' button - showing info dialog");
        show_info_dialog(&window_clone);
    });
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

/// Show SDDM fingerprint usage hint dialog (loaded from UI resource).
fn show_sddm_hint(parent: &ApplicationWindow) {
    info!("Displaying SDDM fingerprint hint dialog");
    let builder = Builder::from_resource("/xyz/xerolinux/xfprintd_gui/ui/sddm_hint_dialog.ui");

    let window: Window = builder
        .object("sddm_hint_window")
        .expect("Failed to get sddm_hint_window");

    let close_button: Button = builder
        .object("sddm_hint_close_button")
        .expect("Failed to get sddm_hint_close_button");

    window.set_transient_for(Some(parent));

    let window_clone = window.clone();
    close_button.connect_clicked(move |_| {
        window_clone.close();
    });

    window.show();
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

/// Perform initial fingerprint scan and enable switches if fingerprints found.
fn perform_initial_fingerprint_scan(ctx: &AppContext) {
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
                update_fingerprint_ui(enrolled, &ctx_clone);
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
fn update_fingerprint_ui(enrolled: HashSet<String>, ctx: &FingerprintContext) {
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
    update_button_states(&enrolled, ctx);

    while let Some(child) = ctx.ui.flow.first_child() {
        ctx.ui.flow.remove(&child);
    }

    create_finger_sections(&enrolled, ctx);

    info!("Finger selection UI updated successfully with hand separation");
}

/// Create finger button sections for left and right hands.
fn create_finger_sections(enrolled: &HashSet<String>, ctx: &FingerprintContext) {
    let left_fingers = &fprintd::FINGERS[0..5];
    let right_fingers = &fprintd::FINGERS[5..10];

    let right_hand_container = create_hand_section("Right Hand", right_fingers, enrolled, ctx);
    ctx.ui.flow.append(&right_hand_container);

    let left_hand_container = create_hand_section("Left Hand", left_fingers, enrolled, ctx);
    ctx.ui.flow.append(&left_hand_container);
}

/// Create hand section (left or right) with finger buttons.
fn create_hand_section(
    title: &str,
    fingers: &[&str],
    enrolled: &HashSet<String>,
    ctx: &FingerprintContext,
) -> GtkBox {
    let hand_container = GtkBox::new(Orientation::Vertical, 10);
    hand_container.set_halign(Align::Center);

    let title_label = Label::new(Some(title));
    title_label.set_css_classes(&["hand-title"]);
    hand_container.append(&title_label);

    let finger_grid = GtkBox::new(Orientation::Horizontal, 8);
    finger_grid.set_halign(Align::Center);
    finger_grid.set_homogeneous(true);

    for finger in fingers {
        let finger_box = create_finger_button(finger, enrolled, ctx);
        finger_grid.append(&finger_box);
    }

    hand_container.append(&finger_grid);
    hand_container
}

/// Create finger button widget.
fn create_finger_button(
    finger: &str,
    enrolled: &HashSet<String>,
    ctx: &FingerprintContext,
) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 5);
    container.set_halign(Align::Center);
    container.set_size_request(120, 120);

    let button = Button::new();
    button.set_size_request(90, 90);

    let is_enrolled = enrolled.contains(&finger.to_string());
    //let is_enrolled = rand::random::<bool>(); silly little debugger

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
    let enrolled_c = enrolled.clone();

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
        let is_enrolled = enrolled_c.contains(&finger_key);
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
fn update_button_states(enrolled: &HashSet<String>, ctx: &FingerprintContext) {
    if let Some(ref finger_key) = ctx.get_selected_finger() {
        let is_enrolled = enrolled.contains(finger_key);

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
