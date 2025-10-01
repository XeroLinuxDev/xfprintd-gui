use crate::util;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Builder, Button, Label};
use log::{error, info};

/// Check if current distribution is supported and show error dialog if not.
pub fn check_distribution_support(main_window: &ApplicationWindow) {
    info!("Checking Linux distribution compatibility");
    if !util::is_supported_distribution() {
        error!("Unsupported distribution detected");
        let distro_name = util::get_distribution_name().unwrap_or_else(|| "Unknown".to_string());
        error!("Current distribution: {}", distro_name);
        error!("This application is designed specifically for XeroLinux");
        error!("Visit https://xerolinux.xyz/ to learn more about XeroLinux");

        // Load error dialog from UI file
        let builder = Builder::from_resource("/xyz/xerolinux/xfprintd_gui/ui/error_dialog.ui");

        let error_window: gtk4::Window = builder
            .object("error_window")
            .expect("Failed to get error_window");

        let distro_label: Label = builder
            .object("distro_label")
            .expect("Failed to get distro_label");

        let ok_button: Button = builder
            .object("ok_button")
            .expect("Failed to get ok_button");

        distro_label.set_label(&format!("Current distribution: {}", distro_name));
        error_window.set_transient_for(Some(main_window));
        let main_window_clone = main_window.clone();
        ok_button.connect_clicked(move |_| {
            main_window_clone.close();
            std::process::exit(1);
        });

        error_window.show();
    } else {
        info!("XeroLinux detected - proceeding with application startup");
    }
}

/// Check fprintd service status.
pub fn check_fprintd_service() {
    match std::process::Command::new("systemctl")
        .args(["is-active", "fprintd"])
        .output()
    {
        Ok(output) => {
            let status_output = String::from_utf8_lossy(&output.stdout);
            let status = status_output.trim();
            if status == "active" {
                info!("fprintd service is running");
            } else {
                log::warn!("fprintd service status: {}", status);
                log::warn!("You may need to start fprintd: sudo systemctl start fprintd");
            }
        }
        Err(e) => {
            log::warn!("Cannot check fprintd service status: {}", e);
        }
    }
}

/// Check for helper tool availability.
pub fn check_helper_tool() {
    let username = std::env::var("USER").unwrap_or_default();
    info!("Running as user: '{}'", username);

    let helper_path = "/opt/xfprintd-gui/xfprintd-gui-helper";
    if std::path::Path::new(helper_path).exists() {
        info!("Helper tool found at: {}", helper_path);
    } else {
        log::warn!("Helper tool not found at: {}", helper_path);
        log::warn!("PAM configuration features may not work");
    }
}

/// Check for pkexec availability.
pub fn check_pkexec_availability() {
    match std::process::Command::new("which").arg("pkexec").output() {
        Ok(output) => {
            if output.status.success() {
                info!("pkexec is available for privilege escalation");
            } else {
                log::warn!("pkexec not found - PAM configuration will not work");
            }
        }
        Err(_) => {
            log::warn!("Cannot check for pkexec availability");
        }
    }
}
