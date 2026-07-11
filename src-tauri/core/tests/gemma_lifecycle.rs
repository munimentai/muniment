use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use muniment_core::llama::lifecycle::{
    GemmaActivation, GemmaActivationBoundary, GemmaActivationFailure, GemmaLifecycleBoundary,
    GemmaLifecycleError, GemmaNoticeDescriptor, GemmaPersistenceError, GemmaRecovery,
    GemmaRevisionDescriptor, GemmaRevisionLifecycle, GemmaRollbackFailure,
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
    notice: GemmaNoticeDescriptor {
        filename: "NOTICE.txt",
        contents: b"notice-v1",
    },
};
static NEW: GemmaRevisionDescriptor = GemmaRevisionDescriptor {
    identity: "gemma-fixture-v2",
    revision: "new",
    model: &NEW_MODEL,
    notice: GemmaNoticeDescriptor {
        filename: "NOTICE.txt",
        contents: b"notice-v2",
    },
};
static KNOWN: [&GemmaRevisionDescriptor; 2] = [&OLD, &NEW];

struct Boundary {
    locks: AtomicUsize,
    current_replacements: AtomicUsize,
    fail_current_at: AtomicUsize,
    revision_failure: AtomicUsize,
}

impl Boundary {
    fn working() -> Self {
        Self {
            locks: AtomicUsize::new(0),
            current_replacements: AtomicUsize::new(0),
            fail_current_at: AtomicUsize::new(0),
            revision_failure: AtomicUsize::new(0),
        }
    }
    fn failing_current() -> Self {
        Self {
            locks: AtomicUsize::new(0),
            current_replacements: AtomicUsize::new(0),
            fail_current_at: AtomicUsize::new(1),
            revision_failure: AtomicUsize::new(0),
        }
    }
    fn failing_revision_at(step: usize) -> Self {
        Self {
            locks: AtomicUsize::new(0),
            current_replacements: AtomicUsize::new(0),
            fail_current_at: AtomicUsize::new(0),
            revision_failure: AtomicUsize::new(step),
        }
    }

