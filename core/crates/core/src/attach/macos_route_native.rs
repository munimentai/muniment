use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::os::fd::AsRawFd;
use std::os::raw::c_void;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use super::{MacosAttachRouteReader, MacosPeerReadError};

/// Reads the connected peer process from a Unix-domain stream.
pub struct NativeMacosAttachRouteReader<'stream> {
    stream: &'stream UnixStream,
    code_check: RefCell<CodeCheck>,
    peer_closed: Cell<bool>,
}

/// Each status stays absent until its Security call runs.
#[derive(Debug, Default, serde::Serialize)]
struct CodeCheck {
    matched: Option<bool>,
    elapsed_ms: Option<u128>,
    audit_token: bool,
    bundle: bool,
    plist_read: bool,
    /// The static checks came from an earlier connection, and no file of the
    /// expected code changed since.
    static_check_cached: bool,
    sec_static_code_create_with_path: Option<OSStatus>,
    sec_static_code_create_with_path_and_attributes: Option<OSStatus>,
    sec_static_code_check_validity: Option<OSStatus>,
    sec_code_copy_designated_requirement: Option<OSStatus>,
    sec_code_copy_guest_with_attributes: Option<OSStatus>,
    sec_code_check_validity: Option<OSStatus>,
    sec_code_copy_signing_information_disk: Option<OSStatus>,
    sec_code_copy_signing_information_peer: Option<OSStatus>,
    code_hash_match: bool,
}

impl<'stream> NativeMacosAttachRouteReader<'stream> {
    /// Creates a route reader for the connected stream.
    pub fn new(stream: &'stream UnixStream) -> Self {
        Self {
            stream,
            code_check: RefCell::new(CodeCheck::default()),
            peer_closed: Cell::new(false),
        }
    }
}

fn peer_read_failure(
    operation: &str,
    process_id: libc::pid_t,
    length: usize,
    errno: Option<i32>,
) -> MacosPeerReadError {
    crate::runtime_eprintln!(
        "muniment-runtime: macos peer read failed operation={operation} peer_pid={process_id} length={length} errno={errno:?}"
    );
    MacosPeerReadError
}

impl MacosAttachRouteReader for NativeMacosAttachRouteReader<'_> {
    fn peer_process(&self) -> Result<(u32, PathBuf), MacosPeerReadError> {
        *self.code_check.borrow_mut() = CodeCheck::default();
        self.peer_closed.set(false);
        let mut process_id: libc::pid_t = 0;
        let mut process_id_length = std::mem::size_of_val(&process_id) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                self.stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut process_id as *mut libc::pid_t).cast(),
                &mut process_id_length,
            )
        } != 0
        {
            // Readiness probes connect and close without a frame.
            // XNU returns ENOTCONN if the peer closes before this read, and
            // that close is quiet.
            let errno = std::io::Error::last_os_error().raw_os_error();
            if errno == Some(libc::ENOTCONN) {
                self.peer_closed.set(true);
                return Err(MacosPeerReadError);
            }
            return Err(peer_read_failure(
                "LOCAL_PEERPID",
                process_id,
                process_id_length as usize,
                errno,
            ));
        }
        if process_id_length as usize != std::mem::size_of_val(&process_id) || process_id <= 0 {
            return Err(peer_read_failure(
                "LOCAL_PEERPID-invalid-result",
                process_id,
                process_id_length as usize,
                None,
            ));
        }
        let peer_pid = process_id as u32;

        let mut buffer = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let path_length = unsafe {
            libc::proc_pidpath(
                process_id,
                buffer.as_mut_ptr().cast(),
                libc::PROC_PIDPATHINFO_MAXSIZE as u32,
            )
        };
        if path_length <= 0 {
            let errno = std::io::Error::last_os_error().raw_os_error();
            return Err(peer_read_failure("proc_pidpath", process_id, 0, errno));
        }
        let path_length = path_length as usize;
        if path_length > buffer.len() {
            return Err(peer_read_failure(
                "proc_pidpath-invalid-length",
                process_id,
                path_length,
                None,
            ));
        }
        buffer.truncate(path_length);
        if buffer.last() == Some(&0) {
            buffer.pop();
        }
        if buffer.is_empty() || buffer.contains(&0) {
            return Err(peer_read_failure(
                "proc_pidpath-invalid-path",
                process_id,
                path_length,
                None,
            ));
        }

        Ok((peer_pid, OsString::from_vec(buffer).into()))
    }

    fn peer_code_matches(&self, expected_desktop_executable: &Path) -> bool {
        let mut check = self.code_check.borrow_mut();
        *check = CodeCheck::default();
        let started = std::time::Instant::now();
        let matched = if let Some(token) = peer_audit_token(self.stream) {
            check.audit_token = true;
            audit_token_satisfies(&token, expected_desktop_executable, &mut check)
        } else {
            false
        };
        check.matched = Some(matched);
        check.elapsed_ms = Some(started.elapsed().as_millis());
        matched
    }

    fn peer_closed(&self) -> bool {
        self.peer_closed.get() || peer_sent_end_of_stream(self.stream)
    }

    fn peer_descends_from_this_process(&self, peer_pid: u32) -> bool {
        descends_from(peer_pid, std::process::id(), parent_pid)
    }

    fn code_check_diagnostic(&self) -> serde_json::Value {
        serde_json::to_value(&*self.code_check.borrow())
            .expect("The code check fields must serialize.")
    }

    fn log_companion_fallback(
        &self,
        peer_pid: u32,
        path_match: bool,
        peer_image_path: Option<&Path>,
        expected_desktop_executable: &Path,
    ) {
        crate::runtime_eprintln!(
            "muniment-runtime: macos attach route=Companion peer_pid={peer_pid} path_match={path_match} peer_path={peer_image_path:?} expected_path={expected_desktop_executable:?} code_check={:?}",
            self.code_check.borrow()
        );
    }
}

