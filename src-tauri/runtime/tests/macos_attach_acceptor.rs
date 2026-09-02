#![cfg(unix)]

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use muniment_core::attach::{
    decode_frame, serve_macos_attach_session_with_reader, DesktopAttachService,
    MacosAttachRouteReader, MacosPeerReadError, ProtocolError, Welcome,
};
use muniment_runtime::{
    MacosAttachAcceptorWithBoundary, MacosAttachBindFailure, MacosAttachServeBoundary,
    RuntimeAttachBoundaries, WindowsAttachAcceptBoundary, WindowsAttachAcceptOutcome,
    WindowsAttachStopSignal,
};

mod common;
use common::TemporaryProfile;

type ServiceFactory = Arc<
    dyn Fn() -> Result<DesktopAttachService<RuntimeAttachBoundaries>, ProtocolError> + Send + Sync,
>;

#[derive(Clone)]
struct BlockingStopSignal(Arc<(Mutex<BlockingState>, Condvar)>);

struct BlockingState {
    waiting: bool,
    stopped: bool,
}

impl WindowsAttachStopSignal for BlockingStopSignal {
    fn signal(&self) {
        let (state, wake) = &*self.0;
        state.lock().unwrap().stopped = true;
        wake.notify_all();
    }
}

struct BlockingBoundary {
    state: Arc<(Mutex<BlockingState>, Condvar)>,
}

impl MacosAttachServeBoundary for BlockingBoundary {
    type StopSignal = BlockingStopSignal;

    fn stop_signal(&self) -> Self::StopSignal {
        BlockingStopSignal(Arc::clone(&self.state))
    }

    fn serve_next(&mut self, _service_factory: ServiceFactory) -> WindowsAttachAcceptOutcome {
        let (state, wake) = &*self.state;
        let mut state = state.lock().unwrap();
        state.waiting = true;
        wake.notify_all();
        while !state.stopped {
            state = wake.wait(state).unwrap();
        }
        WindowsAttachAcceptOutcome::Stopped
    }
}

#[test]
fn a_stop_signal_ends_a_pending_accept() {
    let profile = TemporaryProfile::new("macos-acceptor-stop", true);
    let state = Arc::new((
        Mutex::new(BlockingState {
            waiting: false,
            stopped: false,
        }),
        Condvar::new(),
    ));
    let boundary_state = Arc::clone(&state);
    let mut acceptor = MacosAttachAcceptorWithBoundary::bind_with(
        &profile.profile,
        &profile.config,
        |_| Ok(BlockingBoundary { state: boundary_state }),
    )
    .unwrap();
    let stop = acceptor.stop_signal();
    let server = thread::spawn(move || acceptor.serve_next());

    let (state_lock, wake) = &*state;
    let mut state_guard = state_lock.lock().unwrap();
    while !state_guard.waiting {
        state_guard = wake.wait(state_guard).unwrap();
    }
    drop(state_guard);
    stop.signal();

    assert_eq!(server.join().unwrap(), WindowsAttachAcceptOutcome::Stopped);
}

struct StoppedBoundary;

impl MacosAttachServeBoundary for StoppedBoundary {
    type StopSignal = BlockingStopSignal;

    fn stop_signal(&self) -> Self::StopSignal {
        unreachable!()
    }

    fn serve_next(&mut self, _service_factory: ServiceFactory) -> WindowsAttachAcceptOutcome {
        WindowsAttachAcceptOutcome::Stopped
    }
}

#[test]
fn maps_a_live_endpoint_bind_to_contended() {
    let profile = TemporaryProfile::new("macos-acceptor-contended", true);
    let expected_path = profile.profile.join("muniment/attach-v1.sock");
    let result = MacosAttachAcceptorWithBoundary::<StoppedBoundary>::bind_with(
        &profile.profile,
        &profile.config,
        |path| {
            assert_eq!(path, expected_path);
            Err(io::Error::from(io::ErrorKind::AddrInUse))
        },
    );

    assert!(matches!(result, Err(MacosAttachBindFailure::Contended)));
}

#[test]
fn retains_the_single_opened_activation_state() {
    let profile = TemporaryProfile::new("macos-acceptor-state", true);
    let acceptor = MacosAttachAcceptorWithBoundary::bind_with(
        &profile.profile,
        &profile.config,
        |_| Ok(StoppedBoundary),
    )
    .unwrap();

    let first = acceptor.retention_state().unwrap();
    let second = acceptor.retention_state().unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    first.attach_service().unwrap();
}

struct CompanionRoute;

impl MacosAttachRouteReader for CompanionRoute {
    fn peer_process(&self) -> Result<(u32, PathBuf), MacosPeerReadError> {
        Ok((42, PathBuf::from("/Applications/Other.app/other")))
    }
}

struct SessionBoundary {
    server: Option<UnixStream>,
}

impl MacosAttachServeBoundary for SessionBoundary {
    type StopSignal = BlockingStopSignal;

    fn stop_signal(&self) -> Self::StopSignal {
        unreachable!()
    }

    fn serve_next(&mut self, service_factory: ServiceFactory) -> WindowsAttachAcceptOutcome {
        let mut service = service_factory().unwrap();
        let mut server = self.server.take().unwrap();
        serve_macos_attach_session_with_reader(
            &mut server,
            &CompanionRoute,
            Path::new("/Applications/Muniment.app/muniment"),
            env!("CARGO_PKG_VERSION"),
            Instant::now() + Duration::from_secs(1),
            &mut service,
        )
        .unwrap();
        WindowsAttachAcceptOutcome::Served
    }
}

fn hello_frame() -> Vec<u8> {
    let body = br#"{"protocol":"muniment.attach/1","client":{"kind":"editor-extension","version":"0.0.1"},"supported":{"min":1,"max":1},"client_nonce":"nonce","authorized_client_id":"018f0000-0000-7000-8000-000000000099"}"#;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(body);
    frame
}

#[test]
fn serves_a_session_with_the_composed_runtime_service() {
    let profile = TemporaryProfile::new("macos-acceptor-service", true);
    let (mut client, server) = UnixStream::pair().unwrap();
    client.write_all(&hello_frame()).unwrap();
    let mut acceptor = MacosAttachAcceptorWithBoundary::bind_with(
        &profile.profile,
        &profile.config,
        |_| Ok(SessionBoundary { server: Some(server) }),
    )
    .unwrap();

    assert_eq!(acceptor.serve_next(), WindowsAttachAcceptOutcome::Served);
    let mut prefix = [0_u8; 4];
    client.read_exact(&mut prefix).unwrap();
    let length = u32::from_be_bytes(prefix) as usize;
    let mut response = vec![0_u8; 4 + length];
    response[..4].copy_from_slice(&prefix);
    client.read_exact(&mut response[4..]).unwrap();
    let welcome = decode_frame::<Welcome>(&response).unwrap().unwrap().0;
    assert_eq!(welcome.desktop_version, env!("CARGO_PKG_VERSION"));
    assert!(profile.profile.join("attach-idempotency.sqlite3").is_file());
}
