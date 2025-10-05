//! Application setup and initialization functionality.

use crate::core::{system, FingerprintContext};
use crate::ui::{button_handlers, fingerprint_ui, navigation, pam_ui};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{gio, Application, ApplicationWindow, Builder, CssProvider};
use log::{info, warn};

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

    // Setup UI components by category
    pam_ui::setup_pam_switches(&ctx);
    navigation::setup_navigation_and_dialogs(&ctx, &builder, &window);
    button_handlers::setup_button_handlers(&ctx);
    fingerprint_ui::perform_initial_fingerprint_scan(&ctx);

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

/// Create main application window.
fn create_main_window(app: &Application, builder: &Builder) -> ApplicationWindow {
    let window: ApplicationWindow = builder
        .object("app_window")
        .expect("Failed to get app_window");

    window.set_application(Some(app));
    info!("Setting window icon to fingerprint");
    window.set_icon_name(Some("xfprintd-gui"));

    window
}

/// Helper to extract widgets from builder with consistent error handling.
pub fn extract_widget<T: IsA<glib::Object>>(builder: &Builder, name: &str) -> T {
    builder
        .object(name)
        .unwrap_or_else(|| panic!("Failed to get {}", name))
}

/// Set up UI components and return application context.
fn setup_ui_components(
    _window: &ApplicationWindow,
    rt: Arc<Runtime>,
    builder: &Builder,
) -> AppContext {
    // Extract all widgets using helper
    let stack = extract_widget(builder, "stack");
    let fingers_flow = extract_widget(builder, "fingers_flow");
    let finger_label = extract_widget(builder, "finger_label");
    let action_label = extract_widget(builder, "action_label");
    let button_add = extract_widget(builder, "button_add");
    let button_delete = extract_widget(builder, "button_delete");
    let sw_login = extract_widget(builder, "sw_login");
    let sw_term = extract_widget(builder, "sw_term");
    let sw_prompt = extract_widget(builder, "sw_prompt");

    info!("All UI components successfully initialized from Glade builder");

    // Assemble UI components using builder pattern
    let switches = crate::core::context::PamSwitches::new(sw_login, sw_term, sw_prompt);
    let labels = crate::core::context::FingerprintLabels::new(finger_label, action_label);
    let buttons = crate::core::context::FingerprintButtons::new(button_add, button_delete);
    let ui =
        crate::core::context::UiComponents::new(fingers_flow, stack, switches, labels, buttons);

    let selected_finger = std::rc::Rc::new(std::cell::RefCell::new(None));
    let fingerprint_ctx = FingerprintContext::new(rt, ui, selected_finger);

    AppContext { fingerprint_ctx }
}
