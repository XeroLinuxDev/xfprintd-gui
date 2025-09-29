#![allow(dead_code)]
//! Async helpers for fprintd (net.reactivated.Fprint) over zbus.
//! Reference: https://fprint.freedesktop.org/fprintd-dev/ref-dbus.html

use std::fmt;

use futures_util::StreamExt;
use serde::{Serialize, de::DeserializeOwned};
use zbus::zvariant::{OwnedObjectPath, Type};
use zbus::{Connection, Proxy};

/// D-Bus service (bus name) for fprintd.
pub const SERVICE: &str = "net.reactivated.Fprint";

/// Manager object path.
pub const MANAGER_PATH: &str = "/net/reactivated/Fprint/Manager";

/// Manager interface name.
pub const IFACE_MANAGER: &str = "net.reactivated.Fprint.Manager";

/// Device interface name.
pub const IFACE_DEVICE: &str = "net.reactivated.Fprint.Device";

/// Canonical list of fprintd finger names, used by the UI.
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

/// Represents an async client holding a system bus connection.
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
    /// Connect to the system bus.
    pub async fn system() -> zbus::Result<Self> {
        let conn = Connection::system().await?;
        Ok(Self { conn })
    }

    /// Access the underlying zbus connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Create a Manager helper bound to the fprintd Manager interface.
    pub fn manager(&self) -> Manager {
        Manager {
            conn: self.conn.clone(),
        }
    }

    /// Create a Device helper bound to a specific device object path.
    pub fn device(&self, object_path: OwnedObjectPath) -> Device {
        Device {
            conn: self.conn.clone(),
            object_path,
        }
    }
}

/// Helper for the fprintd Manager interface.
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

    /// Generic call helper.
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

    /// Return the list of device object paths known to fprintd.
    ///
    /// This maps to Manager.GetDevices() -> ao (array of object paths).
    pub async fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>> {
        let (paths,): (Vec<OwnedObjectPath>,) = self.call("GetDevices", &()).await?;
        Ok(paths)
    }

    /// Return the default device object path if available.
    ///
    /// Maps to Manager.GetDefaultDevice() -> o.
    pub async fn get_default_device(&self) -> zbus::Result<OwnedObjectPath> {
        let (path,): (OwnedObjectPath,) = self.call("GetDefaultDevice", &()).await?;
        Ok(path)
    }
}

