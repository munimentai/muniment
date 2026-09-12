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
    signal_gate: Option<Arc<(Mutex<bool>, Condvar)>>,
}

impl WindowsAttachStopSignal for FakeStopSignal {
    fn signal(&self) {
        if let Some(signal_gate) = &self.signal_gate {
            let (open, wake) = &**signal_gate;
            let mut open = open.lock().unwrap();
            while !*open {
                open = wake.wait(open).unwrap();
            }
        }
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
            signal_gate: None,
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
                ready: None,
                refresh_detected: None,
            },
        ),
        WindowsActivationExit::Orderly(0)
    );
    assert!(diagnostics.events.borrow().is_empty());

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn manager_stop_wins_when_refresh_and_manager_stops_are_pending() {
    let directory = temporary_state_directory("upgrade-watch-manager-stop");
    std::fs::create_dir_all(&directory).unwrap();
    let watched = directory.join("runtime");
    std::fs::File::create(&watched).unwrap();
    let replacement = directory.join("replacement");
    std::fs::File::create(&replacement).unwrap();
    let replace_watched = watched.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let replacer = std::thread::spawn(move || {
        ready_rx.recv().unwrap();
        std::fs::rename(replacement, replace_watched).unwrap();
    });
    let (stop_tx, stop_rx) = mpsc::channel();
    stop_tx.send(()).unwrap();
    let signal_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let mut acceptor = fake_acceptor(WindowsAttachAcceptOutcome::Stopped, true);
    acceptor.stop.signal_gate = Some(Arc::clone(&signal_gate));
    let factory = FakeFactory {
        result: Ok(acceptor),
    };
    let diagnostics = FakeDiagnostics::default();
    let (refresh_detected_tx, refresh_detected_rx) = mpsc::channel();
    let gate_thread = std::thread::spawn(move || {
        refresh_detected_rx.recv().unwrap();
        let (open, wake) = &*signal_gate;
        *open.lock().unwrap() = true;
        wake.notify_one();
    });

    assert_eq!(
        run_windows_attach_activation_with_upgrade_watch(
            &factory,
            stop_rx,
            &diagnostics,
            MacosUpgradeWatchTestControl {
                path: watched,
                poll_interval: Duration::from_millis(5),
                ready: Some(ready_tx),
                refresh_detected: Some(refresh_detected_tx),
            },
        ),
        WindowsActivationExit::Orderly(0)
    );
    assert!(diagnostics.events.borrow().is_empty());

    replacer.join().unwrap();
    gate_thread.join().unwrap();
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
    let (ready_tx, ready_rx) = mpsc::channel();
    let replacer = std::thread::spawn(move || {
        ready_rx.recv().unwrap();
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
                ready: Some(ready_tx),
                refresh_detected: None,
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
fn a_failed_accept_reaches_the_record_and_stderr() {
    const CHILD_ENV: &str = "MUNIMENT_TEST_ACCEPT_FAILURE_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "a_failed_accept_reaches_the_record_and_stderr",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(
            stderr
                .matches("attach accept failed: CreateInstance win32=5 (0x00000005)")
                .count(),
            muniment_runtime::MAX_CONSECUTIVE_FAILED_ACCEPTS
        );
        return;
    }

    struct FileDiagnostics(std::path::PathBuf);
    impl WindowsDiagnosticSink for FileDiagnostics {
        fn record(&self, event: WindowsDiagnosticEvent) {
            muniment_runtime::write_windows_diagnostic(&self.0, event).unwrap();
        }
    }

    let directory = temporary_state_directory("accept-error");
    std::fs::create_dir_all(&directory).unwrap();
    let error = muniment_core::attach::WindowsAttachAcceptError::CreateInstance(5);
    let acceptor = fake_acceptor(WindowsAttachAcceptOutcome::AcceptFailed(error), false);
    let observed_acceptor = acceptor.clone();
    let factory = FakeFactory {
        result: Ok(acceptor),
    };
    let (_stop_tx, stop_rx) = mpsc::channel();
    assert_eq!(
        run_windows_attach_activation(&factory, stop_rx, &FileDiagnostics(directory.clone())),
        WindowsActivationExit::Failed(1)
    );
    assert_eq!(
        observed_acceptor.calls.get(),
        muniment_runtime::MAX_CONSECUTIVE_FAILED_ACCEPTS
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("muniment/logs/runtime.log")).unwrap(),
        "event=activation_failed message=runtime activation failed cause=CreateInstance win32=5 (0x00000005)\n"
    );
    std::fs::remove_dir_all(directory).unwrap();
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
