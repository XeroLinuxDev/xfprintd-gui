use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Builder, Button, CssProvider, FlowBox,
    Image, Label, Orientation, Stack, Switch, gio,
};

use std::collections::HashSet;
use std::process::Command;

mod fprintd;
mod pam_config;
use pam_config::PamConfig;

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

fn scan_enrolled() -> HashSet<String> {
    println!("[fprintd] Scanning for enrolled fingers...");
    let mut s = HashSet::new();
    if let Ok(client) = fprintd::Client::system() {
        println!("[fprintd] Connected to system bus. Querying Manager...");
        let mgr = client.manager();
        if let Ok(paths) = mgr.get_devices() {
            println!("[fprintd] Found {} device(s).", paths.len());
            if let Some(path) = paths.first() {
                println!("[fprintd] Using device: {}", path.as_str());
                let dev = client.device((*path).clone());
                if let Ok(list) = dev.list_enrolled_fingers() {
                    println!("[fprintd] Enrolled fingers reported: {:?}", list);
                    for f in list {
                        s.insert(f.clone());
                        if let Some(stripped) = f.strip_suffix("-finger") {
                            s.insert(stripped.to_string());
                        }
                    }
                }
            }
        }
    }
    s
}

fn populate_fingers(
    fingers_flow: &FlowBox,
    stack: &Stack,
    sw_login: &Switch,
    sw_term: &Switch,
    sw_prompt: &Switch,
) {
    // Clear existing children
    while let Some(child) = fingers_flow.first_child() {
        fingers_flow.remove(&child);
    }

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

    let enrolled = scan_enrolled();

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
        // Color already-enrolled fingers
        if enrolled.contains(&finger_string)
            || enrolled.contains(&format!("{}-finger", finger_string))
        {
            event_btn.add_css_class("finger-enrolled");
        }

        let stack_nav = stack.clone();
        let sw_login_c = sw_login.clone();
        let sw_term_c = sw_term.clone();
        let sw_prompt_c = sw_prompt.clone();
        let finger_clone = finger_string.clone();
        let btn_for_css = event_btn.clone();
        event_btn.connect_clicked(move |_| {
            run_enroll_in_terminal(&finger_clone);
            sw_login_c.set_sensitive(true);
            sw_term_c.set_sensitive(true);
            sw_prompt_c.set_sensitive(true);
            btn_for_css.add_css_class("finger-enrolled");
            stack_nav.set_visible_child_name("main");
        });

        fingers_flow.append(&event_btn);
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
            gtk4::style_context_add_provider_for_display(
                &display,
                &css_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

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
                    return gtk4::glib::Propagation::Stop;
                }
                gtk4::glib::Propagation::Proceed
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
                    return gtk4::glib::Propagation::Stop;
                }
                gtk4::glib::Propagation::Proceed
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
                    return gtk4::glib::Propagation::Stop;
                }
                gtk4::glib::Propagation::Proceed
            });
        }

        // Navigation
        {
            let builder_c = builder.clone();
            let stack_nav = stack.clone();
            let sw_login_c = sw_login.clone();
            let sw_term_c = sw_term.clone();
            let sw_prompt_c = sw_prompt.clone();
            enroll_btn.connect_clicked(move |_| {
                if let Some(fingers_flow) = builder_c.object::<FlowBox>("fingers_flow") {
                    populate_fingers(
                        &fingers_flow,
                        &stack_nav,
                        &sw_login_c,
                        &sw_term_c,
                        &sw_prompt_c,
                    );
                }
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

        // Populate buttons based on current enrollment
        populate_fingers(&fingers_flow, &stack, &sw_login, &sw_term, &sw_prompt);

        stack.set_visible_child_name("main");

        window.show();
    });

    app.run();
}
