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
    #[cfg(windows)]
    if target.is_dir() && !is_reparse_point(&target)? && check(&target, &expected).is_ok() {
        // A Git checkout materializes the checked-in fixture directory as a
        // regular directory. It already contains the requested generation,
        // so there is nothing to publish. Subsequent generated targets use a
        // junction, whose destination can be changed atomically.
        return Ok(());
    }
    remove_stale_staging(parent)?;
    #[cfg(windows)]
    remove_stale_generations(parent, &target)?;
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

#[cfg(windows)]
fn remove_stale_generations(parent: &Path, target: &Path) -> io::Result<()> {
    let live = fs::canonicalize(target).ok();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".1.generation.")
            && live.as_ref() != Some(&fs::canonicalize(entry.path())?)
        {
            remove_if_present(&entry.path())?;
        }
    }
    Ok(())
}

fn publish(staging: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    return atomic_exchange(staging, target);

    #[cfg(not(windows))]
    if !target.exists() {
        return fs::rename(staging, target);
    }

    #[cfg(not(windows))]
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
    let generation = left.with_file_name(format!(
        ".1.generation.{}.{}",
        std::process::id(),
        NEXT_EXPORT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::rename(left, &generation)?;
    if right.is_dir() && !is_reparse_point(right)? {
        if let Err(error) = empty_directory(right) {
            let _ = fs::rename(&generation, left);
            return Err(error);
        }
    }
    if let Err(error) = set_junction(right, &generation) {
        let _ = fs::rename(&generation, left);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn empty_directory(directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() && !is_reparse_point(&path)? {
            fs::remove_dir_all(path)?;
        } else if metadata.is_dir() {
            fs::remove_dir(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(path: &Path) -> io::Result<bool> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    Ok(fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(windows)]
fn set_junction(junction: &Path, destination: &Path) -> io::Result<()> {
    use std::{
        os::windows::ffi::OsStrExt, os::windows::fs::OpenOptionsExt, os::windows::io::AsRawHandle,
    };

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    let destination = fs::canonicalize(destination)?;
    let mut print: Vec<u16> = destination.as_os_str().encode_wide().collect();
    const VERBATIM_PREFIX: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if print.starts_with(&VERBATIM_PREFIX) {
        print.drain(..VERBATIM_PREFIX.len());
    }
    if print.starts_with(&[b'U' as u16, b'N' as u16, b'C' as u16, b'\\' as u16]) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fixture junction generations must be on a local volume",
        ));
    }
    let mut substitute: Vec<u16> = r"\??\".encode_utf16().collect();
    substitute.extend(&print);
    let path_bytes = (substitute.len() + 1 + print.len() + 1) * 2;
    let data_length = 8 + path_bytes;
    if data_length > 16 * 1024 - 8 || data_length > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fixture junction path is too long",
        ));
    }
    let mut buffer = Vec::with_capacity(8 + data_length);
    buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend_from_slice(&(data_length as u16).to_le_bytes());
    buffer.extend_from_slice(&0_u16.to_le_bytes());
    buffer.extend_from_slice(&0_u16.to_le_bytes());
    buffer.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    buffer.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
    buffer.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    for unit in substitute
        .iter()
        .chain(std::iter::once(&0))
        .chain(print.iter())
        .chain(std::iter::once(&0))
    {
        buffer.extend_from_slice(&unit.to_le_bytes());
    }

    if !junction.exists() {
        fs::create_dir(junction)?;
    }
    let file = OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .share_mode(7)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(junction)?;
    unsafe extern "system" {
        fn DeviceIoControl(
            device: *mut core::ffi::c_void,
            control_code: u32,
            input: *const core::ffi::c_void,
            input_size: u32,
            output: *mut core::ffi::c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut core::ffi::c_void,
        ) -> i32;
    }
    let mut returned = 0;
    // SAFETY: the handle and input buffer remain valid for this synchronous
    // call; no output or OVERLAPPED structure is requested.
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            buffer.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
