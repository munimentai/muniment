//! The runtime's session relay publisher: the account and run operations
//! `muniment_core::relay_publisher` drives.

use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::Duration;

use muniment_core::active_run::{
    cancel_active_run, queue_message, queue_permission_answer_with_commit, ChatDelivery,
    ChatQueueRequest, RunRegistry,
};
use muniment_core::attach::{
    ErrorCode, ErrorDetails, Operation, ProtocolError, RuntimeActivityRegistry,
};
use muniment_core::journal::EventPayload;
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::relay_encryption::{GateDecision, RelayKeys};
use muniment_core::relay_publisher::{
    relay_state_path, RelayAccount, RelayConnection, RelayPublisher, RelayRuntime, RequestError,
};
use muniment_core::run_events::{ChatEventSubscription, SharedStorage};
use muniment_core::serde_json::{self, json, Value};
use muniment_core::swarm::{ClientEvent, FleetActor};

use crate::host::RuntimeHost;
use crate::{RuntimeAttachState, RuntimeChatEventBroadcast};

/// How long a phone's permission answer waits for its journal commit.
const ANSWER_COMMIT_TIMEOUT: Duration = Duration::from_secs(2);

struct Account {
    host: RuntimeHost,
    runtime_activity: RuntimeActivityRegistry,
    profile_directory: PathBuf,
}

impl RelayAccount for Account {
    fn keys(&self) -> Option<RelayKeys> {
        self.host.account.relay_keys(&self.profile_directory)
    }

    fn connection(&self, pair_id: &str) -> Option<RelayConnection> {
        let api_base = self.host.account.cloud_api_base()?;
        let access_token = self
            .host
            .account
            .session(&self.runtime_activity)
            .ok()?
            .tokens?
            .access_token;
        if access_token.is_empty() {
            return None;
        }
        let status = self
            .host
            .account
            .pairing_status(&access_token, &self.profile_directory)
            .ok()?;
        (status.pair?.pair_id.to_string() == pair_id).then_some(RelayConnection {
            api_base,
            access_token,
        })
    }
}

/// The runtime's runs, its swarm and the state the phone's requests use.
pub(crate) struct Runs {
    pub(crate) active: RunRegistry,
    pub(crate) chat_events: RuntimeChatEventBroadcast,
    pub(crate) storage: SharedStorage,
    pub(crate) swarm: Option<muniment_core::swarm::Swarm>,
    /// The runtime state the phone's control session requests run through.
    pub(crate) state: Option<Weak<RuntimeAttachState>>,
}

/// A request's failure as the phone reads it.
fn request_error(error: &ProtocolError) -> RequestError {
    let code = match error.code() {
        ErrorCode::UnsupportedOperation => "unsupported_operation",
        ErrorCode::Unauthorized | ErrorCode::AuthorizationFailed => "unauthorized",
        ErrorCode::InvalidRequest | ErrorCode::InvalidCursor => "invalid_request",
        ErrorCode::RuntimeDraining | ErrorCode::MigrationNotReady | ErrorCode::DesktopBusy => {
            "unavailable"
        }
        _ => "failed",
    };
    let message = match error.details() {
        Some(ErrorDetails::RequestReason { reason }) => reason.clone(),
        _ => serde_json::to_value(error.message())
            .ok()
            .and_then(|message| message.as_str().map(str::to_owned))
            .unwrap_or_else(|| "The action failed.".into()),
    };
    RequestError::new(code, message)
}

impl Runs {
    /// Sends the phone's action on a run another swarm member runs to that
    /// member, the way any client's action reaches it. Answers whether it
    /// went.
    fn forward(&self, operation: Operation, body: Value) -> Result<bool, String> {
        if body["run_id"]
            .as_str()
            .is_some_and(|run_id| self.active.contains(run_id))
        {
            return Ok(false);
        }
        let Some(swarm) = &self.swarm else {
            return Ok(false);
        };
        Ok(swarm
            .route(operation, &body, &FleetActor::phone(), None)?
            .is_some())
    }
}

impl RelayRuntime for Runs {
    /// This runtime's run events, and the approvals other swarm members hold.
    fn subscribe(&self) -> Option<ChatEventSubscription> {
        let local = self.chat_events.subscribe();
        let Some(swarm) = &self.swarm else {
            return Some(local);
        };
        let remote = swarm.phone_events();
        let (sender, receiver) = std::sync::mpsc::channel();
        let open = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let running = Arc::clone(&open);
        std::thread::spawn(move || {
            while running.load(std::sync::atomic::Ordering::SeqCst) {
                let event = match local.recv_timeout(Duration::from_millis(200)) {
                    Ok(event) => Some(event),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                };
                let delivered = event
                    .into_iter()
                    .chain(remote.try_iter())
                    .all(|event| sender.send(event).is_ok());
                if !delivered {
                    return;
                }
            }
        });
        Some(ChatEventSubscription::new(receiver, move || {
            open.store(false, std::sync::atomic::Ordering::SeqCst);
        }))
    }