/// Pi runs the record server as its own child, and the runtime runs Pi, so a
/// few parents cover the chain. A wrapper such as a shell adds one.
const MAX_ANCESTRY: usize = 8;

/// Whether `ancestor` is a parent of `pid`, or a parent of one of its parents,
/// within `MAX_ANCESTRY` steps. launchd, pid 1, ends the walk.
fn descends_from(pid: u32, ancestor: u32, parent_of: impl Fn(u32) -> Option<u32>) -> bool {
    let mut current = pid;
    for _ in 0..MAX_ANCESTRY {
        let Some(parent) = parent_of(current) else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        if parent <= 1 || parent == current {
            return false;
        }
        current = parent;
    }
    false
}

/// The parent pid the kernel holds for `pid`.
fn parent_pid(pid: u32) -> Option<u32> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is one writable `proc_bsdinfo` of `size` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the kernel filled all `size` bytes.
    Some(unsafe { info.assume_init() }.pbi_ppid)
}

/// Whether the peer closed with nothing unread, read without consuming a byte.
/// A peer that sent a frame before it closed still has bytes to read.
fn peer_sent_end_of_stream(stream: &UnixStream) -> bool {
    let mut byte = 0_u8;
    // SAFETY: the descriptor and one-byte buffer remain valid for this call.
    let read = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    read == 0
}

/// `audit_token_t` from <bsm/audit.h>: eight 32-bit words.
type AuditToken = [u32; 8];

/// Reads the audit token the kernel recorded when the peer connected.
fn peer_audit_token(stream: &UnixStream) -> Option<AuditToken> {
    let mut token: AuditToken = [0; 8];
    let mut length = std::mem::size_of_val(&token) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut length,
        )
    };
    (result == 0 && length as usize == std::mem::size_of_val(&token)).then_some(token)
}

type CFTypeRef = *const c_void;
type OSStatus = i32;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CFTypeRef;
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFDataCreate(allocator: CFTypeRef, bytes: *const u8, length: isize) -> CFTypeRef;
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        string: *const i8,
        encoding: u32,
    ) -> CFTypeRef;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: CFTypeRef,
        buffer: *const u8,
        length: isize,
        is_directory: u8,
    ) -> CFTypeRef;
    fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
    fn CFDataGetLength(data: CFTypeRef) -> isize;
    fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
    fn CFRelease(value: CFTypeRef);
}

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecGuestAttributeAudit: CFTypeRef;
    static kSecGuestAttributeDynamicCode: CFTypeRef;
    static kSecGuestAttributeDynamicCodeInfoPlist: CFTypeRef;
    static kSecCodeInfoUnique: CFTypeRef;
    static kSecCodeInfoCdHashes: CFTypeRef;
    static kSecCodeAttributeArchitecture: CFTypeRef;
    fn SecCodeCopyGuestWithAttributes(
        host: CFTypeRef,
        attributes: CFTypeRef,
        flags: u32,
        guest: *mut CFTypeRef,
    ) -> OSStatus;
    fn SecStaticCodeCreateWithPath(path: CFTypeRef, flags: u32, code: *mut CFTypeRef) -> OSStatus;
    fn SecStaticCodeCreateWithPathAndAttributes(
        path: CFTypeRef,
        flags: u32,
        attributes: CFTypeRef,
        code: *mut CFTypeRef,
    ) -> OSStatus;
    fn SecStaticCodeCheckValidity(code: CFTypeRef, flags: u32, requirement: CFTypeRef) -> OSStatus;
    fn SecCodeCopyDesignatedRequirement(
        code: CFTypeRef,
        flags: u32,
        requirement: *mut CFTypeRef,
    ) -> OSStatus;
    fn SecCodeCheckValidity(code: CFTypeRef, flags: u32, requirement: CFTypeRef) -> OSStatus;
    fn SecRequirementCopyData(requirement: CFTypeRef, flags: u32, data: *mut CFTypeRef)
        -> OSStatus;
    fn SecRequirementCreateWithData(
        data: CFTypeRef,
        flags: u32,
        requirement: *mut CFTypeRef,
    ) -> OSStatus;
    fn SecCodeCopySigningInformation(
        code: CFTypeRef,
        flags: u32,
        information: *mut CFTypeRef,
    ) -> OSStatus;
}

