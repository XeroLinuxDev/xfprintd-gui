//! User Interface handling functionality.
//!
//! This module contains all UI-related components organized by functionality:
//! - `app`: Application setup and initialization
//! - `pam_ui`: PAM authentication switches UI
//! - `navigation`: Navigation buttons and dialogs
//! - `button_handlers`: Button click handlers
//! - `fingerprint_ui`: Fingerprint management UI

pub mod app;
pub mod button_handlers;
pub mod fingerprint_ui;
pub mod navigation;
pub mod pam_ui;

// Re-export commonly used items
pub use app::setup_application_ui;
