//! Keeps this computer's model connections in the account's model vault.
//!
//! Core shares the connections with the fleet as sealed secrets, which the
//! window turns on with `fleet.credentials`. With MuniCloud, the app uploads
//! the connections to the model vault at once and every five minutes, and
//! core issues each full fleet member its own machine token. Turning
//! MuniCloud off takes the connections back from the vault.
//!
//! A paired machine that still holds a bundle this computer sent over the
//! pairing link gets an empty one, so the fleet's sealed copy takes its
//! place. A sharing switch saved in the registry moves to
//! `fleet.credentials`.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use muniment_core::attach::Operation;
use serde_json::{json, Value};
use tauri::Manager;

use super::connection::RuntimeStatus;
use super::{connector, emit_changed, PairedRuntime, Registry, Runtimes};

const INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub(crate) struct Sharing {
    wake: Mutex<bool>,
    woken: Condvar,
    error: Mutex<Option<String>>,
}

impl Sharing {
    /// Why the last upload could not reach the model vault.
    pub(crate) fn error(&self) -> Option<String> {
        self.held().clone()
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Asks the loop to run now.
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

    /// Records the error and answers whether it changed.
    fn record(&self, error: Option<String>) -> bool {
        let mut held = self.held();
        let changed = *held != error;
        *held = error;
        changed
    }
}

/// Starts the loop that keeps the model vault in step.
pub(crate) fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        share(&app);
        app.state::<Runtimes>().sharing.wait();
    });
}

/// Whether the pairing lets this device send a bundle there.
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
    move_switch(registry);
    clear(app, registry, &registry.list());
    let cloud = registry.share_via_cloud();
    let mut error = None;
    if !cloud && registry.vault_held() {
        match vault(json!({"action": "release"})) {
            Ok(_) => {
                let _ = registry.set_vault_held(false);
                let _ = registry.clear_vault_tokens();
            }
            Err(message) => error = Some(message),
        }
    } else if cloud {
        match vault(json!({"action": "upload"})) {
            Ok(_) => {
                let _ = registry.set_vault_held(true);
            }
            Err(message) => error = Some(message),
        }
    }
    if runtimes.sharing.record(error) {
        emit_changed(app);
    }
}

/// Hands a sharing switch saved in the registry to this computer's runtime,
/// which shares the connections with the fleet.
fn move_switch(registry: &Registry) {
    if !registry.share_models() {
        return;
    }
    let moved = crate::model_service::connect().is_ok_and(|mut client| {
        client
            .swarm_request(Operation::FleetCredentials, json!({"share_models": true}))
            .is_ok()
    });
    if moved {
        let _ = registry.set_share_models(false);
    }
}

/// Clears every paired machine that still holds a bundle this computer sent
/// over the pairing link.
fn clear<R: tauri::Runtime>(app: &tauri::AppHandle<R>, registry: &Registry, paired: &[PairedRuntime]) {
    for id in &registry.shared_with() {
        match paired.iter().find(|runtime| &runtime.id == id) {
            Some(runtime) if may_share(runtime) => {
                if send(app, registry, id, &json!({})).is_ok() {
                    let _ = registry.mark_shared(id, false);
                    let _ = registry.set_vault_token(id, None);
                }
            }
            // A removed machine dropped the connections when this device
            // revoked itself there.
            _ => {
                let _ = registry.mark_shared(id, false);
                let _ = registry.set_vault_token(id, None);
            }
        }
    }
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

/// Keeps this computer's model connections in MuniCloud or takes them back,
/// then runs the loop at once.
#[tauri::command]
pub async fn runtime_share_models(
    app: tauri::AppHandle,
    cloud: bool,
) -> Result<super::RuntimeList, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let runtimes = app.state::<Runtimes>();
        let registry = runtimes.registry()?;
        registry
            .set_share_via_cloud(cloud)
            .map_err(|_| "The sharing setting could not be saved.".to_owned())?;
        runtimes.sharing.wake();
        emit_changed(&app);
        Ok(super::list(&app))
    })
    .await
    .map_err(|_| "The sharing setting could not be saved.".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vault_error_reports_only_changes() {
        let sharing = Sharing::default();
        assert!(!sharing.record(None));
        assert!(sharing.record(Some("The vault refused the upload.".into())));
        assert_eq!(
            sharing.error().as_deref(),
            Some("The vault refused the upload.")
        );
        assert!(!sharing.record(Some("The vault refused the upload.".into())));
        assert!(sharing.record(None));
        assert_eq!(sharing.error(), None);
    }
}
