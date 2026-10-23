//! The runtime's side of a desktop app update. The app installs the update,
//! so it asks this runtime to drain first: it saves a drain request beside
//! the update status, and the runtime refuses new runs, waits up to the
//! request's grace period for its work to finish, records the runs it
//! interrupts and answers with the `installing` status. Core resumes the
//! recorded runs when the runtime starts again.
//!
//! While an update waits or downloads, the runtime also keeps the status's
//! blockers current, so the app can name the work an install interrupts.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use muniment_core::attach::{RuntimeUpdateState, RuntimeUpdateStatus};
use muniment_core::serde_json;
use muniment_runtime_core::update::{blocker_names, DEFAULT_UPDATE_GRACE};
use muniment_runtime_core::RuntimeAttachState;

/// The drain request the app saves in the remote access directory.
pub const DRAIN_REQUEST_FILE: &str = "update-drain.json";
const POLL: Duration = Duration::from_millis(500);

/// Takes the saved drain request, `{"grace_seconds": n}`, and answers its
/// grace period, capped at the default grace period.
fn take_request(directory: &Path) -> Option<Duration> {
    let path = directory.join(DRAIN_REQUEST_FILE);
    let bytes = std::fs::read(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let request: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let grace = request["grace_seconds"].as_u64()?;
    Some(Duration::from_secs(grace).min(DEFAULT_UPDATE_GRACE))
}

/// The status with `blockers` in place of its own, when the update waits or
/// downloads and the blockers changed.
fn with_blockers(status: &RuntimeUpdateStatus, blockers: Vec<String>) -> Option<Vec<String>> {
    matches!(
        status.state,
        RuntimeUpdateState::Queued | RuntimeUpdateState::Downloading
    )
    .then_some(blockers)
    .filter(|blockers| *blockers != status.blockers)
}

/// Watches `directory`, the remote access directory, for drain requests on a
/// thread of its own. The thread ends after one drain, since the runtime
/// stops for the update next, or once the runtime's state closes.
pub fn start(state: &Arc<RuntimeAttachState>, directory: PathBuf) {
    let state = Arc::downgrade(state);
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL);
        let Some(state) = state.upgrade() else {
            return;
        };
        let Some(access) = state.remote_access() else {
            continue;
        };
        if let Some(grace) = take_request(&directory) {
            let status = access.update_status();
            let target = status.target_version.as_deref();
            muniment_core::runtime_eprintln!(
                "muniment-runtime: desktop update drain grace_seconds={}",
                grace.as_secs()
            );
            state.drain_for_update(grace);
            if let Err(error) =
                access.set_update_status(RuntimeUpdateState::Installing, target, Vec::new(), None)
            {
                muniment_core::runtime_eprintln!(
                    "muniment-runtime: update status not saved: {error}"
                );
            }
            return;
        }
        let status = access.update_status();
        let blockers = blocker_names(state.runtime_activity().snapshot());
        if let Some(blockers) = with_blockers(&status, blockers) {
            let _ = access.set_update_status(
                status.state,
                status.target_version.as_deref(),
                blockers,
                status.error,
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: RuntimeUpdateState, blockers: &[&str]) -> RuntimeUpdateStatus {
        RuntimeUpdateStatus {
            state,
            target_version: Some("0.4.0".into()),
            blockers: blockers
                .iter()
                .map(|blocker| (*blocker).to_owned())
                .collect(),
            error: None,
            updated_at: "2030-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn a_drain_request_is_taken_once_and_capped() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "muniment-update-drain-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        assert_eq!(take_request(&directory), None);
        std::fs::write(
            directory.join(DRAIN_REQUEST_FILE),
            br#"{"grace_seconds":0}"#,
        )
        .unwrap();
        assert_eq!(take_request(&directory), Some(Duration::ZERO));
        assert_eq!(take_request(&directory), None);
        std::fs::write(
            directory.join(DRAIN_REQUEST_FILE),
            br#"{"grace_seconds":9000}"#,
        )
        .unwrap();
        assert_eq!(take_request(&directory), Some(DEFAULT_UPDATE_GRACE));
        std::fs::write(directory.join(DRAIN_REQUEST_FILE), b"not json").unwrap();
        assert_eq!(take_request(&directory), None);
        assert!(!directory.join(DRAIN_REQUEST_FILE).exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn blockers_follow_activity_only_while_an_update_waits_or_downloads() {
        let active = vec!["active_run".to_owned()];
        assert_eq!(
            with_blockers(&status(RuntimeUpdateState::Queued, &[]), active.clone()),
            Some(active.clone())
        );
        assert_eq!(
            with_blockers(
                &status(RuntimeUpdateState::Downloading, &["active_run"]),
                Vec::new()
            ),
            Some(Vec::new())
        );
        assert_eq!(
            with_blockers(
                &status(RuntimeUpdateState::Queued, &["active_run"]),
                active.clone()
            ),
            None
        );
        for state in [
            RuntimeUpdateState::Idle,
            RuntimeUpdateState::Draining,
            RuntimeUpdateState::Installing,
            RuntimeUpdateState::Failed,
        ] {
            assert_eq!(with_blockers(&status(state, &[]), active.clone()), None);
        }
    }
}