/// Helper for the fprintd Device interface.
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

    /// Expose the device object path as a string.
    pub fn object_path(&self) -> &str {
        self.object_path.as_str()
    }

    /// Generic call helper.
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

    // ===== Methods =====

    /// List enrolled fingers for a user (use "" for current user).
    /// Maps to Device.ListEnrolledFingers(s) -> as.
    pub async fn list_enrolled_fingers(&self, username: &str) -> zbus::Result<Vec<String>> {
        let (fingers,): (Vec<String>,) = self.call("ListEnrolledFingers", &(username,)).await?;
        Ok(fingers)
    }

    /// Delete all enrolled fingers for the user currently claiming the device.
    /// Maps to Device.DeleteEnrolledFingers2() -> () (requires Device.Claim).
    pub async fn delete_enrolled_fingers(&self) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFingers2", &()).await?;
        Ok(())
    }

    /// Delete all enrolled fingers for a specific user (legacy helper).
    /// Maps to Device.DeleteEnrolledFingers(s) -> ().
    ///
    /// Note: Prefer claiming the device and using DeleteEnrolledFingers2.
    pub async fn delete_enrolled_fingers_for_user(&self, username: &str) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFingers", &(username,)).await?;
        Ok(())
    }

    /// Delete a single enrolled finger for the user currently claiming the device.
    /// Maps to Device.DeleteEnrolledFinger(s) -> () (requires Device.Claim).
    pub async fn delete_enrolled_finger(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFinger", &(finger,)).await?;
        Ok(())
    }

    /// Start enrollment for the given finger name.
    /// Maps to Device.EnrollStart(s) -> ().
    pub async fn enroll_start(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("EnrollStart", &(finger,)).await?;
        Ok(())
    }

    /// Stop any ongoing enrollment.
    /// Maps to Device.EnrollStop() -> ().
    pub async fn enroll_stop(&self) -> zbus::Result<()> {
        let _: () = self.call("EnrollStop", &()).await?;
        Ok(())
    }

    /// Start verification for a finger (e.g. "any" or a specific finger).
    /// Maps to Device.VerifyStart(s) -> ().
    pub async fn verify_start(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("VerifyStart", &(finger,)).await?;
        Ok(())
    }

    /// Stop any ongoing verification.
    /// Maps to Device.VerifyStop() -> ().
    pub async fn verify_stop(&self) -> zbus::Result<()> {
        let _: () = self.call("VerifyStop", &()).await?;
        Ok(())
    }

    /// Claim the device for a user (use "" for current user).
    /// Maps to Device.Claim(s) -> ().
    pub async fn claim(&self, username: &str) -> zbus::Result<()> {
        let _: () = self.call("Claim", &(username,)).await?;
        Ok(())
    }

    /// Release the device (if supported).
    /// Maps to Device.Release() -> ().
    pub async fn release(&self) -> zbus::Result<()> {
        let _: () = self.call("Release", &()).await?;
        Ok(())
    }

    // ===== Properties =====

    /// Get the device product name. Property: name (s).
    pub async fn name(&self) -> zbus::Result<String> {
        let proxy = self.proxy().await?;
        proxy.get_property::<String>("name").await
    }

    /// Get the number of enrollment stages. Property: num-enroll-stages (i).
    ///
    /// Note: This is only defined when the device has been claimed; otherwise it may be -1.
    pub async fn num_enroll_stages(&self) -> zbus::Result<i32> {
        let proxy = self.proxy().await?;
        proxy.get_property::<i32>("num-enroll-stages").await
    }

    /// Get the device scan type ("press" or "swipe"). Property: scan-type (s).
    pub async fn scan_type(&self) -> zbus::Result<String> {
        let proxy = self.proxy().await?;
        proxy.get_property::<String>("scan-type").await
    }

    /// Whether a finger is currently present on the sensor. Property: finger-present (b).
    pub async fn finger_present(&self) -> zbus::Result<bool> {
        let proxy = self.proxy().await?;
        proxy.get_property::<bool>("finger-present").await
    }

    /// Whether the sensor is waiting for a finger. Property: finger-needed (b).
    pub async fn finger_needed(&self) -> zbus::Result<bool> {
        let proxy = self.proxy().await?;
        proxy.get_property::<bool>("finger-needed").await
    }

    // ===== Signal listeners =====

    /// Listen for the VerifyFingerSelected signal and invoke the provided handler
    /// for each emission. This future completes only if the signal stream ends
    /// (e.g. connection closed) or an error occurs.
    ///
    /// Signal signature: (s)
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

    /// Listen for the VerifyStatus signal and invoke the provided handler for each emission.
    ///
    /// Signal signature: (s, b)
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

    /// Listen for the EnrollStatus signal and invoke the provided handler for each emission.
    ///
    /// Signal signature: (s, b)
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

// ===== Signal event types =====

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

// ===== High-level utilities =====

/// High-level utility: Find the first available Device helper.
///
/// Returns Ok(None) if no devices are present or if a call fails in a recoverable manner.
pub async fn first_device(client: &Client) -> zbus::Result<Option<Device>> {
    let mgr = client.manager();

    // Prefer default device if provided by fprintd.
    if let Ok(path) = mgr.get_default_device().await {
        return Ok(Some(client.device(path)));
    }

    // Fallback to the first enumerated device.
    match mgr.get_devices().await {
        Ok(paths) => {
            if let Some(path) = paths.first() {
                Ok(Some(client.device(path.clone())))
            } else {
                Ok(None)
            }
        }
        Err(e) => {
            // Propagate the original error; caller can decide how to handle.
            Err(e)
        }
    }
}
