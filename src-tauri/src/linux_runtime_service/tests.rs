use super::activation::{activate_runtime, spawn_runtime};
use muniment_core::attach::linux::AttachFilesystem;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
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
    fn filesystem(&self) -> AttachFilesystem {
        AttachFilesystem::from_runtime_directory(&self.0).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
