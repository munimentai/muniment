use muniment_core::asr::lifecycle::{
    AsrLifecycle, AtomicReplace, ExclusiveLock, LifecycleError, LifecycleState,
};
use muniment_core::asr::{AsrArtifactDescriptor, AsrArtifactManifest};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

const REVISION: &str = "1111111111111111111111111111111111111111";
const REVISION_B: &str = "2222222222222222222222222222222222222222";
const FILES: [AsrArtifactDescriptor; 4] = [
    AsrArtifactDescriptor {
        filename: "one",
        byte_size: 1,
        sha256: "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb",
    },
    AsrArtifactDescriptor {
        filename: "two",
        byte_size: 1,
        sha256: "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d",
    },
    AsrArtifactDescriptor {
        filename: "three",
        byte_size: 1,
        sha256: "2e7d2c03a9507ae265ecf5b5356885a53393a2029d241394997265a1a25aefc6",
    },
    AsrArtifactDescriptor {
        filename: "four",
        byte_size: 1,
        sha256: "18ac3e7343f016890c510e93f935261169d9e3f565436429830faf0934f4f8e4",
    },
];
const MANIFEST: AsrArtifactManifest = AsrArtifactManifest {
    revision: REVISION,
    artifacts: &FILES,
};
const MANIFEST_B: AsrArtifactManifest = AsrArtifactManifest {
    revision: REVISION_B,
    artifacts: &FILES,
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "muniment-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn stage(&self, name: &str) {
        let path = self.0.join("staging").join(name);
        std::fs::create_dir_all(&path).unwrap();
        for (name, value) in [
            ("one", b'a'),
            ("two", b'b'),
            ("three", b'c'),
            ("four", b'd'),
        ] {
            std::fs::write(path.join(name), [value]).unwrap();
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Default)]
struct TestLock(Arc<AtomicBool>);
struct Guard(Arc<AtomicBool>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl ExclusiveLock for TestLock {
    type Guard = Guard;
    fn try_acquire(&self) -> Result<Guard, LifecycleError> {
        self.0
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| Guard(self.0.clone()))
            .map_err(|_| LifecycleError::MutationInProgress)
    }
}

#[derive(Clone, Copy, Default)]
struct Replace {
    fail_replace: bool,
    fail_remove: bool,
}
impl AtomicReplace for Replace {
    fn replace(&self, source: &Path, destination: &Path) -> Result<(), LifecycleError> {
        if self.fail_replace {
            return Err(LifecycleError::Storage { retryable: true });
        }
        std::fs::rename(source, destination)
            .map_err(|_| LifecycleError::Storage { retryable: true })
    }

    fn remove(&self, path: &Path) -> Result<(), LifecycleError> {
        if self.fail_remove {
            return Err(LifecycleError::Storage { retryable: true });
        }
        std::fs::remove_file(path).map_err(|_| LifecycleError::Storage { retryable: true })
    }
}

#[test]
fn first_publish_and_remove() {
    let root = Directory::new();
    root.stage("install");
    let lifecycle = AsrLifecycle::new(&root.0, &MANIFEST, TestLock::default(), Replace::default());
    assert_eq!(lifecycle.resolve(), LifecycleState::NotInstalled);
    assert_eq!(lifecycle.publish("install"), Ok(LifecycleState::Ready));
    assert_eq!(lifecycle.resolve(), LifecycleState::Ready);
    assert_eq!(lifecycle.remove(), Ok(LifecycleState::NotInstalled));
}

#[test]
fn first_publish_interruption_recovers_published_revision() {
    let root = Directory::new();
    root.stage("install");
    let interrupted = AsrLifecycle::new(
        &root.0,
        &MANIFEST,
        TestLock::default(),
        Replace {
            fail_replace: true,
            fail_remove: false,
        },
    );
    assert_eq!(
        interrupted.publish("install"),
        Err(LifecycleError::Storage { retryable: true })
    );
    assert!(!root.0.join("staging/install").exists());
    assert!(root.0.join("revisions").join(REVISION).exists());

    let recovery = AsrLifecycle::new(&root.0, &MANIFEST, TestLock::default(), Replace::default());
    assert_eq!(recovery.recover(), Ok(LifecycleState::Ready));
    assert_eq!(recovery.resolve(), LifecycleState::Ready);
}

#[test]
fn update_preserves_previous_and_interruption_preserves_current() {
    let root = Directory::new();
    root.stage("first");
    AsrLifecycle::new(&root.0, &MANIFEST, TestLock::default(), Replace::default())
        .publish("first")
        .unwrap();
    root.stage("second");
    let known_for_interruption = [&MANIFEST];
    let interrupted = AsrLifecycle::new(
        &root.0,
        &MANIFEST_B,
        TestLock::default(),
        Replace {
            fail_replace: true,
            fail_remove: false,
        },
    )
    .with_known_manifests(&known_for_interruption);
    assert!(matches!(
        interrupted.publish("second"),
        Err(LifecycleError::Storage { retryable: true })
    ));
    assert_eq!(interrupted.resolve(), LifecycleState::Ready);
    let known = [&MANIFEST];
    let lifecycle = AsrLifecycle::new(
        &root.0,
        &MANIFEST_B,
        TestLock::default(),
        Replace::default(),
    )
    .with_known_manifests(&known);
    // Retrying the same update succeeds after its stage was consumed by the
    // interrupted publication.
    lifecycle.publish("second").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.0.join("previous"))
            .unwrap()
            .trim(),
        REVISION
    );
    assert_eq!(
        std::fs::read_to_string(root.0.join("current"))
            .unwrap()
            .trim(),
        REVISION_B
    );
    std::fs::write(
        root.0.join("current"),
        "3333333333333333333333333333333333333333\n",
    )
    .unwrap();
    assert_eq!(lifecycle.recover(), Ok(LifecycleState::Ready));
    assert_eq!(
        std::fs::read_to_string(root.0.join("current"))
            .unwrap()
            .trim(),
        REVISION
    );
    std::fs::write(root.0.join("current"), format!("{REVISION_B}\n")).unwrap();
    assert_eq!(lifecycle.rollback(), Ok(LifecycleState::Ready));
    assert_eq!(
        std::fs::read_to_string(root.0.join("current"))
            .unwrap()
            .trim(),
        REVISION
    );
}

