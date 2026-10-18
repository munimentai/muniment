//! The thread operations of checkpoints: a turn's changes and their
//! revert, and the preview and run of a rewind.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    irreversible_effect, plan_restore, restore, restore_conversation, revert_file, revert_hunk,
    turn_changes, Checkpoint, CheckpointKind, CheckpointStore, IrreversibleKind, RestoreError,
};
use crate::active_run::RunRegistry;
use crate::journal::thread_mutation::{append_thread_rewind_now, ThreadMutationError};
use crate::journal::Provenance;
use crate::run_events::SharedStorage;

/// The most labels a rewind preview names for one kind of effect.
const MAX_EFFECT_LABELS: usize = 12;

/// What a checkpoint operation needs from the runtime.
pub struct CheckpointScope<'a> {
    pub store: &'a CheckpointStore,
    pub storage: &'a SharedStorage,
    pub active: &'a RunRegistry,
    /// The signed-in subject, whose threads these are.
    pub subject: Option<&'a str>,
    pub provenance: Provenance,
    /// Pi's session folder, which holds each thread's saved conversation.
    pub session_root: &'a Path,
    /// Folders a record leaves out wherever they sit, such as the runtime's
    /// own profile.
    pub protected: Vec<PathBuf>,
    /// Whether another member runs the thread, and which.
    pub replica_of: &'a dyn Fn(&str) -> Option<String>,
    /// The folder a thread's runs work in.
    pub folder_of: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// Ends a thread's idle engine process, so its next run reads the
    /// conversation again.
    pub release_thread: &'a dyn Fn(&str),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ByRun {
    thread_id: String,
    run_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revert {
    thread_id: String,
    run_id: String,
    path: String,
    #[serde(default)]
    hunk: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rewind {
    thread_id: String,
    run_id: String,
    /// The preview's token for the files. Absent rewinds the conversation
    /// alone.
    #[serde(default)]
    token: Option<String>,
}

/// The effects of one kind a rewind leaves in place.
#[derive(Serialize)]
struct EffectGroup {
    kind: IrreversibleKind,
    count: usize,
    labels: Vec<String>,
}

fn lock(
    storage: &SharedStorage,
) -> Result<std::sync::MutexGuard<'_, crate::run_events::ChatStorage>, String> {
    storage
        .lock()
        .map_err(|_| "Conversation history is busy.".to_string())
}

/// Answers `run.changes`, `run.revert`, `thread.rewind_preview` and
/// `thread.rewind`.
pub fn operation(
    scope: &CheckpointScope<'_>,
    operation: &str,
    body: Value,
) -> Result<Value, String> {
    let invalid = |_| "The request is invalid.".to_string();
    match operation {
        "run.changes" => {
            let request: ByRun = serde_json::from_value(body).map_err(invalid)?;
            check_run(scope, &request.thread_id, &request.run_id)?;
            changes(scope, &request.thread_id, &request.run_id)
        }
        "run.revert" => {
            let request: Revert = serde_json::from_value(body).map_err(invalid)?;
            check_run(scope, &request.thread_id, &request.run_id)?;
            let (before, after) = pair(scope, &request.thread_id, &request.run_id)?;
            if let Some(reason) = folder_busy(scope, None, Path::new(&after.folder)) {
                return Err(reason);
            }
            let guard = scope.store.lock();
            let outcome = match request.hunk {
                Some(hunk) => {
                    revert_hunk(scope.store, &guard, &before, &after, &request.path, hunk)
                }
                None => revert_file(scope.store, &guard, &before, &after, &request.path),
            }
            .map_err(|error| error.to_string())?;
            drop(guard);
            Ok(json!({
                "outcome": outcome,
                "changes": turn_changes(scope.store, &before, &after),
            }))
        }
        "thread.rewind_preview" => {
            let request: ByRun = serde_json::from_value(body).map_err(invalid)?;
            check_run(scope, &request.thread_id, &request.run_id)?;
            preview(scope, &request.thread_id, &request.run_id)
        }
        "thread.rewind" => {
            let request: Rewind = serde_json::from_value(body).map_err(invalid)?;
            check_run(scope, &request.thread_id, &request.run_id)?;
            rewind(scope, &request)
        }
        _ => Err("The operation is not a checkpoint operation.".into()),
    }
}