/// Releases one owned Core Foundation reference when it drops.
struct Owned(CFTypeRef);

impl Owned {
    fn new(value: CFTypeRef) -> Option<Self> {
        (!value.is_null()).then_some(Self(value))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

/// What the code check reads from the expected desktop on disk: the static
/// checks a connection would otherwise repeat. The bundle seal check hashes
/// every file of the bundle, hundreds of megabytes for the desktop.
#[derive(Clone, Debug)]
struct ExpectedCode {
    /// The designated requirement, as `SecRequirementCopyData` writes it.
    requirement: Vec<u8>,
    /// The bundle's signed Info.plist, which the guest lookup binds.
    plist: Option<Vec<u8>>,
    /// The code directory hashes of each architecture slice of a bundle that
    /// passed its seal check.
    code_hashes: Vec<Vec<u8>>,
    /// The Security statuses of the static checks, for the diagnostic.
    statuses: StaticStatuses,
}

#[derive(Clone, Copy, Debug, Default)]
struct StaticStatuses {
    sec_static_code_create_with_path: Option<OSStatus>,
    sec_static_code_create_with_path_and_attributes: Option<OSStatus>,
    sec_static_code_check_validity: Option<OSStatus>,
    sec_code_copy_designated_requirement: Option<OSStatus>,
    sec_code_copy_signing_information_disk: Option<OSStatus>,
}

impl StaticStatuses {
    fn record(&self, check: &mut CodeCheck) {
        check.sec_static_code_create_with_path = self.sec_static_code_create_with_path;
        check.sec_static_code_create_with_path_and_attributes =
            self.sec_static_code_create_with_path_and_attributes;
        check.sec_static_code_check_validity = self.sec_static_code_check_validity;
        check.sec_code_copy_designated_requirement = self.sec_code_copy_designated_requirement;
        check.sec_code_copy_signing_information_disk = self.sec_code_copy_signing_information_disk;
    }
}

/// One file of the expected code as `lstat` reads it. Any write, rename,
/// replacement, or permission change alters the inode or one of these times.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    path: PathBuf,
    device: u64,
    inode: u64,
    mode: u32,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

/// Stamps every file under `root`, or `root` alone when it is a file, in path
/// order. Symbolic links are stamped, never followed.
fn code_fingerprint(root: &Path) -> Option<Vec<FileStamp>> {
    use std::os::unix::fs::MetadataExt;
    let mut stamps = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).ok()? {
                pending.push(entry.ok()?.path());
            }
        }
        stamps.push(FileStamp {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            size: metadata.size(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            path,
        });
    }
    stamps.sort_by(|left, right| left.path.cmp(&right.path));
    Some(stamps)
}

/// The expected code of one executable path, kept while its files keep
/// their fingerprint.
struct CachedCode {
    fingerprint: Vec<FileStamp>,
    code: ExpectedCode,
}

/// The runtime checks one desktop executable for its whole life, so the map
/// holds one entry outside tests.
static EXPECTED_CODE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, CachedCode>>,
> = std::sync::LazyLock::new(Default::default);

/// Returns the static checks of the expected code, from the cache while no file
/// under `root` changed. A fresh read counts only when the fingerprint is the
/// same before and after it, so a bundle that changes during the read is read
/// again on the next connection. A failed read is never kept.
fn expected_code(
    executable: &Path,
    root: &Path,
    read: impl FnOnce() -> Result<ExpectedCode, StaticStatuses>,
    check: &mut CodeCheck,
) -> Option<ExpectedCode> {
    let before = code_fingerprint(root);
    let mut cache = EXPECTED_CODE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    match (&before, cache.get(executable)) {
        (Some(before), Some(cached)) if &cached.fingerprint == before => {
            check.static_check_cached = true;
            cached.code.statuses.record(check);
            return Some(cached.code.clone());
        }
        _ => {
            cache.remove(executable);
        }
    }
    drop(cache);
    let code = read();
    let statuses = match &code {
        Ok(code) => code.statuses,
        Err(statuses) => *statuses,
    };
    statuses.record(check);
    let code = code.ok()?;
    if let Some(before) = before.filter(|before| code_fingerprint(root).as_ref() == Some(before)) {
        EXPECTED_CODE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                executable.to_path_buf(),
                CachedCode {
                    fingerprint: before,
                    code: code.clone(),
                },
            );
    }
    Some(code)
}

/// Copies the bytes of a `CFData`.
///
/// # Safety
///
/// `data` must be a valid `CFData` reference.
unsafe fn data_bytes(data: CFTypeRef) -> Vec<u8> {
    let length = CFDataGetLength(data);
    let pointer = CFDataGetBytePtr(data);
    if pointer.is_null() || length <= 0 {
        return Vec::new();
    }
    std::slice::from_raw_parts(pointer, length as usize).to_vec()
}

