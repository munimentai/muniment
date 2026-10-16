//! `muniment-server launch`: runs the server and installs the updates its
//! clients request.
//!
//! The launcher is the image's own binary and stays the same across the
//! versions it installs. It keeps each installed version under
//! `<state>/runtime/versions/<version>/` with `current` and `previous`
//! pointers beside them, and runs the current version, or its own binary
//! when that is newer or nothing is installed.
//!
//! While a request in `<state>/remote/runtime-update.json` names a newer
//! version, the launcher downloads the release, checks its signature and
//! digest, unpacks it and runs `--version` on it. It then sends the server
//! `SIGUSR1`, which drains it for up to the grace period and stops it, stops
//! what is left of its process group, snapshots the state databases and
//! starts the new version as a trial. The new version stays only when its
//! `health` check passes within the trial period. Otherwise the launcher
//! restores the snapshot, starts the earlier version again and never retries
//! that version. A failed download retries with a growing delay.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use muniment_core::attach::{
    clear_update_request_for, is_newer_version, read_update_request, read_update_status,
    update_timestamp, write_update_status, RuntimeUpdateRequest, RuntimeUpdateState,
    RuntimeUpdateStatus, UpdateCapability,
};

use crate::release::{self, Feed, ReleaseError};

/// The variable the launcher sets for the server it runs: `launcher` when it
/// installs updates, `manual` when it cannot.
pub const CAPABILITY_VARIABLE: &str = "MUNIMENT_UPDATE_CAPABILITY";
/// Seconds a drain waits for the runtime to go idle.
pub const GRACE_VARIABLE: &str = "MUNIMENT_UPDATE_GRACE_SECS";
/// Seconds the server has to stop after a drain or a stop request.
pub const SHUTDOWN_VARIABLE: &str = "MUNIMENT_UPDATE_SHUTDOWN_SECS";
/// Seconds a new version has to report healthy.
pub const TRIAL_VARIABLE: &str = "MUNIMENT_UPDATE_TRIAL_SECS";
/// Seconds before the first retry of a failed download. Each retry doubles
/// it, up to an hour.
pub const RETRY_VARIABLE: &str = "MUNIMENT_UPDATE_RETRY_SECS";

const UPDATE_DIRECTORY_NAME: &str = "runtime";
const VERSIONS_DIRECTORY_NAME: &str = "versions";
const SNAPSHOT_DIRECTORY_NAME: &str = "snapshot";
const TRIAL_FILE: &str = "trial.json";
const FAILED_FILE: &str = "failed-version";
const POINTER_HEADER: &str = "muniment-runtime-pointer-v1";
/// The pointer value that names the launcher's own binary.
const IMAGE: &str = "image";
const MAX_RETRY: Duration = Duration::from_secs(60 * 60);
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
/// Time past the grace period for the server to finish its own drain.
const DRAIN_MARGIN: Duration = Duration::from_secs(30);
/// Time the leftovers of a stopped server's process group have to exit.
const GROUP_TIMEOUT: Duration = Duration::from_secs(10);

/// How the launcher times its work and where it finds releases.
#[derive(Clone, Debug)]
pub struct Settings {
    pub grace: Duration,
    pub shutdown: Duration,
    pub trial: Duration,
    pub retry: Duration,
    pub poll: Duration,
    pub feed: Feed,
    pub public_key: Option<String>,
    pub arch: Option<&'static str>,
}

impl Settings {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            grace: seconds(GRACE_VARIABLE, 120)?,
            shutdown: seconds(SHUTDOWN_VARIABLE, 60)?,
            trial: seconds(TRIAL_VARIABLE, 120)?,
            retry: seconds(RETRY_VARIABLE, 60)?.max(Duration::from_secs(1)),
            poll: Duration::from_secs(1),
            feed: Feed::from_env(),
            public_key: release::public_key(),
            arch: release::host_arch(),
        })
    }
}

/// A whole number of seconds from `name`, or `default` when it is unset.
pub fn seconds(name: &str, default: u64) -> Result<Duration, String> {
    match std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        None => Ok(Duration::from_secs(default)),
        Some(value) => value
            .trim()
            .parse()
            .map(Duration::from_secs)
            .map_err(|_| format!("{name} must be a whole number of seconds, not {value}")),
    }
}

/// Where the launcher keeps installed versions and its own records.
#[derive(Clone, Debug)]
pub struct Layout {
    state: PathBuf,
    root: PathBuf,
    versions: PathBuf,
    remote: PathBuf,
}

impl Layout {
    pub fn new(state: &Path) -> Self {
        let root = state.join(UPDATE_DIRECTORY_NAME);
        Self {
            state: state.to_path_buf(),
            versions: root.join(VERSIONS_DIRECTORY_NAME),
            root,
            remote: crate::remote::remote_directory(state),
        }
    }

    /// Whether the launcher can install versions here.
    fn writable(&self) -> bool {
        let probe = self.versions.join(".probe");
        std::fs::create_dir_all(&self.versions)
            .and_then(|()| std::fs::write(&probe, b""))
            .and_then(|()| std::fs::remove_file(&probe))
            .is_ok()
    }

    fn binary(&self, version: &str) -> PathBuf {
        self.versions.join(version).join(release::SERVER_NAME)
    }