/// Refuses a thread another member runs, a thread the subject does not
/// own, and a run of another thread.
fn check_run(scope: &CheckpointScope<'_>, thread_id: &str, run_id: &str) -> Result<(), String> {
    if (scope.replica_of)(thread_id).is_some() {
        return Err("Another machine runs this thread. Rewind it on that machine.".into());
    }
    let mut storage = lock(scope.storage)?;
    let owned = crate::thread_ownership::subject_owns_first_run(
        &mut storage.journal,
        thread_id,
        scope.subject,
    )
    .map_err(|_| "The thread could not be read.".to_string())?;
    let thread = storage
        .journal
        .run_thread_id(run_id)
        .map_err(|error| error.to_string())?;
    if !owned || thread.as_deref() != Some(thread_id) {
        return Err("That turn is not in this thread.".into());
    }
    Ok(())
}

fn record(
    scope: &CheckpointScope<'_>,
    thread_id: &str,
    run_id: &str,
    kind: CheckpointKind,
) -> Result<Option<Checkpoint>, String> {
    scope
        .store
        .record(thread_id, run_id, kind)
        .map_err(|_| "The turn's checkpoint could not be read.".to_string())
}

fn pair(
    scope: &CheckpointScope<'_>,
    thread_id: &str,
    run_id: &str,
) -> Result<(Checkpoint, Checkpoint), String> {
    let before = record(scope, thread_id, run_id, CheckpointKind::Before)?;
    let after = record(scope, thread_id, run_id, CheckpointKind::After)?;
    match (before, after) {
        (Some(before), Some(after)) => Ok((before, after)),
        (Some(_), None) => {
            Err("This turn is still running or ended before its files were saved.".into())
        }
        _ => Err("This turn has no checkpoint.".into()),
    }
}

fn changes(scope: &CheckpointScope<'_>, thread_id: &str, run_id: &str) -> Result<Value, String> {
    match pair(scope, thread_id, run_id) {
        Ok((before, after)) => serde_json::to_value(turn_changes(scope.store, &before, &after))
            .map_err(|error| error.to_string()),
        Err(reason) => Ok(json!({
            "run_id": run_id,
            "available": false,
            "unavailable": reason,
            "files": [],
            "more": 0,
        })),
    }
}

/// Why a restore in `folder` must wait: a live run in the thread, or one in
/// another thread that works in the same folder.
fn folder_busy(
    scope: &CheckpointScope<'_>,
    thread_id: Option<&str>,
    folder: &Path,
) -> Option<String> {
    for identity in scope.active.identities() {
        let Some(thread) = identity.thread_id.as_deref() else {
            continue;
        };
        if Some(thread) == thread_id {
            return Some("A reply is running in this thread. Stop it, then rewind.".into());
        }
        if (scope.folder_of)(thread).is_some_and(|other| super::folders_overlap(&other, folder)) {
            return Some("Another reply is using this folder. Rewind after it finishes.".into());
        }
    }
    None
}

fn excluded(scope: &CheckpointScope<'_>, folder: &Path) -> Vec<PathBuf> {
    scope
        .protected
        .iter()
        .filter(|path| path.starts_with(folder) && path.as_path() != folder)
        .cloned()
        .collect()
}

/// Whether the conversation can return to the record: the record saved it,
/// or the thread had none before this turn.
fn conversation_state(
    scope: &CheckpointScope<'_>,
    before: &Checkpoint,
    run_id: &str,
) -> Result<&'static str, String> {
    if before.conversation.is_some() {
        return Ok("restore");
    }
    let earlier = lock(scope.storage)?
        .journal
        .previous_thread_session_binding(run_id)
        .map_err(|error| error.to_string())?;
    Ok(if earlier.is_some() {
        "unavailable"
    } else {
        "new"
    })
}

fn effects(
    scope: &CheckpointScope<'_>,
    runs: &[String],
    folder: &Path,
) -> Result<Vec<EffectGroup>, String> {
    let mut groups: BTreeMap<u8, EffectGroup> = BTreeMap::new();
    for run_id in runs {
        let events = lock(scope.storage)?
            .journal
            .events(run_id)
            .map_err(|error| error.to_string())?;
        let Ok(projection) = crate::journal::reducer::project_chat(&events) else {
            continue;
        };
        for activity in &projection.tool_activity {
            let Some(effect) = irreversible_effect(
                activity.display_name.as_deref().unwrap_or_default(),
                activity.input.as_deref(),
                folder,
            ) else {
                continue;
            };
            let group = groups
                .entry(effect.kind as u8)
                .or_insert_with(|| EffectGroup {
                    kind: effect.kind,
                    count: 0,
                    labels: Vec::new(),
                });
            group.count += 1;
            if group.labels.len() < MAX_EFFECT_LABELS && !group.labels.contains(&effect.label) {
                group.labels.push(effect.label);
            }
        }
    }
    Ok(groups.into_values().collect())
}

