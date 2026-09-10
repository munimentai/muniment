use super::activation::{activate_runtime, spawn_runtime};
use muniment_core::attach::linux::AttachFilesystem;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

pub(super) struct Directory(std::path::PathBuf);
impl Directory {
    pub(super) fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "muniment-desktop-activation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    pub(super) fn filesystem(&self) -> AttachFilesystem {
        AttachFilesystem::from_runtime_directory(&self.0).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_reopened_owner_stops_its_surviving_runtime_and_can_restart() {
    use crate::runtime_owner::RuntimeOwner;
    use std::path::Path;

    let directory = Directory::new();
    let filesystem = directory.filesystem();
    let owner = RuntimeOwner::default();
    // A long-lived process stands in for the runtime without the attach protocol.
    let child = super::process::spawn_runtime(&filesystem, Path::new("/usr/bin/yes")).unwrap();
    let pid = child.id() as i32;
    owner.keep_child(child);
    drop(owner);

    let owner = RuntimeOwner::default();
    activate_runtime(
        &filesystem,
        || panic!("The reopened desktop must attach without another start."),
        || true,
        Duration::from_secs(1),
    )
    .unwrap();
    super::stop_owned_runtime(&owner, &filesystem, || {
        panic!("A standalone runtime does not use systemd.")
    })
    .unwrap();
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    assert!(libc::WIFSIGNALED(status));
    assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);

    let connected = AtomicBool::new(false);
    activate_runtime(
        &filesystem,
        || {
            owner.keep_child(super::process::spawn_runtime(
                &filesystem,
                Path::new("/usr/bin/yes"),
            )?);
            connected.store(true, Ordering::Release);
            Ok(())
        },
        || connected.load(Ordering::Acquire),
        Duration::from_secs(1),
    )
    .unwrap();
    super::stop_owned_runtime(&owner, &filesystem, || {
        panic!("The owner must stop its retained child.")
    })
    .unwrap();
}

#[test]
fn a_service_stop_and_a_competing_start_keep_their_ownership() {
    let directory = Directory::new();
    let filesystem = directory.filesystem();
    let owner = crate::runtime_owner::RuntimeOwner::default();
    assert!(super::stop_owned_runtime(&owner, &filesystem, || Err(())).is_err());
    super::stop_owned_runtime(&owner, &filesystem, || Ok(())).unwrap();
    let competing_filesystem = directory.filesystem();
    let _startup_lock = competing_filesystem.acquire_startup_lock().unwrap();
    assert!(super::stop_owned_runtime(&owner, &filesystem, || {
        panic!("A stop must not race another desktop start.")
    })
    .is_err());
}

#[test]
fn a_pending_runtime_connects_without_another_start() {
    let directory = Directory::new();
    let filesystem = directory.filesystem();
    let _runtime_lock = filesystem.acquire_instance_lock().unwrap();
    let probes = AtomicUsize::new(0);
    activate_runtime(
        &filesystem,
        || panic!("The runtime already holds its lock."),
        || probes.fetch_add(1, Ordering::Relaxed) > 2,
        Duration::from_secs(1),
    )
    .unwrap();
}

#[test]
fn concurrent_desktops_start_one_runtime() {
    let directory = Directory::new();
    let runtime_filesystem = directory.filesystem();
    let runtime_lock = Mutex::new(None);
    let starts = AtomicUsize::new(0);
    let connected = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let directory = &directory;
            let runtime_filesystem = &runtime_filesystem;
            let runtime_lock = &runtime_lock;
            let starts = &starts;
            let connected = &connected;
            scope.spawn(move || {
                activate_runtime(
                    &directory.filesystem(),
                    || {
                        starts.fetch_add(1, Ordering::Relaxed);
                        std::thread::sleep(Duration::from_millis(50));
                        *runtime_lock.lock().unwrap() =
                            Some(runtime_filesystem.acquire_instance_lock().unwrap());
                        connected.store(true, Ordering::Release);
                        Ok(())
                    },
                    || connected.load(Ordering::Acquire),
                    Duration::from_secs(1),
                )
                .unwrap();
            });
        }
    });
    assert_eq!(starts.load(Ordering::Relaxed), 1);
}

#[test]
fn a_failed_start_releases_the_startup_lock_for_a_retry() {
    let directory = Directory::new();
    assert!(activate_runtime(
        &directory.filesystem(),
        || spawn_runtime(&directory.0.join("missing-runtime")).map(|_| ()),
        || false,
        Duration::from_secs(1),
    )
    .is_err());
    let started = AtomicBool::new(false);
    activate_runtime(
        &directory.filesystem(),
        || {
            started.store(true, Ordering::Release);
            Ok(())
        },
        || started.load(Ordering::Acquire),
        Duration::from_secs(1),
    )
    .unwrap();
}

#[test]
fn a_disconnected_runtime_times_out_without_a_desktop_listener() {
    let directory = Directory::new();
    let filesystem = directory.filesystem();
    let _runtime_lock = filesystem.acquire_instance_lock().unwrap();
    assert!(activate_runtime(
        &filesystem,
        || panic!("A failed handshake must not start another runtime."),
        || false,
        Duration::from_millis(50),
    )
    .is_err());
    assert!(!filesystem.endpoint_path().exists());
    assert!(activate_runtime(
        &filesystem,
        || panic!("A zero timeout must not start a runtime."),
        || false,
        Duration::ZERO,
    )
    .is_err());
}
