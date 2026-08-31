#![cfg(unix)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Condvar, Mutex};

use muniment_runtime::{
    run_windows_attach_activation, WindowsActivationExit, WindowsAttachAcceptBoundary,
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
}

impl WindowsAttachAcceptBoundary for FakeAcceptor {
    type StopSignal = FakeStopSignal;

    fn stop_signal(&self) -> Self::StopSignal {
        self.stop.clone()
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