    fn pointer(&self, name: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.root.join(name)).ok()?;
        let mut lines = text.lines();
        (lines.next() == Some(POINTER_HEADER))
            .then(|| lines.next().map(str::to_owned))
            .flatten()
            .filter(|value| value == IMAGE || is_plain_version(value))
    }

    fn set_pointer(&self, name: &str, value: Option<&str>) -> std::io::Result<()> {
        let path = self.root.join(name);
        match value {
            None => match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            },
            Some(value) => write_atomic(&path, format!("{POINTER_HEADER}\n{value}\n").as_bytes()),
        }
    }

    fn failed_version(&self) -> Option<String> {
        std::fs::read_to_string(self.root.join(FAILED_FILE))
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|version| !version.is_empty())
    }

    fn trial(&self) -> Option<Trial> {
        std::fs::read(self.root.join(TRIAL_FILE))
            .ok()
            .and_then(|bytes| muniment_core::serde_json::from_slice(&bytes).ok())
    }
}

/// A version on trial and what ran before it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Trial {
    target: String,
    /// The version that ran before, or `image`.
    running: String,
    /// The `previous` pointer before the trial.
    previous: Option<String>,
}

/// What one update attempt came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    Installed,
    /// The attempt failed in a way that may pass. The request stays and the
    /// launcher retries it later.
    Retry(String),
    /// The version will never install. The runtime runs its earlier one.
    Failed(String),
    /// A stop request ended the attempt.
    Stopped,
}

/// The server process the launcher runs, in a process group of its own so
/// nothing it started outlives it.
pub struct ServerProcess {
    child: Child,
    binary: PathBuf,
    state: PathBuf,
}

impl ServerProcess {
    fn spawn(binary: &Path, state: &Path, capability: UpdateCapability) -> std::io::Result<Self> {
        use std::os::unix::process::CommandExt;
        let child = Command::new(binary)
            .arg("serve")
            .env("MUNIMENT_STATE_DIR", state)
            .env(CAPABILITY_VARIABLE, capability.as_str())
            .stdin(Stdio::null())
            .process_group(0)
            .spawn()?;
        Ok(Self {
            child,
            binary: binary.to_path_buf(),
            state: state.to_path_buf(),
        })
    }

    #[cfg(test)]
    fn id(&self) -> u32 {
        self.child.id()
    }

    fn signal(&self, signal: libc::c_int) {
        // SAFETY: kill only sends a signal to the child this value owns.
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, signal);
        }
    }

    fn exited(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    fn wait_exit(&mut self, timeout: Duration, poll: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.exited() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(poll);
        }
    }

    /// Asks the server to stop, kills it after `timeout`, then ends what is
    /// left of its process group.
    fn stop(&mut self, timeout: Duration, poll: Duration) -> ExitStatus {
        // A server that already exited has no process to signal, and its id
        // may belong to another process by now.
        let status = match self.exited() {
            Some(status) => status,
            None => {
                self.signal(libc::SIGTERM);
                match self.wait_exit(timeout, poll) {
                    Some(status) => status,
                    None => {
                        self.signal_group(libc::SIGKILL);
                        self.child
                            .wait()
                            .expect("the server child can be waited on")
                    }
                }
            }
        };
        self.end_group(poll);
        status
    }

    fn signal_group(&self, signal: libc::c_int) -> bool {
        // SAFETY: the negative id names the process group the child leads.
        unsafe { libc::kill(-(self.child.id() as libc::pid_t), signal) == 0 }
    }

    /// Ends the processes the server left in its group, such as an agent
    /// that would otherwise run on beside the next version.
    fn end_group(&self, poll: Duration) {
        if !self.signal_group(libc::SIGTERM) {
            return;
        }
        let deadline = Instant::now() + GROUP_TIMEOUT;
        while Instant::now() < deadline {
            if !self.signal_group(0) {
                return;
            }
            std::thread::sleep(poll);
        }
        self.signal_group(libc::SIGKILL);
    }

    /// Whether the server runs and its `health` check passes.
    fn healthy(&mut self) -> bool {
        self.exited().is_none()
            && Command::new(&self.binary)
                .arg("health")
                .env("MUNIMENT_STATE_DIR", &self.state)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    }
}

/// Runs the server and applies its update requests.
pub struct Launcher {
    layout: Layout,
    settings: Settings,
    image_binary: PathBuf,
    image_version: String,
    capability: UpdateCapability,
    /// The installed version that runs, or `image`.
    running: String,
    child: Option<ServerProcess>,
    retry: Option<Retry>,
}

#[derive(Clone, Debug)]
struct Retry {
    version: String,
    at: Instant,
    delay: Duration,
}

impl Launcher {
    pub fn new(
        state: &Path,
        settings: Settings,
        image_binary: PathBuf,
        image_version: &str,
    ) -> Self {
        let layout = Layout::new(state);
        let capability =
            if settings.public_key.is_some() && settings.arch.is_some() && layout.writable() {
                UpdateCapability::Launcher
            } else {
                UpdateCapability::Manual
            };
        Self {
            layout,
            settings,
            image_binary,
            image_version: image_version.to_owned(),
            capability,
            running: IMAGE.to_owned(),
            child: None,
            retry: None,
        }
    }

    pub fn capability(&self) -> UpdateCapability {
        self.capability
    }

    /// The version the server runs.
    pub fn running_version(&self) -> &str {
        if self.running == IMAGE {
            &self.image_version
        } else {
            &self.running
        }
    }

    #[cfg(test)]
    fn child(&self) -> Option<&ServerProcess> {
        self.child.as_ref()
    }

