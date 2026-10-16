//! Fleet update state: what each runtime reports about its update, when the
//! app asks a runtime to update, which machines Update all skips, and when
//! this app follows a newer paired desktop.

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use super::version::is_older;

/// How long the app waits before it asks one runtime for one version again.
pub(crate) const REQUEST_BACKOFF: Duration = Duration::from_secs(10 * 60);

/// The update states in which a runtime works toward a version.
const PENDING: [&str; 5] = [
    "queued",
    "downloading",
    "draining",
    "installing",
    "restarting",
];
const STATES: [&str; 7] = [
    "idle",
    "queued",
    "downloading",
    "draining",
    "installing",
    "restarting",
    "failed",
];

/// A runtime's `update_status`, from `runtime.info` or a
/// `runtime.update_status` event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateStatus {
    pub(crate) state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_version: Option<String>,
    pub(crate) blockers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

impl UpdateStatus {
    /// Reads a status body, or `None` when it names no known state.
    pub(crate) fn from_body(body: &Value) -> Option<Self> {
        let state = body["state"]
            .as_str()
            .filter(|state| STATES.contains(state))?;
        Some(Self {
            state: state.to_owned(),
            target_version: body["target_version"].as_str().map(str::to_owned),
            blockers: body["blockers"]
                .as_array()
                .map(|blockers| {
                    blockers
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            error: body["error"].as_str().map(str::to_owned),
        })
    }

    fn targets(&self, version: &str) -> bool {
        self.target_version.as_deref().is_some_and(|target| {
            is_older(target, version) == Some(false) && is_older(version, target) == Some(false)
        })
    }

    /// The runtime works toward `target` now.
    pub(crate) fn pending_for(&self, target: &str) -> bool {
        PENDING.contains(&self.state.as_str()) && self.targets(target)
    }

    /// The runtime failed to install `target` or a newer version, so the app
    /// does not ask it for `target` on its own.
    pub(crate) fn failed_for(&self, target: &str) -> bool {
        self.state == "failed"
            && self
                .target_version
                .as_deref()
                .is_some_and(|failed| is_older(failed, target) != Some(true))
    }

    /// The runtime has acted on a request for `target`: it works toward it
    /// or failed it.
    pub(crate) fn acted_on(&self, target: &str) -> bool {
        self.state != "idle" && self.targets(target)
    }
}

/// The update fields of a `runtime.info` answer. An older runtime sends
/// neither.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UpdateInfo {
    pub(crate) capability: Option<String>,
    pub(crate) status: Option<UpdateStatus>,
}

impl UpdateInfo {
    pub(crate) fn from_runtime_info(info: &Value) -> Self {
        Self {
            capability: info["update_capability"]
                .as_str()
                .filter(|capability| ["launcher", "desktop", "manual"].contains(capability))
                .map(str::to_owned),
            status: UpdateStatus::from_body(&info["update_status"]),
        }
    }
}

/// Whether the app asks a runtime for `target` now: the runtime is behind,
/// installs releases itself, reports no pending or failed update for that
/// target, and the app has not asked it for that target in the last
/// [`REQUEST_BACKOFF`].
pub(crate) fn should_request(
    behind: bool,
    capability: Option<&str>,
    status: Option<&UpdateStatus>,
    target: &str,
    last_request: Option<&(String, Instant)>,
    now: Instant,
) -> bool {
    behind
        && capability != Some("manual")
        && !status.is_some_and(|status| status.pending_for(target) || status.failed_for(target))
        && !last_request.is_some_and(|(version, at)| {
            version == target && now.saturating_duration_since(*at) < REQUEST_BACKOFF
        })
}

/// The version this app updates itself to because a paired desktop runs a
/// newer app. A server or container announces core's version, which never
/// moves this app.
pub(crate) fn self_follow_version(
    kind: Option<&str>,
    version: Option<&str>,
    app_version: &str,
) -> Option<String> {
    let version = version?;
    (kind == Some("desktop") && is_older(app_version, version) == Some(true))
        .then(|| version.to_owned())
}

/// Why Update all leaves a machine alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkipReason {
    Offline,
    NoPermission,
    Manual,
    Current,
}

