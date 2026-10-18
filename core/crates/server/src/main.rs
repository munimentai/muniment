//! The headless Muniment runtime.
//!
//! `muniment-server serve` runs the runtime with a host that has no cloud
//! account, no keychain and no OS service manager: secrets live in files
//! under the state root, with overrides from the environment. State lives in
//! the directory `MUNIMENT_STATE_DIR` names, or `~/.muniment`. The Home
//! folder, which holds agents, memory and projects, is `home` under the state
//! root unless `home.json` names another. The attach endpoint sits under
//! `XDG_RUNTIME_DIR`, or under `run` in the state root when that is unset.
//! SIGTERM and SIGINT stop it cleanly.
//!
//! `pair`, `devices`, `revoke` and `info` manage remote access on the same
//! state directory while `serve` runs in another process.
//!
//! `launch` runs `serve` under a launcher that installs the signed releases
//! clients request, as `launcher.rs` describes.

mod host;
#[cfg(unix)]
mod launcher;
#[cfg(unix)]
mod release;
mod remote;

use std::process::ExitCode;

const USAGE: &str = "\
usage: muniment-server [command]

commands:
  serve                 run the runtime until SIGTERM or SIGINT (the default)
  launch                run the runtime under a launcher that installs the
                        signed releases paired clients request
  health                exit 0 when the runtime's attach endpoint answers
  prepare               install the pinned Pi and its extension packages,
                        then print the Pi executable's path
  pair [--scopes S1,S2] [--json]
                        print a one-time pairing link for a remote client,
                        which pairs a full member unless --scopes limits it
  devices [--json]      list the paired remote devices
  revoke DEVICE_ID      revoke a paired remote device
  info [--json]         print the runtime's id, certificate fingerprint and
                        remote access state

options:
  -h, --help  print this help
  --version   print the version

State lives in MUNIMENT_STATE_DIR, or ~/.muniment. Agents, memory and
projects live in the Home folder home.json names, which serve sets to
<state>/home when none is saved. MUNIMENT_PI_ROOT names
the directory that holds the installed Pi. MUNIMENT_REMOTE_ACCESS (on or
off), MUNIMENT_REMOTE_LISTEN (address:port) and MUNIMENT_REMOTE_ENDPOINTS
(host:port,...) override the stored remote access settings.
MUNIMENT_INSTALL_KIND=container names a container install.

The launcher keeps versions in <state>/runtime. MUNIMENT_UPDATE_GRACE_SECS
(120) bounds the wait for running work before an update stops the runtime,
MUNIMENT_UPDATE_SHUTDOWN_SECS (60) the stop itself, MUNIMENT_UPDATE_TRIAL_SECS
(120) the new version's health check and MUNIMENT_UPDATE_RETRY_SECS (60) the
first retry of a failed download. MUNIMENT_UPDATE_FEED overrides the release
feed base URL, under which <feed>/v<version>/<file> names each file, and
MUNIMENT_UPDATE_PUBLIC_KEY the release signing key.";

/// Docker reads 1 as unhealthy and reserves 2, so every failure exits 1.
const FAILURE: u8 = 1;

fn main() -> ExitCode {
    // Programs this server starts keep only the descriptors they are handed.
    muniment_core::child_fds::seal_inherited();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("serve");
    let rest = args.get(1..).unwrap_or_default();
    let takes_arguments = matches!(command, "pair" | "devices" | "revoke" | "info");
    if !takes_arguments && !rest.is_empty() {
        eprintln!("muniment-server: unexpected argument {}\n{USAGE}", rest[0]);
        return ExitCode::from(FAILURE);
    }
    if command == "launch" {
        return launch();
    }
    let result = match command {
        "serve" => serve(),
        "health" => health(),
        "prepare" => prepare(),
        "pair" => with_state(|state| remote::pair(state, rest)),
        "devices" => with_state(|state| remote::devices(state, rest)),
        "revoke" => with_state(|state| remote::revoke(state, rest)),
        "info" => with_state(|state| remote::info(state, rest)),
        "help" | "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        "--version" => {
            println!("{}", remote::RUNTIME_VERSION);
            Ok(())
        }
        other => Err(format!("unknown command {other}\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("muniment-server: {message}");
            ExitCode::from(FAILURE)
        }
    }
}

#[cfg(unix)]
fn launch() -> ExitCode {
    match launcher::launch() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("muniment-server: {message}");
            ExitCode::from(FAILURE)
        }
    }
}

#[cfg(not(unix))]
fn launch() -> ExitCode {
    eprintln!("muniment-server: launch runs on Unix only");
    ExitCode::from(FAILURE)
}