    /// Settles what an earlier launcher left: a trial it never finished
    /// rolls back, and a status it left mid-update starts over.
    pub fn recover(&mut self) {
        if let Some(trial) = self.layout.trial() {
            eprintln!(
                "muniment-server: launcher found an unfinished trial target_version={}",
                trial.target
            );
            let restored = self.restore_snapshot();
            let reverted = self.revert_pointers(&trial);
            let _ = std::fs::remove_file(self.layout.root.join(TRIAL_FILE));
            let detail = match restored.and(reverted) {
                Ok(()) => format!(
                    "The runtime stopped during the trial of {}, so it runs its earlier version.",
                    trial.target
                ),
                Err(error) => format!(
                    "The runtime stopped during the trial of {} and could not restore its state: {error}",
                    trial.target
                ),
            };
            self.fail_version(&trial.target, detail);
            return;
        }
        if self.capability != UpdateCapability::Launcher {
            return;
        }
        let in_flight = read_update_status(&self.layout.remote).is_some_and(|status| {
            matches!(
                status.state,
                RuntimeUpdateState::Downloading
                    | RuntimeUpdateState::Draining
                    | RuntimeUpdateState::Installing
                    | RuntimeUpdateState::Restarting
            )
        });
        if in_flight {
            match read_update_request(&self.layout.remote) {
                Some(request) => {
                    self.set_status(RuntimeUpdateState::Queued, Some(&request.version), None)
                }
                None => self.set_status(RuntimeUpdateState::Idle, None, None),
            }
        }
    }

    /// The binary to run: the current installed version when it is newer
    /// than the launcher's own, else the launcher's own.
    fn select(&self) -> (String, PathBuf) {
        if let Some(version) = self
            .layout
            .pointer("current")
            .filter(|value| value != IMAGE)
        {
            let binary = self.layout.binary(&version);
            if is_newer_version(&version, &self.image_version) && binary.is_file() {
                return (version, binary);
            }
        }
        (IMAGE.to_owned(), self.image_binary.clone())
    }

    /// Starts the server the pointers select.
    pub fn start(&mut self) -> std::io::Result<()> {
        let (running, binary) = self.select();
        self.child = Some(ServerProcess::spawn(
            &binary,
            &self.layout.state,
            self.capability,
        )?);
        self.running = running;
        eprintln!(
            "muniment-server: launcher started version={} binary={} update_capability={}",
            self.running_version(),
            binary.display(),
            self.capability.as_str()
        );
        Ok(())
    }

    /// The exit status of the server when it stopped on its own.
    pub fn exited(&mut self) -> Option<ExitStatus> {
        self.child.as_mut().and_then(ServerProcess::exited)
    }

    /// Stops the server and answers its exit status.
    pub fn stop(&mut self) -> Option<ExitStatus> {
        let mut child = self.child.take()?;
        Some(child.stop(self.settings.shutdown, self.settings.poll))
    }

    /// The request the launcher should apply now: one that names a version
    /// newer than the running one, that never failed, and whose retry is
    /// due. A request for a version that failed is cleared.
    pub fn due_request(&mut self) -> Option<RuntimeUpdateRequest> {
        if self.capability != UpdateCapability::Launcher {
            return None;
        }
        let request = read_update_request(&self.layout.remote)?;
        if !is_newer_version(&request.version, self.running_version()) {
            return None;
        }
        if let Some(failed) = self.layout.failed_version() {
            if !is_newer_version(&request.version, &failed) {
                let _ = clear_update_request_for(&self.layout.remote, &request.version);
                self.set_status(
                    RuntimeUpdateState::Failed,
                    Some(&request.version),
                    Some(format!(
                        "Version {} failed before, so the runtime waits for a newer release.",
                        request.version
                    )),
                );
                return None;
            }
        }
        match &self.retry {
            Some(retry) if retry.version == request.version && Instant::now() < retry.at => None,
            _ => Some(request),
        }
    }

    /// Downloads, verifies and installs `request`'s version, and runs it.
    pub fn apply(
        &mut self,
        request: &RuntimeUpdateRequest,
        stopping: &AtomicBool,
    ) -> UpdateOutcome {
        let target = request.version.as_str();
        eprintln!("muniment-server: launcher update started target_version={target}");
        let outcome = self.install(target, stopping);
        match &outcome {
            UpdateOutcome::Installed => {
                self.retry = None;
                eprintln!("muniment-server: launcher update installed version={target}");
            }
            UpdateOutcome::Retry(error) => {
                let delay = match &self.retry {
                    Some(retry) if retry.version == target => (retry.delay * 2).min(MAX_RETRY),
                    _ => self.settings.retry,
                };
                self.retry = Some(Retry {
                    version: target.to_owned(),
                    at: Instant::now() + delay,
                    delay,
                });
                self.set_status(
                    RuntimeUpdateState::Queued,
                    Some(target),
                    Some(format!(
                        "{error} The runtime retries in {} seconds.",
                        delay.as_secs()
                    )),
                );
                eprintln!(
                    "muniment-server: launcher update deferred target_version={target} retry_seconds={} error={error}",
                    delay.as_secs()
                );
            }
            UpdateOutcome::Failed(error) => {
                eprintln!(
                    "muniment-server: launcher update failed target_version={target} error={error}"
                );
            }
            UpdateOutcome::Stopped => {
                eprintln!("muniment-server: launcher update stopped target_version={target}");
            }
        }
        outcome
    }

