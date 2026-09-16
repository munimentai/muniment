#[cfg(target_os = "macos")]
use muniment_core::attach::interruptible_connect_with_state;
#[cfg(all(target_os = "linux", test))]
use muniment_core::attach::save_client_credentials as persist_client_credentials;
#[cfg(target_os = "linux")]
use muniment_core::attach::ApprovalRequest;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use muniment_core::attach::ClientError;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::{answer_presented_approval, ApprovalPresenterStopHandle};
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    bounded_claim, load_client_credentials, ClientCredential, CompanionRegistry,
    WorkspaceContextMap, COMPANION_CREDENTIAL_FILE_NAME,
};
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
use muniment_core::attach::{DesktopClientHolder, DesktopClientStopHandle};
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::collections::BTreeSet;
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Arc;
use std::sync::{Condvar, Mutex};
#[cfg(any(unix, target_os = "windows"))]
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{
    approval_waiter_with_claims, attach_listener_start_diagnostic,
    run_authenticated_session_with_service_approvals_registry_and_migration, AttachAcceptError,
    AttachFilesystem, AttachListenerStartFailure, AttachStopHandle, AttachTransport,
    LiveConnectionRegistry, MigrationControlSessionDependencies,
};
#[cfg(all(target_os = "linux", test))]
use muniment_core::attach::linux::{
    run_authenticated_session_with_service_and_approvals,
    run_authenticated_session_with_service_approvals_and_registry,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    evaluate_quiesce, name_attach_connection_route, probe_handoff, verify_migration_control_peer,
    Approval, AttachConnectionRoute, AttachListenerLifecycle, ConfirmedHandoff, HandoffProbeError,
    IdempotencyStore, PeerAuthorityError, PreparedHandoffSlot, RuntimeActivity,
    RuntimeActivityRegistry, SignedWorkspaceApproval,
};
#[cfg(target_os = "linux")]
use muniment_core::browser_control::ProcReader;
#[cfg(all(any(target_os = "linux", target_os = "macos"), test))]
use serde_json::json;
#[cfg(any(unix, target_os = "windows"))]
use serde_json::Value;
#[cfg(any(unix, target_os = "windows"))]
use tauri::{Emitter, Manager};
#[cfg(all(any(target_os = "linux", target_os = "macos"), test))]
use uuid::Uuid;

/// Matches the `muniment-runtime` version in `src-tauri/runtime/Cargo.toml`.
/// Raise this constant when a new run needs a newer runtime.
#[cfg(any(unix, target_os = "windows"))]
pub(crate) const MINIMUM_COMPATIBLE_RUNTIME_VERSION: &str = "0.0.1";

mod commands;
mod listener;
mod migration;
mod state;

pub use commands::*;
#[cfg(unix)]
pub(crate) use listener::start_desktop_client;
#[cfg(target_os = "linux")]
pub use listener::stop_attach_listener;
#[cfg(target_os = "linux")]
pub(crate) use listener::{runtime_clients_ready, start_runtime_clients};
#[cfg(target_os = "windows")]
pub(crate) use listener::{start_approval_presenter, start_desktop_client};
#[cfg(target_os = "linux")]
pub(crate) use migration::control_desktop_migration;
#[cfg(target_os = "linux")]
pub(crate) use state::AttachListenerState;
#[cfg(any(unix, target_os = "windows"))]
pub(crate) use state::DesktopClientSession;
pub use state::{AttachCompanionState, AttachListenerStatus, AuthorizedCompanion};

#[cfg(target_os = "linux")]
use crate::chat::TauriRunStartBoundaries;
#[cfg(any(unix, target_os = "windows"))]
pub(crate) use migration::{runtime_upgrade_pending, runtime_version_compatible};
#[cfg(target_os = "linux")]
#[allow(unused_imports)]
pub use muniment_core::attach::{DesktopAttachService, RunStartIdempotency};

use listener::*;
#[cfg(test)]
use migration::*;
use state::*;

#[cfg(test)]
mod tests;
