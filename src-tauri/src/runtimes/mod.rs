//! The runtimes the app can attach to: this computer's own runtime and the
//! paired runtimes on other desktops, servers and containers. The app
//! attaches to one selected runtime at a time, and the attach service sends
//! every runtime request to it.

mod commands;
mod connection;
mod local_access;
mod registry;
mod requested_update;
mod sharing;
mod ssh;
mod update_state;
mod version;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::attach_service::AttachCompanionState;
pub(crate) use commands::*;
pub(crate) use connection::{Connector, RemoteSupervisor};
use connection::{Observation, RemoteControl, RuntimeStatus};
pub(crate) use local_access::*;
use registry::{PairedRuntime, Registry, LOCAL_RUNTIME_ID};
pub(crate) use requested_update::{check_saved_request, update_requested};
pub(crate) use sharing::*;
pub(crate) use update_state::UpdateStatus;
use update_state::{plan_update_all, self_follow_version, should_request, Candidate, SkipReason};
pub(crate) use version::is_older as is_older_version;
use version::{is_older, may_update, target_version, APP_VERSION};

/// What a runtime answered last, kept for the runtime list.
#[derive(Clone, Debug)]
struct Observed {
    status: RuntimeStatus,
    version: Option<String>,
    install_kind: Option<String>,
    form_factor: Option<String>,
    /// Who installs a requested version there: `launcher`, `desktop` or
    /// `manual`. An older runtime does not say.
    update_capability: Option<String>,
    /// The runtime's update state as it last reported it.
    update_status: Option<UpdateStatus>,
    /// The version this app asked the runtime for and the runtime has not
    /// acted on yet.
    requested: Option<String>,
}

impl Default for Observed {
    fn default() -> Self {
        Self {
            status: RuntimeStatus::Unknown,
            version: None,
            install_kind: None,
            form_factor: None,
            update_capability: None,
            update_status: None,
            requested: None,
        }
    }
}

/// The app's runtimes: the registry on disk and what each runtime answered.
pub(crate) struct Runtimes {
    registry: Option<Registry>,
    observed: Mutex<HashMap<String, Observed>>,
    /// One pairing runs at a time.
    pairing: Mutex<()>,
    /// The last version the app asked each runtime for, and when. The app
    /// asks again only after the backoff, so reconnects do not repeat it.
    auto_updates: Mutex<HashMap<String, (String, Instant)>>,
    /// The last version this app followed a newer paired desktop to, and
    /// when.
    followed: Mutex<Option<(String, Instant)>>,
    sharing: sharing::Sharing,
}

