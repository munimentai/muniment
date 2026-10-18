//! Keeps a spawned program to the descriptors it is handed. A host process
//! can hold descriptors another library opened without close-on-exec, such as
//! an embedded browser's database locks. A child would inherit them and keep
//! them open after the host closed its own copies.

use std::process::Command;

/// The highest descriptor a child's sweep visits when the open-files limit
/// names no bound or a very large one.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const SWEEP_LIMIT: libc::c_int = 1 << 16;

/// Makes `command` start its program with standard input, output and error
/// only: every other descriptor closes when the program starts.
pub fn stdio_only(command: &mut Command) -> &mut Command {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::process::CommandExt;
        let sweep = sweep_end();
        // SAFETY: the closure runs between fork and exec and calls only
        // `fcntl` and, on Linux, `close_range`, which are async-signal-safe.
        unsafe {
            command.pre_exec(move || {
                mark_close_on_exec(sweep);
                Ok(())
            });
        }
    }
    command
}

/// Marks every descriptor this process holds above standard error
/// close-on-exec, so no program it starts later inherits one. A process run by
/// a host that leaked descriptors calls it before it starts any program.
pub fn seal_inherited() {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mark_close_on_exec(sweep_end());
}

/// One past the highest descriptor number the process may hold, read before
/// a fork so the child does no work beyond system calls.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sweep_end() -> libc::c_int {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a valid out pointer for the call.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0
        || limit.rlim_cur == libc::RLIM_INFINITY
    {
        return SWEEP_LIMIT;
    }
    libc::c_int::try_from(limit.rlim_cur)
        .unwrap_or(SWEEP_LIMIT)
        .min(SWEEP_LIMIT)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn mark_close_on_exec(end: libc::c_int) {
    #[cfg(target_os = "linux")]
    {
        const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;
        // SAFETY: close_range only changes descriptor flags in this range.
        if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, CLOSE_RANGE_CLOEXEC) } == 0
        {
            return;
        }
    }
    for descriptor in 3..end {
        // SAFETY: F_GETFD and F_SETFD only read and set descriptor flags. An
        // unused number answers EBADF, which the sweep skips.
        unsafe {
            let flags = libc::fcntl(descriptor, libc::F_GETFD);
            if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC);
            }
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::sync::Mutex;

    /// Sealing changes every descriptor of the test process, so the tests
    /// that spawn children run one at a time.
    static SPAWNS: Mutex<()> = Mutex::new(());

    /// A descriptor that a child would inherit: a duplicate without
    /// close-on-exec.
    fn leaked() -> OwnedFd {
        let file = std::fs::File::open("/dev/null").unwrap();
        // SAFETY: dup returns a new descriptor this test owns.
        let raw = unsafe { libc::dup(file.as_raw_fd()) };
        assert!(raw > 2);
        // SAFETY: `raw` is open and owned by nothing else.
        unsafe { OwnedFd::from_raw_fd(raw) }
    }

    fn child_sees(command: &mut Command, descriptor: i32) -> bool {
        command
            .arg("-c")
            .arg(format!("[ -e /dev/fd/{descriptor} ]"))
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn a_plain_child_inherits_a_leaked_descriptor_and_a_stdio_only_child_does_not() {
        let _held = SPAWNS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let descriptor = leaked();
        let number = descriptor.as_raw_fd();
        assert!(child_sees(&mut Command::new("/bin/sh"), number));
        assert!(!child_sees(
            stdio_only(&mut Command::new("/bin/sh")),
            number
        ));
        // The host keeps its own copy open.
        // SAFETY: F_GETFD only reads the flags of a descriptor this test owns.
        assert!(unsafe { libc::fcntl(number, libc::F_GETFD) } >= 0);
    }

    #[test]
    fn a_sealed_process_starts_children_without_leaked_descriptors() {
        let _held = SPAWNS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let descriptor = leaked();
        seal_inherited();
        assert!(!child_sees(
            &mut Command::new("/bin/sh"),
            descriptor.as_raw_fd()
        ));
    }
}
