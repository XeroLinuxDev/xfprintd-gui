use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, Grid, Image, Label, Orientation,
    Stack, Switch,
};
use std::cell::RefCell;
use std::path::Path;
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

fn build_main_page(app_state: Rc<RefCell<AppState>>, stack: &Stack) -> GtkBox {
    let v = GtkBox::new(Orientation::Vertical, 15);
    v.set_margin_top(20);
    v.set_margin_bottom(20);
    v.set_margin_start(30);
    v.set_margin_end(30);

    let title = Label::new(Some("Fingerprint Authentication"));
    title.set_halign(Align::Center);
    title.set_css_classes(&["title-1"]);
    v.append(&title);

    let icon = if Path::new("assets/fingerprint.png").exists() {
        Image::from_file("assets/fingerprint.png")
    } else {
        Image::from_icon_name("dialog-password-symbolic")
    };
    icon.set_pixel_size(48);
    icon.set_halign(Align::Center);
    v.append(&icon);

    let desc = Label::new(Some(
        "Manage fingerprint authentication on your system, including enrollment and enabling fingerprint login, terminal use, and system prompts.",
    ));
    desc.set_wrap(true);
    desc.set_justify(gtk4::Justification::Center);
    desc.set_halign(Align::Center);
    desc.set_margin_bottom(10);
    v.append(&desc);

    let enroll_btn = Button::with_label("Enroll Fingerprint");
    enroll_btn.set_size_request(180, 36);
    enroll_btn.set_halign(Align::Center);
    enroll_btn.set_css_classes(&["suggested-action"]);
    v.append(&enroll_btn);

    let grid = Grid::new();
    grid.set_margin_top(20);
    grid.set_row_spacing(10);
    grid.set_column_spacing(15);
    grid.set_halign(Align::Center);

    let (login_configured, sudo_configured, polkit_configured) = PamConfig::check_configurations();

    let sw_login = Switch::new();
    sw_login.set_active(login_configured);
    sw_login.set_sensitive(true);
    sw_login.set_tooltip_text(Some("Enroll fingerprint first."));
    grid.attach(
        &Label::new(Some("Enable Authentication On Login")),
        0,
        0,
        1,
        1,
    );
    grid.attach(&sw_login, 1, 0, 1, 1);

    let sw_term = Switch::new();
    sw_term.set_active(sudo_configured);
    sw_term.set_sensitive(true);
    sw_term.set_tooltip_text(Some("Enroll fingerprint first."));
    grid.attach(
        &Label::new(Some("Enable Authentication in Terminal")),
        0,
        1,
        1,
        1,
    );
    grid.attach(&sw_term, 1, 1, 1, 1);

    let sw_prompt = Switch::new();
    sw_prompt.set_active(polkit_configured);
    sw_prompt.set_sensitive(true);
    sw_prompt.set_tooltip_text(Some("Enroll fingerprint first."));
    grid.attach(
        &Label::new(Some("Enable Authentication in System Prompt")),
        0,
        2,
        1,
        1,
    );
    grid.attach(&sw_prompt, 1, 2, 1, 1);

    v.append(&grid);

    {
        sw_login.connect_state_notify(move |switch| {
            if switch.is_active() {
                if PamConfig::apply_patch("/etc/pam.d/login").is_err() {
                    switch.set_active(false);
                }
            } else if PamConfig::remove_patch("/etc/pam.d/login").is_err() {
                switch.set_active(true);
            }
        });
    }

    {
        sw_term.connect_state_notify(move |switch| {
            if switch.is_active() {
                if PamConfig::apply_patch("/etc/pam.d/sudo").is_err() {
                    switch.set_active(false);
                }
            } else if PamConfig::remove_patch("/etc/pam.d/sudo").is_err() {
                switch.set_active(true);
            }
        });
    }

    {
        sw_prompt.connect_state_notify(move |switch| {
            if switch.is_active() {
                if PamConfig::copy_default_polkit().is_err()
                    || PamConfig::apply_patch("/etc/pam.d/polkit-1").is_err()
                {
                    switch.set_active(false);
                }
            } else if PamConfig::remove_patch("/etc/pam.d/polkit-1").is_err() {
                switch.set_active(true);
            }
        });
    }

    app_state.borrow_mut().sw_login = sw_login;
    app_state.borrow_mut().sw_term = sw_term;
    app_state.borrow_mut().sw_prompt = sw_prompt;

    let stack_clone = stack.clone();
    enroll_btn.connect_clicked(move |_| {
        stack_clone.set_visible_child_name("enroll");
    });

    v
}

