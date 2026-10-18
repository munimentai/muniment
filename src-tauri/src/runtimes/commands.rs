//! The window's runtime commands: list, pair by link or over SSH, rename,
//! remove, select, and request updates. No answer carries a pairing secret
//! or a device credential.

use std::sync::Arc;

use muniment_core::attach::{
    connect_remote_runtime, is_valid_display_name, pair_with_link, ClientError, ErrorCode,
    PairingLink, PairingLinkError, RemoteConnectError, RemoteRuntimeRecord,
};
use serde::Serialize;
use tauri::Manager;

use super::connection::{client_options, welcome, Connector, Observation, RemoteControl};
use super::registry::{canonical_id, Registry, SshTarget, LOCAL_RUNTIME_ID};
use super::ssh::{self, Ssh};
use super::update_state::{SkipReason, UpdateInfo};
use super::version::{is_older, may_update, target_version};
use super::{
    activate, connector, emit_changed, list, reconnect, update_all_plan, RuntimeList,
    RuntimeStatus, Runtimes,
};
use crate::attach_service::AttachCompanionState;

fn link_error(error: PairingLinkError) -> String {
    match error {
        PairingLinkError::NotALink => {
            "This is not a pairing link. Copy the whole link that starts with muniment://pair."
        }
        PairingLinkError::UnsupportedVersion => {
            "This link needs a newer version of the app. Update the app and try again."
        }
        PairingLinkError::MissingField | PairingLinkError::InvalidField => {
            "This pairing link is incomplete. Copy the whole link, including the part after #."
        }
    }
    .into()
}

fn pairing_error(error: RemoteConnectError) -> String {
    match error {
        RemoteConnectError::Unreachable => {
            "No address in the link answered. Check that the runtime is running and reachable."
        }
        RemoteConnectError::CertificateMismatch => {
            "The runtime's certificate does not match the link. Create a new link on the runtime."
        }
        RemoteConnectError::RuntimeMismatch => {
            "A different runtime answered. Create a new link on the runtime."
        }
        RemoteConnectError::Unauthorized => {
            "The runtime refused the link. It may be used or expired. Create a new link."
        }
        RemoteConnectError::Client(ClientError::ProtocolIncompatible) => {
            "The runtime runs an incompatible version. Update the app or the runtime."
        }
        RemoteConnectError::Client(_) => "Pairing did not finish. Try again with a new link.",
    }
    .into()
}

fn status_error(status: RuntimeStatus) -> String {
    match status {
        RuntimeStatus::CertificateChanged => {
            "The runtime's certificate changed. Pair it again to trust the new certificate."
        }
        RuntimeStatus::Revoked => "The runtime revoked this device. Pair it again with a new link.",
        RuntimeStatus::VersionMismatch => {
            "The runtime runs an incompatible version. Update the app or the runtime."
        }
        RuntimeStatus::RuntimeMismatch => {
            "A different runtime answered at its address. Pair it again with a new link."
        }
        _ => "The runtime is offline. Check that it is running and reachable.",
    }
    .into()
}

fn device_name_choice(name: Option<String>) -> Result<Option<String>, String> {
    match name.as_deref().map(str::trim) {
        None | Some("") => Ok(Some(super::device_name())),
        Some(name) if is_valid_display_name(name) => Ok(Some(name.to_owned())),
        Some(_) => Err("Enter a device name of up to 80 characters on one line.".into()),
    }
}