    fn install(&mut self, target: &str, stopping: &AtomicBool) -> UpdateOutcome {
        let (Some(public_key), Some(arch)) = (self.settings.public_key.clone(), self.settings.arch)
        else {
            return UpdateOutcome::Retry("This build cannot verify releases.".into());
        };
        self.set_status(RuntimeUpdateState::Downloading, Some(target), None);
        let archive_name = release::archive_name(target, arch);
        let fetched = (|| {
            let sums = self.settings.feed.fetch(target, release::SUMS_NAME)?;
            let signature = self.settings.feed.fetch(target, release::SIGNATURE_NAME)?;
            let archive = self.settings.feed.fetch(target, &archive_name)?;
            release::verify_release(&public_key, &sums, &signature, &archive_name, &archive)?;
            Ok::<_, ReleaseError>(archive)
        })();
        let archive = match fetched {
            Ok(archive) => archive,
            Err(error) if error.is_transient() => {
                return UpdateOutcome::Retry(format!("Version {target}: {error}."))
            }
            Err(error) => return self.fail_version(target, format!("Version {target}: {error}.")),
        };
        if stopping.load(Ordering::SeqCst) {
            return self.stopped(target);
        }
        let staging = self.layout.versions.join(format!(".staging-{target}"));
        let _ = std::fs::remove_dir_all(&staging);
        if let Err(error) = release::unpack(&archive, &staging) {
            let _ = std::fs::remove_dir_all(&staging);
            return self.fail_version(target, format!("Version {target}: {error}."));
        }
        drop(archive);
        if let Err(error) = preflight(&staging.join(release::SERVER_NAME), target) {
            let _ = std::fs::remove_dir_all(&staging);
            return self.fail_version(target, format!("Version {target} {error}."));
        }
        let destination = self.layout.versions.join(target);
        let _ = std::fs::remove_dir_all(&destination);
        if let Err(error) = std::fs::rename(&staging, &destination) {
            let _ = std::fs::remove_dir_all(&staging);
            return UpdateOutcome::Retry(format!("Version {target} could not be staged: {error}."));
        }
        if stopping.load(Ordering::SeqCst) {
            return self.stopped(target);
        }

        // From here the running server stops, so every path ends with a
        // server running again.
        self.set_status(RuntimeUpdateState::Draining, Some(target), None);
        if let Some(mut child) = self.child.take() {
            child.signal(libc::SIGUSR1);
            let drained = child
                .wait_exit(self.settings.grace + DRAIN_MARGIN, self.settings.poll)
                .is_some();
            if drained {
                child.end_group(self.settings.poll);
            } else {
                child.stop(self.settings.shutdown, self.settings.poll);
            }
        }
        self.set_status(RuntimeUpdateState::Installing, Some(target), None);
        let trial = Trial {
            target: target.to_owned(),
            running: self.running.clone(),
            previous: self.layout.pointer("previous"),
        };
        let prepared = self.take_snapshot().and_then(|()| {
            let bytes = muniment_core::serde_json::to_vec(&trial).map_err(std::io::Error::other)?;
            write_atomic(&self.layout.root.join(TRIAL_FILE), &bytes)?;
            let previous = (trial.running != IMAGE).then_some(trial.running.as_str());
            self.layout.set_pointer("previous", previous)?;
            self.layout.set_pointer("current", Some(target))
        });
        if let Err(error) = prepared {
            let _ = self.revert_pointers(&trial);
            let _ = std::fs::remove_file(self.layout.root.join(TRIAL_FILE));
            return self.restart_after_failure(
                target,
                format!("Version {target} could not be installed: {error}."),
            );
        }

        self.set_status(RuntimeUpdateState::Restarting, Some(target), None);
        let healthy = match ServerProcess::spawn(
            &self.layout.binary(target),
            &self.layout.state,
            self.capability,
        ) {
            Ok(mut child) => {
                let deadline = Instant::now() + self.settings.trial;
                let healthy = loop {
                    if child.healthy() {
                        break true;
                    }
                    if child.exited().is_some()
                        || Instant::now() >= deadline
                        || stopping.load(Ordering::SeqCst)
                    {
                        break false;
                    }
                    std::thread::sleep(self.settings.poll);
                };
                if healthy {
                    self.child = Some(child);
                } else {
                    child.stop(self.settings.shutdown, self.settings.poll);
                }
                healthy
            }
            Err(_) => false,
        };
        if !healthy && stopping.load(Ordering::SeqCst) {
            // The next launcher rolls the unfinished trial back.
            return UpdateOutcome::Stopped;
        }
        if healthy {
            self.running = target.to_owned();
            let _ = std::fs::remove_file(self.layout.root.join(TRIAL_FILE));
            let _ = std::fs::remove_dir_all(self.layout.root.join(SNAPSHOT_DIRECTORY_NAME));
            self.prune();
            let _ = clear_update_request_for(&self.layout.remote, target);
            self.set_status(RuntimeUpdateState::Idle, None, None);
            return UpdateOutcome::Installed;
        }
        let restored = self.restore_snapshot();
        let reverted = self.revert_pointers(&trial);
        let _ = std::fs::remove_file(self.layout.root.join(TRIAL_FILE));
        let error = match restored.and(reverted) {
            Ok(()) => format!(
                "Version {target} did not report healthy, so the runtime runs {} again.",
                self.running_version()
            ),
            Err(error) => format!(
                "Version {target} did not report healthy, and the runtime could not restore its state: {error}"
            ),
        };
        self.restart_after_failure(target, error)
    }

    fn stopped(&mut self, target: &str) -> UpdateOutcome {
        self.set_status(RuntimeUpdateState::Queued, Some(target), None);
        UpdateOutcome::Stopped
    }

    /// Starts the version that ran before and marks `target` failed.
    fn restart_after_failure(&mut self, target: &str, error: String) -> UpdateOutcome {
        let outcome = self.fail_version(target, error);
        if let Err(error) = self.start() {
            eprintln!("muniment-server: launcher could not restart the server: {error}");
        }
        outcome
    }

