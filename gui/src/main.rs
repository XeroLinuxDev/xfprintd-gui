use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    gio, Align, Application, ApplicationWindow, Box as GtkBox, Builder, Button, CssProvider,
    FlowBox, Image, Label, Orientation, Stack, Switch,
};

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;

mod fprintd;
mod pam_helper;
use pam_helper::PamHelper;
use tokio::runtime::{Builder as TokioBuilder, Runtime};

fn display_finger_name(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let mut s = name.replace('-', " ");
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        let upper = first.to_ascii_uppercase().to_string();
        s.replace_range(0..first.len_utf8(), &upper);
    }
    s
}

#[derive(Clone)]
struct UiCtx {
    rt: Arc<Runtime>,
    flow: FlowBox,
    stack: Stack,
    sw_login: Switch,
    sw_term: Switch,
    sw_prompt: Switch,
    selected_finger: Rc<RefCell<Option<String>>>,
    finger_label: Label,
    action_label: Label,
}

#[derive(Clone)]
enum UiEvent {
    SetText(String),
    EnrollCompleted,
}

async fn scan_enrolled_async() -> HashSet<String> {
    let mut s = HashSet::new();

    println!("[fprintd] Connecting to system bus for scan_enrolled_async...");
    let client = match fprintd::Client::system().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[fprintd][error] Failed to connect to system bus: {e}");
            return s;
        }
    };

    println!("[fprintd] Querying first fprintd device...");
    let dev = match fprintd::first_device(&client).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            eprintln!("[fprintd][warn] No devices available");
            return s;
        }
        Err(e) => {
            eprintln!("[fprintd][error] GetDevices/GetDefaultDevice failed: {e}");
            return s;
        }
    };

    let username = std::env::var("USER").unwrap_or_default();

    println!("[fprintd] Claiming device for user '{}'", username);
    if let Err(e) = dev.claim(&username).await {
        eprintln!("[fprintd][warn] Claim failed for '{}': {e}", username);
    }

    println!("[fprintd] Listing enrolled fingers for user '{}'", username);
    match dev.list_enrolled_fingers(&username).await {
        Ok(list) => {
            println!("[fprintd] Retrieved {} enrolled fingers", list.len());
            for f in list {
                s.insert(f.clone());
            }
        }
        Err(e) => {
            eprintln!("[fprintd][error] ListEnrolledFingers failed: {e}");
        }
    }

    println!("[fprintd] Releasing device after scan");
    if let Err(e) = dev.release().await {
        eprintln!("[fprintd][warn] Release failed: {e}");
    }

    s
}

#[allow(clippy::too_many_arguments)]
fn start_enrollment(
    rt: Arc<Runtime>,
    finger_key: String,
    action_label: Label,
    refresh_flow: FlowBox,
    sw_login: Switch,
    sw_term: Switch,
    sw_prompt: Switch,
    stack: Stack,
    selected_finger: Rc<RefCell<Option<String>>>,
    finger_label: Label,
) {
    let ctx = UiCtx {
        rt,
        flow: refresh_flow,
        stack,
        sw_login,
        sw_term,
        sw_prompt,
        selected_finger,
        finger_label,
        action_label,
    };
    start_enrollment_ctx(finger_key, ctx);
}

