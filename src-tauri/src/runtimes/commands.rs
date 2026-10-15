//! The window's runtime commands: list, pair by link or over SSH, rename,
//! remove, select, and request updates. No answer carries a pairing secret
//! or a device credential.

use std::sync::Arc;

use muniment_core::attach::{
    is_valid_display_name, pair_with_link, ClientError, ErrorCode, PairingLink, PairingLinkError,
    RemoteConnectError, SCOPE_RUNTIME_ADMIN,
};
use serde::Serialize;
use tauri::Manager;

use super::connection::{client_options, welcome, Connector, Observation, RemoteControl};
use super::registry::{canonical_id, Registry, SshTarget, LOCAL_RUNTIME_ID};
use super::ssh::{self, Ssh};
use super::version::{is_older, APP_VERSION};
use super::{activate, connector, emit_changed, list, RuntimeList, RuntimeStatus, Runtimes};
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
    let (client, record) =
        pair_with_link(&link, &client_options(device_name)).map_err(pairing_error)?;
    let observation = welcome(&client);
    drop(client);
    let Observation::Welcome {
        version,
        install_kind,
    } = observation.clone()
    else {
        unreachable!("a welcome observation");
    };
    let id = registry
        .add(&record, ssh, version, install_kind)
        .map_err(|_| "The paired runtime could not be saved.".to_owned())?;
    Ok((id, observation))
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
    if app
        .state::<AttachCompanionState>()
        .remote_runtime_id()
        .as_deref()
        == Some(id)
    {
        activate(app, Some(id));
    }
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
        Ok(finish_pairing(&app, &id, observation))
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
        drop(tunnel);
        let (id, observation) = paired?;
        Ok(finish_pairing(&app, &id, observation))
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

/// Revokes this device on a paired runtime, as a device holding the admin
/// scope may. Answers whether the runtime confirmed it.
fn revoke_self(connector: &Connector) -> bool {
    let Ok(record) = connector.registry.record(&connector.runtime_id) else {
        return false;
    };
    let revoked = connector.connect().is_ok_and(|mut client| {
        client
            .revoke_remote_device(record.credential.device_id.as_str())
            .is_ok()
    });
    connector.control.stop();
    revoked
}

/// Forgets a paired runtime and deletes its device credential. A device
/// holding the admin scope first revokes itself there. Removing the selected
/// runtime switches the app to this computer.
#[tauri::command]
pub async fn runtime_remove(app: tauri::AppHandle, id: String) -> Result<RuntimeList, String> {
    if id == LOCAL_RUNTIME_ID {
        return Err("This computer's runtime cannot be removed.".into());
    }
    let id = paired_id(&id)?;
    blocking(move || {
        if app
            .state::<AttachCompanionState>()
            .remote_runtime_id()
            .as_deref()
            == Some(id.as_str())
        {
            activate(&app, None);
        }
        let runtimes = app.state::<Runtimes>();
        let registry = runtimes.registry()?;
        let name = registry
            .entry(&id)
            .map_or_else(|| "The runtime".to_owned(), |entry| entry.name);
        let admin = registry
            .record(&id)
            .is_ok_and(|record| record.scopes.contains(SCOPE_RUNTIME_ADMIN));
        let notice = if admin && revoke_self(&connector(&app, registry, &id)) {
            None
        } else if admin {
            Some(format!(
                "{name} did not answer, so it still lists this device. Revoke it on {name}."
            ))
        } else {
            Some(format!(
                "{name} still lists this device. Revoke it on {name} to end its access."
            ))
        };
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
    ok: bool,
    /// The version the runtime's host was asked to install.
    version: Option<String>,
    message: String,
}

/// Asks one paired runtime's host to install this app's version.
fn request_update(connector: &Connector) -> Result<String, String> {
    let mut client = connector.connect().map_err(status_error)?;
    let observation = welcome(&client);
    (connector.observe)(&connector.runtime_id, observation.clone());
    let Observation::Welcome { version, .. } = observation else {
        unreachable!("a welcome observation");
    };
    if version
        .as_deref()
        .and_then(|version| is_older(version, APP_VERSION))
        != Some(true)
    {
        return Err("That runtime already runs this version or a newer one.".into());
    }
    match client.request_runtime_update(APP_VERSION) {
        Ok(_) => Ok(APP_VERSION.to_owned()),
        Err(_) => Err(match client.last_request_error().map(|error| error.code()) {
            Some(ErrorCode::Unauthorized) => {
                "This device cannot update that runtime. Pair it with a link that allows updates."
            }
            Some(ErrorCode::UnsupportedOperation) => "That runtime cannot take update requests.",
            _ => "The update request did not reach the runtime. Try again.",
        }
        .to_owned()),
    }
}

fn update_one<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) -> RuntimeUpdateResult {
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
        if !record.scopes.contains(SCOPE_RUNTIME_ADMIN) {
            return Err(
                "This device cannot update that runtime. Pair it with a link that allows updates."
                    .to_owned(),
            );
        }
        // The selected runtime's requests share its tunnel.
        match attach
            .remote_connector()
            .filter(|active| active.runtime_id == id)
        {
            Some(active) => request_update(&active),
            None => {
                let temporary = Connector {
                    control: Arc::new(RemoteControl::default()),
                    ..connector(app, registry, id)
                };
                let result = request_update(&temporary);
                temporary.control.stop();
                result
            }
        }
    })();
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
        let result = update_one(&app, &id);
        emit_changed(&app);
        Ok(result)
    })
    .await
}