    /// Records that `target` never installs, clears its request and reports
    /// the failure.
    fn fail_version(&mut self, target: &str, error: String) -> UpdateOutcome {
        if let Err(error) = write_atomic(&self.layout.root.join(FAILED_FILE), target.as_bytes()) {
            eprintln!("muniment-server: launcher could not record the failed version: {error}");
        }
        let _ = clear_update_request_for(&self.layout.remote, target);
        self.set_status(
            RuntimeUpdateState::Failed,
            Some(target),
            Some(error.clone()),
        );
        UpdateOutcome::Failed(error)
    }

    fn revert_pointers(&self, trial: &Trial) -> std::io::Result<()> {
        let current = (trial.running != IMAGE).then_some(trial.running.as_str());
        self.layout.set_pointer("current", current)?;
        self.layout
            .set_pointer("previous", trial.previous.as_deref())?;
        let _ = std::fs::remove_dir_all(self.layout.versions.join(&trial.target));
        Ok(())
    }

    /// Removes installed versions that neither pointer names.
    fn prune(&self) {
        let keep: Vec<String> = ["current", "previous"]
            .iter()
            .filter_map(|name| self.layout.pointer(name))
            .collect();
        let Ok(entries) = std::fs::read_dir(&self.layout.versions) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !keep.contains(&name) && is_plain_version(&name) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    fn take_snapshot(&self) -> std::io::Result<()> {
        let snapshot = self.layout.root.join(SNAPSHOT_DIRECTORY_NAME);
        let _ = std::fs::remove_dir_all(&snapshot);
        std::fs::create_dir_all(&snapshot)?;
        for relative in databases(&self.layout.state, &self.layout.root) {
            let target = snapshot.join(&relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(self.layout.state.join(&relative), target)?;
        }
        Ok(())
    }

    /// Puts the databases back as the snapshot holds them, and removes the
    /// database files the trial created.
    fn restore_snapshot(&self) -> std::io::Result<()> {
        let snapshot = self.layout.root.join(SNAPSHOT_DIRECTORY_NAME);
        if !snapshot.is_dir() {
            return Ok(());
        }
        let saved = databases(&snapshot, &snapshot.join(UPDATE_DIRECTORY_NAME));
        for relative in databases(&self.layout.state, &self.layout.root) {
            if !saved.contains(&relative) {
                std::fs::remove_file(self.layout.state.join(&relative))?;
            }
        }
        for relative in &saved {
            let target = self.layout.state.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(snapshot.join(relative), target)?;
        }
        std::fs::remove_dir_all(&snapshot)
    }

    fn set_status(&self, state: RuntimeUpdateState, target: Option<&str>, error: Option<String>) {
        let status = RuntimeUpdateStatus {
            state,
            target_version: target.map(str::to_owned),
            blockers: Vec::new(),
            error,
            updated_at: update_timestamp(),
        };
        if let Err(error) = write_update_status(&self.layout.remote, &status) {
            eprintln!("muniment-server: launcher could not save the update status: {error}");
        }
    }
}

/// Runs the new binary's `--version` and checks that it names `target`.
fn preflight(binary: &Path, target: &str) -> Result<(), String> {
    let mut child = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("did not start: {error}"))?;
    let deadline = Instant::now() + PREFLIGHT_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("did not answer --version in time".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut stdout, &mut output);
    }
    if !status.success() {
        return Err(format!("failed its --version check with {status}"));
    }
    if output.trim() != target {
        return Err(format!("reports version {}", output.trim()));
    }
    Ok(())
}

/// The SQLite files under `state`, relative to it, outside `skip` and the
/// directories that hold no state databases.
fn databases(state: &Path, skip: &Path) -> Vec<PathBuf> {
    fn walk(directory: &Path, state: &Path, skip: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if depth > 4 || directory == skip {
            return;
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                if !matches!(name.as_str(), "run" | "harness" | "node_modules" | ".bun") {
                    walk(&path, state, skip, depth + 1, found);
                }
            } else if kind.is_file() && is_database(&name) {
                if let Ok(relative) = path.strip_prefix(state) {
                    found.push(relative.to_path_buf());
                }
            }
        }
    }
    let mut found = Vec::new();
    walk(state, state, skip, 0, &mut found);
    found.sort();
    found
}

fn is_database(name: &str) -> bool {
    let base = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .or_else(|| name.strip_suffix("-journal"))
        .unwrap_or(name);
    [".sqlite3", ".sqlite", ".db"]
        .iter()
        .any(|suffix| base.ends_with(suffix))
}

fn is_plain_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.starts_with(|c: char| c.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    muniment_core::atomic_file::replace(&temporary, path)
}

