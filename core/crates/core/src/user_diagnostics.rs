use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::Path;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::PathBuf;

/// Resolves the effective user's home from the OS user database.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn effective_user_home() -> io::Result<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStringExt;

    let uid = unsafe { libc::geteuid() };
    let buffer_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let buffer_size = if buffer_size > 0 {
        usize::try_from(buffer_size).map_err(|_| io::Error::from(io::ErrorKind::NotFound))?
    } else {
        16 * 1024
    };
    let mut buffer = vec![0_u8; buffer_size];
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    let record = unsafe { record.assume_init() };
    if record.pw_dir.is_null() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    let home = unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes();
    Ok(PathBuf::from(std::ffi::OsString::from_vec(home.to_vec())))
}

/// Appends a record, after its UTC time, to an owner-only file. A record that
/// would take the file past `max_bytes` first moves the file to `<name>.1`,
/// which replaces the older one, and starts a new file. Each log name then
/// holds at most two files of `max_bytes`, and the lines before a rotation
/// stay readable in `<name>.1`.
///
/// Every writer process locks the file it opened and checks that the name
/// still points at it, so a writer that waited out another's rotation
/// reopens the new file instead of appending to the moved one.
pub fn append_owner_only_record(
    directory: &Path,
    file_name: &std::ffi::CStr,
    max_bytes: u64,
    record: &[u8],
) -> io::Result<()> {
    crate::state_root::guard_log_directory(directory);
    match fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }

    let mut directory_options = OpenOptions::new();
    directory_options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let directory = directory_options.open(directory)?;
    validate_owner_only_directory(&directory.metadata()?)?;
    let rotated = rotated_name(file_name)?;
    let record = crate::runtime_diagnostics::stamped(record);

    // Each pass either writes, or finds the name moved by another writer or
    // by this one's rotation and opens the file the name now holds.
    for _ in 0..16 {
        let mut file = open_owner_only_file(&directory, file_name)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let metadata = file.metadata()?;
        if !names_file(&directory, file_name, &metadata)? {
            continue;
        }
        if metadata.len() > 0 && metadata.len().saturating_add(record.len() as u64) > max_bytes {
            // The rename keeps the lock and the inode, so a writer blocked on
            // this file sees the name moved once it holds the lock.
            if unsafe {
                libc::renameat(
                    directory.as_raw_fd(),
                    file_name.as_ptr(),
                    directory.as_raw_fd(),
                    rotated.as_ptr(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            continue;
        }
        file.write_all(&record)?;
        return file.sync_data();
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "diagnostic log kept moving while this record waited",
    ))
}

/// The name a full log moves to, `<name>.1`.
fn rotated_name(file_name: &std::ffi::CStr) -> io::Result<std::ffi::CString> {
    let mut name = file_name.to_bytes().to_vec();
    name.extend_from_slice(b".1");
    std::ffi::CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

fn open_owner_only_file(directory: &fs::File, file_name: &std::ffi::CStr) -> io::Result<fs::File> {
    let mut attempts = 0;
    let descriptor = loop {
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                file_name.as_ptr(),
                libc::O_RDWR | libc::O_APPEND | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor >= 0 {
            break descriptor;
        }
        let error = io::Error::last_os_error();
        // macOS answers ENOENT from openat(O_CREAT | O_NOFOLLOW) while another
        // writer creates the same entry. The directory handle stays open, so a
        // second attempt finds the file or creates it.
        if error.kind() == io::ErrorKind::NotFound && attempts < 16 {
            attempts += 1;
            continue;
        }
        return Err(error);
    };
    let file = unsafe { fs::File::from_raw_fd(descriptor) };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "diagnostic log path is unsafe",
        ));
    }
    Ok(file)
}

/// Whether `file_name` in `directory` still names the open file `metadata`
/// describes. Another writer's rotation moves the name to a new file.
fn names_file(
    directory: &fs::File,
    file_name: &std::ffi::CStr,
    metadata: &fs::Metadata,
) -> io::Result<bool> {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            file_name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error)
        };
    }
    let status = unsafe { status.assume_init() };
    // The field types differ between platforms.
    #[allow(clippy::unnecessary_cast)]
    let same = status.st_dev as u64 == metadata.dev() && status.st_ino as u64 == metadata.ino();
    Ok(same)
}

