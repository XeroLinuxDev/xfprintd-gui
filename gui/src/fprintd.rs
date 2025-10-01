#![allow(dead_code)]
//! Async helpers for fprintd D-Bus interface.

use std::fmt;

use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Serialize};
use zbus::zvariant::{OwnedObjectPath, Type};
use zbus::{Connection, Proxy};

// D-Bus API Reference:
// BUS_NAME = 'net.reactivated.Fprint'
// MAIN_OBJ = '/net/reactivated/Fprint/Manager'
// SYSTEM_BUS = True
// IS_OBJECT_MANAGER = False

// MAIN_IFACE = 'net.reactivated.Fprint.Manager'
// MANAGER_MOCK_IFACE = 'net.reactivated.Fprint.Manager.Mock'

// DEVICE_IFACE = 'net.reactivated.Fprint.Device'
// DEVICE_MOCK_IFACE = 'net.reactivated.Fprint.Device.Mock'

// VALID_FINGER_NAMES = [
//     'left-thumb',
//     'left-index-finger',
//     'left-middle-finger',
//     'left-ring-finger',
//     'left-little-finger',
//     'right-thumb',
//     'right-index-finger',
//     'right-middle-finger',
//     'right-ring-finger',
//     'right-little-finger'
// ]

// VALID_VERIFY_STATUS = [
//     'verify-no-match',
//     'verify-match',
//     'verify-retry-scan',
//     'verify-too-fast',
//     'verify-swipe-too-short',
//     'verify-finger-not-centered',
//     'verify-remove-and-retry',
//     'verify-disconnected',
//     'verify-unknown-error'
// ]

// VALID_ENROLL_STATUS = [
//     'enroll-completed',
//     'enroll-failed',
//     'enroll-stage-passed',
//     'enroll-retry-scan',
//     'enroll-too-fast',
//     'enroll-swipe-too-short',
//     'enroll-finger-not-centered',
//     'enroll-remove-and-retry',
//     'enroll-data-full',
//     'enroll-disconnected',
//     'enroll-unknown-error'
// ]

/// D-Bus service name for fprintd.
pub const SERVICE: &str = "net.reactivated.Fprint";

/// Manager object path.
pub const MANAGER_PATH: &str = "/net/reactivated/Fprint/Manager";

/// Manager interface name.
pub const IFACE_MANAGER: &str = "net.reactivated.Fprint.Manager";

/// Device interface name.
pub const IFACE_DEVICE: &str = "net.reactivated.Fprint.Device";

/// Supported finger names.
pub const FINGERS: &[&str] = &[
    "left-thumb",
    "left-index-finger",
    "left-middle-finger",
    "left-ring-finger",
    "left-little-finger",
    "right-thumb",
    "right-index-finger",
    "right-middle-finger",
    "right-ring-finger",
    "right-little-finger",
];

/// Async client with system bus connection.
#[derive(Clone)]
pub struct Client {
    conn: Connection,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl Client {
    /// Connect to system bus.
    pub async fn system() -> zbus::Result<Self> {
        let conn = Connection::system().await?;
        Ok(Self { conn })
    }

    /// Get underlying connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Create Manager helper.
    pub fn manager(&self) -> Manager {
        Manager {
            conn: self.conn.clone(),
        }
    }

    /// Create Device helper for specific path.
    pub fn device(&self, object_path: OwnedObjectPath) -> Device {
        Device {
            conn: self.conn.clone(),
            object_path,
        }
    }
}

/// Manager interface helper.
#[derive(Clone)]
pub struct Manager {
    conn: Connection,
}

impl fmt::Debug for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Manager").finish_non_exhaustive()
    }
}

impl Manager {
    async fn proxy(&self) -> zbus::Result<Proxy<'_>> {
        Proxy::new(&self.conn, SERVICE, MANAGER_PATH, IFACE_MANAGER).await
    }

    /// Generic method call.
    async fn call<R>(
        &self,
        method: &str,
        args: &(impl Serialize + Type + fmt::Debug),
    ) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        let proxy = self.proxy().await?;

        proxy.call(method, args).await
    }

    /// Get device object paths.
    pub async fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>> {
        let (paths,): (Vec<OwnedObjectPath>,) = self.call("GetDevices", &()).await?;
        Ok(paths)
    }

    /// Get default device path.
    pub async fn get_default_device(&self) -> zbus::Result<OwnedObjectPath> {
        let (path,): (OwnedObjectPath,) = self.call("GetDefaultDevice", &()).await?;
        Ok(path)
    }
}

/// Device interface helper.
#[derive(Clone)]
pub struct Device {
    conn: Connection,
    object_path: OwnedObjectPath,
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Device")
            .field("object_path", &self.object_path)
            .finish()
    }
}

