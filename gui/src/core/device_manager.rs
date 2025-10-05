//! Device management abstraction for fingerprint operations.

use crate::core::fprintd;
use log::{error, info, warn};

/// Error types for device management operations.
#[derive(Debug)]
pub enum DeviceError {
    ConnectionFailed(String),
    NoDeviceAvailable,
    ClaimFailed(String),
    OperationFailed(String),
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            DeviceError::NoDeviceAvailable => write!(f, "No fingerprint devices available"),
            DeviceError::ClaimFailed(msg) => write!(f, "Failed to claim device: {}", msg),
            DeviceError::OperationFailed(msg) => write!(f, "Operation failed: {}", msg),
        }
    }
}

impl std::error::Error for DeviceError {}

/// RAII-style device manager for fprintd operations.
pub struct DeviceManager {
    device: Option<fprintd::Device>,
}

impl DeviceManager {
    /// Acquire a fingerprint device with automatic cleanup.
    pub async fn acquire() -> Result<Self, DeviceError> {
        info!("Acquiring fingerprint device for operation");

        let client = Self::connect_to_fprintd().await?;
        let device = Self::get_first_device(&client).await?;
        Self::claim_device(&device).await?;

        info!("Successfully acquired and claimed fingerprint device");
        Ok(Self {
            device: Some(device),
        })
    }

    /// Get a reference to the managed device.
    pub fn device(&self) -> Option<&fprintd::Device> {
        self.device.as_ref()
    }

    /// Connect to fprintd system bus.
    async fn connect_to_fprintd() -> Result<fprintd::Client, DeviceError> {
        info!("Connecting to fprintd system bus");
        match fprintd::Client::system().await {
            Ok(client) => {
                info!("Successfully connected to fprintd");
                Ok(client)
            }
            Err(e) => {
                error!("Failed to connect to fprintd system bus: {}", e);
                Err(DeviceError::ConnectionFailed(e.to_string()))
            }
        }
    }

    /// Get the first available fingerprint device.
    async fn get_first_device(client: &fprintd::Client) -> Result<fprintd::Device, DeviceError> {
        info!("Looking for available fingerprint devices");
        match fprintd::first_device(client).await {
            Ok(Some(device)) => {
                info!("Found fingerprint device");
                Ok(device)
            }
            Ok(None) => {
                warn!("No fingerprint devices available");
                warn!("Please connect a fingerprint reader and try again");
                Err(DeviceError::NoDeviceAvailable)
            }
            Err(e) => {
                error!("Failed to enumerate devices: {}", e);
                Err(DeviceError::OperationFailed(format!(
                    "Failed to enumerate devices: {}",
                    e
                )))
            }
        }
    }

    /// Claim the device for exclusive access.
    async fn claim_device(device: &fprintd::Device) -> Result<(), DeviceError> {
        info!("Claiming fingerprint device for exclusive access");
        match device.claim("").await {
            Ok(_) => {
                info!("Successfully claimed device");
                Ok(())
            }
            Err(e) => {
                error!("Failed to claim device: {}", e);
                Err(DeviceError::ClaimFailed(e.to_string()))
            }
        }
    }
}

impl Drop for DeviceManager {
    /// Automatic cleanup when DeviceManager goes out of scope.
    fn drop(&mut self) {
        if let Some(device) = self.device.take() {
            info!("Cleaning up device in destructor");
            tokio::spawn(async move {
                if let Err(e) = device.release().await {
                    warn!("Failed to release device during cleanup: {}", e);
                } else {
                    info!("Successfully released device during cleanup");
                }
            });
        }
    }
}

/// Convenience functions for common device operations.
impl DeviceManager {
    /// Execute enrollment operation with automatic device management.
    pub async fn enroll_finger<F>(finger_key: String, setup_listener: F) -> Result<(), DeviceError>
    where
        F: FnOnce(&fprintd::Device) -> Result<(), DeviceError>,
    {
        let manager = Self::acquire().await?;

        let device = manager
            .device()
            .ok_or_else(|| DeviceError::OperationFailed("Device not available".to_string()))?;

        setup_listener(device)?;

        info!("Starting enrollment process for finger: '{}'", finger_key);
        if let Err(e) = device.enroll_start(&finger_key).await {
            error!("Failed to start enrollment for '{}': {}", finger_key, e);
            let _ = device.enroll_stop().await;
            return Err(DeviceError::OperationFailed(format!(
                "Failed to start enrollment: {}",
                e
            )));
        }

        info!("Enrollment started successfully, waiting for finger scans...");
        Ok(())
    }

    /// Execute removal operation with automatic device management.
    pub async fn delete_finger(finger_key: String) -> Result<(), DeviceError> {
        let manager = Self::acquire().await?;

        let device = manager
            .device()
            .ok_or_else(|| DeviceError::OperationFailed("Device not available".to_string()))?;

        info!("Executing deletion of enrolled finger: '{}'", finger_key);
        if let Err(e) = device.delete_enrolled_finger(&finger_key).await {
            error!("Failed to delete enrolled finger '{}': {}", finger_key, e);
            return Err(DeviceError::OperationFailed(format!(
                "Failed to delete finger: {}",
                e
            )));
        }

        info!("Successfully deleted fingerprint '{}'", finger_key);
        Ok(())
    }
}
