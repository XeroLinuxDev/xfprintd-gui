use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Builder, Button, CssProvider, FlowBox,
    Image, Label, Orientation, Stack, StyleContext, Switch, gio,
};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

mod pam_config;
use pam_config::PamConfig;

#[derive(Clone, Copy, PartialEq)]
enum FingerprintState {
    None,
    Enrolled,
}

struct AppState {
    fp_state: FingerprintState,
    sw_login: Switch,
    sw_term: Switch,
    sw_prompt: Switch,
}

fn run_enroll_in_terminal(finger: &str) {
    let cmd = format!("fprintd-enroll -f {}", finger);

    let result = Command::new("konsole")
        .args(["--noclose", "-e", "bash", "-c", &cmd])
        .spawn();

    if result.is_err() {
        let _ = Command::new("cosmic-term")
            .args(["-e", "bash", "-c", &cmd])
            .spawn();
    }
}

fn main() {
    let app = Application::builder()
        .application_id("xyz.xerolinux.fp_gui")
        .build();

    app.connect_activate(|app| {
        // Register compiled gresources so Builder can load from resource paths
        gio::resources_register_include!("xyz.xerolinux.fp_gui.gresource")
            .expect("Failed to register gresources");
        // Make themed icons packaged in gresources discoverable by the icon theme
        if let Some(display) = gtk4::gdk::Display::default() {
            let theme = gtk4::IconTheme::for_display(&display);
            theme.add_resource_path("/xyz/xerolinux/fp_gui/icons");
            // Load application CSS from gresource
            let css_provider = CssProvider::new();
            css_provider.load_from_resource("/xyz/xerolinux/fp_gui/css/style.css");
            gtk4::StyleContext::add_provider_for_display(
                &display,
                &css_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        // App-wide state
        let dummy_switch = Switch::new();
        let app_state = Rc::new(RefCell::new(AppState {
            fp_state: FingerprintState::None,
            sw_login: dummy_switch.clone(),
            sw_term: dummy_switch.clone(),
            sw_prompt: dummy_switch.clone(),
        }));

        // Load UI from embedded GtkBuilder XML
        let builder = Builder::from_resource("/xyz/xerolinux/fp_gui/ui/main.ui");

        // Toplevel window and stack
        let window: ApplicationWindow = builder
            .object("app_window")
            .expect("Failed to get app_window");
        window.set_application(Some(app));

        let stack: Stack = builder.object("stack").expect("Failed to get stack");

        // Buttons
        let enroll_btn: Button = builder
            .object("enroll_btn")
            .expect("Failed to get enroll_btn");
        let back_btn: Button = builder.object("back_btn").expect("Failed to get back_btn");

        // Switches
        let sw_login: Switch = builder.object("sw_login").expect("Failed to get sw_login");
        let sw_term: Switch = builder.object("sw_term").expect("Failed to get sw_term");
        let sw_prompt: Switch = builder
            .object("sw_prompt")
            .expect("Failed to get sw_prompt");

        // Initial switch state from current PAM configuration
        let (login_configured, sudo_configured, polkit_configured) =
            PamConfig::check_configurations();
        sw_login.set_active(login_configured);
        sw_term.set_active(sudo_configured);
        sw_prompt.set_active(polkit_configured);

        sw_login.set_sensitive(true);
        sw_term.set_sensitive(true);
        sw_prompt.set_sensitive(true);

        sw_login.set_tooltip_text(Some("Enroll fingerprint first."));
        sw_term.set_tooltip_text(Some("Enroll fingerprint first."));
        sw_prompt.set_tooltip_text(Some("Enroll fingerprint first."));

        // Connect switch handlers to (un)patch PAM files
        {
            sw_login.connect_state_set(move |_switch, state| {
                let res = if state {
                    PamConfig::apply_patch("/etc/pam.d/login")
                } else {
                    PamConfig::remove_patch("/etc/pam.d/login")
                };
                if res.is_err() {
                    return gtk4::glib::signal::Inhibit(true);
                }
                gtk4::glib::signal::Inhibit(false)
            });
        }
        {
            sw_term.connect_state_set(move |_switch, state| {
                let res = if state {
                    PamConfig::apply_patch("/etc/pam.d/sudo")
                } else {
                    PamConfig::remove_patch("/etc/pam.d/sudo")
                };
                if res.is_err() {
                    return gtk4::glib::signal::Inhibit(true);
                }
                gtk4::glib::signal::Inhibit(false)
            });
        }
        {
            sw_prompt.connect_state_set(move |_switch, state| {
                let res = if state {
                    PamConfig::copy_default_polkit()
                        .and_then(|_| PamConfig::apply_patch("/etc/pam.d/polkit-1"))
                } else {
                    PamConfig::remove_patch("/etc/pam.d/polkit-1")
                };
                if res.is_err() {
                    return gtk4::glib::signal::Inhibit(true);
                }
                gtk4::glib::signal::Inhibit(false)
            });
        }

        // Store switches in app state for later updates
        {
            let mut st = app_state.borrow_mut();
            st.sw_login = sw_login.clone();
            st.sw_term = sw_term.clone();
            st.sw_prompt = sw_prompt.clone();
        }

        // Navigation
        {
            let stack_nav = stack.clone();
            enroll_btn.connect_clicked(move |_| {
                stack_nav.set_visible_child_name("enroll");
            });
        }
        {
            let stack_nav = stack.clone();
            back_btn.connect_clicked(move |_| {
                stack_nav.set_visible_child_name("main");
            });
        }

        // Populate finger buttons dynamically in the FlowBox
        let fingers_flow: FlowBox = builder
            .object("fingers_flow")
            .expect("Failed to get fingers_flow");

        let fingers = vec![
            "left-thumb",
            "left-index",
            "left-middle",
            "left-ring",
            "left-little",
            "right-thumb",
            "right-index",
            "right-middle",
            "right-ring",
            "right-little",
        ];

        for finger in fingers {
            let finger_string = finger.to_string();

            let btn_box = GtkBox::new(Orientation::Vertical, 4);

            let icon = Image::from_icon_name("dialog-password-symbolic");
            icon.set_pixel_size(32);
            icon.set_halign(Align::Center);

            let label = Label::new(Some(&finger_string.replace('-', " ")));
            label.set_halign(Align::Center);

            btn_box.append(&icon);
            btn_box.append(&label);

            let event_btn = Button::new();
            event_btn.set_child(Some(&btn_box));

            {
                let app_state = app_state.clone();
                let stack_nav = stack.clone();
                let finger_clone = finger_string.clone();
                event_btn.connect_clicked(move |_| {
                    run_enroll_in_terminal(&finger_clone);
                    let mut state = app_state.borrow_mut();
                    state.fp_state = FingerprintState::Enrolled;
                    state.sw_login.set_sensitive(true);
                    state.sw_term.set_sensitive(true);
                    state.sw_prompt.set_sensitive(true);
                    stack_nav.set_visible_child_name("main");
                });
            }

            fingers_flow.append(&event_btn);
        }

        stack.set_visible_child_name("main");

        window.show();
    });

    app.run();
}
