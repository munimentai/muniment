use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use serde::Serialize;
use serde_json::json;

use crate::{
    Authorization, Client, ErrorEnvelope, Event, EventName, Failure, Hello, Id, Operation,
    Protocol, ProtocolError, Request, Response, Success, VersionRange, Welcome,
};

pub const FIXTURE_DIRECTORY: &str = "muniment.attach/1";
static NEXT_EXPORT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Write,
    Check,
}

pub fn export(root: &Path, mode: Mode) -> io::Result<()> {
    let expected = fixture_bytes()?;
    let target = root.join(FIXTURE_DIRECTORY);
    if mode == Mode::Check {
        return check(&target, &expected);
    }

    let parent = target
        .parent()
        .expect("the fixture directory always has a parent");
    fs::create_dir_all(parent)?;
    let export_id = NEXT_EXPORT.fetch_add(1, Ordering::Relaxed);
    let staging = sibling_path(&target, "staging", export_id);
    remove_if_present(&staging)?;
    fs::create_dir(&staging)?;
    for (name, bytes) in &expected {
        fs::write(staging.join(name), bytes)?;
    }

    publish(&staging, &target)?;
    // After an exchange, staging contains the complete displaced generation.
    // Removing it cannot make the live target absent or partially populated.
    remove_if_present(&staging)
}

fn publish(staging: &Path, target: &Path) -> io::Result<()> {
    if !target.exists() {
        return fs::rename(staging, target);
    }

    atomic_exchange(staging, target)
}

#[cfg(target_os = "linux")]
fn atomic_exchange(left: &Path, right: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    const AT_FDCWD: i32 = -100;
    const RENAME_EXCHANGE: u32 = 2;
    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const i8,
            newdirfd: i32,
            newpath: *const i8,
            flags: u32,
        ) -> i32;
    }

    let left = CString::new(left.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let right = CString::new(right.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: both C strings remain alive for the call, and renameat2 retains
    // neither pointer.
    if unsafe {
        renameat2(
            AT_FDCWD,
            left.as_ptr(),
            AT_FDCWD,
            right.as_ptr(),
            RENAME_EXCHANGE,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn atomic_exchange(left: &Path, right: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    const RENAME_SWAP: u32 = 0x0000_0002;
    unsafe extern "C" {
        fn renamex_np(from: *const i8, to: *const i8, flags: u32) -> i32;
    }

    let left = CString::new(left.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let right = CString::new(right.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: both C strings remain alive for the call, and renamex_np retains
    // neither pointer.
    if unsafe { renamex_np(left.as_ptr(), right.as_ptr(), RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
fn atomic_exchange(_left: &Path, _right: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic fixture directory replacement is not supported on this platform",
    ))
}

fn fixture_bytes() -> io::Result<BTreeMap<&'static str, Vec<u8>>> {
    let request_id = id(1)?;
    let subscription_id = id(2)?;
    let run_id = id(3)?;
    let mut fixtures = BTreeMap::new();
    insert(
        &mut fixtures,
        "negotiation-hello.json",
        &Hello {
            protocol: Protocol,
            client: Client {
                kind: "editor_extension".into(),
                version: "0.0.1".into(),
            },
            supported: VersionRange { min: 1, max: 1 },
            client_nonce: "fixture-client-nonce".into(),
        },
    )?;
    insert(
        &mut fixtures,
        "negotiation-welcome.json",
        &Welcome {
            selected: 1,
            desktop_version: "0.0.1".into(),
            server_nonce: "fixture-server-nonce".into(),
            authorization: Authorization::PairingRequired,
            approval_challenge: "fixture-approval-challenge".into(),
        },
    )?;
    insert(
        &mut fixtures,
        "request-run-start.json",
        &Request {
            protocol: Protocol,
            request_id: request_id.clone(),
            operation: Operation::RunStart,
            capability: "fixture-capability".into(),
            idempotency_key: Some(id(4)?),
            body: json!({"prompt":"Summarize the selected file.","workspace_id":"workspace-fixture"}),
        },
    )?;
    insert(
        &mut fixtures,
        "response-run-start.json",
        &Response {
            protocol: Protocol,
            request_id: request_id.clone(),
            ok: Success,
            body: json!({"accepted":true,"run_id":run_id.as_str()}),
        },
    )?;
    insert(
        &mut fixtures,
        "event-run-stream.json",
        &Event {
            protocol: Protocol,
            subscription_id: subscription_id.clone(),
            event: EventName::RunEvent,
            run_id: Some(run_id.clone()),
            run_seq: Some(7),
            body: json!({"kind":"assistant_message","text":"Fixture response."}),
        },
    )?;
    insert(
        &mut fixtures,
        "event-permission-pending.json",
        &Event {
            protocol: Protocol,
            subscription_id,
            event: EventName::PermissionPending,
            run_id: Some(run_id),
            run_seq: Some(8),
            body: json!({"permission_id":"permission-fixture","summary":"Allow reading the selected file?"}),
        },
    )?;
    insert(
        &mut fixtures,
        "error-protocol-incompatible.json",
        &ErrorEnvelope {
            protocol: Protocol,
            request_id: None,
            ok: Failure,
            error: ProtocolError::protocol_incompatible(
                VersionRange { min: 1, max: 1 },
                crate::ErrorAction::UpgradeCompanion,
            ),
        },
    )?;
    Ok(fixtures)
}

fn insert<T: Serialize>(
    fixtures: &mut BTreeMap<&'static str, Vec<u8>>,
    name: &'static str,
    value: &T,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    fixtures.insert(name, bytes);
    Ok(())
}

fn check(target: &Path, expected: &BTreeMap<&str, Vec<u8>>) -> io::Result<()> {
    let actual_names: BTreeSet<String> = match fs::read_dir(target) {
        Ok(entries) => entries
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<io::Result<_>>()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => return Err(error),
    };
    let expected_names: BTreeSet<String> = expected.keys().map(|name| (*name).into()).collect();
    if actual_names != expected_names {
        return Err(io::Error::other(format!(
            "fixture file set is stale (expected {expected_names:?}, found {actual_names:?})"
        )));
    }
    for (name, bytes) in expected {
        if fs::read(target.join(name))? != *bytes {
            return Err(io::Error::other(format!("fixture is byte-stale: {name}")));
        }
    }
    Ok(())
}

fn id(value: u128) -> io::Result<Id> {
    Id::new(format!("{value:032x}")).map_err(io::Error::other)
}

fn sibling_path(target: &Path, purpose: &str, export_id: usize) -> PathBuf {
    target.with_file_name(format!(".1.{purpose}.{}.{export_id}", std::process::id()))
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
