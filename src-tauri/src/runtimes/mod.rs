//! The runtimes the app can attach to: this computer's own runtime and the
//! paired runtimes on other desktops, servers and containers. The app
//! attaches to one selected runtime at a time, and the attach service sends
//! every runtime request to it.

mod commands;
mod connection;
mod local_access;
mod registry;
mod requested_update;
mod ssh;
mod version;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::attach_service::AttachCompanionState;
pub(crate) use commands::*;
pub(crate) use connection::{Connector, RemoteSupervisor};
use connection::{Observation, RemoteControl, RuntimeStatus};
pub(crate) use local_access::*;
use registry::{PairedRuntime, Registry, LOCAL_RUNTIME_ID};
pub(crate) use requested_update::update_requested;
use version::{update_offered, APP_VERSION};

/// What a runtime answered last, kept for the runtime list.
#[derive(Clone, Debug)]
struct Observed {
    status: RuntimeStatus,
    version: Option<String>,
    install_kind: Option<String>,
}

impl Default for Observed {
    fn default() -> Self {
        Self {
            status: RuntimeStatus::Unknown,
            version: None,
            install_kind: None,
        }
    }
}

/// The app's runtimes: the registry on disk and what each runtime answered.
pub(crate) struct Runtimes {
    registry: Option<Registry>,
    observed: Mutex<HashMap<String, Observed>>,
    /// One pairing runs at a time.
    pairing: Mutex<()>,
    /// The runtimes asked to update to this app's version since it started.
    /// The app asks each one once, on the first welcome that names an older
    /// version.
    auto_updates: Mutex<HashSet<String>>,
}

impl Runtimes {
    fn new(registry: Option<Registry>) -> Self {
        Self {
            registry,
            observed: Mutex::new(HashMap::new()),
            pairing: Mutex::new(()),
            auto_updates: Mutex::new(HashSet::new()),
        }
    }

    fn registry(&self) -> Result<&Registry, String> {
        self.registry
            .as_ref()
            .ok_or_else(|| "The app's state folder is unavailable.".to_owned())
    }

    fn observed(&self, id: &str) -> Observed {
        self.observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    /// Whether `id` should be asked to update now: it is paired, it runs an
    /// older version than this app, this device may ask, and the app has not
    /// asked it yet. A yes is recorded, so each runtime is asked once.
    fn claim_auto_update(&self, id: &str) -> bool {
        if id == LOCAL_RUNTIME_ID {
            return false;
        }
        let Some(registry) = &self.registry else {
            return false;
        };
        let Ok(record) = registry.record(id) else {
            return false;
        };
        let version = self
            .observed(id)
            .version
            .or_else(|| registry.entry(id).and_then(|entry| entry.version));
        if !update_offered(version.as_deref(), &record.scopes, APP_VERSION) {
            return false;
        }
        self.auto_updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_owned())
    }

    /// Records an observation and answers whether the list changed.
    fn record(&self, id: &str, observation: Observation) -> bool {
        let mut observed = self
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = observed.entry(id.to_owned()).or_default();
        match observation {
            Observation::Status(status) => {
                let changed = entry.status != status;
                entry.status = status;
                changed
            }
            Observation::Welcome {
                version,
                install_kind,
            } => {
                let changed = entry.version != version || entry.install_kind != install_kind;
                entry.version = version.clone();
                entry.install_kind = install_kind.clone();
                drop(observed);
                if changed {
                    if let Some(registry) = &self.registry {
                        let _ = registry.observe(id, version, install_kind);
                    }
                }
                changed
            }
        }
    }
}

/// One runtime as the Runtimes page and the switcher show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeView {
    id: String,
    name: String,
    /// `local` for this computer, else the install kind the runtime named.
    kind: Option<String>,
    version: Option<String>,
    status: RuntimeStatus,
    active: bool,
    local: bool,
    scopes: Vec<String>,
    /// The device holds `runtime.admin` there.
    admin: bool,
    update_offered: bool,
    ssh_host: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeList {
    app_version: &'static str,
    device_name: String,
    selected: String,
    runtimes: Vec<RuntimeView>,
    /// One line about the command that answered this list, such as a removed
    /// runtime that still lists this device.
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<String>,
}