fn preview(scope: &CheckpointScope<'_>, thread_id: &str, run_id: &str) -> Result<Value, String> {
    let Some(before) = record(scope, thread_id, run_id, CheckpointKind::Before)? else {
        return Ok(json!({
            "thread_id": thread_id,
            "run_id": run_id,
            "available": false,
            "unavailable": "This turn has no checkpoint, so it cannot rewind.",
        }));
    };
    let folder = PathBuf::from(&before.folder);
    let runs = lock(scope.storage)?
        .journal
        .thread_runs_from(run_id)
        .map_err(|error| error.to_string())?;
    let conversation = conversation_state(scope, &before, run_id)?;
    let plan = plan_restore(scope.store, &before, &excluded(scope, &folder));
    let mut answer = json!({
        "thread_id": thread_id,
        "run_id": run_id,
        "available": conversation != "unavailable",
        "turns": runs.len(),
        "conversation": conversation,
        "files": plan,
        "irreversible": effects(scope, &runs, &folder)?,
    });
    if conversation == "unavailable" {
        answer["unavailable"] =
            json!("This turn's conversation was not saved, so it cannot rewind.");
    }
    if let Some(reason) = folder_busy(scope, Some(thread_id), &folder) {
        answer["blocked"] = json!(reason);
    }
    Ok(answer)
}