impl Device {
    async fn proxy(&self) -> zbus::Result<Proxy<'_>> {
        Proxy::new(&self.conn, SERVICE, self.object_path.as_str(), IFACE_DEVICE).await
    }

    /// Get device object path.
    pub fn object_path(&self) -> &str {
        self.object_path.as_str()
    }

    /// Generic method call.
    async fn call<R>(
        &self,
        method: &str,
        args: &(impl Serialize + Type + fmt::Debug),
    ) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        let proxy = self.proxy().await?;

        proxy.call(method, args).await
    }

    /// List enrolled fingers for user ("" for current user).
    pub async fn list_enrolled_fingers(&self, username: &str) -> zbus::Result<Vec<String>> {
        let (fingers,): (Vec<String>,) = self.call("ListEnrolledFingers", &(username,)).await?;
        Ok(fingers)
    }

    /// Delete all enrolled fingers (requires device claim).
    pub async fn delete_enrolled_fingers(&self) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFingers2", &()).await?;
        Ok(())
    }

    /// Delete all enrolled fingers for specific user (legacy).
    pub async fn delete_enrolled_fingers_for_user(&self, username: &str) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFingers", &(username,)).await?;
        Ok(())
    }

    /// Delete single enrolled finger (requires device claim).
    pub async fn delete_enrolled_finger(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFinger", &(finger,)).await?;
        Ok(())
    }

    /// Start enrollment for finger.
    pub async fn enroll_start(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("EnrollStart", &(finger,)).await?;
        Ok(())
    }

    /// Stop enrollment.
    pub async fn enroll_stop(&self) -> zbus::Result<()> {
        let _: () = self.call("EnrollStop", &()).await?;
        Ok(())
    }

    /// Start verification for finger.
    pub async fn verify_start(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("VerifyStart", &(finger,)).await?;
        Ok(())
    }

    /// Stop verification.
    pub async fn verify_stop(&self) -> zbus::Result<()> {
        let _: () = self.call("VerifyStop", &()).await?;
        Ok(())
    }

    /// Claim device for user ("" for current user).
    pub async fn claim(&self, username: &str) -> zbus::Result<()> {
        let _: () = self.call("Claim", &(username,)).await?;
        Ok(())
    }

    /// Release device.
    pub async fn release(&self) -> zbus::Result<()> {
        let _: () = self.call("Release", &()).await?;
        Ok(())
    }

    /// Get device name.
    pub async fn name(&self) -> zbus::Result<String> {
        let proxy = self.proxy().await?;
        proxy.get_property::<String>("name").await
    }

    /// Get enrollment stages count (requires claimed device).
    pub async fn num_enroll_stages(&self) -> zbus::Result<i32> {
        let proxy = self.proxy().await?;
        proxy.get_property::<i32>("num-enroll-stages").await
    }

    /// Get scan type ("press" or "swipe").
    pub async fn scan_type(&self) -> zbus::Result<String> {
        let proxy = self.proxy().await?;
        proxy.get_property::<String>("scan-type").await
    }

    /// Check if finger is present on sensor.
    pub async fn finger_present(&self) -> zbus::Result<bool> {
        let proxy = self.proxy().await?;
        proxy.get_property::<bool>("finger-present").await
    }

    /// Check if sensor needs finger.
    pub async fn finger_needed(&self) -> zbus::Result<bool> {
        let proxy = self.proxy().await?;
        proxy.get_property::<bool>("finger-needed").await
    }

    /// Listen for VerifyFingerSelected signal.
    pub async fn listen_verify_finger_selected<F>(&self, mut handler: F) -> zbus::Result<()>
    where
        F: FnMut(VerifyFingerSelectedEvent) + Send,
    {
        let proxy = self.proxy().await?;
        let mut stream = proxy.receive_signal("VerifyFingerSelected").await?;

        while let Some(msg) = stream.next().await {
            let (finger_name,): (String,) = msg.body().deserialize()?;
            handler(VerifyFingerSelectedEvent { finger_name });
        }

        Ok(())
    }

    /// Listen for VerifyStatus signal.
    pub async fn listen_verify_status<F>(&self, mut handler: F) -> zbus::Result<()>
    where
        F: FnMut(VerifyStatusEvent) + Send,
    {
        let proxy = self.proxy().await?;
        let mut stream = proxy.receive_signal("VerifyStatus").await?;

        while let Some(msg) = stream.next().await {
            let (result, done): (String, bool) = msg.body().deserialize()?;
            handler(VerifyStatusEvent { result, done });
        }

        Ok(())
    }

    /// Listen for EnrollStatus signal.
    pub async fn listen_enroll_status<F>(&self, mut handler: F) -> zbus::Result<()>
    where
        F: FnMut(EnrollStatusEvent) + Send,
    {
        let proxy = self.proxy().await?;
        let mut stream = proxy.receive_signal("EnrollStatus").await?;

        while let Some(msg) = stream.next().await {
            let (result, done): (String, bool) = msg.body().deserialize()?;
            handler(EnrollStatusEvent { result, done });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyFingerSelectedEvent {
    pub finger_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyStatusEvent {
    pub result: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollStatusEvent {
    pub result: String,
    pub done: bool,
}

/// Find first available device.
pub async fn first_device(client: &Client) -> zbus::Result<Option<Device>> {
    let mgr = client.manager();

    // Try default device first
    if let Ok(path) = mgr.get_default_device().await {
        return Ok(Some(client.device(path)));
    }

    // Fall back to first enumerated device
    match mgr.get_devices().await {
        Ok(paths) => {
            if let Some(path) = paths.first() {
                Ok(Some(client.device(path.clone())))
            } else {
                Ok(None)
            }
        }
        Err(e) => Err(e),
    }
}