fn local_view(attach: &AttachCompanionState, active: bool) -> RuntimeView {
    let version = attach.local_runtime_version();
    RuntimeView {
        id: LOCAL_RUNTIME_ID.into(),
        name: "This computer".into(),
        kind: Some("local".into()),
        status: if version.is_some() {
            RuntimeStatus::Online
        } else {
            RuntimeStatus::Offline
        },
        version,
        active,
        local: true,
        scopes: Vec::new(),
        admin: true,
        update_offered: false,
        ssh_host: None,
    }
}

fn paired_view(runtime: PairedRuntime, observed: Observed, active: bool) -> RuntimeView {
    let version = observed.version.or(runtime.entry.version);
    let admin = runtime
        .scopes
        .contains(muniment_core::attach::SCOPE_RUNTIME_ADMIN);
    RuntimeView {
        update_offered: update_offered(version.as_deref(), &runtime.scopes, APP_VERSION),
        id: runtime.id,
        name: runtime.entry.name,
        kind: observed.install_kind.or(runtime.entry.install_kind),
        version,
        status: observed.status,
        active,
        local: false,
        scopes: runtime.scopes.into_iter().collect(),
        admin,
        ssh_host: runtime.entry.ssh.map(|ssh| ssh.host),
    }
}

/// Every runtime, this computer first, with the selected one marked.
pub(crate) fn list<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> RuntimeList {
    let attach = app.state::<AttachCompanionState>();
    let runtimes = app.state::<Runtimes>();
    let active = attach.remote_runtime_id();
    let mut views = vec![local_view(&attach, active.is_none())];
    if let Some(registry) = &runtimes.registry {
        views.extend(registry.list().into_iter().map(|runtime| {
            let observed = runtimes.observed(&runtime.id);
            let selected = active.as_deref() == Some(runtime.id.as_str());
            paired_view(runtime, observed, selected)
        }));
    }
    RuntimeList {
        app_version: APP_VERSION,
        device_name: device_name(),
        selected: active.unwrap_or_else(|| LOCAL_RUNTIME_ID.into()),
        runtimes: views,
        notice: None,
    }
}

pub(crate) fn emit_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let _ = app.emit("runtimes-changed", list(app));
}

fn observer<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> connection::Observer {
    let app = app.clone();
    Arc::new(move |id: &str, observation: Observation| {
        let welcome = matches!(observation, Observation::Welcome { .. });
        let runtimes = app.state::<Runtimes>();
        if runtimes.record(id, observation) {
            emit_changed(&app);
        }
        // Updates are automatic: a runtime that answers with an older version
        // is asked once to install this app's version.
        if welcome && runtimes.claim_auto_update(id) {
            let app = app.clone();
            let id = id.to_owned();
            std::thread::spawn(move || {
                let result = update_one(&app, &id);
                if !result.ok {
                    eprintln!(
                        "muniment-desktop: automatic runtime update not requested runtime={id} reason={}",
                        result.message
                    );
                }
                emit_changed(&app);
            });
        }
    })
}

fn connector<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    registry: &Registry,
    id: &str,
) -> Connector {
    Connector {
        runtime_id: id.to_owned(),
        registry: registry.clone(),
        ssh: registry.entry(id).and_then(|entry| entry.ssh),
        control: Arc::new(RemoteControl::default()),
        observe: observer(app),
    }
}

/// Starts connections to the paired runtime `id` unless it has them. Its
/// chat events reach the window while it is the active runtime.
fn connect<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) {
    let attach = app.state::<AttachCompanionState>();
    if attach.has_remote(id) {
        return;
    }
    let runtimes = app.state::<Runtimes>();
    let Ok(registry) = runtimes.registry() else {
        return;
    };
    let event_app = app.clone();
    let event_runtime = id.to_owned();
    let status_app = app.clone();
    let supervisor = RemoteSupervisor::start(
        connector(app, registry, id),
        move |event| {
            let active = event_app
                .state::<AttachCompanionState>()
                .remote_runtime_id();
            if active.as_deref() == Some(event_runtime.as_str()) {
                crate::attach_service::deliver_subscription_event(&event_app, event);
            }
        },
        move |_| emit_changed(&status_app),
    );
    if let Some(previous) = attach.insert_remote(supervisor) {
        previous.stop();
    }
}