fn rewind(scope: &CheckpointScope<'_>, request: &Rewind) -> Result<Value, String> {
    let (thread_id, run_id) = (request.thread_id.as_str(), request.run_id.as_str());
    let before = record(scope, thread_id, run_id, CheckpointKind::Before)?
        .ok_or("This turn has no checkpoint, so it cannot rewind.")?;
    let folder = PathBuf::from(&before.folder);
    if conversation_state(scope, &before, run_id)? == "unavailable" {
        return Err("This turn's conversation was not saved, so it cannot rewind.".into());
    }
    let guard = scope.store.lock();
    if let Some(reason) = folder_busy(scope, Some(thread_id), &folder) {
        return Err(reason);
    }
    let files = match &request.token {
        Some(token) => Some(
            restore(
                scope.store,
                &guard,
                &before,
                &excluded(scope, &folder),
                token,
            )
            .map_err(|error| match error {
                RestoreError::Changed => {
                    "The folder changed after the preview. Review the rewind again.".to_string()
                }
                RestoreError::SafetyRecord(_) => {
                    "The folder could not be saved before the rewind, so nothing changed."
                        .to_string()
                }
            })?,
        ),
        None => None,
    };
    let runs = {
        let mut storage = lock(scope.storage)?;
        append_thread_rewind_now(
            &mut storage.journal,
            scope.subject,
            thread_id,
            run_id,
            &scope.provenance,
        )
        .map_err(|error| match error {
            ThreadMutationError::NotOwned => "That turn is not in this thread.".to_string(),
            _ => "The conversation could not be rewound.".to_string(),
        })?
    };
    let conversation = match &before.conversation {
        Some(saved) => match restore_conversation(scope.store, scope.session_root, saved) {
            Ok(()) => json!("restored"),
            Err(reason) => json!({"failed": reason}),
        },
        None => json!("new"),
    };
    drop(guard);
    (scope.release_thread)(thread_id);
    let files_complete = files.as_ref().is_none_or(|report| report.complete);
    let complete = files_complete && conversation.is_string();
    Ok(json!({
        "thread_id": thread_id,
        "run_id": run_id,
        "status": if complete { "rewound" } else { "partial" },
        "turns": runs.len(),
        "conversation": conversation,
        "files": files,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::attach::RuntimeActivityRegistry;
    use crate::cas::LocalCas;
    use crate::checkpoint::{CaptureRequest, CheckpointLimits};
    use crate::journal::{EventEnvelope, EventPayload, RunJournal};
    use crate::run_events::ChatStorage;
    use crate::run_start::ActiveRun;

    struct Fixture {
        root: PathBuf,
        folder: PathBuf,
        sessions: PathBuf,
        store: CheckpointStore,
        storage: SharedStorage,
        active: RunRegistry,
        activity: RuntimeActivityRegistry,
        thread: String,
        first: String,
        second: String,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            source: "checkpoint-test".into(),
            source_version: "1".into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        }
    }

    fn started() -> EventEnvelope {
        EventEnvelope {
            event_id: uuid::Uuid::now_v7().to_string(),
            run_id: uuid::Uuid::now_v7().to_string(),
            run_seq: 1,
            event_type: "run.started".into(),
            event_version: 1,
            envelope_version: 1,
            recorded_at: "2026-10-18T12:00:00Z".into(),
            occurred_at: None,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::Inline {
                payload_json: json!({}),
            },
            provenance: provenance(),
            extra: BTreeMap::new(),
        }
    }

    /// A thread of two turns. The second turn's shell command changed the
    /// notes and added a draft, and its saved conversation holds one turn.
    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!("checkpoint-ops-{}", uuid::Uuid::new_v4()));
        let (folder, sessions) = (root.join("folder"), root.join("sessions"));
        fs::create_dir_all(&folder).unwrap();
        fs::create_dir_all(&sessions).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let (folder, sessions) = (root.join("folder"), root.join("sessions"));
        let mut journal = RunJournal::open(root.join("runs.sqlite3")).unwrap();
        let first = started();
        let thread = journal.append_new_run("local", &first).unwrap();
        let second = started();
        journal
            .append_new_run_in_thread("local", &thread, &second)
            .unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal,
            cas: LocalCas::open(&root.join("cas")).unwrap(),
        }));
        let store =
            CheckpointStore::open(&root.join("checkpoints"), CheckpointLimits::default()).unwrap();
        fs::write(folder.join("notes.md"), "Draft\n").unwrap();
        fs::write(sessions.join("thread.jsonl"), "{\"turn\":1}\n").unwrap();
        let capture = |kind| {
            store
                .capture(&CaptureRequest {
                    thread_id: &thread,
                    run_id: &second.run_id,
                    kind,
                    folder: &folder,
                    excluded: &[],
                    conversation: (kind == CheckpointKind::Before)
                        .then(|| ("thread.jsonl", sessions.join("thread.jsonl")))
                        .as_ref()
                        .map(|(locator, path)| (*locator, path.as_path())),
                })
                .unwrap()
        };
        capture(CheckpointKind::Before);
        #[cfg(unix)]
        std::process::Command::new("sh")
            .arg("-c")
            .arg("printf 'Final\\n' > notes.md && printf 'x\\n' > draft.md")
            .current_dir(&folder)
            .status()
            .unwrap();
        #[cfg(not(unix))]
        {
            fs::write(folder.join("notes.md"), "Final\n").unwrap();
            fs::write(folder.join("draft.md"), "x\n").unwrap();
        }
        fs::write(
            sessions.join("thread.jsonl"),
            "{\"turn\":1}\n{\"turn\":2}\n",
        )
        .unwrap();
        capture(CheckpointKind::After);
        Fixture {
            root,
            folder,
            sessions,
            store,
            storage,
            active: RunRegistry::with_limit(4),
            activity: RuntimeActivityRegistry::new(),
            thread,
            first: first.run_id,
            second: second.run_id,
        }
    }

    fn call(
        fixture: &Fixture,
        replica: bool,
        operation: &str,
        body: Value,
    ) -> Result<Value, String> {
        let replica_of = |_: &str| replica.then(|| "studio".to_string());
        let folder = fixture.folder.clone();
        let folder_of = move |_: &str| Some(folder.clone());
        let release_thread = |_: &str| {};
        let scope = CheckpointScope {
            store: &fixture.store,
            storage: &fixture.storage,
            active: &fixture.active,
            subject: None,
            provenance: provenance(),
            session_root: &fixture.sessions,
            protected: Vec::new(),
            replica_of: &replica_of,
            folder_of: &folder_of,
            release_thread: &release_thread,
        };
        super::operation(&scope, operation, body)
    }

    fn run_in(fixture: &Fixture, thread: &str) {
        let run = ActiveRun {
            id: uuid::Uuid::now_v7().to_string(),
            workspace: "local".into(),
            cancelled: Arc::new(AtomicBool::new(false)),
            transport: Arc::new(Mutex::new(None)),
            adapter: Arc::new(Mutex::new(None)),
            permission_answers: Arc::new(Mutex::new(VecDeque::new())),
            _activity: fixture.activity.mark_active_run(),
        };
        fixture.active.install(run, Some(thread)).unwrap();
    }

    fn by_run(fixture: &Fixture) -> Value {
        json!({"thread_id": fixture.thread, "run_id": fixture.second})
    }

    #[test]
    fn a_rewind_restores_the_files_and_the_conversation_and_drops_the_turn() {
        let fixture = fixture();
        let preview = call(&fixture, false, "thread.rewind_preview", by_run(&fixture)).unwrap();
        assert_eq!(preview["available"], true);
        assert_eq!(preview["turns"], 1);
        assert_eq!(preview["conversation"], "restore");
        assert_eq!(preview["files"]["restore"], json!(["notes.md"]));
        assert_eq!(preview["files"]["remove"], json!(["draft.md"]));
        assert!(preview.get("blocked").is_none());

        let mut body = by_run(&fixture);
        body["token"] = preview["files"]["token"].clone();
        let report = call(&fixture, false, "thread.rewind", body).unwrap();
        assert_eq!(report["status"], "rewound");
        assert_eq!(report["conversation"], "restored");
        assert_eq!(report["files"]["restored"], json!(["notes.md"]));
        assert_eq!(report["files"]["removed"], json!(["draft.md"]));
        assert_eq!(
            fs::read_to_string(fixture.folder.join("notes.md")).unwrap(),
            "Draft\n"
        );
        assert!(!fixture.folder.join("draft.md").exists());
        assert_eq!(
            fs::read_to_string(fixture.sessions.join("thread.jsonl")).unwrap(),
            "{\"turn\":1}\n"
        );
        let runs = fixture
            .storage
            .lock()
            .unwrap()
            .journal
            .thread_run_ids(&fixture.thread, 10, None)
            .unwrap()
            .run_ids;
        assert_eq!(runs, std::slice::from_ref(&fixture.first));
    }

    #[test]
    fn a_rewind_waits_while_another_run_uses_the_folder() {
        let fixture = fixture();
        run_in(&fixture, &uuid::Uuid::now_v7().to_string());
        let preview = call(&fixture, false, "thread.rewind_preview", by_run(&fixture)).unwrap();
        assert_eq!(
            preview["blocked"],
            "Another reply is using this folder. Rewind after it finishes."
        );
        let mut body = by_run(&fixture);
        body["token"] = preview["files"]["token"].clone();
        let refused = call(&fixture, false, "thread.rewind", body).unwrap_err();
        assert!(refused.starts_with("Another reply is using this folder."));
        let revert =
            json!({"thread_id": fixture.thread, "run_id": fixture.second, "path": "notes.md"});
        assert!(call(&fixture, false, "run.revert", revert).is_err());
        assert_eq!(
            fs::read_to_string(fixture.folder.join("notes.md")).unwrap(),
            "Final\n"
        );
        assert!(fixture.folder.join("draft.md").exists());
    }

    #[test]
    fn a_rewind_waits_while_the_thread_runs() {
        let fixture = fixture();
        run_in(&fixture, &fixture.thread);
        let refused = call(&fixture, false, "thread.rewind", by_run(&fixture)).unwrap_err();
        assert_eq!(
            refused,
            "A reply is running in this thread. Stop it, then rewind."
        );
    }

    #[test]
    fn another_machines_thread_rewinds_on_that_machine() {
        let fixture = fixture();
        let refused = call(&fixture, true, "thread.rewind_preview", by_run(&fixture)).unwrap_err();
        assert_eq!(
            refused,
            "Another machine runs this thread. Rewind it on that machine."
        );
    }

    #[test]
    fn a_turns_changes_list_shell_edits_and_revert_one_file() {
        let fixture = fixture();
        let changes = call(&fixture, false, "run.changes", by_run(&fixture)).unwrap();
        assert_eq!(changes["available"], true);
        let paths: Vec<_> = changes["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(paths, ["draft.md", "notes.md"]);
        let revert = json!({"thread_id": fixture.thread, "run_id": fixture.second, "path": "notes.md", "hunk": 0});
        let reverted = call(&fixture, false, "run.revert", revert).unwrap();
        assert_eq!(reverted["outcome"]["state"], "reverted");
        assert_eq!(
            fs::read_to_string(fixture.folder.join("notes.md")).unwrap(),
            "Draft\n"
        );
        let first = json!({"thread_id": fixture.thread, "run_id": fixture.first});
        let none = call(&fixture, false, "run.changes", first).unwrap();
        assert_eq!(none["available"], false);
        assert_eq!(none["unavailable"], "This turn has no checkpoint.");
    }
}
