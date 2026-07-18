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
    // After a Unix exchange, staging contains the complete displaced
    // generation. On Windows the atomic replacement has already removed the
    // staging name. Either way, cleanup cannot affect the live target.
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
    if !right.exists() {
        return fs::rename(left, right);
    }
    replace_directory(left, right)
}

#[cfg(windows)]
fn replace_directory(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{mem, os::windows::ffi::OsStrExt, os::windows::fs::OpenOptionsExt};

    #[repr(C)]
    struct RenameInfo {
        flags: u32,
        root_directory: *mut core::ffi::c_void,
        file_name_length: u32,
        file_name: [u16; 1],
    }

    const DELETE: u32 = 0x0001_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_RENAME_INFO_EX: u32 = 22;
    const FILE_RENAME_REPLACE_IF_EXISTS: u32 = 1;
    const FILE_RENAME_POSIX_SEMANTICS: u32 = 2;
    let destination = fs::canonicalize(
        destination
            .parent()
            .expect("fixture destination always has a parent"),
    )?
    .join(
        destination
            .file_name()
            .expect("fixture destination has a name"),
    );
    let name: Vec<u16> = destination.as_os_str().encode_wide().collect();
    let file_name_offset = mem::offset_of!(RenameInfo, file_name);
    let byte_len = file_name_offset + name.len() * mem::size_of::<u16>();
    let mut buffer = vec![0_usize; byte_len.div_ceil(mem::size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<RenameInfo>();
    // SAFETY: `buffer` is suitably aligned and large enough for the fixed
    // fields and the complete UTF-16 name copied immediately after them.
    unsafe {
        (*info).flags = FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS;
        (*info).root_directory = std::ptr::null_mut();
        (*info).file_name_length = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            buffer
                .as_mut_ptr()
                .cast::<u8>()
                .add(file_name_offset)
                .cast(),
            name.len(),
        );
    }
    let source = OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(7)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(source)?;
    unsafe extern "system" {
        fn SetFileInformationByHandle(
            file: *mut core::ffi::c_void,
            class: u32,
            information: *const core::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    use std::os::windows::io::AsRawHandle;
    // SAFETY: the handle and rename buffer remain valid for this synchronous call.
    if unsafe {
        SetFileInformationByHandle(
            source.as_raw_handle(),
            FILE_RENAME_INFO_EX,
            buffer.as_ptr().cast(),
            byte_len as u32,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::replace_directory;
    use std::fs;

    #[test]
    fn failed_atomic_publication_preserves_the_original_target() {
        let root = std::env::temp_dir().join(format!(
            "muniment-attach-publication-failure-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let source = root.join("not-a-directory");
        fs::write(&source, b"replacement").unwrap();
        let target = root.join("live");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("original.json"), b"original\n").unwrap();

        assert!(replace_directory(&source, &target).is_err());
        assert_eq!(
            fs::read(target.join("original.json")).unwrap(),
            b"original\n"
        );
        assert_eq!(fs::read(&source).unwrap(), b"replacement");
        fs::remove_dir_all(root).unwrap();
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