/// What Update all knows about one paired machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Candidate<'a> {
    pub(crate) id: &'a str,
    pub(crate) online: bool,
    pub(crate) behind: bool,
    pub(crate) may_update: bool,
    pub(crate) capability: Option<&'a str>,
}

/// Splits the paired machines into the ones Update all asks, in list order,
/// and the ones it skips with the reason. This computer is not a candidate:
/// the window updates it last.
pub(crate) fn plan_update_all<'a>(
    candidates: &[Candidate<'a>],
) -> (Vec<&'a str>, Vec<(&'a str, SkipReason)>) {
    let mut asked = Vec::new();
    let mut skipped = Vec::new();
    for candidate in candidates {
        let reason = if !candidate.online {
            Some(SkipReason::Offline)
        } else if !candidate.behind {
            Some(SkipReason::Current)
        } else if candidate.capability == Some("manual") {
            Some(SkipReason::Manual)
        } else if !candidate.may_update {
            Some(SkipReason::NoPermission)
        } else {
            None
        };
        match reason {
            Some(reason) => skipped.push((candidate.id, reason)),
            None => asked.push(candidate.id),
        }
    }
    (asked, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(state: &str, target: Option<&str>) -> UpdateStatus {
        UpdateStatus {
            state: state.into(),
            target_version: target.map(str::to_owned),
            blockers: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn a_status_body_needs_a_known_state() {
        let body = json!({
            "state": "draining",
            "target_version": "0.2.0",
            "blockers": ["active_run"],
            "updated_at": "2026-01-01T00:00:00Z"
        });
        assert_eq!(
            UpdateStatus::from_body(&body),
            Some(UpdateStatus {
                state: "draining".into(),
                target_version: Some("0.2.0".into()),
                blockers: vec!["active_run".into()],
                error: None,
            })
        );
        assert_eq!(
            UpdateStatus::from_body(&json!({"state": "exploding"})),
            None
        );
        assert_eq!(UpdateStatus::from_body(&Value::Null), None);
        assert_eq!(
            serde_json::to_value(UpdateStatus::from_body(&body).unwrap()).unwrap(),
            json!({"state": "draining", "targetVersion": "0.2.0", "blockers": ["active_run"]})
        );
    }

    #[test]
    fn runtime_info_without_update_fields_reads_as_unknown() {
        assert_eq!(
            UpdateInfo::from_runtime_info(&json!({"runtime_version": "1.0.0"})),
            UpdateInfo::default()
        );
        let info = UpdateInfo::from_runtime_info(&json!({
            "update_capability": "launcher",
            "update_status": {"state": "failed", "target_version": "0.2.0", "blockers": [], "error": "Did not start.", "updated_at": "x"}
        }));
        assert_eq!(info.capability.as_deref(), Some("launcher"));
        assert_eq!(
            info.status.unwrap().error.as_deref(),
            Some("Did not start.")
        );
    }

    #[test]
    fn a_request_backs_off_and_skips_pending_failed_and_manual_runtimes() {
        let now = Instant::now();
        let target = "1.15.0";
        assert!(should_request(
            true,
            Some("launcher"),
            None,
            target,
            None,
            now
        ));
        // An older runtime that reports no update fields is asked too.
        assert!(should_request(true, None, None, target, None, now));
        assert!(!should_request(false, None, None, target, None, now));
        assert!(!should_request(
            true,
            Some("manual"),
            None,
            target,
            None,
            now
        ));

        // The same target within ten minutes waits. A newer target does not.
        let recent = (target.to_owned(), now - Duration::from_secs(60));
        assert!(!should_request(
            true,
            None,
            None,
            target,
            Some(&recent),
            now
        ));
        assert!(should_request(
            true,
            None,
            None,
            "1.16.0",
            Some(&recent),
            now
        ));
        let old = (target.to_owned(), now - REQUEST_BACKOFF);
        assert!(should_request(true, None, None, target, Some(&old), now));

        // A pending status for the target waits. One for an older target does not.
        for state in PENDING {
            let pending = status(state, Some(target));
            assert!(!should_request(
                true,
                None,
                Some(&pending),
                target,
                None,
                now
            ));
        }
        let older = status("queued", Some("1.14.0"));
        assert!(should_request(true, None, Some(&older), target, None, now));
        let idle = status("idle", None);
        assert!(should_request(true, None, Some(&idle), target, None, now));
    }

    #[test]
    fn a_failed_target_is_not_requested_again_until_a_newer_one_exists() {
        let now = Instant::now();
        let failed = status("failed", Some("1.15.0"));
        assert!(!should_request(
            true,
            None,
            Some(&failed),
            "1.15.0",
            None,
            now
        ));
        assert!(!should_request(
            true,
            None,
            Some(&failed),
            "v1.15.0",
            None,
            now
        ));
        assert!(!should_request(
            true,
            None,
            Some(&failed),
            "1.14.2",
            None,
            now
        ));
        assert!(should_request(
            true,
            None,
            Some(&failed),
            "1.16.0",
            None,
            now
        ));
        assert!(failed.acted_on("1.15.0"));
        assert!(!failed.acted_on("1.16.0"));
        assert!(!status("idle", None).acted_on("1.15.0"));
        assert!(status("queued", Some("1.15.0")).acted_on("1.15.0"));
    }

    #[test]
    fn a_newer_desktop_peer_moves_this_app_and_a_newer_core_does_not() {
        assert_eq!(
            self_follow_version(Some("desktop"), Some("0.4.0"), "0.3.0"),
            Some("0.4.0".into())
        );
        assert_eq!(
            self_follow_version(Some("desktop"), Some("0.3.0"), "0.3.0"),
            None
        );
        assert_eq!(
            self_follow_version(Some("desktop"), Some("0.2.0"), "0.3.0"),
            None
        );
        assert_eq!(
            self_follow_version(Some("desktop"), Some("nightly"), "0.3.0"),
            None
        );
        assert_eq!(self_follow_version(Some("desktop"), None, "0.3.0"), None);
        // A server or container announces core's version, which runs ahead
        // of the app's.
        assert_eq!(
            self_follow_version(Some("container"), Some("1.15.0"), "0.3.0"),
            None
        );
        assert_eq!(
            self_follow_version(Some("server"), Some("1.15.0"), "0.3.0"),
            None
        );
        assert_eq!(self_follow_version(None, Some("1.15.0"), "0.3.0"), None);
    }

    #[test]
    fn update_all_asks_reachable_older_machines_and_names_each_skip() {
        let candidate = |id, online, behind, may_update, capability| Candidate {
            id,
            online,
            behind,
            may_update,
            capability,
        };
        let (asked, skipped) = plan_update_all(&[
            candidate("box", true, true, true, Some("launcher")),
            candidate("off", false, true, true, None),
            candidate("reader", true, true, false, None),
            candidate("hand", true, true, true, Some("manual")),
            candidate("same", true, false, true, Some("launcher")),
            candidate("old", true, true, true, None),
        ]);
        assert_eq!(asked, vec!["box", "old"]);
        assert_eq!(
            skipped,
            vec![
                ("off", SkipReason::Offline),
                ("reader", SkipReason::NoPermission),
                ("hand", SkipReason::Manual),
                ("same", SkipReason::Current),
            ]
        );
        assert_eq!(
            serde_json::to_value(SkipReason::NoPermission).unwrap(),
            "no_permission"
        );
    }
}