fn start_enrollment_ctx(finger_key: String, ctx: UiCtx) {
    let (tx, rx) = mpsc::channel::<UiEvent>();

    {
        let lbl = ctx.action_label.clone();
        let ctx_for_pop = ctx.clone();

        glib::idle_add_local(move || {
            loop {
                match rx.try_recv() {
                    Ok(UiEvent::SetText(text)) => {
                        lbl.set_use_markup(true);
                        lbl.set_markup(&text);
                    }
                    Ok(UiEvent::EnrollCompleted) => {
                        populate_fingers_async_ctx(ctx_for_pop.clone());
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                }
            }
            glib::ControlFlow::Continue
        });
    }

    let _ = tx.send(UiEvent::SetText(
        "<b>Place your finger on the scanner…</b>".to_string(),
    ));

    ctx.rt.spawn(async move {
        println!("[fprintd] Connecting to system bus for enrollment...");
        let client = match fprintd::Client::system().await {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(UiEvent::SetText(format!(
                    "Failed to connect to system bus: {e}"
                )));
                return;
            }
        };

        println!("[fprintd] Selecting enrollment device...");
        let dev = match fprintd::first_device(&client).await {
            Ok(Some(d)) => d,
            Ok(None) => {
                let _ = tx.send(UiEvent::SetText("No fingerprint reader found.".to_string()));
                return;
            }
            Err(e) => {
                let _ = tx.send(UiEvent::SetText(format!("Failed to enumerate device: {e}")));
                return;
            }
        };

        // Claim device
        println!("[fprintd] Claiming device for enrollment (current user)...");
        if let Err(e) = dev.claim("").await {
            let _ = tx.send(UiEvent::SetText(format!("Could not claim device: {e}")));
        }

        // Attach listener first (like fingwit)
        let dev_for_listener = dev.clone();
        let dev_for_stop = dev_for_listener.clone();
        let tx_status = tx.clone();
        tokio::spawn(async move {
            let _ = dev_for_listener
                .listen_enroll_status(move |evt| {
                    println!("[fprintd] EnrollStatus: result={}, done={}", evt.result, evt.done);
                    let text = match evt.result.as_str() {
                        "enroll-completed" => {
                            let _ = tx_status.send(UiEvent::EnrollCompleted);
                            "<b>Well done!</b> Your fingerprint was saved successfully.".to_string()
                        }
                        "enroll-stage-passed" => "Good scan! Do it again...".to_string(),
                        "enroll-remove-and-retry" => "Try again...".to_string(),
                        "enroll-duplicate" => {
                            "<span color='orange'>This fingerprint is already saved, use a different finger.</span>".to_string()
                        }
                        "enroll-failed" => {
                            "<span color='red'><b>Sorry</b>, your fingerprint could not be saved.</span>".to_string()
                        }
                        other => format!("Enrollment status: {} (done={})", other, evt.done),
                    };
                    let _ = tx_status.send(UiEvent::SetText(text));
                    if evt.done {
                        let dev_stop = dev_for_stop.clone();
                        tokio::spawn(async move {
                            let _ = dev_stop.enroll_stop().await;
                            let _ = dev_stop.release().await;
                        });
                    }
                })
                .await;
        });

        // Start enrollment
        println!("[fprintd] EnrollStart for finger '{}'", finger_key);
        if let Err(e) = dev.enroll_start(&finger_key).await {
            let _ = tx.send(UiEvent::SetText(format!("Enrollment error: {e}")));
            let _ = dev.enroll_stop().await;
            let _ = dev.release().await;
        }
    });
}

