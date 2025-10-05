//! Shared context structures for fingerprint operations.

use gtk4::prelude::*;
use gtk4::{Button, FlowBox, Label, Stack, Switch};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Main context for fingerprint operations, unifying enrollment and removal contexts.
#[derive(Clone)]
pub struct FingerprintContext {
    pub rt: Arc<Runtime>,
    pub ui: UiComponents,
    pub selected_finger: Rc<RefCell<Option<String>>>,
}

/// UI components grouped by functionality.
#[derive(Clone)]
pub struct UiComponents {
    pub flow: FlowBox,
    pub stack: Stack,
    pub switches: PamSwitches,
    pub labels: FingerprintLabels,
    pub buttons: FingerprintButtons,
}

/// PAM authentication switches.
#[derive(Clone)]
pub struct PamSwitches {
    pub login: Switch,
    pub term: Switch,
    pub prompt: Switch,
}

/// Fingerprint-related labels.
#[derive(Clone)]
pub struct FingerprintLabels {
    pub finger: Label,
    pub action: Label,
}

/// Fingerprint operation buttons.
#[derive(Clone)]
pub struct FingerprintButtons {
    pub add: Button,
    pub delete: Button,
}

impl UiComponents {
    /// Create UI components from individual widgets.
    pub fn new(
        flow: FlowBox,
        stack: Stack,
        switches: PamSwitches,
        labels: FingerprintLabels,
        buttons: FingerprintButtons,
    ) -> Self {
        Self {
            flow,
            stack,
            switches,
            labels,
            buttons,
        }
    }
}

impl PamSwitches {
    /// Create PAM switches from individual switch widgets.
    pub fn new(login: Switch, term: Switch, prompt: Switch) -> Self {
        Self {
            login,
            term,
            prompt,
        }
    }
}

impl FingerprintLabels {
    /// Create fingerprint labels from individual label widgets.
    pub fn new(finger: Label, action: Label) -> Self {
        Self { finger, action }
    }
}

impl FingerprintButtons {
    /// Create fingerprint buttons from individual button widgets.
    pub fn new(add: Button, delete: Button) -> Self {
        Self { add, delete }
    }
}

impl FingerprintContext {
    /// Create a new fingerprint context from pre-assembled components.
    pub fn new(
        rt: Arc<Runtime>,
        ui: UiComponents,
        selected_finger: Rc<RefCell<Option<String>>>,
    ) -> Self {
        Self {
            rt,
            ui,
            selected_finger,
        }
    }

    /// Check if any PAM switches are active.
    pub fn has_active_pam_switches(&self) -> bool {
        self.ui.switches.login.is_active()
            || self.ui.switches.term.is_active()
            || self.ui.switches.prompt.is_active()
    }

    /// Enable or disable all PAM switches based on fingerprint availability.
    pub fn set_pam_switches_sensitive(&self, sensitive: bool) {
        self.ui.switches.login.set_sensitive(sensitive);
        self.ui.switches.term.set_sensitive(sensitive);
        self.ui.switches.prompt.set_sensitive(sensitive);
    }

    /// Update button states based on selected finger and enrollment status.
    pub fn update_button_states(&self, is_enrolled: bool) {
        self.ui.buttons.add.set_sensitive(!is_enrolled);
        self.ui.buttons.delete.set_sensitive(is_enrolled);
    }

    /// Get the currently selected finger.
    pub fn get_selected_finger(&self) -> Option<String> {
        self.selected_finger.borrow().clone()
    }

    /// Set the currently selected finger.
    pub fn set_selected_finger(&self, finger: Option<String>) {
        *self.selected_finger.borrow_mut() = finger;
    }
}