/// Runs the static checks on the installed desktop: the bundle seal over every
/// architecture, the designated requirement, and the code directory hashes of
/// each architecture slice, each slice's seal checked first.
fn read_expected_code(
    executable: &Path,
    contents: Option<&Path>,
    bundle: Option<&Path>,
) -> Result<ExpectedCode, StaticStatuses> {
    const DEFAULT_FLAGS: u32 = 0;
    let mut statuses = StaticStatuses::default();
    let plist = match contents {
        Some(contents) => Some(std::fs::read(contents.join("Info.plist")).map_err(|_| statuses)?),
        None => None,
    };
    let path = bundle.unwrap_or(executable).as_os_str().as_bytes();
    unsafe {
        let url = Owned::new(CFURLCreateFromFileSystemRepresentation(
            kCFAllocatorDefault,
            path.as_ptr(),
            path.len() as isize,
            u8::from(bundle.is_some()),
        ))
        .ok_or(statuses)?;
        let mut static_code = std::ptr::null();
        let status = SecStaticCodeCreateWithPath(url.0, DEFAULT_FLAGS, &mut static_code);
        statuses.sec_static_code_create_with_path = Some(status);
        if status != 0 {
            return Err(statuses);
        }
        let static_code = Owned::new(static_code).ok_or(statuses)?;
        if bundle.is_some() {
            // Validate the bundle seal before binding its plist to the live code.
            const CHECK_ALL_ARCHITECTURES: u32 = 1 << 0;
            let status = SecStaticCodeCheckValidity(
                static_code.0,
                CHECK_ALL_ARCHITECTURES,
                std::ptr::null(),
            );
            statuses.sec_static_code_check_validity = Some(status);
            if status != 0 {
                return Err(statuses);
            }
        }
        let mut requirement = std::ptr::null();
        let status =
            SecCodeCopyDesignatedRequirement(static_code.0, DEFAULT_FLAGS, &mut requirement);
        statuses.sec_code_copy_designated_requirement = Some(status);
        if status != 0 {
            return Err(statuses);
        }
        let requirement = Owned::new(requirement).ok_or(statuses)?;
        let mut requirement_data = std::ptr::null();
        if SecRequirementCopyData(requirement.0, DEFAULT_FLAGS, &mut requirement_data) != 0 {
            return Err(statuses);
        }
        let requirement_data = Owned::new(requirement_data).ok_or(statuses)?;
        let mut code = ExpectedCode {
            requirement: data_bytes(requirement_data.0),
            plist,
            code_hashes: Vec::new(),
            statuses,
        };
        if bundle.is_none() {
            return Ok(code);
        }
        // CdHashes covers digest algorithms, not architectures. Keep both desktop slices for Rosetta.
        for architecture in [c"arm64", c"x86_64"] {
            const UTF8: u32 = 0x08000100;
            let architecture = Owned::new(CFStringCreateWithCString(
                kCFAllocatorDefault,
                architecture.as_ptr(),
                UTF8,
            ))
            .ok_or(code.statuses)?;
            let attributes = Owned::new(CFDictionaryCreate(
                kCFAllocatorDefault,
                &kSecCodeAttributeArchitecture,
                &architecture.0,
                1,
                std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
                std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
            ))
            .ok_or(code.statuses)?;
            let mut slice = std::ptr::null();
            let status = SecStaticCodeCreateWithPathAndAttributes(
                url.0,
                DEFAULT_FLAGS,
                attributes.0,
                &mut slice,
            );
            code.statuses
                .sec_static_code_create_with_path_and_attributes = Some(status);
            if status != 0 {
                continue;
            }
            let slice = Owned::new(slice).ok_or(code.statuses)?;
            let status = SecStaticCodeCheckValidity(slice.0, DEFAULT_FLAGS, std::ptr::null());
            code.statuses.sec_static_code_check_validity = Some(status);
            if status != 0 {
                return Err(code.statuses);
            }
            let mut disk_information = std::ptr::null();
            let status =
                SecCodeCopySigningInformation(slice.0, DEFAULT_FLAGS, &mut disk_information);
            code.statuses.sec_code_copy_signing_information_disk = Some(status);
            if status != 0 {
                return Err(code.statuses);
            }
            let disk_information = Owned::new(disk_information).ok_or(code.statuses)?;
            let disk_hashes = CFDictionaryGetValue(disk_information.0, kSecCodeInfoCdHashes);
            if !disk_hashes.is_null() {
                for index in 0..CFArrayGetCount(disk_hashes) {
                    code.code_hashes
                        .push(data_bytes(CFArrayGetValueAtIndex(disk_hashes, index)));
                }
            }
        }
        Ok(code)
    }
}

