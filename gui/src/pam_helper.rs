#![allow(dead_code)]

use log::{debug, error, info, warn};
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
        info!("🔍 Checking PAM configuration for service: '{}'", service);
        match Command::new(HELPER_PATH).arg("check").arg(service).status() {
            Ok(status) => {
                let configured = status.success();
                if configured {
                    info!(
                        "✅ PAM service '{}' has fingerprint authentication enabled",
                        service
                    );
                } else {
                    info!(
                        "❌ PAM service '{}' does not have fingerprint authentication",
                        service
                    );
                }
                configured
            }
            Err(e) => {
                error!(
                    "❌ Failed to check PAM configuration for '{}': {}",
                    service, e
                );
                error!(
                    "   Helper tool might not be installed or accessible at: {}",
                    HELPER_PATH
                );
                false
            }
        }
    }

    /// Check all configurations in a single call (more efficient)
    /// Returns (login_configured, sudo_configured, polkit_configured)
    fn check_all_configurations_efficient() -> (bool, bool, bool) {
        info!("🔍 Performing batch check of all PAM configurations");
        match Command::new(HELPER_PATH).arg("check-all").output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut login = false;
                let mut sudo = false;
                let mut polkit = false;

                debug!("📋 PAM helper output:\n{}", stdout);

                for line in stdout.lines() {
                    if let Some(path) = line.strip_prefix("applied: ") {
                        match path {
                            "/etc/pam.d/login" => {
                                login = true;
                                info!("✅ Login PAM configuration: ENABLED");
                            }
                            "/etc/pam.d/sudo" => {
                                sudo = true;
                                info!("✅ Sudo PAM configuration: ENABLED");
                            }
                            "/etc/pam.d/polkit-1" => {
                                polkit = true;
                                info!("✅ Polkit PAM configuration: ENABLED");
                            }
                            _ => {
                                debug!("🔍 Unknown PAM configuration found: {}", path);
                            }
                        }
                    } else if let Some(path) = line.strip_prefix("not-applied: ") {
                        match path {
                            "/etc/pam.d/login" => info!("❌ Login PAM configuration: DISABLED"),
                            "/etc/pam.d/sudo" => info!("❌ Sudo PAM configuration: DISABLED"),
                            "/etc/pam.d/polkit-1" => info!("❌ Polkit PAM configuration: DISABLED"),
                            _ => debug!("🔍 Unknown PAM path not configured: {}", path),
                        }
                    }
                }

                let exit_code = output.status.code().unwrap_or(-1);
                info!("📊 PAM batch check completed (exit code: {})", exit_code);
                (login, sudo, polkit)
            }
            Err(e) => {
                warn!("⚠️  Batch PAM check failed: {}", e);
                warn!("   Falling back to individual service checks");
                // Fallback to individual checks
                Self::check_all_configurations_fallback()
            }
        }
    }

    /// Fallback method using individual checks
    fn check_all_configurations_fallback() -> (bool, bool, bool) {
        warn!("🔄 Using fallback method: checking PAM configurations individually");
        let login = Self::is_configured(LOGIN_SERVICE);
        let sudo = Self::is_configured(SUDO_SERVICE);
        let polkit = Self::is_configured(POLKIT_SERVICE);
        info!(
            "📊 Individual PAM check results: login={}, sudo={}, polkit={}",
            login, sudo, polkit
        );
        (login, sudo, polkit)
    }

    /// Apply fingerprint configuration for a specific service using pkexec
    pub fn apply_configuration(service: &str) -> io::Result<()> {
        info!(
            "🔐 Applying fingerprint PAM configuration for service: '{}'",
            service
        );
        info!("🔑 Requesting root privileges via pkexec");

        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("apply")
            .arg(service)
            .output()
            .map_err(|e| {
                error!("❌ Failed to execute pkexec for PAM configuration: {}", e);
                error!("   Make sure polkit is installed and configured properly");
                io::Error::other(format!("Failed to execute pkexec: {}", e))
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            error!(
                "❌ PAM configuration failed for service '{}': {}",
                service, err
            );
            if !stdout.is_empty() {
                debug!("📋 Helper stdout: {}", stdout);
            }
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        info!(
            "✅ Successfully applied fingerprint PAM configuration for '{}'",
            service
        );
        if !stdout.is_empty() {
            info!("📋 Helper response: {}", stdout.trim());
        }
        Ok(())
    }

    /// Remove fingerprint configuration for a specific service using pkexec
    pub fn remove_configuration(service: &str) -> io::Result<()> {
        info!(
            "🗑️  Removing fingerprint PAM configuration for service: '{}'",
            service
        );
        info!("🔑 Requesting root privileges via pkexec");

        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("remove")
            .arg(service)
            .output()
            .map_err(|e| {
                error!("❌ Failed to execute pkexec for PAM removal: {}", e);
                error!("   Make sure polkit is installed and configured properly");
                io::Error::other(format!("Failed to execute pkexec: {}", e))
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            error!(
                "❌ PAM configuration removal failed for service '{}': {}",
                service, err
            );
            if !stdout.is_empty() {
                debug!("📋 Helper stdout: {}", stdout);
            }
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        info!(
            "✅ Successfully removed fingerprint PAM configuration for '{}'",
            service
        );
        if !stdout.is_empty() {
            info!("📋 Helper response: {}", stdout.trim());
        }
        Ok(())
    }

    /// Apply fingerprint configuration for login service
    pub fn apply_login() -> io::Result<()> {
        info!("🖥️  Configuring fingerprint authentication for login screen");
        Self::apply_configuration(LOGIN_SERVICE)
    }

    /// Remove fingerprint configuration for login service
    pub fn remove_login() -> io::Result<()> {
        info!("🖥️  Removing fingerprint authentication from login screen");
        Self::remove_configuration(LOGIN_SERVICE)
    }

    /// Apply fingerprint configuration for sudo service
    pub fn apply_sudo() -> io::Result<()> {
        info!("💻 Configuring fingerprint authentication for sudo commands");
        Self::apply_configuration(SUDO_SERVICE)
    }

    /// Remove fingerprint configuration for sudo service
    pub fn remove_sudo() -> io::Result<()> {
        info!("💻 Removing fingerprint authentication from sudo commands");
        Self::remove_configuration(SUDO_SERVICE)
    }

    /// Apply fingerprint configuration for polkit service
    pub fn apply_polkit() -> io::Result<()> {
        info!("🔒 Configuring fingerprint authentication for polkit actions");
        Self::apply_configuration(POLKIT_SERVICE)
    }

    /// Remove fingerprint configuration for polkit service
    pub fn remove_polkit() -> io::Result<()> {
        info!("🔒 Removing fingerprint authentication from polkit actions");
        Self::remove_configuration(POLKIT_SERVICE)
    }

    /// Apply configuration to all services using batch operation
    pub fn apply_all_configurations() -> io::Result<()> {
        info!("🔐 Applying fingerprint PAM configuration to ALL services");
        info!("   This will enable fingerprint auth for: login, sudo, and polkit");

        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("apply-all")
            .output()
            .map_err(|e| {
                error!(
                    "❌ Failed to execute pkexec for batch PAM configuration: {}",
                    e
                );
                io::Error::other(format!("Failed to execute pkexec: {}", e))
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            error!("❌ Batch PAM configuration failed: {}", err);
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        info!("✅ Successfully applied fingerprint authentication to all PAM services");
        Ok(())
    }

    /// Remove configuration from all services using batch operation
    pub fn remove_all_configurations() -> io::Result<()> {
        info!("🗑️  Removing fingerprint PAM configuration from ALL services");
        info!("   This will disable fingerprint auth for: login, sudo, and polkit");

        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("remove-all")
            .output()
            .map_err(|e| {
                error!("❌ Failed to execute pkexec for batch PAM removal: {}", e);
                io::Error::other(format!("Failed to execute pkexec: {}", e))
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            error!("❌ Batch PAM removal failed: {}", err);
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        info!("✅ Successfully removed fingerprint authentication from all PAM services");
        Ok(())
    }

    /// Check configuration status for all services (uses efficient batch operation)
    /// Returns (login_configured, sudo_configured, polkit_configured)
    pub fn check_all_configurations() -> (bool, bool, bool) {
        info!("🔍 Checking fingerprint authentication status for all PAM services");
        let result = Self::check_all_configurations_efficient();
        info!(
            "📊 Final PAM status: login={}, sudo={}, polkit={}",
            result.0, result.1, result.2
        );
        result
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
