//! Desktop-only modules: cloud sign-in, cloud chat grants, browser control and
//! the provider OAuth clients.
//!
//! The shared core never names these modules. It takes their results through
//! its port-owned `account` and `chat_launch` values and its launch and attach
//! boundary traits, which the desktop runtime implements with this crate.

pub mod auth;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub mod browser_control;
pub mod chat_grant;
pub mod provider_clients;