#[test]
fn invalid_stage_does_not_touch_pointer_and_recovery_rolls_back() {
    let root = Directory::new();
    root.stage("good");
    let lifecycle = AsrLifecycle::new(&root.0, &MANIFEST, TestLock::default(), Replace::default());
    lifecycle.publish("good").unwrap();
    let current = std::fs::read(root.0.join("current")).unwrap();
    root.stage("bad");
    std::fs::write(root.0.join("staging/bad/four"), b"x").unwrap();
    assert_eq!(
        lifecycle.publish("bad"),
        Err(LifecycleError::VerificationFailed)
    );
    assert_eq!(std::fs::read(root.0.join("current")).unwrap(), current);
    std::fs::write(root.0.join("previous"), &current).unwrap();
    std::fs::write(root.0.join("current"), "unknown\n").unwrap();
    assert_eq!(lifecycle.recover(), Ok(LifecycleState::Ready));
    std::fs::write(root.0.join("current"), "unknown\n").unwrap();
    assert_eq!(lifecycle.rollback(), Ok(LifecycleState::Ready));
}

#[test]
fn concurrent_mutation_is_serialized() {
    let root = Directory::new();
    root.stage("one");
    let lock = TestLock::default();
    let held = lock.try_acquire().unwrap();
    let lifecycle = AsrLifecycle::new(&root.0, &MANIFEST, lock.clone(), Replace::default());
    assert_eq!(
        lifecycle.publish("one"),
        Err(LifecycleError::MutationInProgress)
    );
    drop(held);
    assert_eq!(lifecycle.publish("one"), Ok(LifecycleState::Ready));
}

#[cfg(unix)]
#[test]
fn rejects_traversal_and_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;
    let root = Directory::new();
    let outside = Directory::new();
    let lifecycle = AsrLifecycle::new(&root.0, &MANIFEST, TestLock::default(), Replace::default());
    assert_eq!(
        lifecycle.publish("../outside"),
        Err(LifecycleError::InvalidStage)
    );
    std::fs::create_dir_all(root.0.join("staging")).unwrap();
    symlink(&outside.0, root.0.join("staging/link")).unwrap();
    assert_eq!(lifecycle.publish("link"), Err(LifecycleError::UnsafeEntry));
    std::fs::create_dir_all(root.0.join("revisions")).unwrap();
    symlink(&outside.0, root.0.join("revisions/escape")).unwrap();
    assert_eq!(
        lifecycle.remove(),
        Err(LifecycleError::Storage { retryable: true })
    );
    assert!(outside.0.exists());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_owned_ancestors_without_touching_outside() {
    use std::os::unix::fs::symlink;
    for ancestor in ["staging", "revisions"] {
        let root = Directory::new();
        let outside = Directory::new();
        std::fs::write(outside.0.join("sentinel"), b"unchanged").unwrap();
        symlink(&outside.0, root.0.join(ancestor)).unwrap();
        let lifecycle =
            AsrLifecycle::new(&root.0, &MANIFEST, TestLock::default(), Replace::default());
        assert_eq!(
            lifecycle.publish("install"),
            Err(LifecycleError::UnsafeEntry)
        );
        assert_eq!(
            std::fs::read(outside.0.join("sentinel")).unwrap(),
            b"unchanged"
        );
        assert!(!outside.0.join("install").exists());
        assert!(!outside.0.join(REVISION).exists());
    }

    let parent = Directory::new();
    let outside = Directory::new();
    let unsafe_root = parent.0.join("asr");
    symlink(&outside.0, &unsafe_root).unwrap();
    let lifecycle = AsrLifecycle::new(
        &unsafe_root,
        &MANIFEST,
        TestLock::default(),
        Replace::default(),
    );
    assert_eq!(
        lifecycle.publish("install"),
        Err(LifecycleError::UnsafeEntry)
    );
    assert!(std::fs::read_dir(&outside.0).unwrap().next().is_none());
}

#[test]
fn removal_failure_is_retryable_and_keeps_current_usable() {
    let root = Directory::new();
    root.stage("install");
    AsrLifecycle::new(&root.0, &MANIFEST, TestLock::default(), Replace::default())
        .publish("install")
        .unwrap();
    let failing = AsrLifecycle::new(
        &root.0,
        &MANIFEST,
        TestLock::default(),
        Replace {
            fail_replace: false,
            fail_remove: true,
        },
    );
    assert_eq!(
        failing.remove(),
        Err(LifecycleError::Storage { retryable: true })
    );
    assert_eq!(failing.resolve(), LifecycleState::Ready);
    let retry = AsrLifecycle::new(&root.0, &MANIFEST, TestLock::default(), Replace::default());
    assert_eq!(retry.remove(), Ok(LifecycleState::NotInstalled));
}
