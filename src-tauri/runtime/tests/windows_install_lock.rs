#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use muniment_runtime::install_lock::{acquire, InstallLockAcquireError};

fn absent_directory() -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "muniment-runtime-windows-install-lock-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    path
}

#[test]
fn acquire_creates_the_state_directory_and_lock_file() {
    let state_directory = absent_directory();

    let guard = acquire(&state_directory, Duration::ZERO).unwrap();

    assert!(state_directory.is_dir());
    assert!(state_directory.join("install.lock").is_file());
    drop(guard);
    fs::remove_dir_all(state_directory).unwrap();
}

#[test]
fn contention_times_out_and_release_allows_a_later_acquire() {
    let state_directory = absent_directory();
    let guard = acquire(&state_directory, Duration::ZERO).unwrap();
    let wait = Duration::from_millis(100);
    let started = Instant::now();

    assert!(matches!(
        acquire(&state_directory, wait),
        Err(InstallLockAcquireError::TimedOut)
    ));
    assert!(started.elapsed() >= wait);

    drop(guard);
    acquire(&state_directory, Duration::ZERO).unwrap();
    fs::remove_dir_all(state_directory).unwrap();
}

#[test]
fn release_after_the_bounded_wait_still_times_out() {
    let state_directory = absent_directory();
    let guard = acquire(&state_directory, Duration::ZERO).unwrap();
    let wait = Duration::from_millis(100);
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(wait + Duration::from_millis(25));
        drop(guard);
    });

    assert!(matches!(
        acquire(&state_directory, wait),
        Err(InstallLockAcquireError::TimedOut)
    ));

    releaser.join().unwrap();
    acquire(&state_directory, Duration::ZERO).unwrap();
    fs::remove_dir_all(state_directory).unwrap();
}

#[test]
fn a_waiter_acquires_after_the_contended_guard_releases() {
    let state_directory = absent_directory();
    let guard = acquire(&state_directory, Duration::ZERO).unwrap();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(75));
        drop(guard);
    });

    acquire(&state_directory, Duration::from_secs(1)).unwrap();

    releaser.join().unwrap();
    fs::remove_dir_all(state_directory).unwrap();
}

#[test]
fn reports_unavailable_when_the_state_directory_is_a_file() {
    let state_directory = absent_directory();
    fs::write(&state_directory, []).unwrap();

    assert!(matches!(
        acquire(&state_directory, Duration::ZERO),
        Err(InstallLockAcquireError::Unavailable)
    ));

    fs::remove_file(state_directory).unwrap();
}