fn validate_owner_only_directory(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "diagnostic log directory is unsafe",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_diagnostics::without_record_times;
    use std::path::PathBuf;

    struct Logs(PathBuf);

    impl Logs {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("muniment-rotation-{}", uuid::Uuid::new_v4())))
        }

        fn write(&self, name: &std::ffi::CStr, max_bytes: u64, record: &str) {
            append_owner_only_record(&self.0, name, max_bytes, record.as_bytes()).unwrap();
        }

        fn read(&self, name: &str) -> String {
            without_record_times(&fs::read_to_string(self.0.join(name)).unwrap())
        }
    }

    impl Drop for Logs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const STAMPED: u64 = crate::runtime_diagnostics::RECORD_TIME_BYTES as u64;

    #[test]
    fn a_full_log_moves_to_its_rotated_name_and_keeps_the_last_lines() {
        let logs = Logs::new();
        // Each record is 8 bytes and its time, so four fit under the cap.
        let cap = 4 * (8 + STAMPED);
        for index in 0..4 {
            logs.write(c"runtime.log", cap, &format!("line {index}\n"));
        }
        assert!(!logs.0.join("runtime.log.1").exists());
        logs.write(c"runtime.log", cap, "line 4\n");
        assert_eq!(
            logs.read("runtime.log.1"),
            "line 0\nline 1\nline 2\nline 3\n"
        );
        assert_eq!(logs.read("runtime.log"), "line 4\n");
        for index in 5..9 {
            logs.write(c"runtime.log", cap, &format!("line {index}\n"));
        }
        // The second rotation replaces the older file, so two files bound the log.
        assert_eq!(
            logs.read("runtime.log.1"),
            "line 4\nline 5\nline 6\nline 7\n"
        );
        assert_eq!(logs.read("runtime.log"), "line 8\n");
        for name in ["runtime.log", "runtime.log.1"] {
            let metadata = fs::metadata(logs.0.join(name)).unwrap();
            assert!(metadata.len() <= cap, "{name}");
            assert_eq!(metadata.mode() & 0o777, 0o600, "{name}");
        }
    }

    #[test]
    fn a_small_log_keeps_its_lines_while_another_log_rotates() {
        let logs = Logs::new();
        let cap = 4 * (8 + STAMPED);
        logs.write(c"runtime.log", cap, "small 0\n");
        logs.write(c"runtime.log", cap, "small 1\n");
        for index in 0..12 {
            logs.write(c"runtime-service.log", cap, &format!("large {index}\n"));
        }
        assert!(logs.0.join("runtime-service.log.1").exists());
        assert!(logs.read("runtime-service.log").ends_with("large 11\n"));
        assert_eq!(logs.read("runtime.log"), "small 0\nsmall 1\n");
        assert!(!logs.0.join("runtime.log.1").exists());
    }

    #[test]
    fn an_oversized_record_still_reaches_an_empty_log() {
        let logs = Logs::new();
        logs.write(c"runtime.log", 16, "a record longer than the cap\n");
        assert_eq!(logs.read("runtime.log"), "a record longer than the cap\n");
        assert!(!logs.0.join("runtime.log.1").exists());
        logs.write(c"runtime.log", 16, "next\n");
        assert_eq!(logs.read("runtime.log.1"), "a record longer than the cap\n");
        assert_eq!(logs.read("runtime.log"), "next\n");
    }

    #[test]
    fn concurrent_writers_rotate_once_per_fill_and_keep_whole_records() {
        let logs = Logs::new();
        let cap = 40 * (16 + STAMPED);
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            for writer in 0..4 {
                let logs = &logs;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for index in 0..16 {
                        logs.write(
                            c"runtime.log",
                            cap,
                            &format!("writer {writer} n {index:04}\n"),
                        );
                    }
                });
            }
        });
        let current = logs.read("runtime.log");
        let rotated = logs.read("runtime.log.1");
        // 64 records fill the cap once: the first 40 rotate and 24 stay, so
        // the two files hold every record once.
        assert_eq!(rotated.lines().count(), 40, "{rotated}");
        assert_eq!(current.lines().count(), 24, "{current}");
        let mut records: Vec<&str> = rotated.lines().chain(current.lines()).collect();
        records.sort_unstable();
        let mut expected: Vec<String> = (0..4)
            .flat_map(|writer| (0..16).map(move |index| format!("writer {writer} n {index:04}")))
            .collect();
        expected.sort_unstable();
        assert_eq!(records, expected);
    }
}
