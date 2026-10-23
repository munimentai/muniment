use super::*;

const THREAD: &str = "019965a0-0000-7000-8000-0000000000a1";
const RUN: &str = "019965a0-0000-7000-8000-0000000000b1";

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("checkpoint-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, relative: &str) -> Option<String> {
        fs::read_to_string(self.0.join(relative)).ok()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for entry in fs::read_dir(&self.0).into_iter().flatten().flatten() {
                let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o755));
            }
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn store(dir: &TestDir, limits: CheckpointLimits) -> CheckpointStore {
    CheckpointStore::open(&dir.0, limits).unwrap()
}

fn capture(
    store: &CheckpointStore,
    folder: &TestDir,
    run_id: &str,
    kind: CheckpointKind,
) -> Checkpoint {
    store
        .capture(&CaptureRequest {
            thread_id: THREAD,
            run_id,
            kind,
            folder: &folder.0,
            excluded: &[],
            conversation: None,
        })
        .unwrap()
}

/// Edits the folder the way a turn's shell command would.
fn shell(folder: &TestDir, script: &str) {
    #[cfg(unix)]
    {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(&folder.0)
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[cfg(not(unix))]
    {
        let _ = script;
        folder.write("a.txt", "changed\n");
        fs::remove_file(folder.0.join("sub/b.txt")).unwrap();
        folder.write("new/c.txt", "x\n");
    }
}

#[test]
fn a_rewind_restores_files_a_shell_command_changed_and_removes_new_ones() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let store = store(&root, CheckpointLimits::default());
    folder.write("a.txt", "one\n");
    folder.write("sub/b.txt", "two\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    shell(
        &folder,
        "echo changed > a.txt && rm sub/b.txt && mkdir new && echo x > new/c.txt",
    );

    let plan = plan_restore(&store, &before, &[]);
    assert!(plan.files_available);
    assert_eq!(plan.restore, ["a.txt", "sub/b.txt"]);
    assert_eq!(plan.remove, ["new/c.txt"]);
    assert!(plan.cannot_restore.is_empty());

    let guard = store.lock();
    let report = restore(&store, &guard, &before, &[], &plan.token).unwrap();
    drop(guard);
    assert!(report.complete);
    assert_eq!(report.restored, ["a.txt", "sub/b.txt"]);
    assert_eq!(report.removed, ["new/c.txt"]);
    assert_eq!(folder.read("a.txt").as_deref(), Some("one\n"));
    assert_eq!(folder.read("sub/b.txt").as_deref(), Some("two\n"));
    assert!(!folder.0.join("new").exists());

    let safety = report.safety_checkpoint.unwrap();
    let records = store.thread_records(THREAD).unwrap();
    let saved = records.iter().find(|record| record.id == safety).unwrap();
    assert_eq!(saved.kind, CheckpointKind::Rewind);
    assert!(saved.files.contains_key("new/c.txt"));
}

#[test]
fn a_restore_refuses_when_the_folder_changed_after_the_preview() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let store = store(&root, CheckpointLimits::default());
    folder.write("a.txt", "one\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    folder.write("a.txt", "two\n");
    let plan = plan_restore(&store, &before, &[]);
    folder.write("a.txt", "three\n");
    let guard = store.lock();
    assert!(matches!(
        restore(&store, &guard, &before, &[], &plan.token),
        Err(RestoreError::Changed)
    ));
    assert_eq!(folder.read("a.txt").as_deref(), Some("three\n"));
}

#[test]
fn excluded_folders_stay_as_they_are_and_are_named() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let store = store(&root, CheckpointLimits::default());
    folder.write("a.txt", "one\n");
    folder.write(".git/HEAD", "ref: main\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    assert!(!before.files.keys().any(|path| path.starts_with(".git")));
    folder.write(".git/HEAD", "ref: other\n");
    folder.write("a.txt", "two\n");
    let plan = plan_restore(&store, &before, &[]);
    assert_eq!(plan.not_covered, [".git"]);
    let guard = store.lock();
    let report = restore(&store, &guard, &before, &[], &plan.token).unwrap();
    assert!(report.complete);
    assert_eq!(report.not_covered, [".git"]);
    assert_eq!(folder.read(".git/HEAD").as_deref(), Some("ref: other\n"));
    assert_eq!(folder.read("a.txt").as_deref(), Some("one\n"));
}