/// Sends the update request to every paired runtime the app offers to update.
#[tauri::command]
pub async fn runtime_update_all(app: tauri::AppHandle) -> Result<Vec<RuntimeUpdateResult>, String> {
    blocking(move || {
        let offered: Vec<String> = list(&app)
            .runtimes
            .into_iter()
            .filter(|runtime| runtime.update_offered)
            .map(|runtime| runtime.id)
            .collect();
        let workers: Vec<_> = offered
            .into_iter()
            .map(|id| {
                let app = app.clone();
                std::thread::spawn(move || update_one(&app, &id))
            })
            .collect();
        let results = workers
            .into_iter()
            .filter_map(|worker| worker.join().ok())
            .collect();
        emit_changed(&app);
        Ok(results)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use muniment_core::attach::thread_service::ThreadListService;
    use muniment_core::attach::{
        InstallKind, PairingLinkRequest, RemoteAccess, RemoteAccessOptions, RemoteListener,
        RemoteListenerConfig, SignedWorkspaceApproval,
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
        assert_eq!(request_update(&connector), Ok(APP_VERSION.to_owned()));
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
    fn an_admin_device_revokes_itself_and_a_reader_cannot() {
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
        assert!(!revoke_self(&connector_for(&readers, &reader, &seen)));
        assert_eq!(runtime.access.list_devices().unwrap().len(), 2);
        assert!(revoke_self(&connector_for(&registry, &admin, &seen)));
        assert_eq!(runtime.access.list_devices().unwrap().len(), 1);
        // A revoked device cannot reach the runtime to revoke itself again.
        assert!(!revoke_self(&connector_for(&registry, &admin, &seen)));
        std::fs::remove_dir_all(directory).unwrap();
        std::fs::remove_dir_all(reader_directory).unwrap();
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
        let refused = request_update(&connector_for(&registry, &reader, &seen)).unwrap_err();
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
        let refused = request_update(&connector_for(&registry, &admin, &seen)).unwrap_err();
        assert!(refused.contains("already runs"), "{refused}");
        assert_eq!(current.access.requested_update(), None);
        let scopes: BTreeSet<String> = registry.record(&admin).unwrap().scopes;
        assert!(scopes.contains(SCOPE_RUNTIME_ADMIN));
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