/// Redeems a pairing link and saves the paired runtime. `endpoint` replaces
/// the link's addresses, for a link reached through an SSH tunnel.
pub(crate) fn pair(
    registry: &Registry,
    mut link: PairingLink,
    device_name: Option<String>,
    endpoint: Option<String>,
    ssh: Option<SshTarget>,
) -> Result<(String, Observation), String> {
    if let Some(endpoint) = endpoint {
        link.endpoints = vec![endpoint];
    }
    // Pairing a runtime again replaces this device's credential there.
    let previous = canonical_id(link.runtime_id.as_str())
        .and_then(|id| registry.record(&id).ok())
        .filter(|previous| previous.fingerprint == link.fingerprint);
    let (client, record) =
        pair_with_link(&link, &client_options(device_name)).map_err(pairing_error)?;
    let observation = welcome(&client);
    drop(client);
    let Observation::Welcome {
        version,
        install_kind,
        ..
    } = observation.clone()
    else {
        unreachable!("a welcome observation");
    };
    let id = registry
        .add(&record, ssh, version, install_kind)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => {
                "This link names a machine you already paired, but its certificate is different. \
                 Remove that machine first if you reinstalled it."
                    .to_owned()
            }
            _ => "The paired runtime could not be saved.".to_owned(),
        })?;
    if let Some(mut previous) =
        previous.filter(|previous| previous.credential.device_id != record.credential.device_id)
    {
        previous.endpoints = link.endpoints.clone();
        revoke_previous(&previous);
    }
    Ok((id, observation))
}

/// Links the runtime just paired as `id` into this computer's fleet, and
/// answers the line that says why it is not linked when that failed.
fn link_paired(registry: &Registry, id: &str) -> Option<String> {
    let record = registry.record(id).ok()?;
    let linked = super::fleet_link::link_into_fleet(&record);
    if let Err(reason) = &linked {
        eprintln!("muniment-desktop: the paired runtime was not linked runtime_id={id}");
        return Some(reason.clone());
    }
    None
}

/// Revokes the credential a new pairing replaced, so the runtime lists this
/// device once.
fn revoke_previous(previous: &RemoteRuntimeRecord) {
    let device = previous.credential.device_id.as_str();
    let revoked = connect_remote_runtime(previous, &client_options(None))
        .is_ok_and(|mut client| client.revoke_remote_device(device).is_ok());
    if !revoked {
        eprintln!(
            "muniment-desktop: the replaced device credential was not revoked device_id={device}"
        );
    }
}

fn finish_pairing<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    id: &str,
    observation: Observation,
) -> RuntimeList {
    let runtimes = app.state::<Runtimes>();
    runtimes.record(id, observation);
    runtimes.record(id, Observation::Status(RuntimeStatus::Online));
    // A runtime paired again replaces the connection that used the old record.
    reconnect(app, id);
    emit_changed(app);
    list(app)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "The runtime command did not finish.".to_owned())?
}

#[tauri::command]
pub async fn runtime_list(app: tauri::AppHandle) -> Result<RuntimeList, String> {
    Ok(list(&app))
}

/// Connects once to each paired runtime the app is not attached to, to learn
/// whether it answers and which version it runs.
#[tauri::command]
pub async fn runtime_refresh(app: tauri::AppHandle) -> Result<RuntimeList, String> {
    blocking(move || {
        let registry = app.state::<Runtimes>().registry()?.clone();
        let active = app.state::<AttachCompanionState>().remote_runtime_id();
        let probes: Vec<_> = registry
            .list()
            .into_iter()
            .filter(|runtime| active.as_deref() != Some(runtime.id.as_str()))
            .map(|runtime| {
                let connector = connector(&app, &registry, &runtime.id);
                std::thread::spawn(move || probe(&connector))
            })
            .collect();
        for probe in probes {
            let _ = probe.join();
        }
        emit_changed(&app);
        Ok(list(&app))
    })
    .await
}

fn probe(connector: &Connector) {
    match connector.connect() {
        Ok(client) => {
            (connector.observe)(&connector.runtime_id, welcome(&client));
            (connector.observe)(
                &connector.runtime_id,
                Observation::Status(RuntimeStatus::Online),
            );
        }
        Err(status) => (connector.observe)(&connector.runtime_id, Observation::Status(status)),
    }
    connector.control.stop();
}