    fn failing_current_at(step: usize) -> Self {
        let boundary = Self::working();
        boundary.fail_current_at.store(step, Ordering::Relaxed);
        boundary
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
    fn replace_revision(
        &self,
        staged: &Path,
        destination: &Path,
    ) -> Result<(), GemmaPersistenceError> {
        if self.revision_failure.load(Ordering::Relaxed) == 1 {
            return Err(GemmaPersistenceError::Failed);
        }
        let quarantine = destination.with_extension("replaced");
        if quarantine.exists() {
            fs::remove_dir_all(&quarantine).map_err(|_| GemmaPersistenceError::Failed)?;
        }
        if destination.exists() || fs::symlink_metadata(destination).is_ok() {
            fs::rename(destination, &quarantine).map_err(|_| GemmaPersistenceError::Failed)?;
        }
        if self.revision_failure.load(Ordering::Relaxed) == 2 {
            return Err(GemmaPersistenceError::Failed);
        }
        fs::rename(staged, destination).map_err(|_| GemmaPersistenceError::Failed)
    }
    fn replace_pointer(
        &self,
        temporary: &Path,
        destination: &Path,
    ) -> Result<(), GemmaPersistenceError> {
        if destination.file_name().and_then(|name| name.to_str()) == Some("current") {
            let step = self.current_replacements.fetch_add(1, Ordering::Relaxed) + 1;
            if self.fail_current_at.load(Ordering::Relaxed) == step {
                return Err(GemmaPersistenceError::Failed);
            }
        }
        if destination.exists() {
            fs::remove_file(destination).unwrap();
        }
        fs::rename(temporary, destination).map_err(|_| GemmaPersistenceError::Failed)
    }
}

struct Activation {
    results: Vec<Result<(), GemmaActivationFailure>>,
    attempts: Vec<PathBuf>,
}

impl GemmaActivationBoundary for Activation {
    fn start_and_health(
        &mut self,
        revision: &Path,
        _: &'static GemmaRevisionDescriptor,
    ) -> Result<(), GemmaActivationFailure> {
        self.attempts.push(revision.to_path_buf());
        self.results.remove(0)
    }
}

fn activation(results: Vec<Result<(), GemmaActivationFailure>>) -> Activation {
    Activation {
        results,
        attempts: Vec::new(),
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
    let notice = if bytes == b"def" {
        NEW.notice.contents
    } else {
        OLD.notice.contents
    };
    fs::write(stage.join("NOTICE.txt"), notice).unwrap();
    stage
}

#[test]
fn notice_must_match_the_revision_descriptor_for_publication_and_resolution() {
    let root = root();
    let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();

    let missing = stage(&root, "missing-notice", b"abc");
    fs::remove_file(missing.join("NOTICE.txt")).unwrap();
    assert!(matches!(
        lifecycle.publish(&missing, &Boundary::working()),
        Err(GemmaLifecycleError::RevisionInvalid(_))
    ));

    let wrong_association = stage(&root, "wrong-notice", b"abc");
    fs::write(wrong_association.join("NOTICE.txt"), NEW.notice.contents).unwrap();
    assert!(matches!(
        lifecycle.publish(&wrong_association, &Boundary::working()),
        Err(GemmaLifecycleError::RevisionInvalid(_))
    ));

    lifecycle
        .publish(&stage(&root, "valid", b"abc"), &Boundary::working())
        .unwrap();
    fs::write(root.join("revisions/old/NOTICE.txt"), b"tampered").unwrap();
    assert!(matches!(
        lifecycle.resolve_current(),
        Err(GemmaLifecycleError::RevisionInvalid(_))
    ));
    assert_eq!(
        lifecycle.recover(&Boundary::working()).unwrap(),
        GemmaRecovery::RepairRequired
    );
    fs::remove_dir_all(root).unwrap();
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
fn activation_keeps_healthy_candidate_and_verified_previous() {
    let root = root();
    GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD)
        .unwrap()
        .publish(&stage(&root, "old", b"abc"), &Boundary::working())
        .unwrap();
    let update = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &NEW).unwrap();
    let mut startup = activation(vec![Ok(())]);

    assert_eq!(
        update
            .activate(
                &stage(&root, "new", b"def"),
                &Boundary::working(),
                &mut startup
            )
            .unwrap(),
        GemmaActivation::Activated(root.join("revisions/new"))
    );
    assert_eq!(
        update.resolve_current().unwrap(),
        root.join("revisions/new")
    );
    assert_eq!(
        fs::read_to_string(root.join("previous"))
            .unwrap()
            .lines()
            .last(),
        Some("old")
    );
    assert_eq!(startup.attempts, [root.join("revisions/new")]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_candidate_rolls_back_once_and_never_retries_candidate() {
    let root = root();
    GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD)
        .unwrap()
        .publish(&stage(&root, "old", b"abc"), &Boundary::working())
        .unwrap();
    let update = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &NEW).unwrap();
    let mut startup = activation(vec![Err(GemmaActivationFailure::HealthCheck), Ok(())]);

    assert_eq!(
        update
            .activate(
                &stage(&root, "new", b"def"),
                &Boundary::working(),
                &mut startup
            )
            .unwrap(),
        GemmaActivation::RolledBack {
            current: root.join("revisions/old"),
            rejected: GemmaActivationFailure::HealthCheck,
        }
    );
    assert_eq!(
        update.resolve_current().unwrap(),
        root.join("revisions/old")
    );
    assert_eq!(
        startup.attempts,
        [root.join("revisions/new"), root.join("revisions/old")]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_recovery_startup_requires_repair_after_exactly_two_attempts() {
    let root = root();
    GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD)
        .unwrap()
        .publish(&stage(&root, "old", b"abc"), &Boundary::working())
        .unwrap();
    let update = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &NEW).unwrap();
    let mut startup = activation(vec![
        Err(GemmaActivationFailure::Startup),
        Err(GemmaActivationFailure::HealthCheck),
    ]);

    assert_eq!(
        update
            .activate(
                &stage(&root, "new", b"def"),
                &Boundary::working(),
                &mut startup
            )
            .unwrap(),
        GemmaActivation::RepairRequired {
            rejected: GemmaActivationFailure::Startup,
            rollback: GemmaRollbackFailure::Activation(GemmaActivationFailure::HealthCheck),
        }
    );
    assert_eq!(startup.attempts.len(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_corrupt_or_unreplaceable_previous_requires_repair_without_fallback_startup() {
    for corrupt_previous in [false, true] {
        let root = root();
        GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD)
            .unwrap()
            .publish(&stage(&root, "old", b"abc"), &Boundary::working())
            .unwrap();
        let update = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &NEW).unwrap();
        if corrupt_previous {
            fs::write(root.join("revisions/old/model.gguf"), b"bad").unwrap();
        } else {
            fs::remove_file(root.join("current")).unwrap();
        }
        let retry_stage = stage(&root, "new", b"def");
        let mut startup = activation(vec![Err(GemmaActivationFailure::HealthCheck)]);
        assert!(matches!(
            update
                .activate(&retry_stage, &Boundary::working(), &mut startup)
                .unwrap(),
            GemmaActivation::RepairRequired {
                rollback: GemmaRollbackFailure::PreviousUnavailable,
                ..
            }
        ));
        assert_eq!(startup.attempts.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    let root = root();
    GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD)
        .unwrap()
        .publish(&stage(&root, "old", b"abc"), &Boundary::working())
        .unwrap();
    let update = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &NEW).unwrap();
    let mut startup = activation(vec![Err(GemmaActivationFailure::Startup)]);
    assert!(matches!(
        update
            .activate(
                &stage(&root, "new", b"def"),
                &Boundary::failing_current_at(2),
                &mut startup,
            )
            .unwrap(),
        GemmaActivation::RepairRequired {
            rollback: GemmaRollbackFailure::Persistence,
            ..
        }
    ));
    assert_eq!(startup.attempts.len(), 1);
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
fn verified_stage_repairs_corrupt_existing_revision() {
    let root = root();
    let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
    lifecycle
        .publish(&stage(&root, "first", b"abc"), &Boundary::working())
        .unwrap();
    fs::write(root.join("revisions/old/model.gguf"), b"bad").unwrap();

    let repaired = lifecycle
        .publish(&stage(&root, "repair", b"abc"), &Boundary::working())
        .unwrap();
    assert_eq!(repaired, root.join("revisions/old"));
    assert_eq!(fs::read(repaired.join("model.gguf")).unwrap(), b"abc");
    assert_eq!(lifecycle.resolve_current().unwrap(), repaired);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn interrupted_corrupt_revision_replacement_never_exposes_staged_bytes() {
    for failure_step in [1, 2] {
        let root = root();
        let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
        lifecycle
            .publish(&stage(&root, "first", b"abc"), &Boundary::working())
            .unwrap();
        fs::write(root.join("revisions/old/model.gguf"), b"bad").unwrap();
        let repair = stage(&root, "repair", b"abc");

        assert_eq!(
            lifecycle.publish(&repair, &Boundary::failing_revision_at(failure_step)),
            Err(GemmaLifecycleError::Persistence(
                GemmaPersistenceError::Failed
            ))
        );
        assert!(lifecycle.resolve_current().is_err());
        assert!(repair.exists());

        lifecycle.publish(&repair, &Boundary::working()).unwrap();
        assert_eq!(
            lifecycle.resolve_current().unwrap(),
            root.join("revisions/old")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn rejects_linked_stage_directory_and_model() {
    use std::os::unix::fs::symlink;

    let root = root();
    let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
    let outside = root.with_extension("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("model.gguf"), b"abc").unwrap();
    let linked_stage = root.join("staging/linked-stage");
    symlink(&outside, &linked_stage).unwrap();
    assert_eq!(
        lifecycle.publish(&linked_stage, &Boundary::working()),
        Err(GemmaLifecycleError::InvalidStage)
    );

    let model_stage = root.join("staging/linked-model");
    fs::create_dir(&model_stage).unwrap();
    symlink(outside.join("model.gguf"), model_stage.join("model.gguf")).unwrap();
    assert!(matches!(
        lifecycle.publish(&model_stage, &Boundary::working()),
        Err(GemmaLifecycleError::RevisionInvalid(_))
    ));
    assert!(!root.join("current").exists());
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn recovery_rejects_linked_revision_directory_and_model() {
    use std::os::unix::fs::symlink;

    for link_directory in [true, false] {
        let root = root();
        let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
        lifecycle
            .publish(&stage(&root, "first", b"abc"), &Boundary::working())
            .unwrap();
        let revision = root.join("revisions/old");
        let outside = root.with_extension("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("model.gguf"), b"abc").unwrap();
        if link_directory {
            fs::remove_dir_all(&revision).unwrap();
            symlink(&outside, &revision).unwrap();
        } else {
            fs::remove_file(revision.join("model.gguf")).unwrap();
            symlink(outside.join("model.gguf"), revision.join("model.gguf")).unwrap();
        }

        assert!(matches!(
            lifecycle.resolve_current(),
            Err(GemmaLifecycleError::RevisionInvalid(_))
        ));
        assert_eq!(
            lifecycle.recover(&Boundary::working()).unwrap(),
            GemmaRecovery::RepairRequired
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn linked_pointer_files_require_repair_even_when_the_target_looks_valid() {
    use std::os::unix::fs::symlink;

    for external_contents in [
        "muniment-gemma-pointer-v1\ngemma-fixture-v1\nold\n",
        "malformed",
    ] {
        let root = root();
        let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
        let external = root.with_extension("pointer");
        fs::write(&external, external_contents).unwrap();
        symlink(&external, root.join("current")).unwrap();

        assert_eq!(
            lifecycle.resolve_current(),
            Err(GemmaLifecycleError::InvalidPointer)
        );
        assert_eq!(
            lifecycle.recover(&Boundary::working()).unwrap(),
            GemmaRecovery::RepairRequired
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(external).unwrap();
    }

    let root = root();
    let lifecycle = GemmaRevisionLifecycle::new(root.clone(), &KNOWN, &OLD).unwrap();
    symlink(root.join("does-not-exist"), root.join("current")).unwrap();
    assert_eq!(
        lifecycle.resolve_current(),
        Err(GemmaLifecycleError::InvalidPointer)
    );
    assert_eq!(
        lifecycle.recover(&Boundary::working()).unwrap(),
        GemmaRecovery::RepairRequired
    );
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
