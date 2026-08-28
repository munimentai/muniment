#![cfg(unix)]

use std::collections::VecDeque;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use muniment_runtime::{
    run_windows_attach_accept_loop, WindowsAttachAcceptBoundary, WindowsAttachAcceptOutcome,
};

struct FakeAcceptor {
    outcomes: VecDeque<WindowsAttachAcceptOutcome>,
    calls: Vec<Instant>,
    stop: Sender<()>,
}

impl WindowsAttachAcceptBoundary for FakeAcceptor {
    fn serve_next(&mut self, _accept_deadline: Instant) -> WindowsAttachAcceptOutcome {
        self.calls.push(Instant::now());
        let outcome = self.outcomes.pop_front().unwrap();
        if self.outcomes.is_empty() {
            self.stop.send(()).unwrap();
        }
        outcome
    }
}

#[test]
fn a_stop_value_prevents_an_accept() {
    let (stop_tx, stop_rx) = mpsc::channel();
    stop_tx.send(()).unwrap();
    let mut acceptor = FakeAcceptor {
        outcomes: VecDeque::new(),
        calls: Vec::new(),
        stop: stop_tx,
    };

    run_windows_attach_accept_loop(&mut acceptor, stop_rx);

    assert!(acceptor.calls.is_empty());
}

#[test]
fn a_disconnected_stop_channel_prevents_an_accept() {
    let (stop_tx, stop_rx) = mpsc::channel();
    drop(stop_tx);
    let (unused_stop, _) = mpsc::channel();
    let mut acceptor = FakeAcceptor {
        outcomes: VecDeque::new(),
        calls: Vec::new(),
        stop: unused_stop,
    };

    run_windows_attach_accept_loop(&mut acceptor, stop_rx);

    assert!(acceptor.calls.is_empty());
}

#[test]
fn served_and_idle_outcomes_start_the_next_accept_without_a_retry_delay() {
    let (stop_tx, stop_rx) = mpsc::channel();
    let mut acceptor = FakeAcceptor {
        outcomes: VecDeque::from([
            WindowsAttachAcceptOutcome::Served,
            WindowsAttachAcceptOutcome::Idle,
            WindowsAttachAcceptOutcome::Served,
        ]),
        calls: Vec::new(),
        stop: stop_tx,
    };

    run_windows_attach_accept_loop(&mut acceptor, stop_rx);

    assert_eq!(acceptor.calls.len(), 3);
}

#[test]
fn a_failed_outcome_delays_the_next_accept() {
    let (stop_tx, stop_rx) = mpsc::channel();
    let mut acceptor = FakeAcceptor {
        outcomes: VecDeque::from([
            WindowsAttachAcceptOutcome::Failed,
            WindowsAttachAcceptOutcome::Served,
        ]),
        calls: Vec::new(),
        stop: stop_tx,
    };

    run_windows_attach_accept_loop(&mut acceptor, stop_rx);

    assert_eq!(acceptor.calls.len(), 2);
    assert!(acceptor.calls[1].duration_since(acceptor.calls[0]) >= Duration::from_millis(50));
}
