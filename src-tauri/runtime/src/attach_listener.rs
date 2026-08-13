//! Runtime-owned companion attach listener.

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use muniment_core::attach::linux::{
    approval_waiter_with_claims, run_authenticated_session_with_service_approvals_and_registry,
    ApprovalDecision, AttachAcceptError, AttachFilesystem, AttachTransport, ThreadListService,
};
use muniment_core::attach::{
    bounded_claim, ApprovalCoordinator, ApprovalRequest, CompanionRegistry, SignedWorkspaceApproval,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachListenerError {
    Filesystem,
    InstanceLock,
    Bind,
    Accept,
}

impl fmt::Display for AttachListenerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Filesystem => "runtime attach filesystem setup failed",
            Self::InstanceLock => "runtime attach instance lock could not be acquired",
            Self::Bind => "runtime attach endpoint bind failed",
            Self::Accept => "runtime attach connection accept failed",
        })
    }
}

impl std::error::Error for AttachListenerError {}

/// Owns the profile endpoint and serves companion sessions until `stop` fires.
pub fn run_attach_listener<S, F, E>(
    profile_directory: impl AsRef<Path>,
    companion_registry: &CompanionRegistry,
    approval: SignedWorkspaceApproval,
    approvals: ApprovalCoordinator,
    service_factory: F,
    stop: Receiver<()>,
) -> Result<(), AttachListenerError>
where
    S: ThreadListService + Send + 'static,
    F: Fn() -> Result<S, E> + Send + Sync + 'static,
{
    let filesystem =
        AttachFilesystem::from_runtime_directory(profile_directory.as_ref().as_os_str())
            .map_err(|_| AttachListenerError::Filesystem)?;
    let _instance_lock = filesystem
        .acquire_instance_lock()
        .map_err(|_| AttachListenerError::InstanceLock)?;
    let transport = AttachTransport::bind(&filesystem).map_err(|_| AttachListenerError::Bind)?;
    let stop_handle = transport.stop_handle();
    let finished = Arc::new(AtomicBool::new(false));
    let service_factory = Arc::new(service_factory);
    let live_connections = companion_registry.live_connections();

    std::thread::scope(|scope| {
        let stop_finished = finished.clone();
        let stop_thread = scope.spawn(move || {
            while !stop_finished.load(Ordering::Acquire) {
                match stop.recv_timeout(Duration::from_millis(10)) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
            }
            stop_handle.stop();
        });
        let result = loop {
            let (stream, credentials) = match transport.accept() {
                Ok(accepted) => accepted,
                Err(AttachAcceptError::Closed) => break Ok(()),
                Err(AttachAcceptError::Accept) => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(AttachAcceptError::PeerCredentials | AttachAcceptError::WrongUid(_)) => {
                    continue;
                }
            };
            let service_factory = service_factory.clone();
            let approval = approval.clone();
            let approvals = approvals.clone();
            let live_connections = live_connections.clone();
            std::thread::spawn(move || {
                let Ok(mut service) = service_factory() else {
                    return;
                };
                let waiter = approval_waiter_with_claims(
                    move |challenge: &muniment_core::attach::PairingChallenge,
                          kind: &str,
                          version: &str,
                          remaining: Duration| {
                        Some(request_approval(
                            &approval,
                            &approvals,
                            challenge.as_str(),
                            kind,
                            version,
                            remaining,
                        ))
                    },
                );
                let _ = run_authenticated_session_with_service_approvals_and_registry(
                    stream,
                    credentials,
                    env!("CARGO_PKG_VERSION"),
                    &mut service,
                    waiter,
                    &live_connections,
                );
            });
        };
        finished.store(true, Ordering::Release);
        stop_thread
            .join()
            .expect("attach stop thread does not panic");
        result
    })
}

fn request_approval(
    approval: &SignedWorkspaceApproval,
    approvals: &ApprovalCoordinator,
    challenge: &str,
    claimed_kind: &str,
    claimed_version: &str,
    remaining: Duration,
) -> ApprovalDecision {
    let Some(recorded) = approval.approval() else {
        return ApprovalDecision::Deny;
    };
    let approved = approvals.request(
        ApprovalRequest {
            challenge: challenge.to_owned(),
            claimed_kind: bounded_claim(claimed_kind),
            claimed_version: bounded_claim(claimed_version),
            workspace: recorded.workspace.clone(),
            scopes: recorded.scopes.clone(),
        },
        remaining,
    );
    if !approved {
        return ApprovalDecision::Deny;
    }
    match approval.approval() {
        Some(current) if current.workspace == recorded.workspace => {
            ApprovalDecision::Approve(current)
        }
        _ => ApprovalDecision::Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn approval_request_bounds_untrusted_claims() {
        let approval = SignedWorkspaceApproval::default();
        approval.record("workspace-a".into());
        let approvals = ApprovalCoordinator::default();
        let (request_tx, request_rx) = mpsc::channel();
        approvals.register_presenter(move |request| {
            request_tx
                .send((
                    request.claimed_kind.clone(),
                    request.claimed_version.clone(),
                ))
                .unwrap();
            false
        });

        assert_eq!(
            request_approval(
                &approval,
                &approvals,
                "challenge",
                &"x".repeat(81),
                "1.0\nmalicious",
                Duration::from_secs(1),
            ),
            ApprovalDecision::Deny
        );

        let (claimed_kind, claimed_version) = request_rx.recv().unwrap();
        assert_eq!(claimed_kind, "unknown");
        assert_eq!(claimed_version, "unknown");
    }
}
