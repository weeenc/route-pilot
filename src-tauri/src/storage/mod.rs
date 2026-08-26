//! Persistent application and profile metadata storage.

mod profile_store;
mod settings_store;

pub use profile_store::ProfileStore;
pub use settings_store::{AppSettings, SettingsStore};
