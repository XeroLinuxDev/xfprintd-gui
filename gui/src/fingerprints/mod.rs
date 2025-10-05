//! Fingerprint management functionality.

pub mod enroll;
pub mod remove;

use crate::core::fprintd;
use log::{error, info, warn};
use std::collections::HashSet;

/// Scan for enrolled fingerprints on the system.
/// Returns HashSet of enrolled fingerprint names for current user.
pub async fn scan_enrolled_fingerprints() -> HashSet<String> {
    let mut enrolled_fingerprints = HashSet::new();

    info!("Connecting to fprintd system bus for fingerprint scan");
    let client = match fprintd::Client::system().await {
        Ok(client) => {
            info!("Successfully connected to fprintd system bus");
            client
        }
        Err(e) => {
            error!("Failed to connect to fprintd system bus: {}", e);
            error!("This usually means fprintd service is not running or not installed");
            return enrolled_fingerprints;
        }
    };

    info!("Searching for available fingerprint devices");
    let device = match fprintd::first_device(&client).await {
        Ok(Some(device)) => {
            info!("Found fingerprint device, proceeding with enrollment scan");
            device
        }
        Ok(None) => {
            warn!("No fingerprint devices detected on this system");
            warn!(
                "Please ensure your fingerprint reader is connected and recognized by the system"
            );
            return enrolled_fingerprints;
        }
        Err(e) => {
            error!("Failed to enumerate fingerprint devices: {}", e);
            error!("Check if fprintd service has proper permissions");
            return enrolled_fingerprints;
        }
    };

    let username = std::env::var("USER").unwrap_or_default();
    info!("Scanning enrolled fingerprints for user: '{}'", username);

    info!("Claiming fingerprint device for exclusive access");
    if let Err(e) = device.claim(&username).await {
        warn!("Failed to claim device for user '{}': {}", username, e);
        warn!("Device might be in use by another process");
    } else {
        info!("Successfully claimed fingerprint device");
    }

    info!("Retrieving list of enrolled fingerprints");
    match device.list_enrolled_fingers(&username).await {
        Ok(list) => {
            if list.is_empty() {
                info!("No enrolled fingerprints found for user '{}'", username);
                info!("User will need to enroll fingerprints before using authentication");
            } else {
                info!(
                    "Found {} enrolled fingerprint(s) for user '{}':",
                    list.len(),
                    username
                );
                for (i, finger) in list.iter().enumerate() {
                    info!("{}. {}", i + 1, finger);
                    enrolled_fingerprints.insert(finger.clone());
                }
            }
        }
        Err(e) => {
            error!("Failed to retrieve enrolled fingerprints: {}", e);
            error!("This might indicate permission issues or device problems");
        }
    }

    info!("Releasing fingerprint device");
    if let Err(e) = device.release().await {
        warn!("Failed to release device: {}", e);
        warn!("Device might remain locked until fprintd service restart");
    } else {
        info!("Successfully released fingerprint device");
    }

    info!(
        "Fingerprint scan completed. Found {} enrolled fingerprint(s)",
        enrolled_fingerprints.len()
    );
    enrolled_fingerprints
}
