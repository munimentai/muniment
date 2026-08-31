//! Dormant runtime service composition.

mod run;
mod session;
mod threads;
mod workspace;

pub use run::{
    accept_prompt, answer_permission, cancel_run, configure_run, drive_prompt, queue_run_message,
    resume_run, run_prompt, ConfigureRunError, PromptAcceptance, PromptLaunch,
};
pub use session::{
    ensure_native_session, entitlement_snapshot, list_devices, session_status, sign_in, sign_out,
    EntitlementSnapshotError, EntitlementSnapshotResult, SignOutError,
};
pub use threads::{
    apply_retention, create_thread, delete_thread, rename_thread, select_thread, stream_run,
    subscribe_run_commits, thread_page, thread_summaries,
};
pub use workspace::{
    ensure_home, list_companions, onboard_workspace, open_profile_storage, revoke_companion,
};
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub use workspace::open_companion_registry;

use muniment_core::journal::Provenance;
use std::collections::BTreeMap;

pub(crate) fn runtime_provenance() -> Provenance {
    Provenance {
        source: "muniment-runtime".into(),
        source_version: env!("CARGO_PKG_VERSION").into(),
        actor_id: None,
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra: BTreeMap::new(),
    }
}