impl Runtimes {
    fn new(registry: Option<Registry>) -> Self {
        Self {
            registry,
            observed: Mutex::new(HashMap::new()),
            pairing: Mutex::new(()),
            auto_updates: Mutex::new(HashMap::new()),
            followed: Mutex::new(None),
            sharing: sharing::Sharing::default(),
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

    /// Whether `id` should be asked to update now: it is paired, this device
    /// may ask, and [`should_request`] agrees. A yes is recorded for the
    /// backoff.
    fn claim_auto_update(&self, id: &str, now: Instant) -> bool {
        if id == LOCAL_RUNTIME_ID {
            return false;
        }
        let Some(registry) = &self.registry else {
            return false;
        };
        let Ok(record) = registry.record(id) else {
            return false;
        };
        let observed = self.observed(id);
        let entry = registry.entry(id);
        let version = observed
            .version
            .clone()
            .or_else(|| entry.as_ref().and_then(|entry| entry.version.clone()));
        let kind = observed
            .install_kind
            .clone()
            .or_else(|| entry.and_then(|entry| entry.install_kind));
        let Some(target) = target_version(kind.as_deref()) else {
            return false;
        };
        let behind = version
            .as_deref()
            .and_then(|version| is_older(version, target))
            == Some(true);
        let mut requests = self
            .auto_updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !may_update(&record.scopes)
            || !should_request(
                behind,
                observed.update_capability.as_deref(),
                observed.update_status.as_ref(),
                target,
                requests.get(id),
                now,
            )
        {
            return false;
        }
        requests.insert(id.to_owned(), (target.to_owned(), now));
        true
    }

    /// Records that the app asked `id` for `version`, until the runtime acts.
    fn mark_requested(&self, id: &str, version: &str) {
        let mut observed = self
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = observed.entry(id.to_owned()).or_default();
        if !entry
            .update_status
            .as_ref()
            .is_some_and(|status| status.acted_on(version))
        {
            entry.requested = Some(version.to_owned());
        }
    }

    /// The version this app should follow a paired desktop to now, once per
    /// version within the backoff.
    fn claim_self_follow(
        &self,
        kind: Option<&str>,
        version: Option<&str>,
        now: Instant,
    ) -> Option<String> {
        let version = self_follow_version(kind, version, APP_VERSION)?;
        let mut followed = self
            .followed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((last, at)) = followed.as_ref() {
            let newer = is_older(last, &version) == Some(true);
            if !newer && now.saturating_duration_since(*at) < update_state::REQUEST_BACKOFF {
                return None;
            }
        }
        *followed = Some((version.clone(), now));
        Some(version)
    }

    /// The newest app version a paired desktop runs, when it is newer than
    /// this app's.
    fn newest_desktop_peer(&self) -> Option<String> {
        let observed = self
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        observed
            .values()
            .filter_map(|observed| {
                self_follow_version(
                    observed.install_kind.as_deref(),
                    observed.version.as_deref(),
                    APP_VERSION,
                )
            })
            .reduce(|newest, version| {
                if is_older(&newest, &version) == Some(true) {
                    version
                } else {
                    newest
                }
            })
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
            Observation::Update(info) => {
                let changed = entry.update_capability != info.capability
                    || entry.update_status != info.status;
                entry.update_capability = info.capability;
                entry.update_status = info.status;
                entry.settle_request();
                changed
            }
            Observation::UpdateStatus(status) => {
                let changed = entry.update_status.as_ref() != Some(&status);
                entry.update_status = Some(status);
                entry.settle_request();
                changed
            }
            Observation::Welcome {
                version,
                install_kind,
                form_factor,
            } => {
                let changed = entry.version != version
                    || entry.install_kind != install_kind
                    || entry.form_factor != form_factor;
                entry.version = version.clone();
                entry.settle_request();
                entry.install_kind = install_kind.clone();
                entry.form_factor = form_factor.clone();
                drop(observed);
                if changed {
                    if let Some(registry) = &self.registry {
                        let _ = registry.observe(id, version, install_kind, form_factor);
                    }
                }
                changed
            }
        }
    }
}

impl Observed {
    /// Forgets the app's request once the runtime acted on it or runs that
    /// version.
    fn settle_request(&mut self) {
        let Some(requested) = self.requested.as_deref() else {
            return;
        };
        let reached = self
            .version
            .as_deref()
            .is_some_and(|version| is_older(version, requested) == Some(false));
        let acted = self
            .update_status
            .as_ref()
            .is_some_and(|status| status.acted_on(requested));
        if reached || acted {
            self.requested = None;
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
    /// `laptop`, `all_in_one`, `desktop` or `server`, when the runtime says.
    #[serde(skip_serializing_if = "Option::is_none")]
    form_factor: Option<String>,
    version: Option<String>,
    status: RuntimeStatus,
    active: bool,
    local: bool,
    scopes: Vec<String>,
    /// The device holds `runtime.admin` there.
    admin: bool,
    /// The runtime runs an older version than the one this app brings it to.
    behind: bool,
    /// This device holds the update or the admin scope there.
    may_update: bool,
    /// The runtime is behind, reachable for a request, and installs releases
    /// itself.
    update_offered: bool,
    /// The version the runtime is behind.
    #[serde(skip_serializing_if = "Option::is_none")]
    update_target: Option<String>,
    /// Who installs a requested version there, when the runtime says.
    #[serde(skip_serializing_if = "Option::is_none")]
    update_capability: Option<String>,
    /// The runtime's update state, when the runtime reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    update_status: Option<UpdateStatus>,
    /// This app asked for an update the runtime has not acted on yet.
    update_requested: bool,
    ssh_host: Option<String>,
    /// How the last share of this computer's model connections went there.
    #[serde(skip_serializing_if = "Option::is_none")]
    sharing: Option<sharing::ShareState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeList {
    app_version: &'static str,
    device_name: String,
    selected: String,
    runtimes: Vec<RuntimeView>,
    /// Whether this computer shares its model connections with its machines.
    share_models: bool,
    /// Whether the shared connections live in the account's model vault.
    share_via_cloud: bool,
    /// Why the last share could not reach the model vault.
    #[serde(skip_serializing_if = "Option::is_none")]
    share_error: Option<String>,
    /// One line about the command that answered this list, such as a removed
    /// runtime that still lists this device.
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<String>,
}

fn local_view(
    attach: &AttachCompanionState,
    active: bool,
    newer_peer: Option<String>,
) -> RuntimeView {
    // This computer's runtime ships inside the app, so the app's version
    // names it, as it does for a paired desktop.
    let version = attach
        .local_runtime_version()
        .map(|_| APP_VERSION.to_owned());
    let update_status = crate::app_update::local_status()
        .as_ref()
        .and_then(|status| serde_json::to_value(status).ok())
        .and_then(|body| UpdateStatus::from_body(&body));
    RuntimeView {
        id: LOCAL_RUNTIME_ID.into(),
        name: "This computer".into(),
        kind: Some("local".into()),
        form_factor: muniment_core::form_factor::detect().map(|shape| shape.as_str().to_owned()),
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
        behind: newer_peer.is_some(),
        may_update: true,
        update_offered: false,
        update_target: newer_peer,
        update_capability: Some("desktop".into()),
        update_status,
        update_requested: false,
        ssh_host: None,
        sharing: None,
    }
}

fn paired_view(runtime: PairedRuntime, observed: Observed, active: bool) -> RuntimeView {
    let version = observed.version.or(runtime.entry.version);
    let kind = observed.install_kind.or(runtime.entry.install_kind);
    let form_factor = observed.form_factor.or(runtime.entry.form_factor);
    let target = target_version(kind.as_deref());
    let admin = runtime
        .scopes
        .contains(muniment_core::attach::SCOPE_RUNTIME_ADMIN);
    let behind = version
        .as_deref()
        .zip(target)
        .and_then(|(version, target)| is_older(version, target))
        == Some(true);
    let may = may_update(&runtime.scopes);
    let manual = observed.update_capability.as_deref() == Some("manual");
    RuntimeView {
        behind,
        may_update: may,
        update_offered: behind && may && !manual,
        update_target: target.filter(|_| behind).map(str::to_owned),
        update_capability: observed.update_capability,
        update_status: observed.update_status,
        update_requested: observed.requested.is_some(),
        id: runtime.id,
        name: runtime.entry.name,
        kind,
        form_factor,
        version,
        status: observed.status,
        active,
        local: false,
        scopes: runtime.scopes.into_iter().collect(),
        admin,
        ssh_host: runtime.entry.ssh.map(|ssh| ssh.host),
        sharing: None,
    }
}

/// Every runtime, this computer first, with the selected one marked.
pub(crate) fn list<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> RuntimeList {
    let attach = app.state::<AttachCompanionState>();
    let runtimes = app.state::<Runtimes>();
    let active = attach.remote_runtime_id();
    let mut views = vec![local_view(
        &attach,
        active.is_none(),
        runtimes.newest_desktop_peer(),
    )];
    if let Some(registry) = &runtimes.registry {
        views.extend(registry.list().into_iter().map(|runtime| {
            let observed = runtimes.observed(&runtime.id);
            let selected = active.as_deref() == Some(runtime.id.as_str());
            let sharing = runtimes.sharing.state(&runtime.id);
            RuntimeView {
                sharing,
                ..paired_view(runtime, observed, selected)
            }
        }));
    }
    let share_models = runtimes
        .registry
        .as_ref()
        .is_some_and(Registry::share_models);
    let share_via_cloud = runtimes
        .registry
        .as_ref()
        .is_some_and(Registry::share_via_cloud);
    RuntimeList {
        app_version: APP_VERSION,
        device_name: device_name(),
        selected: active.unwrap_or_else(|| LOCAL_RUNTIME_ID.into()),
        runtimes: views,
        share_models,
        share_via_cloud,
        share_error: runtimes.sharing.error(),
        notice: None,
    }
}

pub(crate) fn emit_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    // The app updater reports before setup manages the runtimes.
    if app.try_state::<Runtimes>().is_none() || app.try_state::<AttachCompanionState>().is_none() {
        return;
    }
    let _ = app.emit("runtimes-changed", list(app));
}

/// Handles a `runtime.update_status` event of this computer's runtime. The
/// local row reads the saved status, so the list only needs to change.
pub(crate) fn local_update_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    _status: serde_json::Value,
) {
    emit_changed(app);
}

/// The machines Update all asks, and the ones it skips with the reason.
pub(crate) fn update_all_plan<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> (Vec<RuntimeView>, Vec<(RuntimeView, SkipReason)>) {
    let views: Vec<RuntimeView> = list(app)
        .runtimes
        .into_iter()
        .filter(|runtime| !runtime.local)
        .collect();
    let candidates: Vec<Candidate> = views
        .iter()
        .map(|view| Candidate {
            id: &view.id,
            online: view.status == RuntimeStatus::Online,
            behind: view.behind,
            may_update: view.may_update,
            capability: view.update_capability.as_deref(),
        })
        .collect();
    let (asked, skipped) = plan_update_all(&candidates);
    let find = |id: &str| views.iter().find(|view| view.id == id).cloned();
    (
        asked.into_iter().filter_map(find).collect(),
        skipped
            .into_iter()
            .filter_map(|(id, reason)| find(id).map(|view| (view, reason)))
            .collect(),
    )
}

fn observer<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> connection::Observer {
    let app = app.clone();
    Arc::new(move |id: &str, observation: Observation| {
        let welcome = match &observation {
            Observation::Welcome {
                version,
                install_kind,
                ..
            } => Some((version.clone(), install_kind.clone())),
            _ => None,
        };
        let runtimes = app.state::<Runtimes>();
        if runtimes.record(id, observation) {
            emit_changed(&app);
        }
        let Some((version, kind)) = welcome else {
            return;
        };
        // A machine that connects gets this computer's connections at once.
        if runtimes
            .registry
            .as_ref()
            .is_some_and(Registry::share_models)
        {
            runtimes.sharing.wake();
        }
        let now = Instant::now();
        // A paired desktop that runs a newer app moves this app to it.
        if let Some(version) = runtimes.claim_self_follow(kind.as_deref(), version.as_deref(), now)
        {
            requested_update::follow(&app, version);
        }
        // Updates are automatic: each welcome of an older runtime asks it to
        // install its kind's target version, within the backoff.
        if runtimes.claim_auto_update(id, now) {
            let app = app.clone();
            let id = id.to_owned();
            std::thread::spawn(move || {
                let result = update_one(&app, &id, false);
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
/// chat events reach the window while it is the active runtime, and its
/// agent runs always do.
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
            if let muniment_core::attach::SubscriptionEvent::AgentRunUpdated(run) = event {
                crate::attach_service::deliver_agent_run(&event_app, &event_runtime, run);
                return;
            }
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
    sharing::start(app);
    if selected.is_some() {
        activate(app, selected.as_deref());
    }
    crate::app_update::settle_status_after_start(APP_VERSION);
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
    muniment_desktop_integration::computer_name::computer_name()
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
                form_factor: None,
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
        // A server updates to the core release the app bundles.
        assert_eq!(view.update_target.as_deref(), Some(version::CORE_VERSION));
        assert_eq!(view.kind.as_deref(), Some("server"));
        assert_eq!(view.status, RuntimeStatus::Unknown);

        let reader = paired(&["thread.read", "run.write"], Some("0.0.1"));
        let view = paired_view(reader, Observed::default(), true);
        assert!(!view.admin);
        assert!(!view.update_offered);
        assert!(view.behind);
        assert!(!view.may_update);
        assert!(view.active);

        // A runtime that installs by hand is behind but takes no request.
        let manual = paired(&["runtime.admin"], Some("0.0.1"));
        let observed = Observed {
            update_capability: Some("manual".into()),
            ..Observed::default()
        };
        let view = paired_view(manual, observed, false);
        assert!(view.behind);
        assert!(!view.update_offered);
        assert_eq!(view.update_capability.as_deref(), Some("manual"));

        // A newer welcome replaces the version the index kept.
        let current = paired(&["runtime.admin"], Some("0.0.1"));
        let observed = Observed {
            status: RuntimeStatus::Online,
            version: Some(version::CORE_VERSION.into()),
            install_kind: Some("container".into()),
            form_factor: None,
            ..Observed::default()
        };
        let view = paired_view(current, observed, false);
        assert!(!view.update_offered);
        assert!(!view.behind);
        assert_eq!(view.update_target, None);
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
            form_factor: Some("laptop".into()),
        };
        assert!(runtimes.record(id, welcome.clone()));
        assert!(!runtimes.record(id, welcome));
        assert_eq!(runtimes.observed(id).version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn a_request_shows_until_the_runtime_acts_on_it() {
        let runtimes = Runtimes::new(None);
        let id = "018f0000-0000-7000-8000-000000000042";
        runtimes.mark_requested(id, "1.15.0");
        assert_eq!(runtimes.observed(id).requested.as_deref(), Some("1.15.0"));
        // A status for another version leaves the request waiting.
        let idle = UpdateStatus::from_body(&serde_json::json!({"state": "idle"})).unwrap();
        assert!(runtimes.record(id, Observation::UpdateStatus(idle)));
        assert_eq!(runtimes.observed(id).requested.as_deref(), Some("1.15.0"));
        let queued = UpdateStatus::from_body(
            &serde_json::json!({"state": "queued", "target_version": "1.15.0"}),
        )
        .unwrap();
        assert!(runtimes.record(id, Observation::UpdateStatus(queued.clone())));
        assert_eq!(runtimes.observed(id).requested, None);
        // A runtime that already works toward the version is not marked.
        runtimes.mark_requested(id, "1.15.0");
        assert_eq!(runtimes.observed(id).requested, None);

        // An older runtime without status clears the request once it runs
        // the version.
        let older = "018f0000-0000-7000-8000-000000000043";
        runtimes.mark_requested(older, "1.15.0");
        let welcome = |version: &str| Observation::Welcome {
            version: Some(version.into()),
            install_kind: Some("server".into()),
            form_factor: None,
        };
        runtimes.record(older, welcome("1.14.0"));
        assert_eq!(
            runtimes.observed(older).requested.as_deref(),
            Some("1.15.0")
        );
        runtimes.record(older, welcome("1.15.0"));
        assert_eq!(runtimes.observed(older).requested, None);
    }

    #[test]
    fn this_app_follows_a_newer_desktop_peer_once_per_backoff() {
        let runtimes = Runtimes::new(None);
        let now = Instant::now();
        let newer = version::tests::bump(APP_VERSION);
        assert_eq!(
            runtimes.claim_self_follow(Some("desktop"), Some(&newer), now),
            Some(newer.clone())
        );
        assert_eq!(
            runtimes.claim_self_follow(Some("desktop"), Some(&newer), now),
            None
        );
        assert_eq!(
            runtimes.claim_self_follow(
                Some("desktop"),
                Some(&newer),
                now + update_state::REQUEST_BACKOFF
            ),
            Some(newer.clone())
        );
        // A newer container core never moves the app.
        let fresh = Runtimes::new(None);
        assert_eq!(
            fresh.claim_self_follow(Some("container"), Some("999.0.0"), now),
            None
        );
        assert_eq!(
            fresh.claim_self_follow(Some("server"), Some("999.0.0"), now),
            None
        );

        // The local row names the newest desktop peer as its target.
        fresh.record(
            "a",
            Observation::Welcome {
                version: Some(newer.clone()),
                install_kind: Some("desktop".into()),
                form_factor: None,
            },
        );
        fresh.record(
            "b",
            Observation::Welcome {
                version: Some("999.0.0".into()),
                install_kind: Some("container".into()),
                form_factor: None,
            },
        );
        assert_eq!(fresh.newest_desktop_peer(), Some(newer));
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
