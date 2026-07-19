use super::*;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, Ipv6Addr};

type StartResults = HashMap<u32, VecDeque<Result<(u64, u64), ProcessReadError>>>;

struct FakeReader {
    pids: Result<Vec<u32>, ProcessReadError>,
    starts: RefCell<StartResults>,
    executables: HashMap<u32, Result<PathBuf, ProcessReadError>>,
    sockets: HashMap<u32, Result<Vec<ProcessSocket>, ProcessReadError>>,
}

impl NativeProcessReader for FakeReader {
    fn process_ids(&self) -> Result<Vec<u32>, ProcessReadError> {
        self.pids.clone()
    }

    fn start_identity(&self, pid: u32) -> Result<(u64, u64), ProcessReadError> {
        self.starts
            .borrow_mut()
            .get_mut(&pid)
            .and_then(VecDeque::pop_front)
            .unwrap_or(Err(ProcessReadError))
    }

    fn executable(&self, pid: u32) -> Result<PathBuf, ProcessReadError> {
        self.executables
            .get(&pid)
            .cloned()
            .unwrap_or(Err(ProcessReadError))
    }

    fn tcp_sockets(&self, pid: u32) -> Result<Vec<ProcessSocket>, ProcessReadError> {
        self.sockets
            .get(&pid)
            .cloned()
            .unwrap_or(Err(ProcessReadError))
    }
}

fn endpoints() -> (SocketAddr, SocketAddr) {
    (
        (Ipv4Addr::LOCALHOST, 41000).into(),
        (Ipv4Addr::LOCALHOST, 41001).into(),
    )
}

fn identity() -> (u64, u64) {
    (77, 123)
}

fn reader(pids: Vec<u32>) -> FakeReader {
    FakeReader {
        pids: Ok(pids),
        starts: RefCell::new(HashMap::new()),
        executables: HashMap::new(),
        sockets: HashMap::new(),
    }
}

fn add_process(
    reader: &mut FakeReader,
    pid: u32,
    starts: impl IntoIterator<Item = Result<(u64, u64), ProcessReadError>>,
    sockets: Result<Vec<ProcessSocket>, ProcessReadError>,
) {
    reader
        .starts
        .get_mut()
        .insert(pid, starts.into_iter().collect());
    reader.sockets.insert(pid, sockets);
}

fn browser_half() -> ProcessSocket {
    let (local, peer) = endpoints();
    ProcessSocket {
        local: peer,
        peer: local,
    }
}

#[test]
fn resolves_one_exact_ipv4_or_ipv6_browser_half() {
    let (local, peer) = endpoints();
    let mut ipv4 = reader(vec![42]);
    add_process(
        &mut ipv4,
        42,
        [
            Ok(identity()),
            Ok(identity()),
            Ok(identity()),
            Ok(identity()),
        ],
        Ok(vec![browser_half()]),
    );
    assert_eq!(
        resolve_browser_process_with_reader(local, peer, &ipv4),
        Ok(BrowserProcessIdentity {
            pid: 42,
            start_identity: identity(),
        })
    );

    let local: SocketAddr = (Ipv6Addr::LOCALHOST, 42000).into();
    let peer: SocketAddr = (Ipv6Addr::LOCALHOST, 42001).into();
    let mut ipv6 = reader(vec![43]);
    add_process(
        &mut ipv6,
        43,
        [
            Ok(identity()),
            Ok(identity()),
            Ok(identity()),
            Ok(identity()),
        ],
        Ok(vec![ProcessSocket {
            local: peer,
            peer: local,
        }]),
    );
    assert!(resolve_browser_process_with_reader(local, peer, &ipv6).is_ok());
}

#[test]
fn rejects_missing_mismatched_and_ambiguous_owners() {
    let (local, peer) = endpoints();
    let mut missing = reader(vec![42]);
    add_process(
        &mut missing,
        42,
        [
            Ok(identity()),
            Ok(identity()),
            Ok(identity()),
            Ok(identity()),
        ],
        Ok(vec![]),
    );
    assert_eq!(
        resolve_browser_process_with_reader(local, peer, &missing),
        Err(ResolutionError::SocketNotFound)
    );

    let mut mismatched = reader(vec![42]);
    add_process(
        &mut mismatched,
        42,
        [
            Ok(identity()),
            Ok(identity()),
            Ok(identity()),
            Ok(identity()),
        ],
        Ok(vec![ProcessSocket { local, peer }]),
    );
    assert_eq!(
        resolve_browser_process_with_reader(local, peer, &mismatched),
        Err(ResolutionError::SocketNotFound)
    );

    let mut ambiguous = reader(vec![42, 43]);
    for pid in [42, 43] {
        add_process(
            &mut ambiguous,
            pid,
            [Ok(identity()), Ok(identity())],
            Ok(vec![browser_half()]),
        );
    }
    assert_eq!(
        resolve_browser_process_with_reader(local, peer, &ambiguous),
        Err(ResolutionError::AmbiguousOwner)
    );
}

