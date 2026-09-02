#![cfg(unix)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Duration;

use muniment_runtime::{
    run_windows_attach_activation, run_windows_attach_activation_with_retention_schedule,
    run_windows_attach_activation_with_upgrade_watch, MacosUpgradeWatchTestControl,
    RuntimeAttachState, WindowsActivationExit, WindowsAttachAcceptBoundary,
    WindowsAttachAcceptOutcome, WindowsAttachBindFailure, WindowsAttachFactory,
    WindowsAttachStopSignal, WindowsDiagnosticEvent, WindowsDiagnosticSink,
};

struct FakeFactory {
    result: Result<FakeAcceptor, WindowsAttachBindFailure>,
}

impl WindowsAttachFactory for FakeFactory {
    type Acceptor = FakeAcceptor;

    fn bind(&self) -> Result<Self::Acceptor, WindowsAttachBindFailure> {
        self.result.clone()
    }
}

#[derive(Clone)]
struct FakeStopSignal {
    stopped: Arc<(Mutex<bool>, Condvar)>,
}

impl WindowsAttachStopSignal for FakeStopSignal {
    fn signal(&self) {
        let (stopped, wake) = &*self.stopped;
        *stopped.lock().unwrap() = true;
        wake.notify_one();
    }
}

#[derive(Clone)]
struct FakeAcceptor {
    calls: Rc<Cell<usize>>,
    outcome: WindowsAttachAcceptOutcome,
    stop: FakeStopSignal,
    wait_for_stop: bool,
    retention_state: Option<Arc<RuntimeAttachState>>,
}

impl WindowsAttachAcceptBoundary for FakeAcceptor {
    type StopSignal = FakeStopSignal;

    fn stop_signal(&self) -> Self::StopSignal {
        self.stop.clone()
    }

    fn retention_state(&self) -> Option<Arc<RuntimeAttachState>> {
        self.retention_state.clone()
    }

    fn serve_next(&mut self) -> WindowsAttachAcceptOutcome {
        self.calls.set(self.calls.get() + 1);
        if self.wait_for_stop {
            let (stopped, wake) = &*self.stop.stopped;
            let mut stopped = stopped.lock().unwrap();
            while !*stopped {
                stopped = wake.wait(stopped).unwrap();
            }
        }
        self.outcome
    }
}

fn fake_acceptor(outcome: WindowsAttachAcceptOutcome, wait_for_stop: bool) -> FakeAcceptor {
    FakeAcceptor {
        calls: Rc::new(Cell::new(0)),
        outcome,
        stop: FakeStopSignal {
            stopped: Arc::new((Mutex::new(false), Condvar::new())),
        },
        wait_for_stop,
        retention_state: None,
    }
}

#[derive(Default)]
struct FakeDiagnostics {
    events: RefCell<Vec<WindowsDiagnosticEvent>>,
}

impl WindowsDiagnosticSink for FakeDiagnostics {
    fn record(&self, event: WindowsDiagnosticEvent) {
        self.events.borrow_mut().push(event);
    }
}

#[test]
fn a_contended_instance_lock_exits_orderly_and_records_the_wait() {
    let (_, stop) = mpsc::channel();
    let diagnostics = FakeDiagnostics::default();
    let factory = FakeFactory {
        result: Err(WindowsAttachBindFailure::Contended),
    };

    assert_eq!(
        run_windows_attach_activation(&factory, stop, &diagnostics),
        WindowsActivationExit::Orderly(0)
    );
    assert_eq!(
        *diagnostics.events.borrow(),
        [WindowsDiagnosticEvent::InstanceLockWait]
    );
}

#[test]
fn another_bind_failure_exits_failed_and_records_the_failure() {
    let (_, stop) = mpsc::channel();
    let diagnostics = FakeDiagnostics::default();
    let factory = FakeFactory {
        result: Err(WindowsAttachBindFailure::Unavailable),
    };

    assert_eq!(
        run_windows_attach_activation(&factory, stop, &diagnostics),
        WindowsActivationExit::Failed(1)
    );
    assert_eq!(
        *diagnostics.events.borrow(),
        [WindowsDiagnosticEvent::ActivationFailed]
    );
}

#[test]
fn a_stop_value_signals_the_bound_acceptor_and_exits_orderly() {
    let (stop_tx, stop_rx) = mpsc::channel();
    stop_tx.send(()).unwrap();
    let acceptor = fake_acceptor(WindowsAttachAcceptOutcome::Stopped, true);
    let observed_acceptor = acceptor.clone();
    let factory = FakeFactory {
        result: Ok(acceptor),
    };
    let diagnostics = FakeDiagnostics::default();

    assert_eq!(
        run_windows_attach_activation(&factory, stop_rx, &diagnostics),
        WindowsActivationExit::Orderly(0)
    );
    assert_eq!(observed_acceptor.calls.get(), 1);
    assert!(diagnostics.events.borrow().is_empty());
}

