//! User Interface handling functionality.

use crate::fingerprints::{enroll, remove};
use crate::pam_helper::PamHelper;
use crate::util;
use crate::{fprintd, system};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    gio, pango, Align, Application, ApplicationWindow, Box as GtkBox, Builder, Button, CssProvider,
    FlowBox, Image, Justification, Label, Orientation, Stack, Switch,
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
    pub rt: Arc<Runtime>,
    pub stack: Stack,
    pub fingers_flow: FlowBox,
    pub selected_finger: Rc<RefCell<Option<String>>>,
    pub finger_label: Label,
    pub action_label: Label,
    pub button_add: Button,
    pub button_delete: Button,
    pub sw_login: Switch,
    pub sw_term: Switch,
    pub sw_prompt: Switch,
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
    setup_navigation_buttons(&ctx, &builder);
    setup_info_button(&window, &builder);
    setup_fingerprint_management(&ctx, &builder);

    perform_initial_fingerprint_scan(&ctx);

    info!("Setting initial view to main page");
    ctx.stack.set_visible_child_name("main");
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

    let selected_finger: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    AppContext {
        rt,
        stack,
        fingers_flow,
        selected_finger,
        finger_label,
        action_label,
        button_add,
        button_delete,
        sw_login,
        sw_term,
        sw_prompt,
    }
}

/// Set up PAM authentication switches.
fn setup_pam_switches(ctx: &AppContext) {
    info!("Checking current PAM authentication configurations");
    let (login_configured, sudo_configured, polkit_configured) =
        PamHelper::check_all_configurations();

    info!("PAM Configuration Status:");
    info!(
        "- Login authentication: {}",
        if login_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "- Sudo authentication: {}",
        if sudo_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "- Polkit authentication: {}",
        if polkit_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );

    ctx.sw_login.set_active(login_configured);
    ctx.sw_term.set_active(sudo_configured);
    ctx.sw_prompt.set_active(polkit_configured);

    info!("Temporarily disabling PAM switches until fingerprint enrollment check");
    ctx.sw_login.set_sensitive(false);
    ctx.sw_term.set_sensitive(false);
    ctx.sw_prompt.set_sensitive(false);

    setup_pam_switch_handlers(ctx);
}

