//! Shares this computer's model connections with its paired machines.
//!
//! While sharing is on, the app reads this computer's bundle from its own
//! runtime and sends it to every paired machine that lets this device start
//! runs, at once, whenever a machine connects, and every five minutes. The
//! runtime refreshes a rotating sign-in before it leaves, so each machine
//! holds a token with at least half an hour left. Turning sharing off sends
//! each machine an empty bundle, which removes what this computer shared.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::Manager;

use super::connection::RuntimeStatus;
use super::{connector, emit_changed, PairedRuntime, Runtimes};

const INTERVAL: Duration = Duration::from_secs(5 * 60);

/// How the last share went on one paired machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShareState {
    Shared,
    /// The pairing does not let this device start runs there.
    NoAccess,
    Failed,
}

#[derive(Default)]
pub(crate) struct Sharing {
    wake: Mutex<bool>,
    woken: Condvar,
    states: Mutex<HashMap<String, ShareState>>,
}

impl Sharing {
    pub(crate) fn state(&self, id: &str) -> Option<ShareState> {
        self.states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .copied()
    }

    /// Asks the loop to share now.
    pub(crate) fn wake(&self) {
        *self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.woken.notify_all();
    }

    fn wait(&self) {
        let held = self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut held, _) = self
            .woken
            .wait_timeout_while(held, INTERVAL, |woken| !*woken)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *held = false;
    }

    /// Records the states and answers whether any changed.
    fn record(&self, next: HashMap<String, ShareState>) -> bool {
        let mut states = self
            .states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = *states != next;
        *states = next;
        changed
    }
}

/// Starts the loop that shares this computer's connections.
pub(crate) fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        share(&app);
        app.state::<Runtimes>().sharing.wait();
    });
}

/// Whether the pairing lets this device share there.
fn may_share(runtime: &PairedRuntime) -> bool {
    runtime.scopes.contains(muniment_core::attach::SCOPE_RUN_WRITE)
        || runtime
            .scopes
            .contains(muniment_core::attach::SCOPE_RUNTIME_ADMIN)
}

fn share<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let runtimes = app.state::<Runtimes>();
    let Ok(registry) = runtimes.registry() else {
        return;
    };
    let paired = registry.list();
    let mut states = HashMap::new();
    if !registry.share_models() {
        // Clear every machine that still holds what this computer shared.
        let held = registry.shared_with();
        for id in &held {
            match paired.iter().find(|runtime| &runtime.id == id) {
                Some(runtime) if may_share(runtime) => {
                    if send(app, registry, id, &json!({})).is_ok() {
                        let _ = registry.mark_shared(id, false);
                    }
                }
                // A removed machine dropped the connections when this device
                // revoked itself there.
                _ => {
                    let _ = registry.mark_shared(id, false);
                }
            }
        }
    } else {
        let bundle = crate::model_service::connect()
            .ok()
            .and_then(|mut client| client.shared_models_bundle().ok());
        for runtime in &paired {
            let state = match &bundle {
                _ if !may_share(runtime) => ShareState::NoAccess,
                None => ShareState::Failed,
                Some(bundle) => match send(app, registry, &runtime.id, bundle) {
                    Ok(()) => {
                        let _ = registry.mark_shared(&runtime.id, true);
                        ShareState::Shared
                    }
                    Err(_) => ShareState::Failed,
                },
            };
            states.insert(runtime.id.clone(), state);
        }
    }
    if runtimes.sharing.record(states) {
        emit_changed(app);
    }
}

fn send<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    registry: &super::Registry,
    id: &str,
    bundle: &Value,
) -> Result<(), RuntimeStatus> {
    let connector = connector(app, registry, id);
    let answer = connector.connect().and_then(|mut client| {
        client
            .share_models(bundle.clone())
            .map(|_| ())
            .map_err(|_| RuntimeStatus::Offline)
    });
    connector.control.stop();
    answer
}

/// Turns sharing on or off and shares at once.
#[tauri::command]
pub async fn runtime_share_models(
    app: tauri::AppHandle,
    share: bool,
) -> Result<super::RuntimeList, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let runtimes = app.state::<Runtimes>();
        runtimes
            .registry()?
            .set_share_models(share)
            .map_err(|_| "The sharing setting could not be saved.".to_owned())?;
        runtimes.sharing.wake();
        emit_changed(&app);
        Ok(super::list(&app))
    })
    .await
    .map_err(|_| "The sharing setting could not be saved.".to_owned())?
}
