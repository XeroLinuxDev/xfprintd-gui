use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Builder, Button, CssProvider, FlowBox,
    Image, Label, Orientation, Stack, Switch, TextView, gio,
};

use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

mod fprintd;
mod pam_config;
use pam_config::PamConfig;

fn run_enroll_in_textview(text_view: &TextView, finger: &str) {
    let cmd = format!("fprintd-enroll -f {}", finger);

    // Clear and print the command at the top
    let buffer = text_view.buffer();
    buffer.set_text("");
    let mut iter = buffer.end_iter();
    buffer.insert(&mut iter, &format!("$ {}\n\n", cmd));
    // Auto-scroll to bottom
    let mut end_scroll = buffer.end_iter();
    text_view.scroll_to_iter(&mut end_scroll, 0.0, false, 0.0, 1.0);

    // Channel to send process output back to the UI thread
    let (tx, rx) = mpsc::channel::<String>();

    // UI updater: poll the channel regularly on the main thread
    let tv = text_view.clone();
    gtk4::glib::timeout_add_local(Duration::from_millis(100), move || {
        loop {
            match rx.try_recv() {
                Ok(line) => {
                    let buffer = tv.buffer();
                    let mut iter = buffer.end_iter();
                    buffer.insert(&mut iter, &line);
                    // Auto-scroll to bottom
                    let mut end_scroll = buffer.end_iter();
                    tv.scroll_to_iter(&mut end_scroll, 0.0, false, 0.0, 1.0);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return gtk4::glib::ControlFlow::Break,
            }
        }
        gtk4::glib::ControlFlow::Continue
    });

    // Spawn the process with piped stdout and stderr
    let mut child = match Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(format!("Failed to start process: {}\n", e));
            return;
        }
    };

    // Pump stdout
    if let Some(stdout) = child.stdout.take() {
        let tx_out = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = line.unwrap_or_default();
                let _ = tx_out.send(format!("{}\n", line));
            }
        });
    }

    // Pump stderr
    if let Some(stderr) = child.stderr.take() {
        let tx_err = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                let line = line.unwrap_or_default();
                let _ = tx_err.send(format!("{}\n", line));
            }
        });
    }

    // Wait for completion and notify
    std::thread::spawn(move || {
        let status = child.wait();
        let msg = match status {
            Ok(s) if s.success() => "\nEnrollment finished.\n".to_string(),
            Ok(s) => format!("\nProcess exited with code: {:?}\n", s.code()),
            Err(e) => format!("\nFailed to wait for process: {}\n", e),
        };
        let _ = tx.send(msg);
        // When this thread ends and all senders drop, the UI timeout will stop automatically.
    });
}

fn scan_enrolled() -> HashSet<String> {
    println!("[fprintd] Scanning for enrolled fingers...");
    let mut s = HashSet::new();

    match fprintd::Client::system() {
        Ok(client) => {
            println!("[fprintd] Connected to system bus. Querying Manager...");
            let mgr = client.manager();
            match mgr.get_devices() {
                Ok(paths) => {
                    println!("[fprintd] Found {} device(s).", paths.len());
                    if let Some(path) = paths.first() {
                        println!("[fprintd] Using device: {}", path.as_str());
                        let dev = client.device((*path).clone());

                        // Try to claim before querying, then release afterwards
                        let username = std::env::var("USER").unwrap_or_else(|_| String::from(""));
                        if let Err(e) = dev.claim(&username) {
                            println!("[fprintd][warn] Claim failed for '{}': {e}", username);
                        }

                        match dev.list_enrolled_fingers(&username) {
                            Ok(list) => {
                                println!("[fprintd] Enrolled fingers reported: {:?}", list);
                                for f in list {
                                    s.insert(f.clone());
                                    if let Some(stripped) = f.strip_suffix("-finger") {
                                        s.insert(stripped.to_string());
                                    }
                                }
                            }
                            Err(e) => {
                                println!("[fprintd][error] ListEnrolledFingers failed: {e}");
                            }
                        }

                        if let Err(e) = dev.release() {
                            println!("[fprintd][warn] Release failed: {e}");
                        }
                    } else {
                        println!("[fprintd][warn] No devices available");
                    }
                }
                Err(e) => {
                    println!("[fprintd][error] GetDevices failed: {e}");
                }
            }
        }
        Err(e) => {
            println!("[fprintd][error] Failed to connect to system bus: {e}");
        }
    }

    s
}

fn populate_fingers(
    fingers_flow: &FlowBox,
    terminal_view: &TextView,
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

        let icon = Image::from_icon_name("fingerprint");
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

        let sw_login_c = sw_login.clone();
        let sw_term_c = sw_term.clone();
        let sw_prompt_c = sw_prompt.clone();
        let term_view_c = terminal_view.clone();
        let finger_clone = finger_string.clone();
        let btn_for_css = event_btn.clone();
        event_btn.connect_clicked(move |_| {
            run_enroll_in_textview(&term_view_c, &finger_clone);
            sw_login_c.set_sensitive(true);
            sw_term_c.set_sensitive(true);
            sw_prompt_c.set_sensitive(true);
            btn_for_css.add_css_class("finger-enrolled");
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
                if let (Some(fingers_flow), Some(terminal_view)) = (
                    builder_c.object::<FlowBox>("fingers_flow"),
                    builder_c.object::<TextView>("terminal_view"),
                ) {
                    populate_fingers(
                        &fingers_flow,
                        &terminal_view,
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
        let terminal_view: TextView = builder
            .object("terminal_view")
            .expect("Failed to get terminal_view");

        // Populate buttons based on current enrollment
        populate_fingers(
            &fingers_flow,
            &terminal_view,
            &sw_login,
            &sw_term,
            &sw_prompt,
        );

        stack.set_visible_child_name("main");

        window.show();
    });

    app.run();
}