/// Set up PAM switch event handlers.
fn setup_pam_switch_handlers(ctx: &AppContext) {
    ctx.sw_login.connect_state_set(move |_switch, state| {
        if state {
            info!("User enabled login fingerprint authentication switch");
        } else {
            info!("User disabled login fingerprint authentication switch");
        }

        let res = if state {
            PamHelper::apply_login()
        } else {
            PamHelper::remove_login()
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
    ctx.sw_term.connect_state_set(move |_switch, state| {
        if state {
            info!("User enabled sudo fingerprint authentication switch");
        } else {
            info!("User disabled sudo fingerprint authentication switch");
        }

        let res = if state {
            PamHelper::apply_sudo()
        } else {
            PamHelper::remove_sudo()
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
    ctx.sw_prompt.connect_state_set(move |_switch, state| {
        if state {
            info!("User enabled polkit fingerprint authentication switch");
        } else {
            info!("User disabled polkit fingerprint authentication switch");
        }

        let res = if state {
            PamHelper::apply_polkit()
        } else {
            PamHelper::remove_polkit()
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
        let stack = ctx.stack.clone();
        manage_btn.connect_clicked(move |_| {
            info!("User clicked 'Manage' button - navigating to management page");
            stack.set_visible_child_name("manage");
        });
    }

    {
        let stack = ctx.stack.clone();
        back_btn.connect_clicked(move |_| {
            info!("User clicked 'Back' button - returning to main page");
            stack.set_visible_child_name("main");
        });
    }

    {
        let stack = ctx.stack.clone();
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

/// Set up fingerprint management buttons.
fn setup_fingerprint_management(ctx: &AppContext, _builder: &Builder) {
    setup_enroll_button(&ctx.button_add, ctx);
    setup_delete_button(&ctx.button_delete, ctx);
}

/// Set up enrollment button.
fn setup_enroll_button(button_add: &Button, ctx: &AppContext) {
    let ctx_clone = ctx.clone();
    button_add.connect_clicked(move |_| {
        if let Some(key) = ctx_clone.selected_finger.borrow().clone() {
            info!("User clicked 'Add' button for finger: '{}'", key);
            info!("Initiating fingerprint enrollment process");

            let enrollment_ctx = enroll::EnrollmentContext {
                rt: ctx_clone.rt.clone(),
                flow: ctx_clone.fingers_flow.clone(),
                stack: ctx_clone.stack.clone(),
                sw_login: ctx_clone.sw_login.clone(),
                sw_term: ctx_clone.sw_term.clone(),
                sw_prompt: ctx_clone.sw_prompt.clone(),
                selected_finger: ctx_clone.selected_finger.clone(),
                finger_label: ctx_clone.finger_label.clone(),
                action_label: ctx_clone.action_label.clone(),
                button_add: ctx_clone.button_add.clone(),
                button_delete: ctx_clone.button_delete.clone(),
            };

            enroll::start_enrollment(key, enrollment_ctx);
        }
    });
}

/// Set up delete button.
fn setup_delete_button(button_delete: &Button, ctx: &AppContext) {
    let ctx_clone = ctx.clone();
    button_delete.connect_clicked(move |_| {
        if let Some(key) = ctx_clone.selected_finger.borrow().clone() {
            let removal_ctx = remove::RemovalContext {
                rt: ctx_clone.rt.clone(),
                flow: ctx_clone.fingers_flow.clone(),
                stack: ctx_clone.stack.clone(),
                sw_login: ctx_clone.sw_login.clone(),
                sw_term: ctx_clone.sw_term.clone(),
                sw_prompt: ctx_clone.sw_prompt.clone(),
                selected_finger: ctx_clone.selected_finger.clone(),
                finger_label: ctx_clone.finger_label.clone(),
                action_label: ctx_clone.action_label.clone(),
                button_add: ctx_clone.button_add.clone(),
                button_delete: ctx_clone.button_delete.clone(),
            };

            remove::start_removal(key, removal_ctx);
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
            ctx_clone.sw_login.set_sensitive(has_any);
            ctx_clone.sw_term.set_sensitive(has_any);
            ctx_clone.sw_prompt.set_sensitive(has_any);
            glib::ControlFlow::Break
        }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
    });

    let rt = ctx.rt.clone();
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

    refresh_fingerprint_display(
        ctx.rt.clone(),
        ctx.fingers_flow.clone(),
        ctx.stack.clone(),
        ctx.selected_finger.clone(),
        ctx.finger_label.clone(),
        ctx.action_label.clone(),
        ctx.button_add.clone(),
        ctx.button_delete.clone(),
        ctx.sw_login.clone(),
        ctx.sw_term.clone(),
        ctx.sw_prompt.clone(),
    );
}

/// Refresh fingerprint display UI.
#[allow(clippy::too_many_arguments)]
pub fn refresh_fingerprint_display(
    rt: Arc<Runtime>,
    fingers_flow: FlowBox,
    stack: Stack,
    selected_finger: Rc<RefCell<Option<String>>>,
    finger_label: Label,
    action_label: Label,
    button_add: Button,
    button_delete: Button,
    sw_login: Switch,
    sw_term: Switch,
    sw_prompt: Switch,
) {
    let (tx, rx) = mpsc::channel::<HashSet<String>>();

    {
        let fingers_flow_clone = fingers_flow.clone();
        let stack_clone = stack.clone();
        let selected_clone = selected_finger.clone();
        let finger_label_clone = finger_label.clone();
        let action_label_clone = action_label.clone();
        let button_add_clone = button_add.clone();
        let button_delete_clone = button_delete.clone();
        let sw_login_clone = sw_login.clone();
        let sw_term_clone = sw_term.clone();
        let sw_prompt_clone = sw_prompt.clone();

        glib::idle_add_local(move || match rx.try_recv() {
            Ok(enrolled) => {
                update_fingerprint_ui(
                    enrolled,
                    &fingers_flow_clone,
                    &stack_clone,
                    &selected_clone,
                    &finger_label_clone,
                    &action_label_clone,
                    &button_add_clone,
                    &button_delete_clone,
                    &sw_login_clone,
                    &sw_term_clone,
                    &sw_prompt_clone,
                );
                glib::ControlFlow::Break
            }
            Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    rt.spawn(async move {
        let enrolled = crate::fingerprints::scan_enrolled_fingerprints().await;
        let _ = tx.send(enrolled);
    });
}

/// Update fingerprint UI with enrolled fingerprints.
#[allow(clippy::too_many_arguments)]
fn update_fingerprint_ui(
    enrolled: HashSet<String>,
    fingers_flow: &FlowBox,
    stack: &Stack,
    selected_finger: &Rc<RefCell<Option<String>>>,
    finger_label: &Label,
    action_label: &Label,
    button_add: &Button,
    button_delete: &Button,
    sw_login: &Switch,
    sw_term: &Switch,
    sw_prompt: &Switch,
) {
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

    sw_login.set_sensitive(has_any);
    sw_term.set_sensitive(has_any);
    sw_prompt.set_sensitive(has_any);

    // Update button states based on selected finger and enrollment status
    update_button_states(&enrolled, selected_finger, button_add, button_delete);

    while let Some(child) = fingers_flow.first_child() {
        fingers_flow.remove(&child);
    }

    create_finger_sections(
        &enrolled,
        fingers_flow,
        selected_finger,
        finger_label,
        action_label,
        button_add,
        button_delete,
        stack,
    );

    info!("Finger selection UI updated successfully with hand separation");
}

/// Create finger button sections for left and right hands.
#[allow(clippy::too_many_arguments)]
fn create_finger_sections(
    enrolled: &HashSet<String>,
    fingers_flow: &FlowBox,
    selected_finger: &Rc<RefCell<Option<String>>>,
    finger_label: &Label,
    action_label: &Label,
    button_add: &Button,
    button_delete: &Button,
    stack: &Stack,
) {
    let left_fingers = &fprintd::FINGERS[0..5];
    let right_fingers = &fprintd::FINGERS[5..10];

    let right_hand_container = create_hand_section(
        "Right Hand",
        right_fingers,
        enrolled,
        selected_finger,
        finger_label,
        action_label,
        button_add,
        button_delete,
        stack,
    );
    fingers_flow.append(&right_hand_container);

    let left_hand_container = create_hand_section(
        "Left Hand",
        left_fingers,
        enrolled,
        selected_finger,
        finger_label,
        action_label,
        button_add,
        button_delete,
        stack,
    );
    fingers_flow.append(&left_hand_container);
}

/// Create hand section (left or right) with finger buttons.
#[allow(clippy::too_many_arguments)]
fn create_hand_section(
    title: &str,
    fingers: &[&str],
    enrolled: &HashSet<String>,
    selected_finger: &Rc<RefCell<Option<String>>>,
    finger_label: &Label,
    action_label: &Label,
    button_add: &Button,
    button_delete: &Button,
    stack: &Stack,
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
        let finger_box = create_finger_button(
            finger,
            enrolled,
            selected_finger.clone(),
            finger_label.clone(),
            action_label.clone(),
            button_add.clone(),
            button_delete.clone(),
            stack.clone(),
        );
        finger_grid.append(&finger_box);
    }

    hand_container.append(&finger_grid);
    hand_container
}

/// Create finger button widget.
#[allow(clippy::too_many_arguments)]
fn create_finger_button(
    finger: &str,
    enrolled: &HashSet<String>,
    selected_finger: Rc<RefCell<Option<String>>>,
    finger_label: Label,
    action_label: Label,
    button_add: Button,
    button_delete: Button,
    stack: Stack,
) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 5);
    container.set_halign(Align::Center);
    container.set_size_request(100, 120);

    let button = Button::new();
    button.set_size_request(90, 90);

    let is_enrolled = enrolled.contains(&finger.to_string());

    let image = if is_enrolled {
        Image::from_icon_name("fingerprint-enrolled")
    } else {
        Image::from_icon_name("fingerprint-unenrolled")
    };
    image.set_pixel_size(48);
    button.set_child(Some(&image));

    if is_enrolled {
        button.add_css_class("finger-enrolled");
    } else {
        button.add_css_class("finger-unenrolled");
    }

    let finger_key = finger.to_string();
    let selected_c = selected_finger.clone();
    let finger_label_c = finger_label.clone();
    let action_label_c = action_label.clone();
    let stack_c = stack.clone();
    let button_add_c = button_add.clone();
    let button_delete_c = button_delete.clone();
    let enrolled_c = enrolled.clone();

    button.connect_clicked(move |_| {
        *selected_c.borrow_mut() = Some(finger_key.clone());
        finger_label_c.set_label(&util::display_finger_name(&finger_key));
        action_label_c.set_use_markup(false);
        action_label_c.set_label("Select an action below.");
        stack_c.set_visible_child_name("finger");
        info!("User selected finger: '{}'", finger_key);

        // Update button states when finger is selected
        update_button_states(&enrolled_c, &selected_c, &button_add_c, &button_delete_c);
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
fn update_button_states(
    enrolled: &HashSet<String>,
    selected_finger: &Rc<RefCell<Option<String>>>,
    button_add: &Button,
    button_delete: &Button,
) {
    if let Some(ref finger_key) = *selected_finger.borrow() {
        let is_enrolled = enrolled.contains(finger_key);

        // Enable Add button if fingerprint is NOT enrolled
        button_add.set_sensitive(!is_enrolled);

        // Enable Remove button if fingerprint IS enrolled
        button_delete.set_sensitive(is_enrolled);

        info!(
            "Updated button states for finger '{}': Add={}, Remove={}",
            finger_key, !is_enrolled, is_enrolled
        );
    } else {
        // No finger selected, disable both buttons
        button_add.set_sensitive(false);
        button_delete.set_sensitive(false);
        info!("No finger selected, both buttons disabled");
    }
}
