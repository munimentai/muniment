use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use muniment_core::llama::lifecycle::{
    GemmaLifecycleBoundary, GemmaLifecycleError, GemmaPersistenceError, GemmaRecovery,
    GemmaRevisionDescriptor, GemmaRevisionLifecycle,
};
use muniment_core::llama::ResidentModelDescriptor;

static OLD_MODEL: ResidentModelDescriptor = ResidentModelDescriptor {
    filename: "model.gguf",
    byte_size: 3,
    sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    alias: "fixture",
    context_tokens: 1,
};
static NEW_MODEL: ResidentModelDescriptor = ResidentModelDescriptor {
    filename: "model.gguf",
    byte_size: 3,
    sha256: "cb8379ac2098aa165029e3938a51da0bcecfc008fd6795f401178647f96c5b34",
    alias: "fixture",
    context_tokens: 1,
};
static OLD: GemmaRevisionDescriptor = GemmaRevisionDescriptor {
    identity: "gemma-fixture-v1",
    revision: "old",
    model: &OLD_MODEL,
};
static NEW: GemmaRevisionDescriptor = GemmaRevisionDescriptor {
    identity: "gemma-fixture-v2",
    revision: "new",
    model: &NEW_MODEL,
};
static KNOWN: [&GemmaRevisionDescriptor; 2] = [&OLD, &NEW];

struct Boundary {
    locks: AtomicUsize,
    fail_current: AtomicBool,
}

impl Boundary {
    fn working() -> Self {
        Self {
            locks: AtomicUsize::new(0),
            fail_current: AtomicBool::new(false),
        }
    }
    fn failing_current() -> Self {
        Self {
            locks: AtomicUsize::new(0),
            fail_current: AtomicBool::new(true),
        }
    }
}

impl GemmaLifecycleBoundary for Boundary {
    type LockGuard = ();
    fn lock_exclusive(&self, path: &Path) -> Result<(), GemmaPersistenceError> {
        assert_eq!(path.file_name().unwrap(), "install.lock");
        self.locks.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn sync_file(&self, _: &Path) -> Result<(), GemmaPersistenceError> {
        Ok(())
    }
    fn sync_directory(&self, _: &Path) -> Result<(), GemmaPersistenceError> {
        Ok(())
    }
    fn replace_pointer(
        &self,
        temporary: &Path,
        destination: &Path,
    ) -> Result<(), GemmaPersistenceError> {
        if destination.file_name().and_then(|name| name.to_str()) == Some("current")
            && self.fail_current.swap(false, Ordering::Relaxed)
        {
            return Err(GemmaPersistenceError::Failed);
        }
        if destination.exists() {
            fs::remove_file(destination).unwrap();
        }
        fs::rename(temporary, destination).map_err(|_| GemmaPersistenceError::Failed)
    }
}

fn root() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "muniment-gemma-lifecycle-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("staging")).unwrap();
    root
}

fn stage(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let stage = root.join("staging").join(name);
    fs::create_dir(&stage).unwrap();
    fs::write(stage.join("model.gguf"), bytes).unwrap();
    stage
}

#[test]
fn publishes_only_verified_stage_under_an_exclusive_lock() {
    let root = root();
    let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
    let bad = stage(&root, "bad", b"abd");
    let boundary = Boundary::working();
    assert!(matches!(
        lifecycle.publish(&bad, &boundary),
        Err(GemmaLifecycleError::RevisionInvalid(_))
    ));
    assert!(!root.join("current").exists());

    let good = stage(&root, "good", b"abc");
    let revision = lifecycle.publish(&good, &boundary).unwrap();
    assert_eq!(revision, root.join("revisions/old"));
    assert_eq!(lifecycle.resolve_current().unwrap(), revision);
    assert_eq!(boundary.locks.load(Ordering::Relaxed), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn interrupted_pointer_replace_keeps_verified_current_and_previous() {
    let root = root();
    let old = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
    old.publish(&stage(&root, "old", b"abc"), &Boundary::working())
        .unwrap();
    let old_pointer = fs::read(root.join("current")).unwrap();

    let update = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &NEW).unwrap();
    assert_eq!(
        update.publish(&stage(&root, "new", b"def"), &Boundary::failing_current()),
        Err(GemmaLifecycleError::Persistence(
            GemmaPersistenceError::Failed
        ))
    );
    assert_eq!(fs::read(root.join("current")).unwrap(), old_pointer);
    assert_eq!(fs::read(root.join("previous")).unwrap(), old_pointer);
    assert_eq!(
        update.resolve_current().unwrap(),
        root.join("revisions/old")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_current_restores_verified_previous_and_never_promotes_staging() {
    let root = root();
    let old = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
    old.publish(&stage(&root, "old", b"abc"), &Boundary::working())
        .unwrap();
    let update = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &NEW).unwrap();
    update
        .publish(&stage(&root, "new", b"def"), &Boundary::working())
        .unwrap();
    fs::write(root.join("revisions/new/model.gguf"), b"bad").unwrap();
    stage(&root, "tempting", b"def");
    fs::write(root.join(".current.tmp"), b"interrupted pointer").unwrap();

    let boundary = Boundary::working();
    assert_eq!(
        update.recover(&boundary).unwrap(),
        GemmaRecovery::RestoredPrevious(root.join("revisions/old"))
    );
    assert_eq!(
        update.resolve_current().unwrap(),
        root.join("revisions/old")
    );
    assert_eq!(boundary.locks.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_or_unsafe_pointers_require_repair_without_leaking_paths() {
    let root = root();
    let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
    for pointer in [
        "muniment-gemma-pointer-v1\nunknown\nold\n",
        "muniment-gemma-pointer-v1\ngemma-fixture-v1\n../old\n",
        "malformed",
    ] {
        fs::write(root.join("current"), pointer).unwrap();
        let error = lifecycle.resolve_current().unwrap_err();
        assert!(!format!("{error:?} {error}").contains(root.to_str().unwrap()));
        assert_eq!(
            lifecycle.recover(&Boundary::working()).unwrap(),
            GemmaRecovery::RepairRequired
        );
    }
    fs::remove_file(root.join("current")).unwrap();
    assert_eq!(
        lifecycle.recover(&Boundary::working()).unwrap(),
        GemmaRecovery::NotInstalled
    );
    fs::remove_dir_all(root).unwrap();
}