#[tauri::command]
pub async fn runtime_pair_link(
    app: tauri::AppHandle,
    link: String,
    device_name: Option<String>,
) -> Result<RuntimeList, String> {
    let link = PairingLink::parse(link.trim()).map_err(link_error)?;
    let device_name = device_name_choice(device_name)?;
    blocking(move || {
        let runtimes = app.state::<Runtimes>();
        let _pairing = runtimes
            .pairing
            .try_lock()
            .map_err(|_| "Another pairing is in progress.".to_owned())?;
        let registry = runtimes.registry()?.clone();
        let (id, observation) = pair(&registry, link, device_name, None, None)?;
        let notice = link_paired(&registry, &id);
        Ok(RuntimeList {
            notice,
            ..finish_pairing(&app, &id, observation)
        })
    })
    .await
}

/// Runs `<command> pair --json` on the SSH host, opens a local forward to
/// the runtime port, and pairs through it.
#[tauri::command]
pub async fn runtime_pair_ssh(
    app: tauri::AppHandle,
    host: String,
    command: Option<String>,
    port: Option<u32>,
    device_name: Option<String>,
) -> Result<RuntimeList, String> {
    let target = ssh::target(
        &host,
        command
            .as_deref()
            .filter(|command| !command.trim().is_empty())
            .unwrap_or(ssh::DEFAULT_COMMAND),
        port.unwrap_or(u32::from(ssh::DEFAULT_REMOTE_PORT)),
    )
    .map_err(|error| error.message())?;
    let device_name = device_name_choice(device_name)?;
    blocking(move || {
        let runtimes = app.state::<Runtimes>();
        let _pairing = runtimes
            .pairing
            .try_lock()
            .map_err(|_| "Another pairing is in progress.".to_owned())?;
        let registry = runtimes.registry()?.clone();
        let ssh = Ssh::from_environment().map_err(|error| error.message())?;
        let output = ssh.pair(&target).map_err(|error| error.message())?;
        let tunnel = ssh
            .open_tunnel(&target.host, target.remote_port)
            .map_err(|error| error.message())?;
        let paired = pair(
            &registry,
            output.link,
            device_name,
            Some(tunnel.endpoint()),
            Some(target),
        );
        // The paired record dials through the tunnel, so linking runs before
        // it closes.
        let notice = paired
            .as_ref()
            .ok()
            .and_then(|(id, _)| link_paired(&registry, id));
        drop(tunnel);
        let (id, observation) = paired?;
        Ok(RuntimeList {
            notice,
            ..finish_pairing(&app, &id, observation)
        })
    })
    .await
}

fn paired_id(id: &str) -> Result<String, String> {
    canonical_id(id).ok_or_else(|| "That runtime is not paired.".to_owned())
}

#[tauri::command]
pub async fn runtime_rename(
    app: tauri::AppHandle,
    id: String,
    name: String,
) -> Result<RuntimeList, String> {
    if id == LOCAL_RUNTIME_ID {
        return Err("This computer keeps its name.".into());
    }
    let id = paired_id(&id)?;
    blocking(move || {
        app.state::<Runtimes>()
            .registry()?
            .rename(&id, &name)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::InvalidInput => {
                    "Enter a name of up to 80 characters on one line.".to_owned()
                }
                std::io::ErrorKind::NotFound => "That runtime is not paired.".to_owned(),
                _ => "The new name could not be saved.".to_owned(),
            })?;
        emit_changed(&app);
        Ok(list(&app))
    })
    .await
}

/// Whether the runtime no longer holds this device's credential: `Some(true)`
/// after this call revoked it, `Some(false)` when the runtime had already
/// refused it, and none when the runtime did not answer.
fn released(connector: &Connector) -> Option<bool> {
    let record = connector.registry.record(&connector.runtime_id).ok()?;
    let answer = match connector.connect() {
        Ok(mut client) => client
            .revoke_remote_device(record.credential.device_id.as_str())
            .is_ok()
            .then_some(true),
        Err(RuntimeStatus::Revoked) => Some(false),
        Err(_) => None,
    };
    connector.control.stop();
    answer
}

