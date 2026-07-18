use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io,
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
    let _export_lock = ExportLock::acquire(&target.with_file_name(".1.export.lock"))?;
    remove_stale_staging(parent)?;
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

struct ExportLock {
    _file: File,
}

impl ExportLock {
    fn acquire(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        lock_exclusive(&file)?;
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
fn lock_exclusive(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    const LOCK_EX: i32 = 2;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    // SAFETY: flock only borrows the valid descriptor for the duration of the call.
    if unsafe { flock(file.as_raw_fd(), LOCK_EX) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn lock_exclusive(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut core::ffi::c_void,
    }
    unsafe extern "system" {
        fn LockFileEx(
            file: *mut core::ffi::c_void,
            flags: u32,
            reserved: u32,
            bytes_low: u32,
            bytes_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }
    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 2;
    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: std::ptr::null_mut(),
    };
    // SAFETY: the file and stack-allocated OVERLAPPED remain valid until the
    // synchronous lock request completes.
    if unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK,
            0,
            1,
            0,
            &mut overlapped,
        )
    } != 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn remove_stale_staging(parent: &Path) -> io::Result<()> {
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".1.staging.")
        {
            remove_if_present(&entry.path())?;
        }
    }
    Ok(())
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

#[cfg(windows)]
fn atomic_exchange(left: &Path, right: &Path) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};

    type Handle = *mut core::ffi::c_void;
    const MOVEFILE_REPLACE_EXISTING: u32 = 1;
    const INVALID_HANDLE_VALUE: Handle = -1_isize as Handle;
    #[link(name = "KtmW32")]
    unsafe extern "system" {
        fn CreateTransaction(
            attributes: *const core::ffi::c_void,
            uow: *const core::ffi::c_void,
            options: u32,
            isolation_level: u32,
            isolation_flags: u32,
            timeout: u32,
            description: *const u16,
        ) -> Handle;
        fn CommitTransaction(transaction: Handle) -> i32;
    }
    unsafe extern "system" {
        fn MoveFileTransactedW(
            existing: *const u16,
            new: *const u16,
            progress: *const core::ffi::c_void,
            data: *const core::ffi::c_void,
            flags: u32,
            transaction: Handle,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    let displaced = left.with_file_name(format!(
        ".1.staging.{}.{}.displaced",
        std::process::id(),
        NEXT_EXPORT.fetch_add(1, Ordering::Relaxed)
    ));
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    };
    let left = wide(left);
    let right = wide(right);
    let displaced_wide = wide(&displaced);
    // SAFETY: all pointers passed below reference live, NUL-terminated buffers;
    // the transaction handle is closed on every path after successful creation.
    let transaction =
        unsafe { CreateTransaction(ptr::null(), ptr::null(), 0, 0, 0, 0, ptr::null()) };
    if transaction == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let moved_old = unsafe {
        MoveFileTransactedW(
            right.as_ptr(),
            displaced_wide.as_ptr(),
            ptr::null(),
            ptr::null(),
            MOVEFILE_REPLACE_EXISTING,
            transaction,
        )
    } != 0;
    let moved_new = moved_old
        && unsafe {
            MoveFileTransactedW(
                left.as_ptr(),
                right.as_ptr(),
                ptr::null(),
                ptr::null(),
                MOVEFILE_REPLACE_EXISTING,
                transaction,
            )
        } != 0;
    let committed = moved_new && unsafe { CommitTransaction(transaction) } != 0;
    let error = if committed {
        None
    } else {
        Some(io::Error::last_os_error())
    };
    unsafe { CloseHandle(transaction) };
    if let Some(error) = error {
        return Err(error);
    }
    remove_if_present(&displaced)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios", windows)))]
compile_error!("atomic fixture publication is not implemented for this platform");

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