fn populate_fingers_async_ctx(ctx: UiCtx) {
    let (tx, rx) = mpsc::channel::<HashSet<String>>();

    {
        let fingers_flow_clone = ctx.flow.clone();
        let stack_clone = ctx.stack.clone();
        let selected_clone = ctx.selected_finger.clone();
        let finger_label_clone = ctx.finger_label.clone();
        let action_label_clone = ctx.action_label.clone();
        let sw_login_clone = ctx.sw_login.clone();
        let sw_term_clone = ctx.sw_term.clone();
        let sw_prompt_clone = ctx.sw_prompt.clone();

        glib::idle_add_local(move || match rx.try_recv() {
            Ok(enrolled) => {
                let has_any = !enrolled.is_empty();
                println!(
                    "[ui] Populating finger grid; enrolled present: {} (count = {})",
                    has_any,
                    enrolled.len()
                );
                sw_login_clone.set_sensitive(has_any);
                sw_term_clone.set_sensitive(has_any);
                sw_prompt_clone.set_sensitive(has_any);

                while let Some(child) = fingers_flow_clone.first_child() {
                    fingers_flow_clone.remove(&child);
                }

                for key in fprintd::FINGERS {
                    let finger_string = key.to_string();

                    let btn_box = GtkBox::new(Orientation::Vertical, 4);

                    let icon = Image::from_icon_name("fingerprint");
                    icon.set_pixel_size(32);
                    icon.set_halign(Align::Center);

                    let label = Label::new(Some(&display_finger_name(&finger_string)));
                    label.set_halign(Align::Center);

                    btn_box.append(&icon);
                    btn_box.append(&label);

                    let event_btn = Button::new();
                    event_btn.set_child(Some(&btn_box));
                    if enrolled.contains(&finger_string) {
                        event_btn.add_css_class("finger-enrolled");
                    }

                    let stack_for_click = stack_clone.clone();
                    let selected_for_click = selected_clone.clone();
                    let finger_label_for_click = finger_label_clone.clone();
                    let action_label_for_click = action_label_clone.clone();
                    let finger_key_for_click = finger_string.clone();
                    event_btn.connect_clicked(move |_| {
                        println!("[ui] Finger selected: {}", finger_key_for_click);
                        selected_for_click.replace(Some(finger_key_for_click.clone()));
                        finger_label_for_click
                            .set_label(&display_finger_name(&finger_key_for_click));
                        action_label_for_click.set_label("");
                        stack_for_click.set_visible_child_name("finger");
                    });

                    fingers_flow_clone.append(&event_btn);
                }

                glib::ControlFlow::Break
            }
            Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    ctx.rt.spawn(async move {
        let enrolled = scan_enrolled_async().await;
        let _ = tx.send(enrolled);
    });
}

#[allow(clippy::too_many_arguments)]
fn populate_fingers_async(
    rt: Arc<Runtime>,
    fingers_flow: &FlowBox,
    stack: &Stack,
    selected_finger: Rc<RefCell<Option<String>>>,
    finger_label: &Label,
    action_label: &Label,
    sw_login: &Switch,
    sw_term: &Switch,
    sw_prompt: &Switch,
) {
    let ctx = UiCtx {
        rt,
        flow: fingers_flow.clone(),
        stack: stack.clone(),
        sw_login: sw_login.clone(),
        sw_term: sw_term.clone(),
        sw_prompt: sw_prompt.clone(),
        selected_finger,
        finger_label: finger_label.clone(),
        action_label: action_label.clone(),
    };
    populate_fingers_async_ctx(ctx);
}

fn main() {
    let app = Application::builder()
        .application_id("xyz.xerolinux.fp_gui")
        .build();

    app.connect_activate(|app| {

        let rt = Arc::new(
            TokioBuilder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime"),
        );


        gio::resources_register_include!("xyz.xerolinux.fp_gui.gresource")
            .expect("Failed to register gresources");

        if let Some(display) = gtk4::gdk::Display::default() {
            let theme = gtk4::IconTheme::for_display(&display);
            theme.add_resource_path("/xyz/xerolinux/fp_gui/icons");

            let css_provider = CssProvider::new();
            css_provider.load_from_resource("/xyz/xerolinux/fp_gui/css/style.css");
            gtk4::style_context_add_provider_for_display(
                &display,
                &css_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }


        let builder = Builder::from_resource("/xyz/xerolinux/fp_gui/ui/main.ui");


        let window: ApplicationWindow = builder
            .object("app_window")
            .expect("Failed to get app_window");
        window.set_application(Some(app));

        let stack: Stack = builder.object("stack").expect("Failed to get stack");


        let enroll_btn: Button = builder
            .object("enroll_btn")
            .expect("Failed to get enroll_btn");
        let back_btn: Button = builder.object("back_btn").expect("Failed to get back_btn");


        let sw_login: Switch = builder.object("sw_login").expect("Failed to get sw_login");
        let sw_term: Switch = builder.object("sw_term").expect("Failed to get sw_term");
        let sw_prompt: Switch = builder
            .object("sw_prompt")
            .expect("Failed to get sw_prompt");


        let (login_configured, sudo_configured, polkit_configured) =
            PamHelper::check_all_configurations();
        sw_login.set_active(login_configured);
        sw_term.set_active(sudo_configured);
        sw_prompt.set_active(polkit_configured);

        sw_login.set_sensitive(false);
        sw_term.set_sensitive(false);
        sw_prompt.set_sensitive(false);


        {

            let (tx, rx) = mpsc::channel::<bool>();
            {
                let sw_login_c = sw_login.clone();
                let sw_term_c = sw_term.clone();
                let sw_prompt_c = sw_prompt.clone();
                glib::idle_add_local(move || {
                    match rx.try_recv() {
                        Ok(has_any) => {
                            sw_login_c.set_sensitive(has_any);
                            sw_term_c.set_sensitive(has_any);
                            sw_prompt_c.set_sensitive(has_any);
                            glib::ControlFlow::Break
                        }
                        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                        Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
                    }
                });
            }
            let rt2 = rt.clone();
            rt2.spawn(async move {
                let has_any = !scan_enrolled_async().await.is_empty();
                let _ = tx.send(has_any);
            });
        }

        sw_login.set_tooltip_text(Some("Enroll fingerprint first."));
        sw_term.set_tooltip_text(Some("Enroll fingerprint first."));
        sw_prompt.set_tooltip_text(Some("Enroll fingerprint first."));


        {
            sw_login.connect_state_set(move |_switch, state| {
                let res = if state {
                    PamHelper::apply_login()
                } else {
                    PamHelper::remove_login()
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
                    PamHelper::apply_sudo()
                } else {
                    PamHelper::remove_sudo()
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
                    PamHelper::apply_polkit()
                } else {
                    PamHelper::remove_polkit()
                };
                if res.is_err() {
                    return gtk4::glib::Propagation::Stop;
                }
                gtk4::glib::Propagation::Proceed
            });
        }


        {
            let builder_c = builder.clone();
            let stack_nav = stack.clone();
            enroll_btn.connect_clicked(move |_| {
                if let Some(_fingers_flow) = builder_c.object::<FlowBox>("fingers_flow") {

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
        let button_cancel: Button = builder
            .object("button_cancel")
            .expect("Failed to get button_cancel");


        let selected_finger: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));


        {
            let rt = rt.clone();
            let flow = fingers_flow.clone();
            let sw_login_c = sw_login.clone();
            let sw_term_c = sw_term.clone();
            let sw_prompt_c = sw_prompt.clone();
            let action_label_c = action_label.clone();
            let selected = selected_finger.clone();
            let stack_c = stack.clone();
            let finger_label_c = finger_label.clone();
            button_add.connect_clicked(move |_| {
                if let Some(key) = selected.borrow().clone() {
                    println!("[ui] Add/Enroll clicked for finger: {}", key);
                    start_enrollment(
                        rt.clone(),
                        key,
                        action_label_c.clone(),
                        flow.clone(),
                        sw_login_c.clone(),
                        sw_term_c.clone(),
                        sw_prompt_c.clone(),
                        stack_c.clone(),
                        selected.clone(),
                        finger_label_c.clone(),
                    );
                }
            });
        }
        {
            let rt = rt.clone();
            let action_label_c = action_label.clone();
            let flow = fingers_flow.clone();
            let sw_login_c = sw_login.clone();
            let sw_term_c = sw_term.clone();
            let sw_prompt_c = sw_prompt.clone();
            let selected = selected_finger.clone();
            let stack_c = stack.clone();
            let finger_label_c2 = finger_label.clone();
            button_delete.connect_clicked(move |_| {
                if let Some(key) = selected.borrow().clone() {
                    println!("[ui] Delete clicked for finger: {}", key);
                    let finger_name = key.clone();
                    action_label_c.set_label("Deleting enrolled fingerprint...");


                    let (tx_done, rx_done) = mpsc::channel::<Result<(), String>>();
                    {
                        let action_label_c = action_label_c.clone();
                        let flow = flow.clone();
                        let sw_login_c = sw_login_c.clone();
                        let sw_term_c = sw_term_c.clone();
                        let sw_prompt_c = sw_prompt_c.clone();
                        let stack_c = stack_c.clone();
                        let selected_c = selected.clone();
                        let finger_label_c2 = finger_label_c2.clone();
                        let rt_ui = rt.clone();
                        glib::idle_add_local(move || {
                            match rx_done.try_recv() {
                                Ok(res) => {
                                    match res {
                                        Ok(()) => {
                                            action_label_c.set_use_markup(true);
                                            action_label_c.set_markup("<span color='orange'>Fingerprint deleted.</span>");
                                        }
                                        Err(msg) => {
                                            action_label_c.set_use_markup(true);
                                            action_label_c.set_markup(&msg);
                                        }
                                    }

                                    let rt_c = rt_ui.clone();
                                    populate_fingers_async(
                                        rt_c.clone(),
                                        &flow,
                                        &stack_c,
                                        selected_c.clone(),
                                        &finger_label_c2,
                                        &action_label_c,
                                        &sw_login_c,
                                        &sw_term_c,
                                        &sw_prompt_c,
                                    );

                                    glib::ControlFlow::Break
                                }
                                Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                                Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
                            }
                        });
                    }


                    rt.spawn({
                        let finger_name = finger_name.clone();
                        async move {
                            println!("[fprintd] Connecting to system bus for delete...");
                            match fprintd::Client::system().await {
                                Ok(client) => {
                                    println!("[fprintd] Locating device for delete...");
                                    match fprintd::first_device(&client).await {
                                        Ok(Some(dev)) => {
                                            println!("[fprintd] Claiming device (current user)...");
                                            if let Err(e) = dev.claim("").await {
                                                eprintln!("[fprintd][warn] Claim failed: {e}");
                                            }
                                            println!("[fprintd] Deleting enrolled finger '{}'", finger_name);
                                            if let Err(e) = dev.delete_enrolled_finger(&finger_name).await {
                                                eprintln!("[fprintd][error] DeleteEnrolledFinger failed: {e}");
                                                let _ = tx_done.send(Err(format!("<span color='red'><b>Delete failed</b>: {e}</span>")));
                                                return;
                                            }
                                            println!("[fprintd] Releasing device after delete");
                                            if let Err(e) = dev.release().await {
                                                eprintln!("[fprintd][warn] Release failed after delete: {e}");
                                            }
                                            println!("[fprintd] Delete flow completed successfully");
                                            let _ = tx_done.send(Ok(()));
                                        }
                                        Ok(None) => {
                                            eprintln!("[fprintd][warn] No fingerprint devices available.");
                                            let _ = tx_done.send(Err("<span color='orange'>No fingerprint devices available.</span>".to_string()));
                                        }
                                        Err(e) => {
                                            eprintln!("[fprintd][error] Failed to enumerate devices: {e}");
                                            let _ = tx_done.send(Err(format!("<span color='red'><b>Failed</b> to enumerate devices: {e}</span>")));
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[fprintd][error] System bus connect failed: {e}");
                                    let _ = tx_done.send(Err(format!("<span color='red'><b>Bus connect failed</b>: {e}</span>")));
                                }
                            }
                        }
                    });
                }
            });
        }
        {
            let stack_nav = stack.clone();
            button_cancel.connect_clicked(move |_| {
                println!("[ui] Cancel clicked; navigating back to enroll page");
                stack_nav.set_visible_child_name("enroll");
            });
        }


        populate_fingers_async(
            rt.clone(),
            &fingers_flow,
            &stack,
            selected_finger.clone(),
            &finger_label,
            &action_label,
            &sw_login,
            &sw_term,
            &sw_prompt,
        );

        stack.set_visible_child_name("main");

        window.show();
    });

    app.run();
}