/// Connects every paired runtime, so the thread list holds each one's
/// threads and a thread on any of them opens at once.
pub(crate) fn connect_all<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let runtimes = app.state::<Runtimes>();
    let Ok(registry) = runtimes.registry() else {
        return;
    };
    for runtime in registry.list() {
        connect(app, &runtime.id);
    }
}

/// Restarts the paired runtime `id`'s connections after its record changed,
/// keeping it active when it was.
fn reconnect<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) {
    let attach = app.state::<AttachCompanionState>();
    let was_active = attach.remote_runtime_id().as_deref() == Some(id);
    if let Some(previous) = attach.remove_remote(id) {
        previous.stop();
    }
    connect(app, id);
    if was_active {
        attach.set_active(Some(id.to_owned()));
    }
}

/// Sends requests to `selected`, a paired runtime id, or to this computer's
/// runtime for `None`, for a new thread there. Every runtime stays connected.
pub(crate) fn activate<R: tauri::Runtime>(app: &tauri::AppHandle<R>, selected: Option<&str>) {
    if let Some(id) = selected {
        connect(app, id);
    }
    app.state::<AttachCompanionState>()
        .set_active(selected.map(str::to_owned));
    // The next message starts a thread on the runtime the app now shows.
    if let Some(chat) = app.try_state::<crate::chat::ChatState>() {
        chat.session_thread.fresh(None);
    }
    emit_changed(app);
}

/// Sends requests to the runtime `thread_id` lives on, when the merged
/// thread list named it. A thread stays on the runtime it started on.
pub(crate) fn follow_thread<R: tauri::Runtime>(app: &tauri::AppHandle<R>, thread_id: &str) {
    let attach = app.state::<AttachCompanionState>();
    let Some(runtime) = attach.thread_runtime(thread_id) else {
        return;
    };
    let target = (runtime != LOCAL_RUNTIME_ID).then_some(runtime);
    if let Some(id) = &target {
        connect(app, id);
    }
    if attach.set_active(target) {
        emit_changed(app);
    }
}

/// Names the selected paired runtime in place of the background service when
/// a request could not reach it, with the state it answered last.
pub(crate) fn name_unreachable<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    error: String,
) -> String {
    if error != crate::auth::background_service_error() {
        return error;
    }
    let Some(id) = app.state::<AttachCompanionState>().remote_runtime_id() else {
        return error;
    };
    let runtimes = app.state::<Runtimes>();
    let name = runtimes
        .registry
        .as_ref()
        .and_then(|registry| registry.entry(&id))
        .map_or_else(|| "The selected runtime".to_owned(), |entry| entry.name);
    unreachable_message(&name, runtimes.observed(&id).status)
}

pub(crate) fn unreachable_message(name: &str, status: RuntimeStatus) -> String {
    match status {
        RuntimeStatus::CertificateChanged => {
            format!("{name} answered with a new certificate. Pair it again in Settings, Runtimes.")
        }
        RuntimeStatus::Revoked => {
            format!("{name} revoked this computer. Pair it again in Settings, Runtimes.")
        }
        RuntimeStatus::VersionMismatch => {
            format!("{name} runs an incompatible version. Update the app or the runtime.")
        }
        RuntimeStatus::RuntimeMismatch => {
            format!("A different runtime answers at {name}'s address. Pair it again.")
        }
        RuntimeStatus::Connecting | RuntimeStatus::Unknown => {
            format!("{name} has not answered yet. Wait a moment, then retry.")
        }
        RuntimeStatus::Offline | RuntimeStatus::Online => {
            format!("{name} is offline. Check that it is running and reachable.")
        }
    }
}

/// Manages the runtimes and attaches to the selected one. The caller has
/// started the local runtime's clients.
pub(crate) fn setup<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let registry = Registry::open_default().ok();
    let selected = registry.as_ref().and_then(Registry::selected);
    app.manage(Runtimes::new(registry));
    connect_all(app);
    if selected.is_some() {
        activate(app, selected.as_deref());
    }
    requested_update::check_saved_request(app);
}

