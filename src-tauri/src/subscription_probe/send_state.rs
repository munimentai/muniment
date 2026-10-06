use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum InvokeState {
    NotStarted,
    Pending,
    Accepted,
    Rejected,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SendError {
    None,
    Busy,
    Unavailable,
    Unauthorized,
    Rejected,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SendSource {
    None,
    Unknown,
    LocalMode,
    Auth,
    Thread,
    Submit,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SendState {
    invoke: InvokeState,
    error: SendError,
    source: SendSource,
    draft_present: bool,
    send_present: bool,
    send_disabled: bool,
    stop_present: bool,
}

pub(super) fn record(
    root: &Path,
    turn: Option<usize>,
    state: &SendState,
) -> Result<(), &'static str> {
    if matches!(state.invoke, InvokeState::Rejected) == matches!(state.error, SendError::None)
        || matches!(state.invoke, InvokeState::Rejected) == matches!(state.source, SendSource::None)
        || (state.send_disabled && !state.send_present)
    {
        return Err("The probe send state is invalid.");
    }
    let turn = turn
        .filter(|turn| *turn < 4)
        .ok_or("The probe turn is invalid.")?;
    let plan: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("subscription-probe.json"))
            .map_err(|_| "The probe plan is missing.")?,
    )
    .map_err(|_| "The probe plan is invalid.")?;
    let requested = plan["models"][turn]["id"].as_str().unwrap_or("");
    if plan["phase"] != "chat"
        || requested.is_empty()
        || requested.len() > 128
        || !requested
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err("The probe send plan is invalid.");
    }
    let snapshot =
        serde_json::json!({ "phase": "chat", "turn": turn, "requested": requested, "send": state });
    std::fs::write(
        root.join("subscription-probe-send.json"),
        snapshot.to_string(),
    )
    .map_err(|_| "The probe could not save its send state.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_state_records_bounded_metadata_and_rejects_contradictions() {
        let root = std::env::temp_dir().join(format!(
            "muniment-send-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let plan = serde_json::json!({ "phase": "chat", "models": [{ "id": "sol" }] });
        std::fs::write(root.join("subscription-probe.json"), plan.to_string()).unwrap();
        let mut state = SendState {
            invoke: InvokeState::Rejected,
            error: SendError::Busy,
            source: SendSource::Submit,
            draft_present: true,
            send_present: true,
            send_disabled: false,
            stop_present: false,
        };
        record(&root, Some(0), &state).unwrap();
        let saved = std::fs::read_to_string(root.join("subscription-probe-send.json")).unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(snapshot["send"]["invoke"], "rejected");
        assert_eq!(snapshot["send"]["error"], "busy");
        assert_eq!(snapshot["send"]["source"], "submit");
        assert_eq!(snapshot["requested"], "sol");
        for turn in [None, Some(4), Some(usize::MAX)] {
            assert!(record(&root, turn, &state).is_err());
        }
        state.source = SendSource::None;
        assert!(record(&root, Some(0), &state).is_err());
        state.source = SendSource::Thread;
        state.error = SendError::None;
        assert!(record(&root, Some(0), &state).is_err());
        state.invoke = InvokeState::Accepted;
        assert!(record(&root, Some(0), &state).is_err());
        state.source = SendSource::None;
        state.send_present = false;
        state.send_disabled = true;
        assert!(record(&root, Some(0), &state).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("subscription-probe-send.json")).unwrap(),
            saved
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn send_state_rejects_unbounded_fields_and_wrong_types() {
        let valid = serde_json::json!({
            "invoke": "pending", "error": "none", "source": "none", "draftPresent": true,
            "sendPresent": false, "sendDisabled": false, "stopPresent": true,
        });
        assert!(serde_json::from_value::<SendState>(valid.clone()).is_ok());
        for (field, value) in [
            ("invoke", serde_json::json!("PRIVATE")),
            ("error", serde_json::json!("PRIVATE")),
            ("source", serde_json::json!("PRIVATE")),
            ("source", serde_json::json!(1)),
            ("draftPresent", serde_json::json!(1)),
            ("prompt", serde_json::json!("PRIVATE")),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<SendState>(invalid).is_err());
        }
    }
}
