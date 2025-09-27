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
    pub fn call<R>(
        &self,
        method: &str,
        args: &(impl Serialize + Type + std::fmt::Debug),
    ) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        let proxy = self.proxy()?;
        println!("[fprintd][Manager] call {}({:?})", method, args);
        let result = proxy.call(method, args);
        if let Err(ref e) = result {
            println!("[fprintd][Manager] {}() error: {}", method, e);
        }
        result
    }

    /// Generic call helper with timeout override.
    pub fn call_with_timeout<R>(
        &self,
        method: &str,
        args: &(impl Serialize + Type + std::fmt::Debug),
        timeout: Option<Duration>,
    ) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        let proxy = self.proxy_with_timeout(timeout)?;
        println!(
            "[fprintd][Manager] call {}({:?}) with timeout {:?}",
            method, args, timeout
        );
        let result = proxy.call(method, args);
        if let Err(ref e) = result {
            println!("[fprintd][Manager] {}() error: {}", method, e);
        }
        result
    }

    /// Return the list of device object paths known to fprintd.
    ///
    /// This maps to Manager.GetDevices() -> ao (array of object paths).
    pub fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>> {
        let (paths,): (Vec<OwnedObjectPath>,) = self.call("GetDevices", &())?;
        Ok(paths)
    }

    /// Return the default device object path if available.
    ///
    /// Maps to Manager.GetDefaultDevice() -> o.
    pub fn get_default_device(&self) -> zbus::Result<OwnedObjectPath> {
        let (path,): (OwnedObjectPath,) = self.call("GetDefaultDevice", &())?;
        Ok(path)
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
    pub fn call<R>(
        &self,
        method: &str,
        args: &(impl Serialize + Type + std::fmt::Debug),
    ) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        let proxy = self.proxy()?;
        let path = self.object_path.as_str();

        println!("[fprintd][Device {}] call {}({:?})", path, method, args);
        let result = proxy.call(method, args);
        if let Err(ref e) = result {
            println!("[fprintd][Device {}] {}() error: {}", path, method, e);
        }
        result
    }

    /// Generic call helper with timeout override.
    pub fn call_with_timeout<R>(
        &self,
        method: &str,
        args: &(impl Serialize + Type + std::fmt::Debug),
        timeout: Option<Duration>,
    ) -> zbus::Result<R>
    where
        R: DeserializeOwned + Type,
    {
        let proxy = self.proxy_with_timeout(timeout)?;
        let path = self.object_path.as_str();
        println!(
            "[fprintd][Device {}] call {}({:?}) with timeout {:?}",
            path, method, args, timeout
        );
        let result = proxy.call(method, args);
        if let Err(ref e) = result {
            println!("[fprintd][Device {}] {}() error: {}", path, method, e);
        }
        result
    }

    /// List enrolled fingers for a user (use "" for current user).
    /// Maps to Device.ListEnrolledFingers(s) -> as.
    pub fn list_enrolled_fingers(&self, username: &str) -> zbus::Result<Vec<String>> {
        let (fingers,): (Vec<String>,) = self.call("ListEnrolledFingers", &(username,))?;
        Ok(fingers)
    }

    /// Delete all enrolled fingers for the user currently claiming the device.
    /// Maps to Device.DeleteEnrolledFingers2() -> () (requires Device.Claim).
    pub fn delete_enrolled_fingers(&self) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFingers2", &())?;
        Ok(())
    }

    /// Delete all enrolled fingers for a specific user (legacy helper).
    /// Maps to Device.DeleteEnrolledFingers(s) -> ().
    ///
    /// Note: Prefer claiming the device and using DeleteEnrolledFingers2.
    pub fn delete_enrolled_fingers_for_user(&self, username: &str) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFingers", &(username,))?;
        Ok(())
    }

    /// Delete a single enrolled finger for the user currently claiming the device.
    /// Maps to Device.DeleteEnrolledFinger(s) -> () (requires Device.Claim).
    pub fn delete_enrolled_finger(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("DeleteEnrolledFinger", &(finger,))?;
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

    /// Start verification for a finger (e.g. "any" or a specific finger).
    /// Maps to Device.VerifyStart(s) -> ().
    pub fn verify_start(&self, finger: &str) -> zbus::Result<()> {
        let _: () = self.call("VerifyStart", &(finger,))?;
        Ok(())
    }

    /// Stop any ongoing verification.
    /// Maps to Device.VerifyStop() -> ().
    pub fn verify_stop(&self) -> zbus::Result<()> {
        let _: () = self.call("VerifyStop", &())?;
        Ok(())
    }

    /// Claim the device for a user (use "" for current user).
    pub fn claim(&self, username: &str) -> zbus::Result<()> {
        let _: () = self.call("Claim", &(username,))?;
        Ok(())
    }

    /// Release the device (if supported).
    pub fn release(&self) -> zbus::Result<()> {
        let _: () = self.call("Release", &())?;
        Ok(())
    }

    /// Get the device product name. Property: name (s).
    pub fn name(&self) -> zbus::Result<String> {
        let proxy = self.proxy()?;
        proxy.get_property("name")
    }

    /// Get the number of enrollment stages. Property: num-enroll-stages (i).
    ///
    /// Note: This is only defined when the device has been claimed; otherwise it may be -1.
    pub fn num_enroll_stages(&self) -> zbus::Result<i32> {
        let proxy = self.proxy()?;
        proxy.get_property("num-enroll-stages")
    }

    /// Get the device scan type ("press" or "swipe"). Property: scan-type (s).
    pub fn scan_type(&self) -> zbus::Result<String> {
        let proxy = self.proxy()?;
        proxy.get_property("scan-type")
    }

    /// Whether a finger is currently present on the sensor. Property: finger-present (b).
    pub fn finger_present(&self) -> zbus::Result<bool> {
        let proxy = self.proxy()?;
        proxy.get_property("finger-present")
    }

    /// Whether the sensor is waiting for a finger. Property: finger-needed (b).
    pub fn finger_needed(&self) -> zbus::Result<bool> {
        let proxy = self.proxy()?;
        proxy.get_property("finger-needed")
    }
}

/// High-level utility: Find the first available Device helper.
///
/// Returns `None` if no devices are present or if a call fails.
pub fn first_device(client: &Client) -> Option<Device<'_>> {
    let mgr = client.manager();
    if let Ok(path) = mgr.get_default_device() {
        return Some(client.device(path));
    }
    let paths = mgr.get_devices().ok()?;
    let path = paths.first()?;
    Some(client.device((*path).clone()))
}
