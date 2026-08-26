//! Tauri IPC command handlers.
//!
//! Commands are introduced with the feature that owns them so the frontend never
//! controls operating-system processes directly.

pub mod profile;
pub mod settings;
pub mod vpn;
