//! Core functionality and business logic.

pub mod context;
pub mod device_manager;
pub mod fprintd;
pub mod system;
pub mod util;

// Re-export commonly used items
pub use context::*;