    fn stop(&self, run_id: &str) -> Result<(), String> {
        if self.forward(Operation::RunCancel, json!({"run_id": run_id}))? {
            return Ok(());
        }
        cancel_active_run(&self.active, run_id, None)
    }

    fn queue(&self, run_id: &str, delivery: ChatDelivery, text: &str) -> Result<(), String> {
        let operation = match delivery {
            ChatDelivery::Steer => Operation::RunSteer,
            ChatDelivery::FollowUp => Operation::RunFollowUp,
        };
        if self.forward(operation, json!({"run_id": run_id, "text": text}))? {
            return Ok(());
        }
        queue_message(
            &self.active,
            ChatQueueRequest {
                run_id: run_id.to_owned(),
                workspace: None,
                delivery,
                message: text.to_owned(),
            },
        )
    }

    fn answer(
        &self,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<(), String> {
        // A gate another swarm member holds takes the answer through the swarm.
        if !self.active.contains(run_id) {
            if let Some(swarm) = &self.swarm {
                let result = swarm.answer_gate(run_id, gate_id, &answer)?;
                return match result["status"].as_str() {
                    Some("answered") | Some("sent") => Ok(()),
                    _ => Err("The approval was already answered.".into()),
                };
            }
        }
        queue_permission_answer_with_commit(
            &self.active,
            None,
            run_id.to_owned(),
            gate_id.to_owned(),
            answer,
        )?
        .recv_timeout(ANSWER_COMMIT_TIMEOUT)
        .map_err(|_| "The permission answer did not commit in time.".to_string())?
        .map(|_| ())
        .ok_or_else(|| "The permission answer was rejected.".to_string())
    }

    fn serves_control(&self) -> bool {
        self.state.is_some()
    }

    fn request(
        &self,
        operation: Operation,
        body: Value,
        request_id: &str,
    ) -> Result<Value, RequestError> {
        let state = self
            .state
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or_else(|| RequestError::new("unavailable", "This machine is stopping."))?;
        state
            .phone_request(operation, body, request_id)
            .map_err(|error| request_error(&error))
    }

    fn fleet_events(&self) -> Option<std::sync::mpsc::Receiver<ClientEvent>> {
        self.swarm
            .as_ref()
            .map(muniment_core::swarm::Swarm::subscribe)
    }

    /// Reads the decision the journal recorded for a gate the desktop answered.
    fn gate_decision(&self, run_id: &str, gate_id: &str) -> Option<GateDecision> {
        let events = self.storage.lock().ok()?.journal.events(run_id).ok()?;
        events.iter().rev().find_map(|event| {
            let EventPayload::Inline { payload_json } = &event.payload else {
                return None;
            };
            if event.event_type != "permission.resolved" || payload_json["gate_id"] != gate_id {
                return None;
            }
            let answer =
                serde_json::from_value::<ChatPermissionAnswer>(payload_json["decision"].clone())
                    .ok()?;
            Some(match answer {
                ChatPermissionAnswer::Cancelled | ChatPermissionAnswer::Confirm(false) => {
                    GateDecision::Deny
                }
                _ => GateDecision::Allow,
            })
        })
    }
}

/// Starts the relay publisher for a host with a cloud. A host without one
/// gets none.
pub(crate) fn start(
    host: &RuntimeHost,
    runtime_activity: RuntimeActivityRegistry,
    profile_directory: PathBuf,
    runs: Runs,
) -> Option<RelayPublisher> {
    host.account.cloud_api_base()?;
    let state_path = relay_state_path(&profile_directory);
    Some(RelayPublisher::start(
        Arc::new(Account {
            host: host.clone(),
            runtime_activity,
            profile_directory,
        }),
        Arc::new(runs),
        state_path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_without_a_cloud_starts_no_publisher() {
        let profile = std::env::temp_dir().join(format!(
            "muniment-relay-host-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let storage = crate::open_profile_storage(&profile).unwrap();
        let publisher = start(
            &RuntimeHost::default(),
            RuntimeActivityRegistry::default(),
            profile.clone(),
            Runs {
                active: RunRegistry::default(),
                chat_events: RuntimeChatEventBroadcast::default(),
                storage,
                swarm: None,
                state: None,
            },
        );
        assert!(publisher.is_none());
        std::fs::remove_dir_all(profile).unwrap();
    }
}