#[test]
fn a_file_too_large_to_save_makes_the_restore_partial() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let limits = CheckpointLimits {
        max_file_bytes: 8,
        ..CheckpointLimits::default()
    };
    let store = store(&root, limits);
    folder.write("small.txt", "one\n");
    folder.write("large.txt", "a long first version\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    folder.write("small.txt", "two\n");
    folder.write("large.txt", "a long second version\n");
    let plan = plan_restore(&store, &before, &[]);
    assert_eq!(plan.restore, ["small.txt"]);
    assert_eq!(
        plan.cannot_restore,
        [Unrestorable {
            path: "large.txt".into(),
            reason: UnrestorableReason::TooLarge
        }]
    );
    let guard = store.lock();
    let report = restore(&store, &guard, &before, &[], &plan.token).unwrap();
    assert!(!report.complete);
    assert_eq!(report.restored, ["small.txt"]);
    assert_eq!(
        folder.read("large.txt").as_deref(),
        Some("a long second version\n")
    );
}

#[cfg(unix)]
#[test]
fn a_write_that_fails_is_reported_and_the_restore_is_not_complete() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let store = store(&root, CheckpointLimits::default());
    folder.write("open/a.txt", "one\n");
    folder.write("locked/b.txt", "two\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    folder.write("open/a.txt", "changed\n");
    folder.write("locked/b.txt", "changed\n");
    fs::set_permissions(folder.0.join("locked"), fs::Permissions::from_mode(0o555)).unwrap();
    let plan = plan_restore(&store, &before, &[]);
    let guard = store.lock();
    let report = restore(&store, &guard, &before, &[], &plan.token).unwrap();
    assert!(!report.complete);
    assert_eq!(report.restored, ["open/a.txt"]);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].path, "locked/b.txt");
    assert_eq!(report.failed[0].reason, "Permission denied.");
    assert_eq!(folder.read("locked/b.txt").as_deref(), Some("changed\n"));
}

