#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use muniment_core::attach::ClientError;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::{answer_presented_approval, ApprovalPresenterStopHandle};
#[cfg(unix)]
use muniment_core::attach::{
    handshake_desktop_client_stream, serve_approval_presenter_at, serve_desktop_client_at,
    InterruptibleConnectState,
};
#[cfg(target_os = "windows")]
use muniment_core::attach::{
    serve_windows_approval_presenter, serve_windows_chat_events, serve_windows_desktop_client,
    WindowsChatEventStopHandle,
};
use muniment_core::attach::{ApprovalCoordinator, ProtocolError};
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::{DesktopClientHolder, DesktopClientStopHandle, SubscriptionEvent};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Arc;
use std::sync::{Condvar, Mutex};
#[cfg(any(unix, target_os = "windows"))]
use std::time::Duration;

#[cfg(target_os = "linux")]
use muniment_core::attach::linux::AttachFilesystem;
#[cfg(all(unix, test))]
use serde_json::{json, Value};
#[cfg(any(unix, target_os = "windows"))]
use tauri::{Emitter, Manager};

/// Matches the `muniment-runtime` version in `src-tauri/runtime/Cargo.toml`.
/// Raise this constant when a new run needs a newer runtime.
#[cfg(any(unix, target_os = "windows"))]
pub(crate) const MINIMUM_COMPATIBLE_RUNTIME_VERSION: &str = "0.0.1";

mod commands;
mod listener;
mod migration;
mod state;

pub use commands::*;
#[cfg(any(unix, target_os = "windows"))]
pub(crate) use listener::{deliver_agent_run, deliver_subscription_event};
#[cfg(unix)]
pub(crate) use listener::runtime_profile_endpoint;
#[cfg(target_os = "macos")]
pub(crate) use listener::start_desktop_client;
#[cfg(target_os = "linux")]
pub(crate) use listener::{runtime_clients_ready, start_runtime_clients};
#[cfg(target_os = "windows")]
pub(crate) use listener::{start_approval_presenter, start_desktop_client};
#[cfg(any(unix, target_os = "windows"))]
pub(crate) use state::DesktopClientSession;
pub use state::{AttachCompanionState, AttachListenerStatus, AuthorizedCompanion};

#[cfg(any(unix, target_os = "windows"))]
pub(crate) use migration::{runtime_upgrade_pending, runtime_version_compatible};

#[cfg(test)]
use listener::*;
#[cfg(test)]
use migration::*;
use state::*;

#[cfg(test)]
mod tests;
