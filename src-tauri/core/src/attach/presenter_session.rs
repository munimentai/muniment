use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

use super::{ApprovalCoordinator, ApprovalPresenterConnection, PresenterGuard, CHALLENGE_LIFETIME};

/// Holds the exclusive approval presenter claim for one connection.
pub struct ApprovalPresenterSession {
    presenter: Option<PresenterGuard>,
    shutdown: UnixStream,
}

/// Claims the coordinator and serves its approval requests over the connection.
pub fn serve_approval_presenter(
    coordinator: ApprovalCoordinator,
    connection: ApprovalPresenterConnection,
) -> Option<ApprovalPresenterSession> {
    let shutdown = connection.try_clone_stream().ok()?;
    let connection = Mutex::new(connection);
    let decision_coordinator = coordinator.clone();
    let presenter = coordinator.claim_presenter(move |request| {
        let Ok(mut connection) = connection.lock() else {
            return false;
        };
        let approve = connection.present(request, CHALLENGE_LIFETIME);
        decision_coordinator.decide(&request.challenge, approve)
    })?;
    Some(ApprovalPresenterSession {
        presenter: Some(presenter),
        shutdown,
    })
}

impl Drop for ApprovalPresenterSession {
    fn drop(&mut self) {
        let _ = self.shutdown.shutdown(Shutdown::Both);
        drop(self.presenter.take());
    }
}