#[cfg(unix)]
#[test]
fn a_link_that_replaced_a_folder_goes_and_the_folder_returns() {
    let (root, folder, outside) = (
        TestDir::new("store"),
        TestDir::new("folder"),
        TestDir::new("outside"),
    );
    let store = store(&root, CheckpointLimits::default());
    folder.write("sub/b.txt", "two\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    fs::remove_dir_all(folder.0.join("sub")).unwrap();
    std::os::unix::fs::symlink(&outside.0, folder.0.join("sub")).unwrap();
    let plan = plan_restore(&store, &before, &[]);
    let guard = store.lock();
    let report = restore(&store, &guard, &before, &[], &plan.token).unwrap();
    assert!(report.complete);
    assert_eq!(report.removed, ["sub"]);
    assert!(!fs::symlink_metadata(folder.0.join("sub"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(folder.read("sub/b.txt").as_deref(), Some("two\n"));
    assert!(!outside.0.join("b.txt").exists());
}

#[test]
fn an_incomplete_record_restores_no_file() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let limits = CheckpointLimits {
        max_files: 1,
        ..CheckpointLimits::default()
    };
    let store = store(&root, limits);
    folder.write("a.txt", "one\n");
    folder.write("b.txt", "two\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    assert_eq!(before.incomplete, Some(Incomplete::TooManyFiles));
    folder.write("c.txt", "three\n");
    let plan = plan_restore(&store, &before, &[]);
    assert!(!plan.files_available);
    assert!(plan.unavailable.unwrap().contains("more than 1 files"));
    let guard = store.lock();
    let report = restore(&store, &guard, &before, &[], &plan.token).unwrap();
    assert!(!report.complete);
    assert!(report.removed.is_empty());
    assert!(folder.0.join("c.txt").exists());
}

#[test]
fn the_saved_conversation_is_written_back() {
    let (root, folder, sessions) = (
        TestDir::new("store"),
        TestDir::new("folder"),
        TestDir::new("sessions"),
    );
    let store = store(&root, CheckpointLimits::default());
    sessions.write("thread.jsonl", "{\"turn\":1}\n");
    let session = sessions.0.join("thread.jsonl");
    let before = store
        .capture(&CaptureRequest {
            thread_id: THREAD,
            run_id: RUN,
            kind: CheckpointKind::Before,
            folder: &folder.0,
            excluded: &[],
            conversation: Some(("thread.jsonl", &session)),
        })
        .unwrap();
    sessions.write("thread.jsonl", "{\"turn\":1}\n{\"turn\":2}\n");
    restore_conversation(&store, &sessions.0, before.conversation.as_ref().unwrap()).unwrap();
    assert_eq!(
        sessions.read("thread.jsonl").as_deref(),
        Some("{\"turn\":1}\n")
    );
}

#[test]
fn a_turn_lists_its_changes_and_reverts_one_hunk_at_a_time() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let store = store(&root, CheckpointLimits::default());
    let lines: Vec<String> = (1..=20).map(|n| format!("line {n}\n")).collect();
    folder.write("notes.md", &lines.concat());
    folder.write("gone.txt", "bye\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    let mut edited = lines.clone();
    edited[1] = "line two\n".into();
    edited[17] = "line eighteen\n".into();
    folder.write("notes.md", &edited.concat());
    shell(&folder, "rm gone.txt && printf 'new\\n' > made.txt");
    let after = capture(&store, &folder, RUN, CheckpointKind::After);

    let changes = turn_changes(&store, &before, &after);
    assert!(changes.available);
    let paths: Vec<_> = changes
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, ["gone.txt", "made.txt", "notes.md"]);
    let notes = &changes.files[2];
    assert_eq!((notes.additions, notes.deletions), (Some(2), Some(2)));
    assert_eq!(notes.state, TurnFileState::Applied);
    assert_eq!(changes.diff.files[2].hunks.len(), 2);

    let guard = store.lock();
    let first = revert_hunk(&store, &guard, &before, &after, "notes.md", 0).unwrap();
    assert_eq!(first.state, TurnFileState::ChangedSince);
    let second = revert_hunk(&store, &guard, &before, &after, "notes.md", 1).unwrap();
    assert_eq!(second.state, TurnFileState::Reverted);
    assert_eq!(folder.read("notes.md").unwrap(), lines.concat());
    drop(guard);

    let changes = turn_changes(&store, &before, &after);
    assert_eq!(changes.files[2].state, TurnFileState::Reverted);
    assert_eq!(changes.files[2].hunks_applied, [false, false]);

    let guard = store.lock();
    revert_file(&store, &guard, &before, &after, "gone.txt").unwrap();
    revert_file(&store, &guard, &before, &after, "made.txt").unwrap();
    assert_eq!(folder.read("gone.txt").as_deref(), Some("bye\n"));
    assert!(!folder.0.join("made.txt").exists());
}

#[test]
fn a_revert_refuses_a_file_that_changed_after_the_turn() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let store = store(&root, CheckpointLimits::default());
    folder.write("a.txt", "one\n");
    let before = capture(&store, &folder, RUN, CheckpointKind::Before);
    folder.write("a.txt", "two\n");
    let after = capture(&store, &folder, RUN, CheckpointKind::After);
    folder.write("a.txt", "three\n");
    let guard = store.lock();
    assert_eq!(
        revert_file(&store, &guard, &before, &after, "a.txt"),
        Err(RevertError::ChangedSince)
    );
    assert_eq!(
        revert_hunk(&store, &guard, &before, &after, "a.txt", 0),
        Err(RevertError::ChangedSince)
    );
    assert_eq!(folder.read("a.txt").as_deref(), Some("three\n"));
    drop(guard);
    let changes = turn_changes(&store, &before, &after);
    assert_eq!(changes.files[0].state, TurnFileState::ChangedSince);
}

#[test]
fn a_sweep_keeps_the_newest_records_and_their_objects() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let limits = CheckpointLimits {
        keep_per_thread: 2,
        ..CheckpointLimits::default()
    };
    let store = store(&root, limits);
    let runs = [
        "019965a0-0000-7000-8000-0000000000c1",
        "019965a0-0000-7000-8000-0000000000c2",
        "019965a0-0000-7000-8000-0000000000c3",
    ];
    for (index, run) in runs.iter().enumerate() {
        folder.write("a.txt", &format!("version {index}\n"));
        capture(&store, &folder, run, CheckpointKind::Before);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(store.prune(SystemTime::now()).unwrap(), 1);
    let records = store.thread_records(THREAD).unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| record.run_id.as_str())
            .collect::<Vec<_>>(),
        &runs[1..]
    );
    let objects = store.cas.object_hashes().unwrap().count();
    assert_eq!(objects, 2);
    assert!(store
        .record(THREAD, runs[0], CheckpointKind::Before)
        .unwrap()
        .is_none());
}

#[test]
fn folders_overlap_when_one_holds_the_other() {
    let folder = TestDir::new("folder");
    fs::create_dir_all(folder.0.join("inner")).unwrap();
    assert!(folders_overlap(&folder.0, &folder.0.join("inner")));
    assert!(folders_overlap(&folder.0.join("inner"), &folder.0));
    let other = TestDir::new("other");
    assert!(!folders_overlap(&folder.0, &other.0));
}

#[test]
fn record_ids_must_be_plain_ids() {
    let (root, folder) = (TestDir::new("store"), TestDir::new("folder"));
    let store = store(&root, CheckpointLimits::default());
    let result = store.capture(&CaptureRequest {
        thread_id: "../escape",
        run_id: RUN,
        kind: CheckpointKind::Before,
        folder: &folder.0,
        excluded: &[],
        conversation: None,
    });
    assert!(matches!(result, Err(CheckpointError::InvalidId)));
}