/// Forgets a paired runtime and deletes its device credential. The device
/// first revokes itself there. Removing the selected runtime switches the app
/// to this computer.
#[tauri::command]
pub async fn runtime_remove(app: tauri::AppHandle, id: String) -> Result<RuntimeList, String> {
    if id == LOCAL_RUNTIME_ID {
        return Err("This computer's runtime cannot be removed.".into());
    }
    let id = paired_id(&id)?;
    blocking(move || {
        let runtimes = app.state::<Runtimes>();
        if let Some(connections) = app.state::<AttachCompanionState>().remove_remote(&id) {
            connections.stop();
        }
        let registry = runtimes.registry()?;
        let name = registry
            .entry(&id)
            .map_or_else(|| "The runtime".to_owned(), |entry| entry.name);
        // A runtime that revoked this device already forgot it. One that is
        // offline, or older than self revocation, keeps the credential until
        // its owner revokes it there.
        let notice = released(&connector(&app, registry, &id))
            .is_none()
            .then(|| {
                format!("{name} still lists this device. Revoke it on {name} to end its access.")
            });
        registry
            .remove(&id)
            .map_err(|_| "The runtime could not be removed.".to_owned())?;
        runtimes
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        emit_changed(&app);
        Ok(RuntimeList {
            notice,
            ..list(&app)
        })
    })
    .await
}

#[tauri::command]
pub async fn runtime_select(app: tauri::AppHandle, id: String) -> Result<RuntimeList, String> {
    let selected = if id == LOCAL_RUNTIME_ID {
        None
    } else {
        Some(paired_id(&id)?)
    };
    blocking(move || {
        let registry = app.state::<Runtimes>().registry()?.clone();
        registry
            .select(selected.as_deref().unwrap_or(LOCAL_RUNTIME_ID))
            .map_err(|_| "That runtime is not paired.".to_owned())?;
        if app.state::<AttachCompanionState>().remote_runtime_id() != selected {
            activate(&app, selected.as_deref());
        }
        Ok(list(&app))
    })
    .await
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeUpdateResult {
    id: String,
    name: String,
    pub(crate) ok: bool,
    /// The version the runtime's host was asked to install.
    version: Option<String>,
    pub(crate) message: String,
}

/// Asks one paired runtime's host to install its kind's target version. A
/// runtime that installs by hand, or already works toward that version, is
/// not asked. Only an `explicit` request asks again for a version the
/// runtime failed to install.
fn request_update(connector: &Connector, explicit: bool) -> Result<String, String> {
    let mut client = connector.connect().map_err(status_error)?;
    let observation = welcome(&client);
    (connector.observe)(&connector.runtime_id, observation.clone());
    let info = client
        .read_runtime_info()
        .map(|info| UpdateInfo::from_runtime_info(&info))
        .unwrap_or_default();
    (connector.observe)(&connector.runtime_id, Observation::Update(info.clone()));
    let Observation::Welcome {
        version,
        install_kind,
        ..
    } = observation
    else {
        unreachable!("a welcome observation");
    };
    let Some(target) = target_version(install_kind.as_deref()) else {
        return Err("This app does not know which version that runtime should run.".into());
    };
    if version
        .as_deref()
        .and_then(|version| is_older(version, target))
        != Some(true)
    {
        return Err("That runtime already runs this version or a newer one.".into());
    }
    if info.capability.as_deref() == Some("manual") {
        return Err("That machine installs updates by hand. Update it there.".into());
    }
    if let Some(status) = &info.status {
        if status.pending_for(target) {
            return Ok(target.to_owned());
        }
        if !explicit && status.failed_for(target) {
            return Err("That machine could not install this version.".into());
        }
    }
    match client.request_runtime_update(target) {
        Ok(_) => Ok(target.to_owned()),
        Err(_) => Err(
            match client.last_request_error().map(|error| error.code()) {
                Some(ErrorCode::Unauthorized) => {
                    "This device cannot update that runtime. Pair it again to allow updates."
                }
                Some(ErrorCode::UnsupportedOperation) => {
                    "That runtime cannot take update requests."
                }
                _ => "The update request did not reach the runtime. Try again.",
            }
            .to_owned(),
        ),
    }
}

pub(crate) fn update_one<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    id: &str,
    explicit: bool,
) -> RuntimeUpdateResult {
    let runtimes = app.state::<Runtimes>();
    let attach = app.state::<AttachCompanionState>();
    let name = runtimes
        .registry
        .as_ref()
        .and_then(|registry| registry.entry(id))
        .map(|entry| entry.name)
        .unwrap_or_default();
    let result = (|| {
        let registry = runtimes.registry()?;
        let record = registry
            .record(id)
            .map_err(|_| "That runtime is not paired.".to_owned())?;
        if !may_update(&record.scopes) {
            return Err(
                "This device cannot update that runtime. Pair it again to allow updates."
                    .to_owned(),
            );
        }
        // The selected runtime's requests share its tunnel.
        match attach
            .remote_connector()
            .filter(|active| active.runtime_id == id)
        {
            Some(active) => request_update(&active, explicit),
            None => {
                let temporary = Connector {
                    control: Arc::new(RemoteControl::default()),
                    ..connector(app, registry, id)
                };
                let result = request_update(&temporary, explicit);
                temporary.control.stop();
                result
            }
        }
    })();
    if let Ok(version) = &result {
        runtimes.mark_requested(id, version);
    }
    match result {
        Ok(version) => RuntimeUpdateResult {
            id: id.to_owned(),
            name,
            ok: true,
            message: format!("Update to {version} requested."),
            version: Some(version),
        },
        Err(message) => RuntimeUpdateResult {
            id: id.to_owned(),
            name,
            ok: false,
            version: None,
            message,
        },
    }
}