fn build_enroll_page(app_state: Rc<RefCell<AppState>>, stack: &Stack) -> GtkBox {
    let outer = GtkBox::new(Orientation::Vertical, 15);
    outer.set_margin_top(20);
    outer.set_margin_bottom(20);
    outer.set_margin_start(30);
    outer.set_margin_end(30);

    let back_btn = Button::with_label("← Back");
    back_btn.set_halign(Align::Start);
    {
        let stack_back = stack.clone();
        back_btn.connect_clicked(move |_| {
            stack_back.set_visible_child_name("main");
        });
    }
    outer.append(&back_btn);

    let title = Label::new(Some("Select a finger to enroll:"));
    title.set_halign(Align::Center);
    title.set_margin_top(10);
    outer.append(&title);

    let desc = Label::new(Some(
        "Click (tap) on a finger below to begin enrolling that fingerprint. \
Once enrolled, you will be able to use it for login or system prompts.",
    ));
    desc.set_wrap(true);
    desc.set_justify(gtk4::Justification::Center);
    desc.set_halign(Align::Center);
    desc.set_margin_bottom(12);
    outer.append(&desc);

    let grid = Grid::new();
    grid.set_column_spacing(20);
    grid.set_row_spacing(20);
    grid.set_halign(Align::Center);

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

    for (i, finger) in fingers.iter().enumerate() {
        let finger_string = finger.to_string();

        let btn_box = GtkBox::new(Orientation::Vertical, 4);

        let icon = if Path::new("assets/fingerprint.png").exists() {
            Image::from_file("assets/fingerprint.png")
        } else {
            Image::from_icon_name("dialog-password-symbolic")
        };
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
            let stack = stack.clone();
            let finger_clone = finger_string.clone();
            event_btn.connect_clicked(move |_| {
                run_enroll_in_terminal(&finger_clone);
                let mut state = app_state.borrow_mut();
                state.fp_state = FingerprintState::Enrolled;
                state.sw_login.set_sensitive(true);
                state.sw_term.set_sensitive(true);
                state.sw_prompt.set_sensitive(true);
                stack.set_visible_child_name("main");
            });
        }

        grid.attach(&event_btn, (i % 5) as i32, (i / 5) as i32, 1, 1);
    }

    outer.append(&grid);
    outer
}

fn main() {
    let app = Application::builder()
        .application_id("com.example.fp_gui")
        .build();

    app.connect_activate(|app| {
        let dummy_switch = Switch::new();
        let app_state = Rc::new(RefCell::new(AppState {
            fp_state: FingerprintState::None,
            sw_login: dummy_switch.clone(),
            sw_term: dummy_switch.clone(),
            sw_prompt: dummy_switch.clone(),
        }));

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Fingerprint GUI")
            .default_width(560)
            .default_height(420)
            .build();

        let stack = Stack::new();
        let page_main = build_main_page(app_state.clone(), &stack);
        let page_enroll = build_enroll_page(app_state.clone(), &stack);

        stack.add_named(&page_main, Some("main"));
        stack.add_named(&page_enroll, Some("enroll"));
        stack.set_visible_child_name("main");

        window.set_child(Some(&stack));
        window.show();
    });

    app.run();
}
