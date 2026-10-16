//! The attach connections to the selected paired runtime: the shared desktop
//! client, which reconnects like the local one, and the chat-event
//! subscription. An SSH runtime's connections share one tunnel, which opens
//! with the first connection and closes when the app leaves the runtime.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use muniment_core::attach::{
    connect_remote_runtime, serve_desktop_client_with, ClientError, DesktopClient,
    DesktopClientHolder, DesktopClientStopHandle, RemoteClientOptions, RemoteConnectError,
    SubscriptionEvent,
};
use serde::Serialize;

use super::registry::{Registry, SshTarget};
use super::ssh::{Ssh, SshTunnel};
use super::update_state::{UpdateInfo, UpdateStatus};
use super::version::APP_VERSION;

/// The client kind this app names in its hello.
pub(crate) const CLIENT_KIND: &str = "desktop-client";
/// The bound on each network step and each request.
pub(crate) const IO_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_INTERVAL: Duration = Duration::from_millis(500);
const OFFLINE_BACKOFF: Duration = Duration::from_secs(5);
const REFUSED_BACKOFF: Duration = Duration::from_secs(30);

/// How a runtime answered the last connection attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeStatus {
    /// Not reached since the app started.
    Unknown,
    Connecting,
    Online,
    Offline,
    /// A different certificate answered at the runtime's address.
    CertificateChanged,
    /// The runtime refused this device's credential.
    Revoked,
    /// The runtime speaks a protocol version this app does not.
    VersionMismatch,
    /// A different runtime answered at the runtime's address.
    RuntimeMismatch,
}

impl RuntimeStatus {
    pub(crate) fn of_error(error: &RemoteConnectError) -> Self {
        match error {
            RemoteConnectError::Unreachable => Self::Offline,
            RemoteConnectError::CertificateMismatch => Self::CertificateChanged,
            RemoteConnectError::RuntimeMismatch => Self::RuntimeMismatch,
            RemoteConnectError::Unauthorized => Self::Revoked,
            RemoteConnectError::Client(ClientError::ProtocolIncompatible) => Self::VersionMismatch,
            RemoteConnectError::Client(_) => Self::Offline,
        }
    }

    /// How long the supervisor waits before it tries a runtime again. A
    /// refusal waits longer, since only a new pairing changes it.
    fn backoff(self) -> Duration {
        match self {
            Self::CertificateChanged
            | Self::Revoked
            | Self::RuntimeMismatch
            | Self::VersionMismatch => REFUSED_BACKOFF,
            _ => OFFLINE_BACKOFF,
        }
    }
}

pub(crate) fn client_options(device_name: Option<String>) -> RemoteClientOptions {
    RemoteClientOptions {
        kind: CLIENT_KIND.into(),
        version: APP_VERSION.into(),
        device_name,
        scopes: None,
        io_timeout: IO_TIMEOUT,
    }
}

/// What one connection attempt learned about a runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Observation {
    Status(RuntimeStatus),
    Welcome {
        version: Option<String>,
        install_kind: Option<String>,
        form_factor: Option<String>,
    },
    /// The update fields of the runtime's `runtime.info`.
    Update(UpdateInfo),
    /// A `runtime.update_status` event.
    UpdateStatus(UpdateStatus),
}

pub(crate) type Observer = Arc<dyn Fn(&str, Observation) + Send + Sync>;

/// The stop flag and the SSH tunnel that one runtime's connections share.
#[derive(Default)]
pub(crate) struct RemoteControl {
    stopped: Mutex<bool>,
    wake: Condvar,
    tunnel: Mutex<Option<SshTunnel>>,
}

