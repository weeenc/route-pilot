//! OpenVPN process and Management Interface orchestration.
//!
//! This module remains platform-neutral; implementation begins in Milestone 5.

pub mod locator;
pub mod management;
pub mod manager;
pub mod parser;
#[cfg(target_os = "macos")]
pub mod privileged_helper;
pub mod process;
pub mod routing;
#[cfg(target_os = "windows")]
pub(crate) mod windows_adapter;