#[tauri::command]
pub async fn runtime_update(
    app: tauri::AppHandle,
    id: String,
) -> Result<RuntimeUpdateResult, String> {
    let id = paired_id(&id)?;
    blocking(move || {
        let result = update_one(&app, &id, true);
        emit_changed(&app);
        Ok(result)
    })
    .await
}

/// A machine Update all left alone, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkippedRuntime {
    id: String,
    name: String,
    reason: SkipReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateAllReport {
    /// The requests, one per machine asked, in list order.
    results: Vec<RuntimeUpdateResult>,
    skipped: Vec<SkippedRuntime>,
}

/// Asks every paired machine that is behind and reachable to update, and
/// names each machine it skips. The window updates this computer after.
#[tauri::command]
pub async fn runtime_update_all(app: tauri::AppHandle) -> Result<UpdateAllReport, String> {
    blocking(move || {
        let (asked, skipped) = update_all_plan(&app);
        let workers: Vec<_> = asked
            .into_iter()
            .map(|view| {
                let app = app.clone();
                std::thread::spawn(move || update_one(&app, &view.id, true))
            })
            .collect();
        let results = workers
            .into_iter()
            .filter_map(|worker| worker.join().ok())
            .collect();
        emit_changed(&app);
        Ok(UpdateAllReport {
            results,
            skipped: skipped
                .into_iter()
                .map(|(view, reason)| SkippedRuntime {
                    id: view.id,
                    name: view.name,
                    reason,
                })
                .collect(),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::super::version::APP_VERSION;
    use super::*;
    use crate::runtimes::update_state::UpdateInfo;
    use muniment_core::attach::SCOPE_RUNTIME_ADMIN;

    /// Revokes this device on a paired runtime and answers whether the
    /// runtime confirmed it.
    fn revoke_self(connector: &Connector) -> bool {
        released(connector) == Some(true)
    }
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use muniment_core::attach::thread_service::ThreadListService;
    use muniment_core::attach::{
        InstallKind, PairingLinkRequest, RemoteAccess, RemoteAccessOptions, RemoteListener,
        RemoteListenerConfig, RuntimeUpdateState, SignedWorkspaceApproval, UpdateCapability,
    };

    struct Service;

    impl ThreadListService for Service {}

    /// A runtime that serves the remote attach route on a loopback port.
    struct LoopbackRuntime {
        access: RemoteAccess,
        directory: std::path::PathBuf,
        stop: Option<std::sync::mpsc::Sender<()>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl LoopbackRuntime {
        fn start(version: &str) -> Self {
            let directory = std::env::temp_dir().join(format!(
                "muniment-loopback-runtime-{}",
                uuid::Uuid::now_v7()
            ));
            let access = RemoteAccess::open(
                &directory,
                RemoteAccessOptions {
                    runtime_name: Some("Build box".into()),
                    runtime_version: version.into(),
                    install_kind: InstallKind::Container,
                },
            )
            .unwrap();
            // A container's launcher installs the releases it is asked for.
            access.set_update_capability(UpdateCapability::Launcher);
            let listener =
                RemoteListener::bind(RemoteListenerConfig::new("127.0.0.1:0".parse().unwrap()))
                    .unwrap();
            access
                .set_advertised_endpoints(vec![listener.local_addr().unwrap().to_string()])
                .unwrap();
            let (stop, stopped) = std::sync::mpsc::channel();
            let served = access.clone();
            let thread = std::thread::spawn(move || {
                listener
                    .serve(
                        served,
                        SignedWorkspaceApproval::default(),
                        || Ok::<_, ()>(Service),
                        stopped,
                    )
                    .unwrap();
            });
            Self {
                access,
                directory,
                stop: Some(stop),
                thread: Some(thread),
            }
        }

        fn link(&self, scopes: &[&str]) -> PairingLink {
            self.access
                .create_pairing_link(PairingLinkRequest {
                    scopes: Some(scopes.iter().map(|scope| (*scope).to_owned()).collect()),
                    endpoints: None,
                })
                .unwrap()
                .link
        }
    }

    impl Drop for LoopbackRuntime {
        fn drop(&mut self) {
            drop(self.stop.take());
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn registry() -> (Registry, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("muniment-paired-{}", uuid::Uuid::now_v7()));
        (Registry::new(directory.clone()), directory)
    }

    fn connector_for(
        registry: &Registry,
        id: &str,
        seen: &Arc<Mutex<Vec<Observation>>>,
    ) -> Connector {
        let seen = Arc::clone(seen);
        Connector {
            runtime_id: id.to_owned(),
            registry: registry.clone(),
            ssh: None,
            control: Arc::new(RemoteControl::default()),
            observe: Arc::new(move |_, observation| seen.lock().unwrap().push(observation)),
        }
    }

    fn wait_for(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_link_pairs_once_connects_requests_an_update_and_reports_revocation() {
        let runtime = LoopbackRuntime::start("0.0.1");
        let (registry, directory) = registry();
        let link = runtime.link(&["thread.read", "run.write", "runtime.admin"]);
        let (id, observation) = pair(
            &registry,
            link.clone(),
            Some("Test laptop".into()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(id, link.runtime_id.as_str());
        assert_eq!(
            observation,
            Observation::Welcome {
                version: Some("0.0.1".into()),
                install_kind: Some("container".into()),
                form_factor: muniment_core::form_factor::detect()
                    .map(|shape| shape.as_str().to_owned()),
            }
        );
        let paired = registry.list();
        assert_eq!(paired.len(), 1);
        assert_eq!(paired[0].entry.name, "Build box");
        assert!(paired[0].scopes.contains(SCOPE_RUNTIME_ADMIN));
        let devices = runtime.access.list_devices().unwrap();
        assert_eq!(devices.len(), 1);

        // The link redeems once.
        assert_eq!(
            pair(&registry, link, None, None, None).unwrap_err(),
            pairing_error(RemoteConnectError::Unauthorized)
        );

        let seen = Arc::new(Mutex::new(Vec::new()));
        let connector = connector_for(&registry, &id, &seen);
        // A container is asked for the app's version.
        assert_eq!(
            request_update(&connector, false),
            Ok(APP_VERSION.to_owned())
        );
        assert_eq!(
            runtime.access.requested_update().unwrap().version,
            APP_VERSION
        );

        let supervisor = super::super::RemoteSupervisor::start(
            connector_for(&registry, &id, &seen),
            |_| {},
            |_| {},
        );
        wait_for(|| supervisor.connected());
        assert!(seen
            .lock()
            .unwrap()
            .contains(&Observation::Status(RuntimeStatus::Online)));

        let device = registry.record(&id).unwrap().credential.device_id;
        assert!(runtime.access.revoke_device(device.as_str()).unwrap());
        wait_for(|| {
            seen.lock()
                .unwrap()
                .contains(&Observation::Status(RuntimeStatus::Revoked))
        });
        assert_eq!(connector.connect().unwrap_err(), RuntimeStatus::Revoked);
        supervisor.stop();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn admin_and_reader_devices_each_revoke_themselves() {
        let runtime = LoopbackRuntime::start("0.0.1");
        let (readers, reader_directory) = registry();
        let (registry, directory) = registry();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (admin, _) = pair(
            &registry,
            runtime.link(&["thread.read", "runtime.admin"]),
            None,
            None,
            None,
        )
        .unwrap();
        // One registry keeps one record per runtime, so the reader pairs in its own.
        let (reader, _) = pair(&readers, runtime.link(&["thread.read"]), None, None, None).unwrap();
        assert_eq!(runtime.access.list_devices().unwrap().len(), 2);
        assert!(revoke_self(&connector_for(&readers, &reader, &seen)));
        assert_eq!(runtime.access.list_devices().unwrap().len(), 1);
        assert!(revoke_self(&connector_for(&registry, &admin, &seen)));
        assert_eq!(runtime.access.list_devices().unwrap().len(), 0);
        // A revoked device cannot reach the runtime to revoke itself again,
        // and the runtime's refusal shows it holds the credential no more.
        assert!(!revoke_self(&connector_for(&registry, &admin, &seen)));
        assert_eq!(
            released(&connector_for(&registry, &admin, &seen)),
            Some(false)
        );
        std::fs::remove_dir_all(directory).unwrap();
        std::fs::remove_dir_all(reader_directory).unwrap();
    }

    #[test]
    fn pairing_a_runtime_again_revokes_the_credential_it_replaces() {
        let runtime = LoopbackRuntime::start("0.0.1");
        let (registry, directory) = registry();
        let (id, _) = pair(&registry, runtime.link(&["thread.read"]), None, None, None).unwrap();
        let first = registry.record(&id).unwrap().credential.device_id;
        pair(&registry, runtime.link(&["thread.read"]), None, None, None).unwrap();
        let devices = runtime.access.list_devices().unwrap();
        assert_eq!(devices.len(), 1);
        assert_ne!(devices[0].device_id, first.as_str());
        assert_eq!(
            devices[0].device_id,
            registry.record(&id).unwrap().credential.device_id.as_str()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_update_needs_the_admin_scope_and_an_older_runtime() {
        let older = LoopbackRuntime::start("0.0.1");
        let current = LoopbackRuntime::start(APP_VERSION);
        let (registry, directory) = registry();
        let seen = Arc::new(Mutex::new(Vec::new()));

        let (reader, _) = pair(
            &registry,
            older.link(&["thread.read", "run.write"]),
            None,
            None,
            None,
        )
        .unwrap();
        let refused = request_update(&connector_for(&registry, &reader, &seen), false).unwrap_err();
        assert!(refused.contains("cannot update"), "{refused}");
        assert_eq!(older.access.requested_update(), None);

        let (admin, _) = pair(
            &registry,
            current.link(&["thread.read", "runtime.admin"]),
            None,
            None,
            None,
        )
        .unwrap();
        let refused = request_update(&connector_for(&registry, &admin, &seen), false).unwrap_err();
        assert!(refused.contains("already runs"), "{refused}");
        assert_eq!(current.access.requested_update(), None);
        let scopes: BTreeSet<String> = registry.record(&admin).unwrap().scopes;
        assert!(scopes.contains(SCOPE_RUNTIME_ADMIN));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_request_skips_manual_pending_and_failed_runtimes_and_reads_their_status() {
        let runtime = LoopbackRuntime::start("0.0.1");
        let (registry, directory) = registry();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (id, _) = pair(
            &registry,
            runtime.link(&["thread.read", "runtime.admin"]),
            None,
            None,
            None,
        )
        .unwrap();
        let connector = connector_for(&registry, &id, &seen);

        // The runtime reported a failed install of this version: only an
        // explicit request asks again.
        runtime
            .access
            .set_update_status(
                RuntimeUpdateState::Failed,
                Some(APP_VERSION),
                Vec::new(),
                Some("Did not report healthy.".into()),
            )
            .unwrap();
        let refused = request_update(&connector, false).unwrap_err();
        assert!(refused.contains("could not install"), "{refused}");
        assert_eq!(runtime.access.requested_update(), None);
        assert!(seen.lock().unwrap().iter().any(|observation| matches!(
            observation,
            Observation::Update(UpdateInfo { capability: Some(capability), status: Some(status) })
                if capability == "launcher" && status.state == "failed"
        )));
        assert_eq!(request_update(&connector, true), Ok(APP_VERSION.to_owned()));
        assert_eq!(
            runtime.access.requested_update().unwrap().version,
            APP_VERSION
        );

        // A runtime that works toward the version is not asked again.
        runtime
            .access
            .set_update_status(
                RuntimeUpdateState::Downloading,
                Some(APP_VERSION),
                Vec::new(),
                None,
            )
            .unwrap();
        runtime.access.clear_update_request().unwrap();
        assert_eq!(
            request_update(&connector, false),
            Ok(APP_VERSION.to_owned())
        );
        assert_eq!(runtime.access.requested_update(), None);

        // A runtime that installs by hand takes no request.
        runtime
            .access
            .set_update_capability(UpdateCapability::Manual);
        let refused = request_update(&connector, true).unwrap_err();
        assert!(refused.contains("by hand"), "{refused}");
        assert_eq!(runtime.access.requested_update(), None);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_runtime_with_a_new_certificate_reports_the_change() {
        let runtime = LoopbackRuntime::start("0.0.1");
        let (registry, directory) = registry();
        let (id, _) = pair(&registry, runtime.link(&["thread.read"]), None, None, None).unwrap();
        let mut record = registry.record(&id).unwrap();
        record.fingerprint =
            muniment_core::attach::CertificateFingerprint::of_certificate(b"other");
        record.save(&registry.record_path(&id).unwrap()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(
            connector_for(&registry, &id, &seen).connect().unwrap_err(),
            RuntimeStatus::CertificateChanged
        );
        drop(runtime);
        let mut record = registry.record(&id).unwrap();
        record.endpoints = vec!["127.0.0.1:9".into()];
        record.save(&registry.record_path(&id).unwrap()).unwrap();
        assert_eq!(
            connector_for(&registry, &id, &seen).connect().unwrap_err(),
            RuntimeStatus::Offline
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn errors_name_the_next_step_and_never_echo_the_link() {
        let link = "muniment://pair?v=1#".to_owned() + &"ab".repeat(16);
        let message = link_error(PairingLink::parse(&link).unwrap_err());
        assert!(message.contains("after #"));
        assert!(!message.contains(&"ab".repeat(16)));
        assert!(link_error(PairingLinkError::NotALink).contains("muniment://pair"));
        for error in [
            RemoteConnectError::Unreachable,
            RemoteConnectError::CertificateMismatch,
            RemoteConnectError::RuntimeMismatch,
            RemoteConnectError::Unauthorized,
            RemoteConnectError::Client(ClientError::ProtocolIncompatible),
            RemoteConnectError::Client(ClientError::ConnectionClosed),
        ] {
            let message = pairing_error(error);
            assert!(message.ends_with('.'), "{message}");
        }
        assert!(status_error(RuntimeStatus::Revoked).contains("Pair it again"));
        assert!(status_error(RuntimeStatus::Offline).contains("offline"));
    }

    #[test]
    fn a_device_name_defaults_to_this_computer_and_must_be_one_line() {
        assert_eq!(
            device_name_choice(None).unwrap(),
            Some(super::super::device_name())
        );
        assert_eq!(
            device_name_choice(Some("  Laptop ".into()))
                .unwrap()
                .as_deref(),
            Some("Laptop")
        );
        assert!(device_name_choice(Some("a\nb".into())).is_err());
        assert!(device_name_choice(Some("x".repeat(81))).is_err());
    }
}
