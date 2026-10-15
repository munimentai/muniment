//! Desktop-only modules: cloud sign-in, cloud chat grants, browser control,
//! the provider OAuth clients and the computer's display name.
//!
//! The shared core never names these modules. It takes their results through
//! its port-owned `account` and `chat_launch` values and its launch and attach
//! boundary traits, which the desktop runtime implements with this crate.

pub mod auth;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub mod browser_control;
pub mod chat_grant;
pub mod computer_name;
pub mod provider_clients;
pub mod workspace_metadata;
