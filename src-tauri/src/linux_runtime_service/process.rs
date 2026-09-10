use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Child;

use muniment_core::attach::linux::AttachFilesystem;

#[derive(serde::Serialize, serde::Deserialize)]
struct Identity {
    pid: i32,
    boot: String,
    started: u64,
    device: u64,
    inode: u64,
}

fn record_path(filesystem: &AttachFilesystem) -> PathBuf {
    // The validated directory descriptor pins the private directory across path replacements.
    PathBuf::from(format!(
        "/proc/self/fd/{}/desktop-runtime.json",
        filesystem.attach_directory().as_raw_fd()
    ))
}

fn invalid_identity() -> io::Error {
    io::Error::other("The runtime process identity is invalid.")
}

impl Identity {
    fn read(pid: i32) -> io::Result<Self> {
        if pid <= 1 {
            return Err(invalid_identity());
        }
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let started = stat
            .rsplit_once(')')
            .and_then(|(_, fields)| fields.split_whitespace().nth(19))
            .and_then(|field| field.parse().ok())
            .ok_or_else(invalid_identity)?;
        let executable = std::fs::metadata(format!("/proc/{pid}/exe"))?;
        Ok(Self {
            pid,
            boot: std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?,
            started,
            device: executable.dev(),
            inode: executable.ino(),
        })
    }

    fn matches(&self, other: &Self) -> bool {
        self.pid == other.pid
            && self.boot == other.boot
            && self.started == other.started
            && self.device == other.device
            && self.inode == other.inode
    }
}

pub(super) fn spawn_runtime(
    filesystem: &AttachFilesystem,
    executable: &Path,
) -> Result<Child, String> {
    let mut child = super::activation::spawn_runtime(executable)?;
    let result = (|| -> io::Result<()> {
        let identity = Identity::read(child.id().try_into().map_err(|_| invalid_identity())?)?;
        let path = record_path(filesystem);
        let temporary =
            path.with_file_name(format!("desktop-runtime-{}.tmp", uuid::Uuid::now_v7()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(&serde_json::to_vec(&identity)?)?;
            std::fs::rename(&temporary, path)
        })();
        let _ = std::fs::remove_file(temporary);
        result
    })();
    if let Err(error) = result {
        // A start without a saved identity would leave the next desktop unable to stop it.
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "Muniment could not save its runtime process identity: {error}"
        ));
    }
    Ok(child)
}

// Return false when the record has no live match, so the owner can stop a systemd service.
pub(super) fn stop_runtime(filesystem: &AttachFilesystem) -> io::Result<bool> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(record_path(filesystem))
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    validate_record(&file)?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(invalid_identity());
    }
    let identity: Identity = serde_json::from_slice(&bytes)?;
    if identity.pid <= 1 || identity.started == 0 {
        return Err(invalid_identity());
    }
    // Pin the process before verification. A recycled PID cannot redirect the signal.
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, identity.pid, 0) };
    if descriptor < 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(false)
        } else {
            Err(error)
        };
    }
    let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor as i32) };
    let current = match Identity::read(identity.pid) {
        Ok(current) => current,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !identity.matches(&current) {
        return Ok(false);
    }
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            descriptor.as_raw_fd(),
            libc::SIGKILL,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if result < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
        return Err(io::Error::last_os_error());
    }
    let mut poll = libc::pollfd {
        fd: descriptor.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    if unsafe { libc::poll(&mut poll, 1, 5000) } <= 0 || poll.revents & libc::POLLIN == 0 {
        return Err(io::Error::other("The runtime stop timed out."));
    }
    Ok(true)
}

fn validate_record(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(invalid_identity());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux_runtime_service::tests::Directory;

    #[test]
    fn stale_and_invalid_identities_never_signal_a_live_process() {
        let directory = Directory::new();
        let filesystem = directory.filesystem();
        let mut child = spawn_runtime(&filesystem, Path::new("/usr/bin/yes")).unwrap();
        let path = record_path(&filesystem);
        let original = std::fs::read(&path).unwrap();
        for field in ["boot", "started", "device", "inode"] {
            let mut identity: Identity = serde_json::from_slice(&original).unwrap();
            match field {
                "boot" => identity.boot = "another boot".into(),
                "started" => identity.started += 1,
                "device" => identity.device += 1,
                _ => identity.inode += 1,
            }
            std::fs::write(&path, serde_json::to_vec(&identity).unwrap()).unwrap();
            assert!(!stop_runtime(&filesystem).unwrap());
            assert!(child.try_wait().unwrap().is_none());
        }
        for bytes in [b"".as_slice(), b"{}", b"{\"pid\":-1}", &[b'x'; 4097]] {
            std::fs::write(&path, bytes).unwrap();
            assert!(stop_runtime(&filesystem).is_err());
            assert!(child.try_wait().unwrap().is_none());
        }
        for pid in [-1, 0, 1] {
            let mut identity: Identity = serde_json::from_slice(&original).unwrap();
            identity.pid = pid;
            std::fs::write(&path, serde_json::to_vec(&identity).unwrap()).unwrap();
            assert!(stop_runtime(&filesystem).is_err());
        }
        std::fs::write(&path, original).unwrap();
        assert!(stop_runtime(&filesystem).unwrap());
        child.wait().unwrap();
        assert!(!stop_runtime(&filesystem).unwrap());
    }

    #[test]
    fn a_missing_record_allows_the_service_stop_but_a_symlink_does_not() {
        let directory = Directory::new();
        let filesystem = directory.filesystem();
        assert!(!stop_runtime(&filesystem).unwrap());
        std::os::unix::fs::symlink("/dev/null", record_path(&filesystem)).unwrap();
        assert!(stop_runtime(&filesystem).is_err());
        assert!(spawn_runtime(&filesystem, Path::new("/missing-runtime")).is_err());
    }
}
