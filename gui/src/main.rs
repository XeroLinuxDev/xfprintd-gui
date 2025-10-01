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

use log::{error, info, warn};
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

    info!("🔌 Attempting to connect to fprintd system bus for fingerprint scan");
    let client = match fprintd::Client::system().await {
        Ok(c) => {
            info!("✅ Successfully connected to fprintd system bus");
            c
        }
        Err(e) => {
            error!("❌ Failed to connect to fprintd system bus: {e}");
            error!("   This usually means fprintd service is not running or not installed");
            return s;
        }
    };

    info!("🔍 Searching for available fingerprint devices");
    let dev = match fprintd::first_device(&client).await {
        Ok(Some(d)) => {
            info!("✅ Found fingerprint device, proceeding with enrollment scan");
            d
        }
        Ok(None) => {
            warn!("⚠️  No fingerprint devices detected on this system");
            warn!("   Please ensure your fingerprint reader is connected and recognized by the system");
            return s;
        }
        Err(e) => {
            error!("❌ Failed to enumerate fingerprint devices: {e}");
            error!("   Check if fprintd service has proper permissions");
            return s;
        }
    };

    let username = std::env::var("USER").unwrap_or_default();
    info!("👤 Scanning enrolled fingerprints for user: '{}'", username);

    info!("🔒 Claiming fingerprint device for exclusive access");
    if let Err(e) = dev.claim(&username).await {
        warn!("⚠️  Failed to claim device for user '{}': {e}", username);
        warn!("   Device might be in use by another process");
    } else {
        info!("✅ Successfully claimed fingerprint device");
    }

    info!("📋 Retrieving list of enrolled fingerprints");
    match dev.list_enrolled_fingers(&username).await {
        Ok(list) => {
            if list.is_empty() {
                info!("📭 No enrolled fingerprints found for user '{}'", username);
                info!("   User will need to enroll fingerprints before using authentication");
            } else {
                info!(
                    "📄 Found {} enrolled fingerprint(s) for user '{}':",
                    list.len(),
                    username
                );
                for (i, f) in list.iter().enumerate() {
                    info!("   {}. {}", i + 1, f);
                    s.insert(f.clone());
                }
            }
        }
        Err(e) => {
            error!("❌ Failed to retrieve enrolled fingerprints: {e}");
            error!("   This might indicate permission issues or device problems");
        }
    }

    info!("🔓 Releasing fingerprint device");
    if let Err(e) = dev.release().await {
        warn!("⚠️  Failed to release device: {e}");
        warn!("   Device might remain locked until fprintd service restart");
    } else {
        info!("✅ Successfully released fingerprint device");
    }

    info!(
        "📊 Fingerprint scan completed. Found {} enrolled fingerprint(s)",
        s.len()
    );
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
        "<b>🔍 Place your finger on the scanner…</b>".to_string(),
    ));

    ctx.rt.spawn(async move {
        info!("🚀 Starting fingerprint enrollment process for finger: {}", finger_key);
        info!("🔌 Connecting to fprintd system bus for enrollment");
        let client = match fprintd::Client::system().await {
            Ok(c) => {
                info!("✅ Successfully connected to fprintd for enrollment");
                c
            },
            Err(e) => {
                error!("❌ Failed to connect to fprintd system bus during enrollment: {e}");
                let _ = tx.send(UiEvent::SetText(format!(
                    "Failed to connect to system bus: {e}"
                )));
                return;
            }
        };

        info!("🔍 Looking for available fingerprint device for enrollment");
        let dev = match fprintd::first_device(&client).await {
            Ok(Some(d)) => {
                info!("✅ Found fingerprint device, ready for enrollment");
                d
            },
            Ok(None) => {
                warn!("⚠️  No fingerprint devices available for enrollment");
                warn!("   Please connect a fingerprint reader and try again");
                let _ = tx.send(UiEvent::SetText("<span color='orange'>No fingerprint devices available.</span>".to_string()));
                return;
            }
            Err(e) => {
                error!("❌ Failed to enumerate devices during enrollment: {e}");
                let _ = tx.send(UiEvent::SetText(format!("Failed to enumerate device: {e}")));
                return;
            }
        };

        // Claim device
        info!("🔒 Claiming fingerprint device for enrollment (current user)");
        if let Err(e) = dev.claim("").await {
            error!("❌ Failed to claim device for enrollment: {e}");
            let _ = tx.send(UiEvent::SetText(format!("Could not claim device: {e}")));
        } else {
            info!("✅ Successfully claimed device for enrollment");
        }

        // Attach listener first (like fingwit)
        let dev_for_listener = dev.clone();
        let dev_for_stop = dev_for_listener.clone();
        let tx_status = tx.clone();
        tokio::spawn(async move {
            info!("👂 Setting up enrollment status listener for real-time feedback");
            let _ = dev_for_listener
                .listen_enroll_status(move |evt| {
                    info!("📡 Enrollment status update: result='{}', done={}", evt.result, evt.done);
                    let text = match evt.result.as_str() {
                        "enroll-completed" => {
                            info!("🎉 Fingerprint enrollment completed successfully!");
                            let _ = tx_status.send(UiEvent::EnrollCompleted);
                            "<span color='green'><b>🎉 Well done!</b> Your fingerprint was saved successfully.</span>".to_string()
                        }
                        "enroll-stage-passed" => {
                            info!("✅ Enrollment stage passed, continuing...");
                            "<span color='blue'><b>✅ Good scan!</b> Place your finger again...</span>".to_string()
                        },
                        "enroll-remove-and-retry" => {
                            warn!("⚠️  Enrollment stage failed, user needs to retry");
                            "<span color='orange'><b>⚠️  Remove finger</b> and try again...</span>".to_string()
                        },
                        "enroll-data-full" => {
                            info!("📊 Enrollment data buffer full, processing...");
                            "<span color='blue'><b>📊 Processing...</b> Keep finger steady</span>".to_string()
                        },
                        "enroll-swipe-too-short" => {
                            warn!("⚠️  Finger swipe too short");
                            "<span color='orange'><b>👆 Swipe too short</b> - try a longer swipe</span>".to_string()
                        },
                        "enroll-finger-not-centered" => {
                            warn!("⚠️  Finger not centered properly");
                            "<span color='orange'><b>🎯 Center your finger</b> and try again</span>".to_string()
                        },
                        "enroll-duplicate" => {
                            warn!("⚠️  Duplicate fingerprint detected");
                            "<span color='orange'><b>🔄 Already enrolled!</b> This fingerprint is already saved, use a different finger.</span>".to_string()
                        }
                        "enroll-failed" => {
                            error!("❌ Fingerprint enrollment failed");
                            "<span color='red'><b>❌ Enrollment failed!</b> Sorry, your fingerprint could not be saved.</span>".to_string()
                        }
                        other => {
                            info!("📊 Enrollment status: '{}' (done={})", other, evt.done);
                            format!("<span color='gray'><b>📊 Status:</b> {} ({})</span>", other, if evt.done { "completed" } else { "in progress" })
                        },
                    };
                    let _ = tx_status.send(UiEvent::SetText(text));
                    if evt.done {
                        info!("🏁 Enrollment process finished, cleaning up device");
                        let dev_stop = dev_for_stop.clone();
                        tokio::spawn(async move {
                            if let Err(e) = dev_stop.enroll_stop().await {
                                warn!("⚠️  Failed to stop enrollment: {e}");
                            }
                            if let Err(e) = dev_stop.release().await {
                                warn!("⚠️  Failed to release device after enrollment: {e}");
                            } else {
                                info!("✅ Successfully cleaned up after enrollment");
                            }
                        });
                    }
                })
                .await;
        });

        // Start enrollment
        info!("▶️  Starting enrollment process for finger: '{}'", finger_key);
        if let Err(e) = dev.enroll_start(&finger_key).await {
            error!("❌ Failed to start enrollment for '{}': {e}", finger_key);
            let _ = tx.send(UiEvent::SetText(format!("Enrollment error: {e}")));
            let _ = dev.enroll_stop().await;
            let _ = dev.release().await;
        } else {
            info!("✅ Enrollment started successfully, waiting for finger scans...");
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
                info!(
                    "🔄 Updating UI: enrolled fingerprints found: {} (count = {})",
                    has_any,
                    enrolled.len()
                );

                if has_any {
                    info!("🔓 Enabling PAM authentication switches (fingerprints available)");
                    info!("   - Login switch: enabled");
                    info!("   - Sudo switch: enabled");
                    info!("   - Polkit switch: enabled");
                } else {
                    info!("🔒 Disabling PAM authentication switches (no fingerprints enrolled)");
                    info!("   User must enroll fingerprints before enabling authentication");
                }

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
                        info!(
                            "👆 User selected finger: '{}' for action",
                            finger_key_for_click
                        );
                        info!("🔄 Switching to finger action view");
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
    // Initialize logger
    simple_logger::SimpleLogger::new().init().unwrap();

    info!(
        "🚀 Starting XFPrintD GUI Application v{}",
        env!("CARGO_PKG_VERSION")
    );
    info!("📱 Application ID: xyz.xerolinux.xfprintd_gui");

    // System environment checks
    info!("🔍 Performing system environment checks");

    // Check if fprintd service is available
    match std::process::Command::new("systemctl")
        .args(["is-active", "fprintd"])
        .output()
    {
        Ok(output) => {
            let status_output = String::from_utf8_lossy(&output.stdout);
            let status = status_output.trim();
            if status == "active" {
                info!("✅ fprintd service is running");
            } else {
                warn!("⚠️  fprintd service status: {}", status);
                warn!("   You may need to start fprintd: sudo systemctl start fprintd");
            }
        }
        Err(e) => {
            warn!("⚠️  Cannot check fprintd service status: {}", e);
        }
    }

    // Check if current user is in proper groups
    let username = std::env::var("USER").unwrap_or_default();
    info!("👤 Running as user: '{}'", username);

    // Check for helper tool
    let helper_path = "/opt/xfprintd-gui/xfprintd-gui-helper";
    if std::path::Path::new(helper_path).exists() {
        info!("✅ Helper tool found at: {}", helper_path);
    } else {
        warn!("⚠️  Helper tool not found at: {}", helper_path);
        warn!("   PAM configuration features may not work");
    }

    // Check for pkexec availability
    match std::process::Command::new("which").arg("pkexec").output() {
        Ok(output) => {
            if output.status.success() {
                info!("✅ pkexec is available for privilege escalation");
            } else {
                warn!("⚠️  pkexec not found - PAM configuration will not work");
            }
        }
        Err(_) => {
            warn!("⚠️  Cannot check for pkexec availability");
        }
    }

    let app = Application::builder()
        .application_id("xyz.xerolinux.xfprintd_gui")
        .build();

    app.connect_activate(|app| {
        info!("🔧 Initializing application components");

        let rt = Arc::new(
            TokioBuilder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime"),
        );
        info!("⚡ Tokio async runtime initialized");


        gio::resources_register_include!("xyz.xerolinux.xfprintd_gui.gresource")
            .expect("Failed to register gresources");

        if let Some(display) = gtk4::gdk::Display::default() {
            info!("🎨 Setting up UI theme and styling");
            let theme = gtk4::IconTheme::for_display(&display);
            theme.add_resource_path("/xyz/xerolinux/xfprintd_gui/icons");

            let css_provider = CssProvider::new();
            css_provider.load_from_resource("/xyz/xerolinux/xfprintd_gui/css/style.css");
            gtk4::style_context_add_provider_for_display(
                &display,
                &css_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            info!("✅ UI theme and styling loaded successfully");
        } else {
            warn!("⚠️  No default display found - UI theming may not work properly");
        }


        let builder = Builder::from_resource("/xyz/xerolinux/xfprintd_gui/ui/main.ui");


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


        info!("🔐 Checking current PAM authentication configurations");
        let (login_configured, sudo_configured, polkit_configured) =
            PamHelper::check_all_configurations();

        info!("📋 PAM Configuration Status:");
        info!("   - Login authentication: {}", if login_configured { "✅ ENABLED" } else { "❌ DISABLED" });
        info!("   - Sudo authentication: {}", if sudo_configured { "✅ ENABLED" } else { "❌ DISABLED" });
        info!("   - Polkit authentication: {}", if polkit_configured { "✅ ENABLED" } else { "❌ DISABLED" });

        sw_login.set_active(login_configured);
        sw_term.set_active(sudo_configured);
        sw_prompt.set_active(polkit_configured);

        info!("🔒 Temporarily disabling PAM switches until fingerprint enrollment check");
        sw_login.set_sensitive(false);
        sw_term.set_sensitive(false);
        sw_prompt.set_sensitive(false);


        {
            info!("🔍 Starting background fingerprint enrollment check");

            let (tx, rx) = mpsc::channel::<bool>();
            {
                let sw_login_c = sw_login.clone();
                let sw_term_c = sw_term.clone();
                let sw_prompt_c = sw_prompt.clone();
                glib::idle_add_local(move || {
                    match rx.try_recv() {
                        Ok(has_any) => {
                            if has_any {
                                info!("✅ Enrollment check complete: fingerprints found, enabling switches");
                            } else {
                                info!("ℹ️  Enrollment check complete: no fingerprints found, switches remain disabled");
                            }
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
                info!("🔍 Starting system fingerprint device detection and enrollment scan");
                let enrolled = scan_enrolled_async().await;
                let has_any = !enrolled.is_empty();

                if has_any {
                    info!("✅ System ready: {} enrolled fingerprint(s) detected", enrolled.len());
                    info!("🔓 PAM authentication switches will be enabled");
                } else {
                    info!("ℹ️  No enrolled fingerprints found on initial scan");
                    info!("🔒 PAM authentication switches will remain disabled until enrollment");
                    info!("💡 Click 'Enroll' to add your first fingerprint");
                }

                let _ = tx.send(has_any);
            });
        }

        {
            sw_login.connect_state_set(move |_switch, state| {
                if state {
                    info!("🔛 User enabled login fingerprint authentication switch");
                } else {
                    info!("🔲 User disabled login fingerprint authentication switch");
                }

                let res = if state {
                    PamHelper::apply_login()
                } else {
                    PamHelper::remove_login()
                };

                match res {
                    Ok(()) => {
                        if state {
                            info!("✅ Successfully enabled fingerprint authentication for login");
                        } else {
                            info!("✅ Successfully disabled fingerprint authentication for login");
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed to {} fingerprint authentication for login: {}",
                               if state { "enable" } else { "disable" }, e);
                        return gtk4::glib::Propagation::Stop;
                    }
                }
                gtk4::glib::Propagation::Proceed
            });
        }
        {
            sw_term.connect_state_set(move |_switch, state| {
                if state {
                    info!("🔛 User enabled sudo fingerprint authentication switch");
                } else {
                    info!("🔲 User disabled sudo fingerprint authentication switch");
                }

                let res = if state {
                    PamHelper::apply_sudo()
                } else {
                    PamHelper::remove_sudo()
                };

                match res {
                    Ok(()) => {
                        if state {
                            info!("✅ Successfully enabled fingerprint authentication for sudo");
                        } else {
                            info!("✅ Successfully disabled fingerprint authentication for sudo");
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed to {} fingerprint authentication for sudo: {}",
                               if state { "enable" } else { "disable" }, e);
                        return gtk4::glib::Propagation::Stop;
                    }
                }
                gtk4::glib::Propagation::Proceed
            });
        }
        {
            sw_prompt.connect_state_set(move |_switch, state| {
                if state {
                    info!("🔛 User enabled polkit fingerprint authentication switch");
                } else {
                    info!("🔲 User disabled polkit fingerprint authentication switch");
                }

                let res = if state {
                    PamHelper::apply_polkit()
                } else {
                    PamHelper::remove_polkit()
                };

                match res {
                    Ok(()) => {
                        if state {
                            info!("✅ Successfully enabled fingerprint authentication for polkit");
                        } else {
                            info!("✅ Successfully disabled fingerprint authentication for polkit");
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed to {} fingerprint authentication for polkit: {}",
                               if state { "enable" } else { "disable" }, e);
                        return gtk4::glib::Propagation::Stop;
                    }
                }
                gtk4::glib::Propagation::Proceed
            });
        }


        {
            let builder_c = builder.clone();
            let stack_nav = stack.clone();
            enroll_btn.connect_clicked(move |_| {
                info!("➕ User clicked 'Enroll' button - navigating to enrollment page");
                if let Some(_fingers_flow) = builder_c.object::<FlowBox>("fingers_flow") {

                }
                stack_nav.set_visible_child_name("enroll");
            });
        }
        {
            let stack_nav = stack.clone();
            back_btn.connect_clicked(move |_| {
                info!("⬅️  User clicked 'Back' button - returning to main page");
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
                    info!("➕ User clicked 'Add' button for finger: '{}'", key);
                    info!("🚀 Initiating fingerprint enrollment process");
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
                    info!("🗑️  User clicked 'Delete' button for finger: '{}'", key);
                    info!("🔄 Starting fingerprint deletion process");
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
                            info!("🔌 Connecting to fprintd system bus for deletion of '{}'", finger_name);
                            match fprintd::Client::system().await {
                                Ok(client) => {
                                    info!("✅ Successfully connected to fprintd for deletion");
                                    info!("🔍 Searching for fingerprint device to perform deletion");
                                    match fprintd::first_device(&client).await {
                                        Ok(Some(dev)) => {
                                            info!("✅ Found fingerprint device for deletion");
                                            info!("🔒 Claiming device for deletion operation");
                                            if let Err(e) = dev.claim("").await {
                                                warn!("⚠️  Failed to claim device for deletion: {e}");
                                            } else {
                                                info!("✅ Successfully claimed device for deletion");
                                            }
                                            info!("🗑️  Executing deletion of enrolled finger: '{}'", finger_name);
                                            if let Err(e) = dev.delete_enrolled_finger(&finger_name).await {
                                                error!("❌ Failed to delete enrolled finger '{}': {e}", finger_name);
                                                let _ = tx_done.send(Err(format!("<span color='red'><b>Delete failed</b>: {e}</span>")));
                                                return;
                                            }
                                            info!("✅ Successfully deleted fingerprint '{}'", finger_name);
                                            info!("🔓 Releasing device after deletion");
                                            if let Err(e) = dev.release().await {
                                                warn!("⚠️  Failed to release device after deletion: {e}");
                                            } else {
                                                info!("✅ Successfully released device after deletion");
                                            }
                                            info!("🎉 Fingerprint deletion completed successfully");
                                            let _ = tx_done.send(Ok(()));
                                        }
                                        Ok(None) => {
                                            warn!("⚠️  No fingerprint devices available for deletion");
                                            warn!("   Please ensure fingerprint reader is connected");
                                            let _ = tx_done.send(Err("<span color='orange'>No fingerprint devices available.</span>".to_string()));
                                        }
                                        Err(e) => {
                                            error!("❌ Failed to enumerate devices for deletion: {e}");
                                            let _ = tx_done.send(Err(format!("<span color='red'><b>Failed</b> to enumerate devices: {e}</span>")));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("❌ Failed to connect to system bus for deletion: {e}");
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
                info!("❌ User clicked 'Cancel' button - returning to enrollment page");
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

        info!("🔄 Setting initial view to main page");
        stack.set_visible_child_name("main");

        info!("🪟 Displaying main application window");
        window.show();

        info!("✅ XFPrintD GUI application startup complete");
    });

    app.run();
}