#[test]
fn a_disconnected_stop_channel_signals_the_bound_acceptor() {
    let (stop_tx, stop_rx) = mpsc::channel();
    drop(stop_tx);
    let acceptor = fake_acceptor(WindowsAttachAcceptOutcome::Stopped, true);
    let factory = FakeFactory {
        result: Ok(acceptor),
    };
    let diagnostics = FakeDiagnostics::default();

    assert_eq!(
        run_windows_attach_activation(&factory, stop_rx, &diagnostics),
        WindowsActivationExit::Orderly(0)
    );
    assert!(diagnostics.events.borrow().is_empty());
}

#[test]
fn manager_stop_with_an_upgrade_watch_exits_orderly() {
    let directory = temporary_state_directory("upgrade-watch-stop");
    std::fs::create_dir_all(&directory).unwrap();
    let watched = directory.join("runtime");
    std::fs::File::create(&watched).unwrap();
    let (stop_tx, stop_rx) = mpsc::channel();
    stop_tx.send(()).unwrap();
    let acceptor = fake_acceptor(WindowsAttachAcceptOutcome::Stopped, true);
    let factory = FakeFactory {
        result: Ok(acceptor),
    };
    let diagnostics = FakeDiagnostics::default();

    assert_eq!(
        run_windows_attach_activation_with_upgrade_watch(
            &factory,
            stop_rx,
            &diagnostics,
            MacosUpgradeWatchTestControl {
                path: watched,
                poll_interval: Duration::from_millis(5),
            },
        ),
        WindowsActivationExit::Orderly(0)
    );
    assert!(diagnostics.events.borrow().is_empty());

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn executable_replacement_stops_the_acceptor_with_refresh_status() {
    let directory = temporary_state_directory("upgrade-watch");
    std::fs::create_dir_all(&directory).unwrap();
    let watched = directory.join("runtime");
    std::fs::File::create(&watched).unwrap();
    let replacement = directory.join("replacement");
    std::fs::File::create(&replacement).unwrap();
    let replace_watched = watched.clone();
    let replacer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(25));
        std::fs::rename(replacement, replace_watched).unwrap();
    });
    let (stop_tx, stop_rx) = mpsc::channel();
    let acceptor = fake_acceptor(WindowsAttachAcceptOutcome::Stopped, true);
    let factory = FakeFactory {
        result: Ok(acceptor),
    };
    let diagnostics = FakeDiagnostics::default();

    assert_eq!(
        run_windows_attach_activation_with_upgrade_watch(
            &factory,
            stop_rx,
            &diagnostics,
            MacosUpgradeWatchTestControl {
                path: watched,
                poll_interval: Duration::from_millis(5),
            },
        ),
        WindowsActivationExit::Orderly(75)
    );
    assert!(diagnostics.events.borrow().is_empty());

    drop(stop_tx);
    replacer.join().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn repeated_accept_failures_exit_failed_and_record_the_failure() {
    let (_stop_tx, stop_rx) = mpsc::channel();
    let acceptor = fake_acceptor(WindowsAttachAcceptOutcome::Failed, false);
    let observed_acceptor = acceptor.clone();
    let factory = FakeFactory {
        result: Ok(acceptor),
    };
    let diagnostics = FakeDiagnostics::default();

    assert_eq!(
        run_windows_attach_activation(&factory, stop_rx, &diagnostics),
        WindowsActivationExit::Failed(1)
    );
    assert_eq!(
        observed_acceptor.calls.get(),
        muniment_runtime::MAX_CONSECUTIVE_FAILED_ACCEPTS
    );
    assert_eq!(
        *diagnostics.events.borrow(),
        [WindowsDiagnosticEvent::ActivationFailed]
    );
}

#[test]
fn a_bound_acceptor_runs_retention_until_the_activation_ends() {
    let directory = temporary_state_directory("retention");
    std::fs::create_dir_all(&directory).unwrap();
    let state = Arc::new(RuntimeAttachState::open(&directory, &directory).unwrap());
    let mut acceptor = fake_acceptor(WindowsAttachAcceptOutcome::Failed, false);
    acceptor.retention_state = Some(state);
    let factory = FakeFactory {
        result: Ok(acceptor),
    };
    let (_stop_tx, stop_rx) = mpsc::channel();
    let (checked_tx, checked_rx) = mpsc::channel();
    let diagnostics = FakeDiagnostics::default();

    assert_eq!(
        run_windows_attach_activation_with_retention_schedule(
            &factory,
            stop_rx,
            &diagnostics,
            Duration::from_millis(10),
            Some(checked_tx),
        ),
        WindowsActivationExit::Failed(1)
    );
    assert!(checked_rx.try_iter().count() >= 2);
    assert!(checked_rx.recv_timeout(Duration::from_millis(30)).is_err());

    std::fs::remove_dir_all(directory).unwrap();
}

fn temporary_state_directory(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "muniment-windows-activation-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
