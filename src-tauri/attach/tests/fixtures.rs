use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::{atomic::AtomicBool, Arc};

use muniment_attach::fixtures::{export, Mode, FIXTURE_DIRECTORY};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "muniment-attach-fixtures-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_dir(root: &Path) -> PathBuf {
    root.join(FIXTURE_DIRECTORY)
}

#[test]
fn export_is_deterministic_and_replaces_obsolete_files() {
    let root = TestDirectory::new();
    export(&root.0, Mode::Write).unwrap();
    let before = read_fixtures(&fixture_dir(&root.0));
    fs::write(fixture_dir(&root.0).join("obsolete.json"), b"obsolete\n").unwrap();

    export(&root.0, Mode::Write).unwrap();

    assert_eq!(read_fixtures(&fixture_dir(&root.0)), before);
    assert!(before.iter().all(|(_, bytes)| {
        bytes.ends_with(b"\n") && !bytes[..bytes.len() - 1].ends_with(b"\n")
    }));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn replacement_never_exposes_a_missing_or_partial_live_directory() {
    let root = TestDirectory::new();
    export(&root.0, Mode::Write).unwrap();
    let expected_names: Vec<_> = read_fixtures(&fixture_dir(&root.0))
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let running = Arc::new(AtomicBool::new(true));
    let reader_root = root.0.clone();
    let reader_running = Arc::clone(&running);
    let reader = std::thread::spawn(move || {
        while reader_running.load(Ordering::Acquire) {
            let live = fixture_dir(&reader_root);
            assert!(
                fs::metadata(&live).unwrap().is_dir(),
                "the live fixture directory must always exist"
            );
            for name in &expected_names {
                assert!(
                    live.join(name).is_file(),
                    "every published generation must be complete"
                );
            }
        }
    });

    for _ in 0..500 {
        export(&root.0, Mode::Write).unwrap();
    }
    running.store(false, Ordering::Release);
    reader.join().unwrap();
}

#[test]
fn check_rejects_a_missing_fixture_without_writing() {
    let root = TestDirectory::new();
    export(&root.0, Mode::Write).unwrap();
    let path = fixture_dir(&root.0).join("request-run-start.json");
    fs::remove_file(&path).unwrap();

    assert!(export(&root.0, Mode::Check).is_err());
    assert!(!path.exists());
}

#[test]
fn check_rejects_an_extra_fixture_without_writing() {
    let root = TestDirectory::new();
    export(&root.0, Mode::Write).unwrap();
    let path = fixture_dir(&root.0).join("extra.json");
    fs::write(&path, b"extra\n").unwrap();

    assert!(export(&root.0, Mode::Check).is_err());
    assert_eq!(fs::read(path).unwrap(), b"extra\n");
}

#[test]
fn check_rejects_a_byte_stale_fixture_without_writing() {
    let root = TestDirectory::new();
    export(&root.0, Mode::Write).unwrap();
    let path = fixture_dir(&root.0).join("response-run-start.json");
    fs::write(&path, b"{}\n").unwrap();

    assert!(export(&root.0, Mode::Check).is_err());
    assert_eq!(fs::read(path).unwrap(), b"{}\n");
}

fn read_fixtures(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut fixtures: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    fixtures.sort_by(|left, right| left.0.cmp(&right.0));
    fixtures
}