impl RemoteControl {
    pub(crate) fn stopped(&self) -> bool {
        *self
            .stopped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Waits `duration` unless the connection stops first, and answers
    /// whether it still runs.
    pub(crate) fn wait(&self, duration: Duration) -> bool {
        let stopped = self
            .stopped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (stopped, _) = self
            .wake
            .wait_timeout_while(stopped, duration, |stopped| !*stopped)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !*stopped
    }

    /// Stops every connection and closes the tunnel. A tunnel that is busy,
    /// such as one still opening, closes in the background once it is free.
    pub(crate) fn stop(self: &Arc<Self>) {
        *self
            .stopped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.wake.notify_all();
        match self.tunnel.try_lock() {
            Ok(mut tunnel) => drop(tunnel.take()),
            Err(std::sync::TryLockError::Poisoned(error)) => drop(error.into_inner().take()),
            Err(std::sync::TryLockError::WouldBlock) => {
                let control = Arc::clone(self);
                std::thread::spawn(move || {
                    drop(
                        control
                            .tunnel
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take(),
                    );
                });
            }
        }
    }

    /// The loopback endpoint of a live tunnel, opening one when none runs.
    fn tunnel_endpoint(&self, target: &SshTarget) -> Option<String> {
        let mut tunnel = self
            .tunnel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(open) = tunnel.as_mut() {
            if open.alive() {
                return Some(open.endpoint());
            }
        }
        *tunnel = None;
        if self.stopped() {
            return None;
        }
        let opened = Ssh::from_environment()
            .and_then(|ssh| ssh.open_tunnel(&target.host, target.remote_port))
            .ok()?;
        let endpoint = opened.endpoint();
        *tunnel = Some(opened);
        // A stop that ran while the tunnel opened finds no tunnel to close.
        if self.stopped() {
            *tunnel = None;
            return None;
        }
        Some(endpoint)
    }
}

/// Opens connections to one paired runtime.
#[derive(Clone)]
pub(crate) struct Connector {
    pub(crate) runtime_id: String,
    pub(crate) registry: Registry,
    pub(crate) ssh: Option<SshTarget>,
    pub(crate) control: Arc<RemoteControl>,
    pub(crate) observe: Observer,
}

impl Connector {
    /// Connects once with the saved device credential, through the tunnel
    /// for an SSH runtime.
    pub(crate) fn connect(&self) -> Result<DesktopClient, RuntimeStatus> {
        let mut record = self
            .registry
            .record(&self.runtime_id)
            .map_err(|_| RuntimeStatus::Offline)?;
        if let Some(target) = &self.ssh {
            record.endpoints = vec![self
                .control
                .tunnel_endpoint(target)
                .ok_or(RuntimeStatus::Offline)?];
        }
        connect_remote_runtime(&record, &client_options(None))
            .map_err(|error| RuntimeStatus::of_error(&error))
    }