/// Runs a remote access command on the state root, without adopting an
/// earlier install's files: `serve` does that.
fn with_state(command: impl FnOnce(&std::path::Path) -> Result<(), String>) -> Result<(), String> {
    let state = muniment_core::state_root::state_directory()
        .ok_or("no state directory: set MUNIMENT_STATE_DIR or HOME")?;
    command(&state)
}

#[cfg(target_os = "linux")]
fn serve() -> Result<(), String> {
    use muniment_core::attach::{ReceivedSignal, TerminationSignalWait, UpdateCapability};
    use muniment_runtime_core::{
        adopt_state_directory, run_runtime_with_updates, RuntimeDirectories, RuntimeUpdateOptions,
    };

    // The signal mask must be set before any thread starts, so every thread
    // the runtime spawns leaves SIGTERM, SIGINT and SIGUSR1 to this wait.
    let termination =
        TerminationSignalWait::with_update_drain().map_err(|error| error.to_string())?;
    let capability = match std::env::var(launcher::CAPABILITY_VARIABLE).as_deref() {
        Ok("launcher") => UpdateCapability::Launcher,
        _ => UpdateCapability::Manual,
    };
    let grace = launcher::seconds(launcher::GRACE_VARIABLE, 120)?;
    let state = adopt_state_directory().map_err(|error| error.to_string())?;
    let endpoint = host::prepare_runtime_directory(&state)?;
    let remote = remote::route_options()?;
    eprintln!(
        "muniment-server: started version={} state_directory={} endpoint={}",
        remote::RUNTIME_VERSION,
        state.display(),
        host::endpoint_path(&endpoint)?.display()
    );
    eprintln!(
        "muniment-server: remote access install_kind={} {}",
        remote.access.install_kind.as_str(),
        remote::describe_for_log(&state)?
    );
    let (stop, stopped) = std::sync::mpsc::channel();
    let (drain, drain_requested) = std::sync::mpsc::channel();
    std::thread::spawn(move || loop {
        match termination.next() {
            Ok(ReceivedSignal::UpdateDrain) => {
                eprintln!("muniment-server: update drain requested");
                let _ = drain.send(());
            }
            Ok(ReceivedSignal::Terminate) | Err(_) => {
                eprintln!("muniment-server: stopping reason=signal");
                let _ = stop.send(());
                return;
            }
        }
    });
    // A headless runtime has no account anyone signs in to, so it serves its
    // threads and runs in local mode.
    let marker = state.join(muniment_core::local_mode::LOCAL_MODE_MARKER);
    if !marker.is_file() {
        std::fs::write(&marker, "1").map_err(|error| format!("local mode marker: {error}"))?;
    }
    // No one picks a Home folder on a headless runtime, so agents, memory and
    // projects live in one under the state root unless one is saved.
    let home = muniment_core::home::initialize_state_home(&state)
        .map_err(|error| format!("home: {error}"))?;
    eprintln!("muniment-server: home={}", home.display());
    let directories = RuntimeDirectories {
        profile: state.clone(),
        config: state.clone(),
        endpoint,
    };
    run_runtime_with_updates(
        host::headless_host(&state),
        &directories,
        Some(remote),
        Some(RuntimeUpdateOptions {
            capability,
            drain: drain_requested,
            grace,
        }),
        stopped,
    )
    .map_err(|error| error.to_string())?;
    eprintln!("muniment-server: stopped");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn serve() -> Result<(), String> {
    Err("serve runs on Linux only".into())
}

#[cfg(target_os = "linux")]
fn health() -> Result<(), String> {
    use std::time::{Duration, Instant};

    let state = muniment_core::state_root::state_directory()
        .ok_or("no state directory: set MUNIMENT_STATE_DIR or HOME")?;
    let endpoint = host::endpoint_path(&host::runtime_directory(&state))?;
    muniment_core::attach::read_handoff_probe_welcome(
        &endpoint,
        Instant::now() + Duration::from_secs(5),
    )
    .map(|_| ())
    .map_err(|error| format!("{}: {error}", endpoint.display()))
}

#[cfg(not(target_os = "linux"))]
fn health() -> Result<(), String> {
    Err("health runs on Linux only".into())
}

/// Installs Pi and its packages where `serve` finds them, so a first run
/// needs no download. The install checks the archive's size and SHA-256
/// against the pins this binary was built with.
fn prepare() -> Result<(), String> {
    let state =
        muniment_runtime_core::adopt_state_directory().map_err(|error| error.to_string())?;
    let root = host::pi_install_root(&state);
    let executable = muniment_core::sidecar::pi_install::acquire_pi(
        &root,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .map_err(|error| format!("Pi install in {} failed: {error:?}", root.display()))?;
    muniment_core::pi_packages::prepare_pi_packages(
        &muniment_core::state_root::agent_directory(&state),
        &executable,
    )
    .map_err(|error| format!("Pi package install failed: {error}"))?;
    println!("{}", executable.display());
    Ok(())
}
