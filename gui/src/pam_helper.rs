#![allow(dead_code)]

use std::io;
use std::process::Command;

/// Utility for managing PAM fingerprint configurations via the helper tool
pub struct PamHelper;

/// PAM service identifiers
const LOGIN_SERVICE: &str = "login";
const SUDO_SERVICE: &str = "sudo";
const POLKIT_SERVICE: &str = "polkit-1";

/// Path to the helper binary (as installed by PKGBUILD)
const HELPER_PATH: &str = "/opt/fingerprint_gui/fingerprint-gui-helper";

impl PamHelper {
    /// Check if fingerprint configuration is applied for a specific service
    pub fn is_configured(service: &str) -> bool {
        match Command::new(HELPER_PATH).arg("check").arg(service).status() {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    /// Check all configurations in a single call (more efficient)
    /// Returns (login_configured, sudo_configured, polkit_configured)
    fn check_all_configurations_efficient() -> (bool, bool, bool) {
        match Command::new(HELPER_PATH).arg("check-all").output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut login = false;
                let mut sudo = false;
                let mut polkit = false;

                for line in stdout.lines() {
                    if let Some(path) = line.strip_prefix("applied: ") {
                        match path {
                            "/etc/pam.d/login" => login = true,
                            "/etc/pam.d/sudo" => sudo = true,
                            "/etc/pam.d/polkit-1" => polkit = true,
                            _ => {}
                        }
                    }
                }

                (login, sudo, polkit)
            }
            Err(_) => {
                // Fallback to individual checks
                Self::check_all_configurations_fallback()
            }
        }
    }

    /// Fallback method using individual checks
    fn check_all_configurations_fallback() -> (bool, bool, bool) {
        let login = Self::is_configured(LOGIN_SERVICE);
        let sudo = Self::is_configured(SUDO_SERVICE);
        let polkit = Self::is_configured(POLKIT_SERVICE);
        (login, sudo, polkit)
    }

    /// Apply fingerprint configuration for a specific service using pkexec
    pub fn apply_configuration(service: &str) -> io::Result<()> {
        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("apply")
            .arg(service)
            .output()
            .map_err(|e| io::Error::other(format!("Failed to execute pkexec: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        Ok(())
    }

    /// Remove fingerprint configuration for a specific service using pkexec
    pub fn remove_configuration(service: &str) -> io::Result<()> {
        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("remove")
            .arg(service)
            .output()
            .map_err(|e| io::Error::other(format!("Failed to execute pkexec: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        Ok(())
    }

    /// Apply fingerprint configuration for login service
    pub fn apply_login() -> io::Result<()> {
        Self::apply_configuration(LOGIN_SERVICE)
    }

    /// Remove fingerprint configuration for login service
    pub fn remove_login() -> io::Result<()> {
        Self::remove_configuration(LOGIN_SERVICE)
    }

    /// Apply fingerprint configuration for sudo service
    pub fn apply_sudo() -> io::Result<()> {
        Self::apply_configuration(SUDO_SERVICE)
    }

    /// Remove fingerprint configuration for sudo service
    pub fn remove_sudo() -> io::Result<()> {
        Self::remove_configuration(SUDO_SERVICE)
    }

    /// Apply fingerprint configuration for polkit service
    pub fn apply_polkit() -> io::Result<()> {
        Self::apply_configuration(POLKIT_SERVICE)
    }

    /// Remove fingerprint configuration for polkit service
    pub fn remove_polkit() -> io::Result<()> {
        Self::remove_configuration(POLKIT_SERVICE)
    }

    /// Apply configuration to all services using batch operation
    pub fn apply_all_configurations() -> io::Result<()> {
        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("apply-all")
            .output()
            .map_err(|e| io::Error::other(format!("Failed to execute pkexec: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        Ok(())
    }

    /// Remove configuration from all services using batch operation
    pub fn remove_all_configurations() -> io::Result<()> {
        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("remove-all")
            .output()
            .map_err(|e| io::Error::other(format!("Failed to execute pkexec: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        Ok(())
    }

    /// Check configuration status for all services (uses efficient batch operation)
    /// Returns (login_configured, sudo_configured, polkit_configured)
    pub fn check_all_configurations() -> (bool, bool, bool) {
        Self::check_all_configurations_efficient()
    }

    /// Check if login service is configured
    pub fn is_login_configured() -> bool {
        Self::is_configured(LOGIN_SERVICE)
    }

    /// Check if sudo service is configured
    pub fn is_sudo_configured() -> bool {
        Self::is_configured(SUDO_SERVICE)
    }

    /// Check if polkit service is configured
    pub fn is_polkit_configured() -> bool {
        Self::is_configured(POLKIT_SERVICE)
    }
}
