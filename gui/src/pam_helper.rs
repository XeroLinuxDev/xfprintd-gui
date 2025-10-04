use log::{debug, error, info, warn};
use std::io;
use std::process::Command;

/// Utility for managing PAM fingerprint configurations.
pub struct PamHelper;

/// Helper binary path.
const HELPER_PATH: &str = "/opt/xfprintd-gui/xfprintd-gui-helper";

/// PAM file paths.
pub const SUDO_PATH: &str = "/etc/pam.d/sudo";
pub const POLKIT_PATH: &str = "/etc/pam.d/polkit-1";

// Login manager paths (dynamically selected)
const LOGIN_PATH_GENERIC: &str = "/etc/pam.d/login";
const LOGIN_PATH_SDDM: &str = "/etc/pam.d/sddm";

/// Returns the appropriate login PAM path based on active display manager.
/// Uses SDDM path if sddm.service is enabled, otherwise uses generic login path.
pub fn get_login_path() -> &'static str {
    if is_sddm_enabled() {
        info!("SDDM is enabled, using /etc/pam.d/sddm");
        LOGIN_PATH_SDDM
    } else {
        info!("SDDM not detected, using /etc/pam.d/login");
        LOGIN_PATH_GENERIC
    }
}

/// Check if SDDM is enabled via systemd.
pub fn is_sddm_enabled() -> bool {
    match Command::new("systemctl")
        .arg("is-enabled")
        .arg("sddm.service")
        .output()
    {
        Ok(output) => {
            let result = output.status.success();
            if result {
                info!("systemctl reports sddm.service is enabled");
            } else {
                debug!("systemctl reports sddm.service is not enabled");
            }
            result
        }
        Err(e) => {
            debug!("Failed to check sddm.service status: {}", e);
            false
        }
    }
}

impl PamHelper {
    /// Check configuration status for all services (batch operation).
    /// Returns (login_configured, sudo_configured, polkit_configured).
    pub fn check_all_configurations() -> (bool, bool, bool) {
        info!("Checking fingerprint authentication status for all PAM services");
        info!("Performing batch check of all PAM configurations");

        let login_path = get_login_path();
        info!("Using login path: {}", login_path);

        match Command::new(HELPER_PATH)
            .arg("check")
            .arg(login_path)
            .arg(SUDO_PATH)
            .arg(POLKIT_PATH)
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut login = false;
                let mut sudo = false;
                let mut polkit = false;

                debug!("PAM helper output:\n{}", stdout);

                for line in stdout.lines() {
                    if let Some(path) = line.strip_prefix("applied: ") {
                        // Check both possible login paths
                        if path == LOGIN_PATH_GENERIC || path == LOGIN_PATH_SDDM {
                            login = true;
                            info!("Login PAM configuration: ENABLED ({})", path);
                        } else {
                            match path {
                                SUDO_PATH => {
                                    sudo = true;
                                    info!("Sudo PAM configuration: ENABLED");
                                }
                                POLKIT_PATH => {
                                    polkit = true;
                                    info!("Polkit PAM configuration: ENABLED");
                                }
                                _ => {
                                    debug!("Unknown PAM configuration found: {}", path);
                                }
                            }
                        }
                    } else if let Some(path) = line.strip_prefix("not-applied: ") {
                        // Check both possible login paths
                        if path == LOGIN_PATH_GENERIC || path == LOGIN_PATH_SDDM {
                            info!("Login PAM configuration: DISABLED ({})", path);
                        } else {
                            match path {
                                SUDO_PATH => info!("Sudo PAM configuration: DISABLED"),
                                POLKIT_PATH => info!("Polkit PAM configuration: DISABLED"),
                                _ => debug!("Unknown PAM path not configured: {}", path),
                            }
                        }
                    }
                }

                let exit_code = output.status.code().unwrap_or(-1);
                info!("PAM batch check completed (exit code: {})", exit_code);
                info!(
                    "Final PAM status: login={}, sudo={}, polkit={}",
                    login, sudo, polkit
                );
                (login, sudo, polkit)
            }
            Err(e) => {
                warn!("Batch PAM check failed: {}", e);
                warn!("Fallback to individual service checks");
                warn!("Using fallback method: checking PAM configurations individually");

                let login_path = get_login_path();
                let login = Self::is_configured(login_path);
                let sudo = Self::is_configured(SUDO_PATH);
                let polkit = Self::is_configured(POLKIT_PATH);

                info!(
                    "Individual PAM check results: login={}, sudo={}, polkit={}",
                    login, sudo, polkit
                );
                info!(
                    "Final PAM status: login={}, sudo={}, polkit={}",
                    login, sudo, polkit
                );
                (login, sudo, polkit)
            }
        }
    }

    /// Check if fingerprint configuration is applied for path.
    fn is_configured(path: &str) -> bool {
        info!("Checking PAM configuration for path: '{}'", path);

        match Command::new(HELPER_PATH).arg("check").arg(path).status() {
            Ok(status) => {
                let configured = status.success();
                if configured {
                    info!("PAM path '{}' has fingerprint authentication enabled", path);
                } else {
                    info!(
                        "PAM path '{}' does not have fingerprint authentication",
                        path
                    );
                }
                configured
            }
            Err(e) => {
                error!("Failed to check PAM configuration for '{}': {}", path, e);
                error!(
                    "Helper tool might not be installed or accessible at: {}",
                    HELPER_PATH
                );
                false
            }
        }
    }

    /// Apply fingerprint configuration for PAM file path using pkexec.
    pub fn apply_configuration(path: &str) -> io::Result<()> {
        info!(
            "Applying fingerprint PAM configuration for path: '{}'",
            path
        );
        info!("Requesting root privileges via pkexec");

        // Build JSON object with optional default file
        let json_arg = if path == POLKIT_PATH {
            format!(
                r#"{{"file":"{}","default":"/usr/lib/pam.d/polkit-1"}}"#,
                path
            )
        } else {
            format!(r#"{{"file":"{}"}}"#, path)
        };

        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("apply")
            .arg(&json_arg)
            .output()
            .map_err(|e| {
                error!("Failed to execute pkexec for PAM configuration: {}", e);
                error!("Make sure polkit is installed and configured properly");
                io::Error::other(format!("Failed to execute pkexec: {}", e))
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            error!("PAM configuration failed for path '{}': {}", path, err);
            if !stdout.is_empty() {
                debug!("Helper stdout: {}", stdout);
            }
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        info!(
            "Successfully applied fingerprint PAM configuration for '{}'",
            path
        );
        if !stdout.is_empty() {
            info!("Helper response: {}", stdout.trim());
        }
        Ok(())
    }

    /// Remove fingerprint configuration for PAM file path using pkexec.
    pub fn remove_configuration(path: &str) -> io::Result<()> {
        info!(
            "Removing fingerprint PAM configuration for path: '{}'",
            path
        );
        info!("Requesting root privileges via pkexec");

        let output = Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("remove")
            .arg(path)
            .output()
            .map_err(|e| {
                error!("Failed to execute pkexec for PAM removal: {}", e);
                error!("Make sure polkit is installed and configured properly");
                io::Error::other(format!("Failed to execute pkexec: {}", e))
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            error!(
                "PAM configuration removal failed for path '{}': {}",
                path, err
            );
            if !stdout.is_empty() {
                debug!("Helper stdout: {}", stdout);
            }
            return Err(io::Error::other(format!("Helper failed: {}", err)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        info!(
            "Successfully removed fingerprint PAM configuration for '{}'",
            path
        );
        if !stdout.is_empty() {
            info!("Helper response: {}", stdout.trim());
        }
        Ok(())
    }
}