/// Closes the selected runtime's connections and its SSH tunnel.
pub(crate) fn shutdown<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(attach) = app.try_state::<AttachCompanionState>() {
        for remote in attach.take_remotes() {
            remote.stop();
        }
    }
}

/// The name this computer offers a runtime when it pairs.
pub(crate) fn device_name() -> String {
    let name = host_name().unwrap_or_default();
    let name = name.trim().trim_end_matches(".local").trim();
    if muniment_core::attach::is_valid_display_name(name) {
        name.to_owned()
    } else {
        "Desktop".to_owned()
    }
}

#[cfg(unix)]
fn host_name() -> Option<String> {
    let mut buffer = [0_u8; 256];
    // SAFETY: the buffer outlives the call and its length is passed with it.
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0)?;
    String::from_utf8(buffer[..end].to_vec()).ok()
}

#[cfg(not(unix))]
fn host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::RuntimeEntry;
    use std::collections::BTreeSet;

    fn scopes(scopes: &[&str]) -> BTreeSet<String> {
        scopes.iter().map(|scope| (*scope).to_owned()).collect()
    }

    fn paired(scopes_held: &[&str], version: Option<&str>) -> PairedRuntime {
        PairedRuntime {
            id: "018f0000-0000-7000-8000-000000000042".into(),
            entry: RuntimeEntry {
                name: "Build box".into(),
                ssh: None,
                version: version.map(str::to_owned),
                install_kind: Some("server".into()),
            },
            scopes: scopes(scopes_held),
        }
    }

    #[test]
    fn a_paired_view_offers_an_update_only_to_an_admin_device_on_an_older_runtime() {
        let older = paired(&["thread.read", "runtime.admin"], Some("0.0.1"));
        let view = paired_view(older, Observed::default(), false);
        assert!(view.admin);
        assert!(view.update_offered);
        assert_eq!(view.kind.as_deref(), Some("server"));
        assert_eq!(view.status, RuntimeStatus::Unknown);

        let reader = paired(&["thread.read", "run.write"], Some("0.0.1"));
        let view = paired_view(reader, Observed::default(), true);
        assert!(!view.admin);
        assert!(!view.update_offered);
        assert!(view.active);

        // A newer welcome replaces the version the index kept.
        let current = paired(&["runtime.admin"], Some("0.0.1"));
        let observed = Observed {
            status: RuntimeStatus::Online,
            version: Some(APP_VERSION.into()),
            install_kind: Some("container".into()),
        };
        let view = paired_view(current, observed, false);
        assert!(!view.update_offered);
        assert_eq!(view.kind.as_deref(), Some("container"));
        assert_eq!(view.status, RuntimeStatus::Online);
    }

    #[test]
    fn observations_report_only_changes() {
        let runtimes = Runtimes::new(None);
        let id = "018f0000-0000-7000-8000-000000000042";
        assert!(runtimes.record(id, Observation::Status(RuntimeStatus::Connecting)));
        assert!(!runtimes.record(id, Observation::Status(RuntimeStatus::Connecting)));
        let welcome = Observation::Welcome {
            version: Some("1.0.0".into()),
            install_kind: Some("desktop".into()),
        };
        assert!(runtimes.record(id, welcome.clone()));
        assert!(!runtimes.record(id, welcome));
        assert_eq!(runtimes.observed(id).version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn an_unreachable_runtime_is_named_with_its_state() {
        assert_eq!(
            unreachable_message("Build box", RuntimeStatus::Offline),
            "Build box is offline. Check that it is running and reachable."
        );
        assert!(unreachable_message("Build box", RuntimeStatus::Revoked).contains("revoked"));
        assert!(
            unreachable_message("Build box", RuntimeStatus::Connecting).starts_with("Build box")
        );
    }

    #[test]
    fn the_device_name_is_a_valid_display_name() {
        assert!(muniment_core::attach::is_valid_display_name(&device_name()));
    }
}
