//! Generic PAM switch handler functionality.

use crate::pam_helper::PamHelper;
#[allow(unused_imports)]
use gtk4::prelude::*;
use gtk4::{glib, Switch};
use log::{error, info};

/// PAM service configuration for switch handlers.
#[derive(Clone)]
pub struct PamService {
    pub name: &'static str,
    pub path: &'static str,
}

/// Available PAM services that can be configured.
pub mod services {
    use super::PamService;
    use crate::pam_helper::{get_login_path, POLKIT_PATH, SUDO_PATH};

    pub fn login() -> PamService {
        PamService {
            name: "login",
            path: get_login_path(),
        }
    }

    pub const SUDO: PamService = PamService {
        name: "sudo",
        path: SUDO_PATH,
    };

    pub const POLKIT: PamService = PamService {
        name: "polkit",
        path: POLKIT_PATH,
    };
}

/// Set up a generic PAM switch handler for any service.
pub fn setup_pam_switch(switch: &Switch, service: PamService) {
    let service_name = service.name.to_string();
    let service_path = service.path;

    switch.connect_state_set(move |_switch, state| {
        handle_pam_toggle(state, &service_name, service_path)
    });
}

/// Handle PAM toggle for any service (generic implementation).
fn handle_pam_toggle(state: bool, service_name: &str, service_path: &str) -> glib::Propagation {
    if state {
        info!(
            "User enabled {} fingerprint authentication switch",
            service_name
        );
    } else {
        info!(
            "User disabled {} fingerprint authentication switch",
            service_name
        );
    }

    let result = if state {
        PamHelper::apply_configuration(service_path)
    } else {
        PamHelper::remove_configuration(service_path)
    };

    match result {
        Ok(()) => {
            if state {
                info!(
                    "Successfully enabled fingerprint authentication for {}",
                    service_name
                );
            } else {
                info!(
                    "Successfully disabled fingerprint authentication for {}",
                    service_name
                );
            }
            glib::Propagation::Proceed
        }
        Err(e) => {
            error!(
                "Failed to {} fingerprint authentication for {}: {}",
                if state { "enable" } else { "disable" },
                service_name,
                e
            );
            glib::Propagation::Stop
        }
    }
}