/// Runs the launcher until SIGTERM or SIGINT, or until the server stops on
/// its own, and answers the exit code the server ended with.
pub fn launch() -> Result<u8, String> {
    use muniment_core::attach::TerminationSignalWait;

    // The signal mask must be set before any thread starts.
    let termination = TerminationSignalWait::new().map_err(|error| error.to_string())?;
    let state = muniment_core::state_root::state_directory()
        .ok_or("no state directory: set MUNIMENT_STATE_DIR or HOME")?;
    let settings = Settings::from_env()?;
    let image_binary = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut launcher = Launcher::new(
        &state,
        settings.clone(),
        image_binary,
        crate::remote::RUNTIME_VERSION,
    );
    if launcher.capability() == UpdateCapability::Manual {
        let reason = if settings.public_key.is_none() {
            "this build carries no release public key"
        } else if settings.arch.is_none() {
            "no release exists for this architecture"
        } else {
            "the versions directory is not writable"
        };
        eprintln!("muniment-server: launcher installs no updates: {reason}");
    }
    let stopping = std::sync::Arc::new(AtomicBool::new(false));
    let (stop, stopped) = std::sync::mpsc::channel();
    let flag = std::sync::Arc::clone(&stopping);
    std::thread::spawn(move || {
        if termination.wait().is_ok() {
            flag.store(true, Ordering::SeqCst);
            let _ = stop.send(());
        }
    });
    launcher.recover();
    launcher
        .start()
        .map_err(|error| format!("the server did not start: {error}"))?;
    loop {
        if stopping.load(Ordering::SeqCst) {
            eprintln!("muniment-server: launcher stopping reason=signal");
            return Ok(launcher.stop().map_or(0, exit_code));
        }
        if let Some(status) = launcher.exited() {
            eprintln!("muniment-server: launcher stopping reason=server_exited status={status}");
            launcher.stop();
            return Ok(exit_code(status));
        }
        if let Some(request) = launcher.due_request() {
            launcher.apply(&request, &stopping);
            continue;
        }
        let _ = stopped.recv_timeout(settings.poll * 5);
    }
}