#[test]
fn rejects_owner_churn_pid_reuse_and_inspection_failure() {
    let (local, peer) = endpoints();
    let mut during_scan = reader(vec![42]);
    add_process(
        &mut during_scan,
        42,
        [Ok(identity()), Ok((78, 0))],
        Ok(vec![browser_half()]),
    );
    assert_eq!(
        resolve_browser_process_with_reader(local, peer, &during_scan),
        Err(ResolutionError::ProcessIdentityChanged)
    );

    let mut after_scan = reader(vec![42]);
    add_process(
        &mut after_scan,
        42,
        [Ok(identity()), Ok(identity()), Ok((78, 0)), Ok((78, 0))],
        Ok(vec![browser_half()]),
    );
    assert_eq!(
        resolve_browser_process_with_reader(local, peer, &after_scan),
        Err(ResolutionError::ProcessIdentityChanged)
    );

    let mut denied = reader(vec![42]);
    add_process(&mut denied, 42, [Ok(identity())], Err(ProcessReadError));
    assert_eq!(
        resolve_browser_process_with_reader(local, peer, &denied),
        Err(ResolutionError::InspectionUnavailable)
    );
}

#[test]
fn rejects_invalid_loopback_endpoints() {
    for (local, peer) in [
        (
            "0.0.0.0:41000".parse().unwrap(),
            "127.0.0.1:41001".parse().unwrap(),
        ),
        (
            "192.0.2.1:41000".parse().unwrap(),
            "127.0.0.1:41001".parse().unwrap(),
        ),
        (
            "127.0.0.1:0".parse().unwrap(),
            "127.0.0.1:41001".parse().unwrap(),
        ),
        (
            "127.0.0.1:41000".parse().unwrap(),
            "[::1]:41001".parse().unwrap(),
        ),
        (
            "127.0.0.1:41000".parse().unwrap(),
            "127.0.0.1:41000".parse().unwrap(),
        ),
    ] {
        assert_eq!(
            resolve_browser_process_with_reader(local, peer, &reader(vec![])),
            Err(ResolutionError::InvalidEndpoint)
        );
    }
}

#[test]
fn authorizes_canonical_executable_and_rejects_mismatch_or_reuse() {
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let other = std::env::temp_dir();
    let observed = BrowserProcessIdentity {
        pid: 42,
        start_identity: identity(),
    };

    let mut success = reader(vec![]);
    success
        .starts
        .get_mut()
        .insert(42, [Ok(identity()), Ok(identity())].into_iter().collect());
    success.executables.insert(42, Ok(executable.clone()));
    assert!(verify_browser_process_with_reader(observed, &executable, &success).is_ok());

    let mut mismatch = reader(vec![]);
    mismatch
        .starts
        .get_mut()
        .insert(42, [Ok(identity()), Ok(identity())].into_iter().collect());
    mismatch.executables.insert(42, Ok(other));
    assert_eq!(
        verify_browser_process_with_reader(observed, &executable, &mismatch),
        Err(VerificationError::ExecutableMismatch)
    );

    let mut reused = reader(vec![]);
    reused
        .starts
        .get_mut()
        .insert(42, [Ok(identity()), Ok((78, 0))].into_iter().collect());
    reused.executables.insert(42, Ok(executable.clone()));
    assert_eq!(
        verify_browser_process_with_reader(observed, &executable, &reused),
        Err(VerificationError::ProcessIdentityChanged)
    );
}

#[test]
fn composition_authorizes_only_the_resolved_executable() {
    let (local, peer) = endpoints();
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut success = reader(vec![42]);
    add_process(
        &mut success,
        42,
        std::iter::repeat_n(Ok(identity()), 6),
        Ok(vec![browser_half()]),
    );
    success.executables.insert(42, Ok(executable.clone()));
    assert!(authorize_browser_process_with_reader(local, peer, &executable, &success).is_ok());

    let mut wrong = reader(vec![42]);
    add_process(
        &mut wrong,
        42,
        std::iter::repeat_n(Ok(identity()), 6),
        Ok(vec![browser_half()]),
    );
    wrong.executables.insert(42, Ok(std::env::temp_dir()));
    assert_eq!(
        authorize_browser_process_with_reader(local, peer, &executable, &wrong),
        Err(AuthorizationError::ExecutableVerificationFailed)
    );
}

#[test]
fn public_errors_are_bounded_and_redacted() {
    let rendered = [
        format!("{}", ResolutionError::InspectionUnavailable),
        format!("{}", ResolutionError::AmbiguousOwner),
        format!("{}", VerificationError::ExecutableMismatch),
        format!("{}", AuthorizationError::OwnerResolutionFailed),
    ];
    for error in rendered {
        assert!(error.len() < 100);
        for secret in ["42", "41000", "/secret/browser", "permission denied"] {
            assert!(!error.contains(secret));
        }
    }
}
