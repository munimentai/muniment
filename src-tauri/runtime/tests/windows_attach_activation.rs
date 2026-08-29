#![cfg(unix)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::time::Instant;

use muniment_runtime::{
    run_windows_attach_activation, WindowsActivationExit, WindowsAttachAcceptBoundary,
    WindowsAttachAcceptOutcome, WindowsAttachBindFailure, WindowsAttachFactory,
    WindowsDiagnosticEvent, WindowsDiagnosticSink,
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
struct FakeAcceptor {
    calls: Rc<Cell<usize>>,
    stop: Sender<()>,
}

impl WindowsAttachAcceptBoundary for FakeAcceptor {
    fn serve_next(&mut self, _accept_deadline: Instant) -> WindowsAttachAcceptOutcome {
        self.calls.set(self.calls.get() + 1);
        self.stop.send(()).unwrap();
        WindowsAttachAcceptOutcome::Served
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
fn a_bound_acceptor_serves_until_stop_and_exits_orderly() {
    let (stop_tx, stop_rx) = mpsc::channel();
    let acceptor = FakeAcceptor {
        calls: Rc::new(Cell::new(0)),
        stop: stop_tx,
    };
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