/// Checks the process named by `token` against the designated requirement of the
/// installed desktop executable. The lookup by audit token fails for a process whose
/// pid version changed, so an exec after connect never passes. The static checks of
/// the installed desktop run once while its files keep their fingerprint, and every
/// connection checks its live code against their result.
fn audit_token_satisfies(
    token: &AuditToken,
    expected_desktop_executable: &Path,
    check: &mut CodeCheck,
) -> bool {
    const DEFAULT_FLAGS: u32 = 0;
    let contents = expected_desktop_executable
        .parent()
        .filter(|parent| parent.file_name() == Some(std::ffi::OsStr::new("MacOS")))
        .and_then(Path::parent)
        .filter(|parent| parent.file_name() == Some(std::ffi::OsStr::new("Contents")));
    let bundle = contents.and_then(Path::parent);
    check.bundle = bundle.is_some();
    let Some(code) = expected_code(
        expected_desktop_executable,
        bundle.unwrap_or(expected_desktop_executable),
        || read_expected_code(expected_desktop_executable, contents, bundle),
        check,
    ) else {
        return false;
    };
    check.plist_read = code.plist.is_some();
    unsafe {
        let Some(requirement_data) = Owned::new(CFDataCreate(
            kCFAllocatorDefault,
            code.requirement.as_ptr(),
            code.requirement.len() as isize,
        )) else {
            return false;
        };
        let mut requirement = std::ptr::null();
        if SecRequirementCreateWithData(requirement_data.0, DEFAULT_FLAGS, &mut requirement) != 0 {
            return false;
        }
        let Some(requirement) = Owned::new(requirement) else {
            return false;
        };
        let Some(token_data) = Owned::new(CFDataCreate(
            kCFAllocatorDefault,
            token.as_ptr().cast(),
            std::mem::size_of_val(token) as isize,
        )) else {
            return false;
        };
        // Executable-path bundle discovery can lose the signed Info.plist binding.
        // Supply the raw plist with the audit token. Security checks its signed hash.
        // Keep both the bundle seal check and the live designated requirement check.
        let plist_data = match &code.plist {
            Some(bytes) => {
                let Some(data) = Owned::new(CFDataCreate(
                    kCFAllocatorDefault,
                    bytes.as_ptr(),
                    bytes.len() as isize,
                )) else {
                    return false;
                };
                Some(data)
            }
            None => None,
        };
        let mut keys = vec![kSecGuestAttributeAudit];
        let mut values = vec![token_data.0];
        if let Some(data) = &plist_data {
            keys.extend([
                kSecGuestAttributeDynamicCode,
                kSecGuestAttributeDynamicCodeInfoPlist,
            ]);
            values.extend([kCFBooleanTrue, data.0]);
        }
        let Some(attributes) = Owned::new(CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr(),
            values.as_ptr(),
            keys.len() as isize,
            std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
            std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
        )) else {
            return false;
        };
        let mut guest = std::ptr::null();
        let status = SecCodeCopyGuestWithAttributes(
            std::ptr::null(),
            attributes.0,
            DEFAULT_FLAGS,
            &mut guest,
        );
        check.sec_code_copy_guest_with_attributes = Some(status);
        if status != 0 {
            return false;
        }
        let Some(guest) = Owned::new(guest) else {
            return false;
        };
        let status = SecCodeCheckValidity(guest.0, DEFAULT_FLAGS, requirement.0);
        check.sec_code_check_validity = Some(status);
        if status != 0 {
            return false;
        }
        if bundle.is_none() {
            return true;
        }

        // Bind the live code to the installed build, not just its signing identity.
        // A replaced executable must not admit an older process with the same requirement.
        // Compare the expected bundle's hashes, not the peer clone's on-disk hashes.
        let mut peer_information = std::ptr::null();
        let status = SecCodeCopySigningInformation(guest.0, DEFAULT_FLAGS, &mut peer_information);
        check.sec_code_copy_signing_information_peer = Some(status);
        if status != 0 {
            return false;
        }
        let Some(peer_information) = Owned::new(peer_information) else {
            return false;
        };
        let peer_hash = CFDictionaryGetValue(peer_information.0, kSecCodeInfoUnique);
        if peer_hash.is_null() {
            return false;
        }
        let peer_hash = data_bytes(peer_hash);
        check.code_hash_match = !peer_hash.is_empty() && code.code_hashes.contains(&peer_hash);
        check.code_hash_match
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn connected_pair() -> (UnixStream, UnixStream, PathBuf) {
        let directory = PathBuf::from("/tmp").join(format!(
            "muniment-route-native-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server, directory)
    }

    #[test]
    fn signed_bundle_routes_require_the_live_signature_and_unchanged_bundle() {
        check_signed_bundle_route(false);
    }

    #[test]
    fn signed_clone_routes_require_the_live_signature_and_unchanged_bundle() {
        check_signed_bundle_route(true);
    }

    fn check_signed_bundle_route(from_clone: bool) {
        use super::super::{name_macos_attach_connection_route, MacosAttachConnectionRoute};
        use std::io::Read;
        use std::process::{Child, Command};

        struct Fixture {
            directory: PathBuf,
            child: Option<Child>,
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                if let Some(child) = &mut self.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                let _ = std::fs::remove_dir_all(&self.directory);
            }
        }
        fn run(command: &mut Command) {
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut fixture = Fixture {
            directory: PathBuf::from("/tmp").join(format!(
                "muniment-signed-route-{}",
                uuid::Uuid::now_v7().simple()
            )),
            child: None,
        };
        let app = fixture.directory.join("Peer.app");
        let contents = app.join("Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        let executable = contents.join("MacOS/peer");
        let plist = contents.join("Info.plist");
        let plist_bytes = br#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>peer</string>
<key>CFBundleIdentifier</key><string>ai.muniment.route-test</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>"#;
        std::fs::write(&plist, plist_bytes).unwrap();
        let source = fixture.directory.join("peer.c");
        std::fs::write(
            &source,
            r#"#include <sys/socket.h>
#include <sys/un.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
    if (argc != 2) return BUILD;
    struct sockaddr_un address = {0};
    address.sun_family = AF_UNIX;
    if (strlen(argv[1]) >= sizeof(address.sun_path)) return 2;
    strcpy(address.sun_path, argv[1]);
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0 || connect(fd, (void *)&address, sizeof(address))) return 3;
    if (write(fd, "!", 1) != 1) return 4;
    char done;
    read(fd, &done, 1);
    close(fd);
    return 0;
}
"#,
        )
        .unwrap();
        let compile = |output: &Path, build: &str| {
            let mut command = Command::new("/usr/bin/clang");
            // A universal build leaves an empty `peer-*` folder per run in
            // the temporary folder. The fixture folder holds it instead.
            command.env("TMPDIR", &fixture.directory);
            if from_clone {
                command.args(["-arch", "arm64", "-arch", "x86_64"]);
            }
            run(command
                .arg(format!("-DBUILD={build}"))
                .arg(&source)
                .arg("-o")
                .arg(output));
        };
        let sign = || {
            run(Command::new("/usr/bin/codesign")
                .args([
                    "--force",
                    "--sign",
                    "-",
                    "--identifier",
                    "ai.muniment.route-test",
                ])
                .args([
                    "--requirements",
                    // A leading `=` makes codesign read the text, not a file path.
                    "=designated => identifier \"ai.muniment.route-test\"",
                ])
                .arg(&app));
        };
        compile(&executable, "1");
        sign();
        let endpoint = fixture.directory.join("s.sock");
        let listener = UnixListener::bind(&endpoint).unwrap();
        listener.set_nonblocking(true).unwrap();
        let clone = fixture.directory.join(
            "X/ai.muniment.desktop.code_sign_clone/code_sign_clone.fixture/muniment.app.bundle",
        );
        std::fs::create_dir_all(clone.parent().unwrap()).unwrap();
        run(Command::new("/usr/bin/ditto").arg(&app).arg(&clone));
        let clone_executable = clone.join("Contents/MacOS/peer");
        let peer_executable = if from_clone {
            &clone_executable
        } else {
            &executable
        };
        fixture.child = Some(
            Command::new(peer_executable)
                .arg(&endpoint)
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut server = loop {
            match listener.accept() {
                Ok((server, _)) => break server,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "The peer did not connect."
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        };
        server
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        server.read_exact(&mut [0]).unwrap();
        let peer_pid = fixture.child.as_ref().unwrap().id();
        // proc_pidpath resolves /tmp to /private/tmp on macOS.
        let expected = executable.canonicalize().unwrap();
        let reader = NativeMacosAttachRouteReader::new(&server);
        assert_eq!(
            name_macos_attach_connection_route(&reader, &expected),
            MacosAttachConnectionRoute::DesktopClient { peer_pid },
            "{:?}",
            reader.code_check.borrow()
        );
        assert_eq!(reader.code_check.borrow().sec_code_check_validity, Some(0));
        assert!(reader.code_check.borrow().code_hash_match);
        assert_eq!(reader.code_check_diagnostic()["matched"], true);
        assert!(reader.code_check_diagnostic()["elapsed_ms"].is_number());
        assert_eq!(reader.code_check_diagnostic()["static_check_cached"], false);
        // The next connection reuses the static checks and still checks the live code.
        assert!(reader.peer_code_matches(&expected));
        assert_eq!(reader.code_check_diagnostic()["static_check_cached"], true);
        assert_eq!(reader.code_check.borrow().sec_code_check_validity, Some(0));
        assert!(reader.code_check.borrow().code_hash_match);
        assert_eq!(
            name_macos_attach_connection_route(&reader, &clone_executable),
            MacosAttachConnectionRoute::DesktopClient { peer_pid },
            "{:?}",
            reader.code_check.borrow()
        );

        std::fs::write(&plist, b"invalid plist").unwrap();
        assert_eq!(
            name_macos_attach_connection_route(&reader, &expected),
            MacosAttachConnectionRoute::Companion { peer_pid }
        );
        assert_eq!(reader.code_check_diagnostic()["static_check_cached"], false);
        std::fs::write(&plist, plist_bytes).unwrap();
        // The restored bundle is checked again before the cache serves it.
        assert!(reader.peer_code_matches(&expected));
        assert_eq!(reader.code_check_diagnostic()["static_check_cached"], false);
        assert!(reader.peer_code_matches(&expected));
        assert_eq!(reader.code_check_diagnostic()["static_check_cached"], true);

        // Replace the file without changing the running image or the designated requirement.
        let replacement = fixture.directory.join("replacement");
        compile(&replacement, "2");
        std::fs::rename(replacement, &executable).unwrap();
        sign();
        assert!(!reader.peer_code_matches(&expected));
        assert_eq!(reader.code_check_diagnostic()["static_check_cached"], false);
        assert_eq!(reader.code_check.borrow().sec_code_check_validity, Some(0));
        assert!(!reader.code_check.borrow().code_hash_match);
        assert_eq!(reader.code_check_diagnostic()["matched"], false);
        assert_eq!(
            name_macos_attach_connection_route(&reader, &expected),
            MacosAttachConnectionRoute::Companion { peer_pid }
        );
    }

    #[test]
    fn a_closed_peer_fails_the_peer_read_without_desktop_admission() {
        use super::super::{name_macos_attach_connection_route, MacosAttachConnectionRoute};
        use std::io::Read;

        let (client, mut server, directory) = connected_pair();
        // Darwin rejects socket options once the peer has closed.
        server
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        drop(client);
        assert_eq!(server.read(&mut [0]).unwrap(), 0);
        let reader = NativeMacosAttachRouteReader::new(&server);
        assert_eq!(reader.peer_process(), Err(MacosPeerReadError));
        // The probe's close is quiet: the route logs no fallback for it.
        assert!(reader.peer_closed());
        assert_eq!(
            name_macos_attach_connection_route(&reader, &std::env::current_exe().unwrap()),
            MacosAttachConnectionRoute::Companion { peer_pid: 0 }
        );
        assert!(!reader.code_check.borrow().audit_token);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_probe_that_closes_after_the_pid_read_is_closed_and_a_frame_is_not() {
        use std::io::Write;

        let (client, server, directory) = connected_pair();
        let reader = NativeMacosAttachRouteReader::new(&server);
        assert!(reader.peer_process().is_ok());
        assert!(!reader.peer_closed());
        drop(client);
        assert!(reader.peer_closed());
        std::fs::remove_dir_all(directory).unwrap();

        let (mut client, server, directory) = connected_pair();
        let reader = NativeMacosAttachRouteReader::new(&server);
        assert!(reader.peer_process().is_ok());
        client.write_all(b"frame").unwrap();
        drop(client);
        assert!(!reader.peer_closed());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ancestry_reaches_the_runtime_through_its_children_only() {
        // cli 30 -> pi 20 -> runtime 10 -> launchd 1
        let parents = |pid: u32| match pid {
            30 => Some(20),
            20 => Some(10),
            10 => Some(1),
            40 => Some(1),
            _ => None,
        };
        assert!(descends_from(30, 10, parents));
        assert!(descends_from(20, 10, parents));
        assert!(!descends_from(40, 10, parents));
        assert!(!descends_from(10, 10, parents));
        assert!(!descends_from(99, 10, parents));
        // A chain past the bound stops the walk.
        assert!(!descends_from(100, 1_000, |pid| Some(pid + 1)));
    }

    #[test]
    fn this_process_is_the_parent_of_its_child() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .unwrap();
        assert_eq!(parent_pid(child.id()), Some(std::process::id()));
        assert!(descends_from(child.id(), std::process::id(), parent_pid));
        assert!(!descends_from(std::process::id(), child.id(), parent_pid));
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn the_peer_token_names_this_process_and_its_signature() {
        let (_client, server, directory) = connected_pair();
        let reader = NativeMacosAttachRouteReader::new(&server);
        let (pid, _) = reader.peer_process().unwrap();
        assert_eq!(pid, std::process::id());
        let token = peer_audit_token(&server).unwrap();
        // audit_token_t word 5 holds the pid.
        assert_eq!(token[5], std::process::id());
        // The test binary is the peer, so its own designated requirement admits it.
        let this_executable = std::env::current_exe().unwrap();
        assert!(reader.peer_code_matches(&this_executable));
        // Another binary's designated requirement does not.
        assert!(!reader.peer_code_matches(Path::new("/bin/ls")));
        assert_eq!(
            super::super::name_macos_attach_connection_route(&reader, Path::new("/bin/ls")),
            super::super::MacosAttachConnectionRoute::Companion { peer_pid: pid }
        );
        // A path mismatch must not erase the failed code check from diagnostics.
        assert!(reader.code_check.borrow().audit_token);
        assert_ne!(reader.code_check.borrow().sec_code_check_validity, Some(0));
        std::fs::remove_dir_all(directory).unwrap();
    }
    /// Measures the code check against an installed bundle, which it only
    /// reads. Run it with `--ignored --nocapture`; the test process is the
    /// peer, so the check runs every step and fails the requirement.
    #[test]
    #[ignore = "reads an installed app bundle"]
    fn bench_the_code_check_against_an_installed_bundle() {
        let bundle = std::env::var_os("MUNIMENT_CODE_CHECK_BUNDLE").map_or_else(
            || PathBuf::from("/Applications/muniment.app"),
            PathBuf::from,
        );
        let executable = bundle.join("Contents/MacOS/muniment-desktop");
        let (_client, server, directory) = connected_pair();
        let reader = NativeMacosAttachRouteReader::new(&server);
        for attempt in 0..5 {
            assert!(!reader.peer_code_matches(&executable));
            println!(
                "code check attempt={attempt} {}",
                reader.code_check_diagnostic()
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn stub_code(requirement: &[u8]) -> ExpectedCode {
        ExpectedCode {
            requirement: requirement.to_vec(),
            plist: None,
            code_hashes: Vec::new(),
            statuses: StaticStatuses::default(),
        }
    }

    #[test]
    fn any_change_to_a_bundle_file_changes_its_fingerprint() {
        let root = PathBuf::from("/tmp").join(format!(
            "muniment-fingerprint-{}",
            uuid::Uuid::now_v7().simple()
        ));
        let contents = root.join("Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        let executable = contents.join("MacOS/app");
        std::fs::write(&executable, b"one").unwrap();
        let first = code_fingerprint(&root).unwrap();
        assert_eq!(code_fingerprint(&root).unwrap(), first);
        assert_eq!(first.len(), 4);
        // Same size, new content.
        std::fs::write(&executable, b"two").unwrap();
        let rewritten = code_fingerprint(&root).unwrap();
        assert_ne!(rewritten, first);
        // A replacement by rename.
        std::fs::write(root.join("replacement"), b"two").unwrap();
        std::fs::rename(root.join("replacement"), &executable).unwrap();
        let replaced = code_fingerprint(&root).unwrap();
        assert_ne!(replaced, rewritten);
        // A new file deep in the bundle, and a mode change.
        std::fs::write(contents.join("Resources"), b"").unwrap();
        let added = code_fingerprint(&root).unwrap();
        assert_ne!(added, replaced);
        std::fs::set_permissions(
            &executable,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        assert_ne!(code_fingerprint(&root).unwrap(), added);
        // A file alone is its own fingerprint, and a missing one has none.
        assert_eq!(code_fingerprint(&executable).unwrap().len(), 1);
        assert_eq!(code_fingerprint(&root.join("missing")), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_static_check_is_cached_until_the_bundle_changes() {
        let root = PathBuf::from("/tmp").join(format!(
            "muniment-expected-code-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(root.join("Contents/MacOS")).unwrap();
        let executable = root.join("Contents/MacOS/app");
        std::fs::write(&executable, b"one").unwrap();
        let reads = Cell::new(0);
        let read = |requirement: &'static [u8]| {
            let reads = &reads;
            move || {
                reads.set(reads.get() + 1);
                Ok(stub_code(requirement))
            }
        };
        let check = |read| {
            let mut check = CodeCheck::default();
            let code = expected_code(&executable, &root, read, &mut check);
            (code.map(|code| code.requirement), check.static_check_cached)
        };
        assert_eq!(check(read(b"first")), (Some(b"first".to_vec()), false));
        assert_eq!(check(read(b"unused")), (Some(b"first".to_vec()), true));
        assert_eq!(reads.get(), 1);
        // A changed bundle is read again.
        std::fs::write(&executable, b"two").unwrap();
        assert_eq!(check(read(b"second")), (Some(b"second".to_vec()), false));
        assert_eq!(check(read(b"unused")), (Some(b"second".to_vec()), true));
        assert_eq!(reads.get(), 2);
        // A failed read is not kept, and drops the old entry.
        std::fs::write(&executable, b"three").unwrap();
        let mut failed = CodeCheck::default();
        let failure = StaticStatuses {
            sec_static_code_check_validity: Some(-67061),
            ..StaticStatuses::default()
        };
        assert!(expected_code(&executable, &root, || Err(failure), &mut failed).is_none());
        assert_eq!(failed.sec_static_code_check_validity, Some(-67061));
        assert!(!failed.static_check_cached);
        assert_eq!(check(read(b"third")), (Some(b"third".to_vec()), false));
        assert_eq!(reads.get(), 3);
        // A bundle that changes during the read is read again next time.
        std::fs::write(&executable, b"before").unwrap();
        let mut during = CodeCheck::default();
        let changing = || {
            std::fs::write(&executable, b"four").unwrap();
            Ok(stub_code(b"during"))
        };
        assert!(expected_code(&executable, &root, changing, &mut during).is_some());
        assert!(!during.static_check_cached);
        assert_eq!(check(read(b"fourth")), (Some(b"fourth".to_vec()), false));
        assert_eq!(check(read(b"unused")), (Some(b"fourth".to_vec()), true));
        std::fs::remove_dir_all(root).unwrap();
    }
}