    /// One supervised attempt: it reports what the runtime answered and,
    /// after a failure, waits out the backoff before the next attempt.
    fn attempt(&self) -> Option<DesktopClient> {
        if self.control.stopped() {
            return None;
        }
        match self.connect() {
            Ok(client) => {
                (self.observe)(&self.runtime_id, welcome(&client));
                Some(client)
            }
            Err(status) => {
                if !self.control.stopped() {
                    (self.observe)(&self.runtime_id, Observation::Status(status));
                }
                self.control.wait(status.backoff());
                None
            }
        }
    }
}

pub(crate) fn welcome(client: &DesktopClient) -> Observation {
    let info = client.runtime_info();
    Observation::Welcome {
        version: info
            .map(|info| info.runtime_version.clone())
            .or_else(|| Some(client.runtime_version().to_owned()).filter(|v| !v.is_empty())),
        install_kind: info
            .and_then(|info| info.install_kind)
            .map(|kind| kind.as_str().to_owned()),
        form_factor: info
            .and_then(|info| info.form_factor)
            .map(|shape| shape.as_str().to_owned()),
    }
}

/// The supervised connections to the selected paired runtime.
pub(crate) struct RemoteSupervisor {
    pub(crate) runtime_id: String,
    pub(crate) holder: DesktopClientHolder,
    connector: Connector,
    connected: Arc<AtomicBool>,
    stop: DesktopClientStopHandle,
    control: Arc<RemoteControl>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl RemoteSupervisor {
    /// Starts the desktop client and the chat-event subscription. `deliver`
    /// receives each event while the supervisor runs, and `connection`
    /// hears each change of the desktop client's connection.
    pub(crate) fn start(
        connector: Connector,
        deliver: impl Fn(SubscriptionEvent) + Send + 'static,
        connection: impl Fn(bool) + Send + 'static,
    ) -> Self {
        let runtime_id = connector.runtime_id.clone();
        let kept = connector.clone();
        let holder = DesktopClientHolder::new();
        let connected = Arc::new(AtomicBool::new(false));
        let stop = DesktopClientStopHandle::new();
        let control = Arc::clone(&connector.control);
        (connector.observe)(&runtime_id, Observation::Status(RuntimeStatus::Connecting));

        let client_connector = connector.clone();
        let client_holder = holder.clone();
        let client_stop = stop.clone();
        let client_connected = Arc::clone(&connected);
        let worker = std::thread::spawn(move || {
            let observer = client_connector.clone();
            serve_desktop_client_with(
                || client_connector.attempt(),
                client_stop,
                client_holder,
                RETRY_INTERVAL,
                move |online| {
                    client_connected.store(online, Ordering::SeqCst);
                    if !observer.control.stopped() {
                        if !online {
                            (observer.observe)(
                                &observer.runtime_id,
                                Observation::Status(RuntimeStatus::Connecting),
                            );
                        } else {
                            (observer.observe)(
                                &observer.runtime_id,
                                Observation::Status(RuntimeStatus::Online),
                            );
                        }
                        connection(online);
                    }
                },
            );
        });

        // A chat-event read can wait out the runtime's idle timeout, so this
        // thread is not joined. It drops every event once the supervisor
        // stops and ends at its next read.
        std::thread::spawn(move || serve_chat_events(&connector, deliver));

        Self {
            runtime_id,
            holder,
            connector: kept,
            connected,
            stop,
            control,
            worker: Some(worker),
        }
    }

    pub(crate) fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Opens connections of their own, through the same tunnel.
    pub(crate) fn connector(&self) -> Connector {
        self.connector.clone()
    }

    /// Stops both connections and closes the tunnel. The desktop client's
    /// worker ends in the background, since a connect can take a timeout.
    pub(crate) fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.control.stop();
        self.stop.stop();
        self.connected.store(false, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            std::thread::spawn(move || {
                let _ = worker.join();
            });
        }
    }
}

impl Drop for RemoteSupervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Reads the runtime's update capability and status, which an older runtime
/// does not send.
pub(crate) fn observe_update_info(connector: &Connector, client: &mut DesktopClient) {
    if let Ok(info) = client.read_runtime_info() {
        (connector.observe)(
            &connector.runtime_id,
            Observation::Update(UpdateInfo::from_runtime_info(&info)),
        );
    }
}

fn serve_chat_events(connector: &Connector, deliver: impl Fn(SubscriptionEvent)) {
    // Each reconnect resumes after the newest chat event already delivered.
    let mut cursor = None;
    while !connector.control.stopped() {
        if let Some(mut client) = connector.attempt() {
            observe_update_info(connector, &mut client);
            // An older runtime refuses the update status, and the
            // subscription then carries chat events only.
            if client
                .subscribe_chat_events_with_update_status(cursor)
                .is_ok()
            {
                while let Ok(event) = client.read_subscription_event() {
                    if connector.control.stopped() {
                        return;
                    }
                    match event {
                        SubscriptionEvent::RuntimeUpdateStatus(body) => {
                            if let Some(status) = UpdateStatus::from_body(&body) {
                                (connector.observe)(
                                    &connector.runtime_id,
                                    Observation::UpdateStatus(status),
                                );
                            }
                        }
                        event => deliver(event),
                    }
                }
                cursor = client.chat_cursor().or(cursor);
            }
        }
        if !connector.control.wait(RETRY_INTERVAL) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_errors_map_to_runtime_states() {
        assert_eq!(
            RuntimeStatus::of_error(&RemoteConnectError::Unreachable),
            RuntimeStatus::Offline
        );
        assert_eq!(
            RuntimeStatus::of_error(&RemoteConnectError::CertificateMismatch),
            RuntimeStatus::CertificateChanged
        );
        assert_eq!(
            RuntimeStatus::of_error(&RemoteConnectError::Unauthorized),
            RuntimeStatus::Revoked
        );
        assert_eq!(
            RuntimeStatus::of_error(&RemoteConnectError::RuntimeMismatch),
            RuntimeStatus::RuntimeMismatch
        );
        assert_eq!(
            RuntimeStatus::of_error(&RemoteConnectError::Client(
                ClientError::ProtocolIncompatible
            )),
            RuntimeStatus::VersionMismatch
        );
        assert_eq!(
            RuntimeStatus::of_error(&RemoteConnectError::Client(ClientError::ConnectionClosed)),
            RuntimeStatus::Offline
        );
        assert_eq!(
            serde_json::to_value(RuntimeStatus::CertificateChanged).unwrap(),
            "certificate_changed"
        );
        assert!(RuntimeStatus::Revoked.backoff() > RuntimeStatus::Offline.backoff());
    }

    #[test]
    fn a_stopped_control_wakes_its_waits_at_once() {
        let control = Arc::new(RemoteControl::default());
        let waiter = Arc::clone(&control);
        let started = std::time::Instant::now();
        let worker = std::thread::spawn(move || waiter.wait(Duration::from_secs(30)));
        std::thread::sleep(Duration::from_millis(50));
        control.stop();
        assert!(!worker.join().unwrap());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(control.stopped());
        assert!(!control.wait(Duration::from_secs(30)));
    }

    #[test]
    fn a_runtime_that_is_not_paired_reports_offline_and_stops_cleanly() {
        let directory =
            std::env::temp_dir().join(format!("muniment-remote-{}", uuid::Uuid::now_v7()));
        let observed = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&observed);
        let connector = Connector {
            runtime_id: "018f0000-0000-7000-8000-000000000042".into(),
            registry: Registry::new(directory.clone()),
            ssh: None,
            control: Arc::new(RemoteControl::default()),
            observe: Arc::new(move |_, observation| seen.lock().unwrap().push(observation)),
        };
        let supervisor = RemoteSupervisor::start(connector, |_| {}, |_| {});
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !observed
            .lock()
            .unwrap()
            .contains(&Observation::Status(RuntimeStatus::Offline))
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!supervisor.connected());
        assert_eq!(
            observed.lock().unwrap()[0],
            Observation::Status(RuntimeStatus::Connecting)
        );
        supervisor.stop();
        let _ = std::fs::remove_dir_all(directory);
    }
}
