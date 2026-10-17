//! Shares this computer's model connections with its paired machines.
//!
//! While sharing is on, the app shares at once, whenever a machine connects,
//! and every five minutes, with every paired machine that lets this device
//! start runs. Through the pairing link, the app reads this computer's bundle
//! from its own runtime and sends it to each machine. The runtime refreshes a
//! rotating sign-in before it leaves, so each machine holds a token with at
//! least half an hour left. With MuniCloud, the runtime uploads the bundle to
//! the account's model vault instead, and each machine gets a machine token
//! and reads the vault's leases itself. Turning sharing off sends each
//! machine an empty bundle, and turning MuniCloud off takes the connections
//! back from the vault.
//!
//! Runtimes linked into a fleet also share their connections as sealed
//! secrets, which the runtime turns on with `fleet.credentials`. A machine
//! that receives one connection both ways keeps the copy that came first.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::Manager;

use super::connection::RuntimeStatus;
use super::{connector, emit_changed, PairedRuntime, Registry, Runtimes};

const INTERVAL: Duration = Duration::from_secs(5 * 60);

/// How the last share went on one paired machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShareState {
    Shared,
    /// The pairing does not let this device start runs there.
    NoAccess,
    /// The account shares the vault with as many machines as it may.
    Limit,
    Failed,
}

#[derive(Default)]
pub(crate) struct Sharing {
    wake: Mutex<bool>,
    woken: Condvar,
    states: Mutex<(HashMap<String, ShareState>, Option<String>)>,
}

impl Sharing {
    pub(crate) fn state(&self, id: &str) -> Option<ShareState> {
        self.held().0.get(id).copied()
    }

    /// Why the last share could not reach the model vault.
    pub(crate) fn error(&self) -> Option<String> {
        self.held().1.clone()
    }

    fn held(&self) -> std::sync::MutexGuard<'_, (HashMap<String, ShareState>, Option<String>)> {
        self.states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    fn record(&self, next: HashMap<String, ShareState>, error: Option<String>) -> bool {
        let mut held = self.held();
        let changed = held.0 != next || held.1 != error;
        *held = (next, error);
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
    let sharing = registry.share_models();
    let cloud = sharing && registry.share_via_cloud();
    let mut error = None;
    // MuniCloud off: take the connections back before sharing another way.
    if !cloud && registry.vault_held() {
        match vault(json!({"action": "release"})) {
            Ok(_) => {
                let _ = registry.set_vault_held(false);
                let _ = registry.clear_vault_tokens();
            }
            Err(message) => error = Some(message),
        }
    }
    let mut states = HashMap::new();
    if !sharing {
        clear(app, registry, &paired);
    } else if cloud {
        match vault(json!({"action": "upload"})) {
            Ok(_) => {
                let _ = registry.set_vault_held(true);
                for runtime in &paired {
                    let state = if may_share(runtime) {
                        share_vault(app, registry, runtime)
                    } else {
                        ShareState::NoAccess
                    };
                    states.insert(runtime.id.clone(), state);
                }
            }
            Err(message) => {
                error = Some(message);
                for runtime in &paired {
                    states.insert(runtime.id.clone(), ShareState::Failed);
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
    if runtimes.sharing.record(states, error) {
        emit_changed(app);
    }
}

/// Clears every machine that still holds what this computer shared.
fn clear<R: tauri::Runtime>(app: &tauri::AppHandle<R>, registry: &Registry, paired: &[PairedRuntime]) {
    for id in &registry.shared_with() {
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
}

/// Points one machine at the model vault with its own machine token.
fn share_vault<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    registry: &Registry,
    runtime: &PairedRuntime,
) -> ShareState {
    let link = match registry.vault_token(&runtime.id) {
        Some(link) => link,
        None => {
            let Some(machine) = machine(registry, &runtime.id) else {
                return ShareState::Failed;
            };
            match vault(json!({"action": "machine_token", "machine": machine})) {
                Ok(answer) => {
                    let link = answer["cloud"].clone();
                    let _ = registry.set_vault_token(&runtime.id, Some(link.clone()));
                    link
                }
                Err(message) if message.contains("pairing limit") => return ShareState::Limit,
                Err(_) => return ShareState::Failed,
            }
        }
    };
    match send(app, registry, &runtime.id, &json!({"cloud": link})) {
        Ok(()) => {
            let _ = registry.mark_shared(&runtime.id, true);
            ShareState::Shared
        }
        Err(_) => ShareState::Failed,
    }
}

/// The machine as the model vault's machine token request names it.
fn machine(registry: &Registry, id: &str) -> Option<Value> {
    let record = registry.record(id).ok()?;
    let endpoints: Vec<Value> = record
        .endpoints
        .iter()
        .filter_map(|endpoint| {
            let (host, port) = endpoint.rsplit_once(':')?;
            let host = host.trim_start_matches('[').trim_end_matches(']');
            Some(json!({"host": host, "port": port.parse::<u16>().ok()?}))
        })
        .collect();
    Some(json!({
        "runtime_id": record.runtime_id.as_str(),
        "runtime_name": record.runtime_name,
        "certificate_sha256": record.fingerprint.to_hex(),
        "endpoints": endpoints,
    }))
}

/// Runs one model vault action on this computer's runtime.
fn vault(body: Value) -> Result<Value, String> {
    let mut client = crate::model_service::connect()
        .map_err(|_| "This computer's runtime is not running.".to_owned())?;
    client.model_vault(body).map_err(|error| {
        crate::auth::desktop_request_error(error, client.last_request_error())
    })
}

fn send<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    registry: &Registry,
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

/// Turns sharing on or off, and MuniCloud with it when `cloud` names it, then
/// shares at once.
#[tauri::command]
pub async fn runtime_share_models(
    app: tauri::AppHandle,
    share: bool,
    cloud: Option<bool>,
) -> Result<super::RuntimeList, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let runtimes = app.state::<Runtimes>();
        let registry = runtimes.registry()?;
        let saved = registry.set_share_models(share).and_then(|()| match cloud {
            Some(cloud) => registry.set_share_via_cloud(cloud),
            None => Ok(()),
        });
        saved.map_err(|_| "The sharing setting could not be saved.".to_owned())?;
        runtimes.sharing.wake();
        emit_changed(&app);
        Ok(super::list(&app))
    })
    .await
    .map_err(|_| "The sharing setting could not be saved.".to_owned())?
}
