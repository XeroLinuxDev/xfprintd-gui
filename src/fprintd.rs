#![allow(dead_code)]
// D-Bus helpers for fprintd using zbus (blocking)
//
// Provides blocking wrappers for the system-bus fprintd (net.reactivated.Fprint) service.
// Reference: https://fprint.freedesktop.org/fprintd-dev/ref-dbus.html
//
// Note: Calls that interact with the sensor (enroll/verify/identify) block the
// calling thread; run them off the UI thread if needed.

use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, Type};

/// D-Bus service (bus name) for fprintd.
pub const SERVICE: &str = "net.reactivated.Fprint";

/// Manager object path.
pub const MANAGER_PATH: &str = "/net/reactivated/Fprint/Manager";

/// Manager interface name.
pub const IFACE_MANAGER: &str = "net.reactivated.Fprint.Manager";

/// Device interface name.
pub const IFACE_DEVICE: &str = "net.reactivated.Fprint.Device";

/// Default method call timeout. Use `None` for zbus defaults.
pub const DEFAULT_TIMEOUT: Option<Duration> = None;

/// A client holding a system bus connection.
#[derive(Debug)]
pub struct Client {
    conn: Connection,
}

impl Client {
    /// Connect to the system bus (blocking).
    pub fn system() -> zbus::Result<Self> {
        let conn = Connection::system()?;
        Ok(Self { conn })
    }

    /// Access the underlying zbus connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Create a Manager helper bound to the fprintd Manager interface.
    pub fn manager(&self) -> Manager<'_> {
        Manager { conn: &self.conn }
    }

    /// Create a Device helper bound to a specific device object path.
    pub fn device<'c>(&'c self, object_path: OwnedObjectPath) -> Device<'c> {
        Device {
            conn: &self.conn,
            object_path,
        }
    }
}

/// Helper for the fprintd Manager interface.
#[derive(Debug, Clone, Copy)]
pub struct Manager<'c> {
    conn: &'c Connection,
}

impl<'c> Manager<'c> {
    fn proxy(&self) -> zbus::Result<Proxy<'_>> {
        Proxy::new(self.conn, SERVICE, MANAGER_PATH, IFACE_MANAGER)
    }

    fn proxy_with_timeout(&self, _timeout: Option<Duration>) -> zbus::Result<Proxy<'_>> {
        // Blocking Proxy in zbus v3 lacks per-proxy timeout; use defaults or configure the connection.
        self.proxy()
    }

    /// Generic call helper.
    pub fn call<R>(&self, method: &str, args: &(impl Serialize + Type)) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        self.proxy()?.call(method, args)
    }

    /// Generic call helper with timeout override.
    pub fn call_with_timeout<R>(
        &self,
        method: &str,
        args: &(impl Serialize + Type),
        timeout: Option<Duration>,
    ) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        self.proxy_with_timeout(timeout)?.call(method, args)
    }

    /// Return the list of device object paths known to fprintd.
    ///
    /// This maps to Manager.GetDevices() -> ao (array of object paths).
    pub fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>> {
        let (paths,): (Vec<OwnedObjectPath>,) = self.call("GetDevices", &())?;
        Ok(paths)
    }
}

/// Helper for the fprintd Device interface.
#[derive(Debug, Clone)]
pub struct Device<'c> {
    conn: &'c Connection,
    object_path: OwnedObjectPath,
}

impl<'c> Device<'c> {
    fn proxy(&self) -> zbus::Result<Proxy<'_>> {
        Proxy::new(self.conn, SERVICE, self.object_path.as_str(), IFACE_DEVICE)
    }

    fn proxy_with_timeout(&self, _timeout: Option<Duration>) -> zbus::Result<Proxy<'_>> {
        // Timeout adjustment not supported on the blocking Proxy; use defaults.
        self.proxy()
    }

    /// Generic call helper.
    pub fn call<R>(&self, method: &str, args: &(impl Serialize + Type)) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        self.proxy()?.call(method, args)
    }

    /// Generic call helper with timeout override.
    pub fn call_with_timeout<R>(
        &self,
        method: &str,
        args: &(impl Serialize + Type),
        timeout: Option<Duration>,
    ) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        self.proxy_with_timeout(timeout)?.call(method, args)
    }

    /// List enrolled fingers for the current user.
    /// Maps to Device.ListEnrolledFingers() -> as.
    pub fn list_enrolled_fingers(&self) -> zbus::Result<Vec<String>> {
        let (fingers,): (Vec<String>,) = self.call("ListEnrolledFingers", &())?;
        Ok(fingers)
    }

    /// Delete all enrolled fingers for the current user.
    /// Maps to Device.DeleteEnrolledFingers() -> ().
    pub fn delete_enrolled_fingers(&self) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFingers", &())?;
        Ok(())
    }

    /// Start enrollment for the given finger name.
    /// Maps to Device.EnrollStart(s) -> ().
    pub fn enroll_start(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("EnrollStart", &(finger,))?;
        Ok(())
    }

    /// Stop any ongoing enrollment.
    /// Maps to Device.EnrollStop() -> ().
    pub fn enroll_stop(&self) -> zbus::Result<()> {
        let _: () = self.call("EnrollStop", &())?;
        Ok(())
    }

    /// Start verification.
    /// Maps to Device.VerifyStart() -> ().
    pub fn verify_start(&self) -> zbus::Result<()> {
        let _: () = self.call("VerifyStart", &())?;
        Ok(())
    }

    /// Stop any ongoing verification.
    /// Maps to Device.VerifyStop() -> ().
    pub fn verify_stop(&self) -> zbus::Result<()> {
        let _: () = self.call("VerifyStop", &())?;
        Ok(())
    }

    /// Claim the device (if supported).
    pub fn claim(&self) -> zbus::Result<()> {
        let _: () = self.call("Claim", &())?;
        Ok(())
    }

    /// Release the device (if supported).
    pub fn release(&self) -> zbus::Result<()> {
        let _: () = self.call("Release", &())?;
        Ok(())
    }
}

/// High-level utility: Find the first available Device helper.
///
/// Returns `None` if no devices are present or if a call fails.
pub fn first_device(client: &Client) -> Option<Device<'_>> {
    let mgr = client.manager();
    let paths = mgr.get_devices().ok()?;
    let path = paths.first()?;
    Some(client.device((*path).clone()))
}