fn exit_code(status: ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    match status.code() {
        Some(code) => u8::try_from(code).unwrap_or(1),
        None => status
            .signal()
            .map_or(1, |signal| (128 + signal).min(255) as u8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::tests::{archive, sums, TestKey};

    const IMAGE_VERSION: &str = "1.0.0";

    /// A stand-in server: `serve` records its version in `serving` and runs
    /// until SIGUSR1 or SIGTERM, `health` passes while it serves, and
    /// `--version` prints its version.
    fn script(version: &str, reported: &str, healthy: bool) -> String {
        format!(
            r#"#!/bin/sh
case "$1" in
  --version) echo "{reported}" ;;
  serve)
    echo "{version}" > "$MUNIMENT_STATE_DIR/serving"
    {write}
    trap 'exit 0' USR1 TERM
    while :; do sleep 0.05; done ;;
  health)
    {health} ;;
esac
"#,
            write = if healthy {
                ""
            } else {
                r#"echo trial > "$MUNIMENT_STATE_DIR/runs.sqlite3"; echo trial > "$MUNIMENT_STATE_DIR/new.sqlite3""#
            },
            health = if healthy {
                r#"test "$(cat "$MUNIMENT_STATE_DIR/serving")" = "{version}""#
                    .replace("{version}", version)
            } else {
                "exit 1".to_owned()
            },
        )
    }

    struct Fixture {
        directory: PathBuf,
        state: PathBuf,
        feed: PathBuf,
        key: TestKey,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir().join(format!(
                "muniment-launcher-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let state = directory.join("state");
            let feed = directory.join("feed");
            std::fs::create_dir_all(&state).unwrap();
            std::fs::create_dir_all(&feed).unwrap();
            std::fs::write(state.join("runs.sqlite3"), b"before").unwrap();
            let image = directory.join("image");
            std::fs::create_dir_all(&image).unwrap();
            let binary = image.join(release::SERVER_NAME);
            std::fs::write(&binary, script(IMAGE_VERSION, IMAGE_VERSION, true)).unwrap();
            make_executable(&binary);
            Self {
                directory,
                state,
                feed,
                key: TestKey::new(),
            }
        }

        fn image_binary(&self) -> PathBuf {
            self.directory.join("image").join(release::SERVER_NAME)
        }

        /// Publishes a signed release whose server reports `reported` and
        /// passes its health check when `healthy`.
        fn publish(&self, version: &str, reported: &str, healthy: bool) {
            let name = release::archive_name(version, release::host_arch().unwrap());
            let body = script(version, reported, healthy);
            let archive = archive(&[
                (release::SERVER_NAME, body.as_bytes(), 0o755),
                ("libstub.so", b"library", 0o644),
            ]);
            let sums = sums(&[(&name, &archive)]);
            let directory = self.feed.join(format!("v{version}"));
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(&name), &archive).unwrap();
            std::fs::write(
                directory.join(release::SIGNATURE_NAME),
                self.key.sign(&sums),
            )
            .unwrap();
            std::fs::write(directory.join(release::SUMS_NAME), sums).unwrap();
        }

        fn launcher(&self) -> Launcher {
            let settings = Settings {
                grace: Duration::from_secs(5),
                shutdown: Duration::from_secs(5),
                trial: Duration::from_secs(3),
                retry: Duration::from_secs(60),
                poll: Duration::from_millis(50),
                feed: Feed::new(&format!("file://{}", self.feed.display())),
                public_key: Some(self.key.public()),
                arch: release::host_arch(),
            };
            Launcher::new(&self.state, settings, self.image_binary(), IMAGE_VERSION)
        }

        fn request(&self, version: &str) -> RuntimeUpdateRequest {
            let request = RuntimeUpdateRequest {
                version: version.into(),
                requested_at: "2026-01-01T00:00:00Z".into(),
                requested_by: "local".into(),
            };
            let remote = crate::remote::remote_directory(&self.state);
            std::fs::create_dir_all(&remote).unwrap();
            let path = remote.join("runtime-update.json");
            std::fs::write(&path, muniment_core::serde_json::to_vec(&request).unwrap()).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&remote, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            request
        }

        fn status(&self) -> RuntimeUpdateStatus {
            read_update_status(&crate::remote::remote_directory(&self.state)).unwrap()
        }

        fn pending(&self) -> Option<RuntimeUpdateRequest> {
            read_update_request(&crate::remote::remote_directory(&self.state))
        }

        fn serving(&self) -> String {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(text) = std::fs::read_to_string(self.state.join("serving")) {
                    if !text.trim().is_empty() {
                        return text.trim().to_owned();
                    }
                }
                assert!(Instant::now() < deadline, "no server came up");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn started(fixture: &Fixture) -> Launcher {
        let mut launcher = fixture.launcher();
        assert_eq!(launcher.capability(), UpdateCapability::Launcher);
        launcher.recover();
        launcher.start().unwrap();
        assert_eq!(fixture.serving(), IMAGE_VERSION);
        launcher
    }

    #[test]
    fn a_signed_release_installs_and_runs() {
        let fixture = Fixture::new("install");
        fixture.publish("1.1.0", "1.1.0", true);
        let mut launcher = started(&fixture);
        let request = fixture.request("1.1.0");
        assert_eq!(launcher.due_request(), Some(request.clone()));
        std::fs::remove_file(fixture.state.join("serving")).unwrap();

        let outcome = launcher.apply(&request, &AtomicBool::new(false));

        assert_eq!(outcome, UpdateOutcome::Installed);
        assert_eq!(fixture.serving(), "1.1.0");
        assert_eq!(launcher.running_version(), "1.1.0");
        let layout = Layout::new(&fixture.state);
        assert_eq!(layout.pointer("current").as_deref(), Some("1.1.0"));
        assert_eq!(layout.pointer("previous"), None);
        assert!(layout.binary("1.1.0").is_file());
        assert!(layout.versions.join("1.1.0/libstub.so").is_file());
        assert_eq!(fixture.status().state, RuntimeUpdateState::Idle);
        assert_eq!(fixture.pending(), None);
        assert_eq!(launcher.due_request(), None);
        assert!(!layout.root.join(SNAPSHOT_DIRECTORY_NAME).exists());

        // A second update keeps the first as the previous version.
        fixture.publish("1.2.0", "1.2.0", true);
        let request = fixture.request("1.2.0");
        assert_eq!(
            launcher.apply(&request, &AtomicBool::new(false)),
            UpdateOutcome::Installed
        );
        assert_eq!(layout.pointer("current").as_deref(), Some("1.2.0"));
        assert_eq!(layout.pointer("previous").as_deref(), Some("1.1.0"));
        launcher.stop();

        // A new launcher runs the installed version.
        let mut again = fixture.launcher();
        again.start().unwrap();
        assert_eq!(again.running_version(), "1.2.0");
        again.stop();
    }

    #[test]
    fn a_release_that_fails_preflight_never_stops_the_server() {
        let fixture = Fixture::new("preflight");
        fixture.publish("1.1.0", "0.9.0", true);
        let mut launcher = started(&fixture);
        let pid = launcher.child().unwrap().id();
        let request = fixture.request("1.1.0");

        let outcome = launcher.apply(&request, &AtomicBool::new(false));

        assert!(
            matches!(outcome, UpdateOutcome::Failed(ref error) if error.contains("reports version 0.9.0"))
        );
        assert_eq!(launcher.child().unwrap().id(), pid);
        assert!(launcher.exited().is_none());
        let layout = Layout::new(&fixture.state);
        assert_eq!(layout.pointer("current"), None);
        assert!(!layout.versions.join("1.1.0").exists());
        assert_eq!(layout.failed_version().as_deref(), Some("1.1.0"));
        let status = fixture.status();
        assert_eq!(status.state, RuntimeUpdateState::Failed);
        assert_eq!(status.target_version.as_deref(), Some("1.1.0"));
        assert_eq!(fixture.pending(), None);
        launcher.stop();
    }

    #[test]
    fn a_release_that_fails_its_trial_rolls_back() {
        let fixture = Fixture::new("trial");
        fixture.publish("1.1.0", "1.1.0", false);
        let mut launcher = started(&fixture);
        std::fs::create_dir_all(fixture.state.join("memory/cache")).unwrap();
        std::fs::write(fixture.state.join("memory/cache/index.sqlite3"), b"index").unwrap();
        let request = fixture.request("1.1.0");
        std::fs::remove_file(fixture.state.join("serving")).unwrap();

        let outcome = launcher.apply(&request, &AtomicBool::new(false));

        assert!(
            matches!(outcome, UpdateOutcome::Failed(ref error) if error.contains("did not report healthy"))
        );
        // The databases are as they were before the trial wrote to them.
        assert_eq!(
            std::fs::read(fixture.state.join("runs.sqlite3")).unwrap(),
            b"before"
        );
        assert!(!fixture.state.join("new.sqlite3").exists());
        assert_eq!(
            std::fs::read(fixture.state.join("memory/cache/index.sqlite3")).unwrap(),
            b"index"
        );
        // The image's own version runs again.
        let deadline = Instant::now() + Duration::from_secs(5);
        while fixture.serving() != IMAGE_VERSION {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(launcher.running_version(), IMAGE_VERSION);
        let layout = Layout::new(&fixture.state);
        assert_eq!(layout.pointer("current"), None);
        assert!(!layout.versions.join("1.1.0").exists());
        assert!(layout.trial().is_none());
        assert_eq!(layout.failed_version().as_deref(), Some("1.1.0"));
        assert_eq!(fixture.status().state, RuntimeUpdateState::Failed);
        launcher.stop();
    }

    #[test]
    fn a_failed_version_waits_for_a_newer_release() {
        let fixture = Fixture::new("skip");
        fixture.publish("1.1.0", "0.9.0", true);
        let mut launcher = started(&fixture);
        let request = fixture.request("1.1.0");
        assert!(matches!(
            launcher.apply(&request, &AtomicBool::new(false)),
            UpdateOutcome::Failed(_)
        ));

        // The same version again is refused and its request cleared.
        fixture.request("1.1.0");
        assert_eq!(launcher.due_request(), None);
        assert_eq!(fixture.pending(), None);
        let status = fixture.status();
        assert_eq!(status.state, RuntimeUpdateState::Failed);
        assert!(status.error.unwrap().contains("waits for a newer release"));

        // A newer release installs.
        fixture.publish("1.1.1", "1.1.1", true);
        let request = fixture.request("1.1.1");
        assert_eq!(launcher.due_request(), Some(request.clone()));
        assert_eq!(
            launcher.apply(&request, &AtomicBool::new(false)),
            UpdateOutcome::Installed
        );
        launcher.stop();
    }

    #[test]
    fn a_missing_release_retries_later() {
        let fixture = Fixture::new("retry");
        let mut launcher = started(&fixture);
        let request = fixture.request("1.1.0");
        let pid = launcher.child().unwrap().id();

        let outcome = launcher.apply(&request, &AtomicBool::new(false));

        assert!(matches!(outcome, UpdateOutcome::Retry(_)));
        assert_eq!(launcher.child().unwrap().id(), pid);
        assert_eq!(fixture.pending(), Some(request));
        let status = fixture.status();
        assert_eq!(status.state, RuntimeUpdateState::Queued);
        assert!(status.error.unwrap().contains("retries in 60 seconds"));
        assert_eq!(launcher.due_request(), None);
        assert_eq!(Layout::new(&fixture.state).failed_version(), None);
        launcher.stop();
    }

    #[test]
    fn a_tampered_release_never_installs() {
        let fixture = Fixture::new("tampered");
        fixture.publish("1.1.0", "1.1.0", true);
        let name = release::archive_name("1.1.0", release::host_arch().unwrap());
        let path = fixture.feed.join("v1.1.0").join(&name);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.push(0);
        std::fs::write(&path, bytes).unwrap();
        let mut launcher = started(&fixture);
        let request = fixture.request("1.1.0");

        let outcome = launcher.apply(&request, &AtomicBool::new(false));

        assert!(matches!(outcome, UpdateOutcome::Failed(ref error) if error.contains("digest")));
        assert_eq!(fixture.serving(), IMAGE_VERSION);
        launcher.stop();
    }

    #[test]
    fn a_trial_the_launcher_never_finished_rolls_back() {
        let fixture = Fixture::new("recover");
        let layout = Layout::new(&fixture.state);
        std::fs::create_dir_all(layout.versions.join("1.1.0")).unwrap();
        std::fs::create_dir_all(layout.root.join(SNAPSHOT_DIRECTORY_NAME)).unwrap();
        std::fs::write(
            layout
                .root
                .join(SNAPSHOT_DIRECTORY_NAME)
                .join("runs.sqlite3"),
            b"snapshot",
        )
        .unwrap();
        layout.set_pointer("current", Some("1.1.0")).unwrap();
        let trial = Trial {
            target: "1.1.0".into(),
            running: IMAGE.into(),
            previous: None,
        };
        write_atomic(
            &layout.root.join(TRIAL_FILE),
            &muniment_core::serde_json::to_vec(&trial).unwrap(),
        )
        .unwrap();

        let mut launcher = fixture.launcher();
        launcher.recover();

        assert_eq!(layout.pointer("current"), None);
        assert!(layout.trial().is_none());
        assert_eq!(
            std::fs::read(fixture.state.join("runs.sqlite3")).unwrap(),
            b"snapshot"
        );
        assert_eq!(layout.failed_version().as_deref(), Some("1.1.0"));
        launcher.start().unwrap();
        assert_eq!(launcher.running_version(), IMAGE_VERSION);
        launcher.stop();
    }

    #[test]
    fn a_newer_image_wins_over_an_older_installed_version() {
        let fixture = Fixture::new("image");
        let layout = Layout::new(&fixture.state);
        std::fs::create_dir_all(layout.versions.join("0.9.0")).unwrap();
        std::fs::write(layout.binary("0.9.0"), script("0.9.0", "0.9.0", true)).unwrap();
        make_executable(&layout.binary("0.9.0"));
        layout.set_pointer("current", Some("0.9.0")).unwrap();
        let mut launcher = fixture.launcher();
        launcher.start().unwrap();
        assert_eq!(launcher.running_version(), IMAGE_VERSION);
        assert_eq!(fixture.serving(), IMAGE_VERSION);
        launcher.stop();
    }

    #[test]
    fn a_launcher_without_a_key_installs_nothing() {
        let fixture = Fixture::new("manual");
        let mut settings = fixture.launcher().settings;
        settings.public_key = None;
        let mut launcher = Launcher::new(
            &fixture.state,
            settings,
            fixture.image_binary(),
            IMAGE_VERSION,
        );
        assert_eq!(launcher.capability(), UpdateCapability::Manual);
        fixture.request("1.1.0");
        assert_eq!(launcher.due_request(), None);
    }

    #[test]
    fn databases_include_sqlite_files_and_their_journals() {
        assert!(is_database("runs.sqlite3"));
        assert!(is_database("runs.sqlite3-wal"));
        assert!(is_database("index.sqlite3-shm"));
        assert!(is_database("state.db"));
        assert!(!is_database("access.json"));
        assert!(!is_database("runs.sqlite3.tmp"));
    }
}
