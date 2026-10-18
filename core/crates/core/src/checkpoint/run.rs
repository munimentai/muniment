//! The records a run makes: its folder and conversation as it starts, and
//! its folder as it ends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::{CaptureRequest, CheckpointKind, CheckpointLimits, CheckpointStore};
use crate::pi_launch::PiLaunchBoundaries;
use crate::run_events::SharedStorage;

/// The folder under a runtime profile that holds its checkpoints.
pub const CHECKPOINT_FOLDER: &str = "checkpoints";

/// The one store of a profile, shared by its runs and its operations.
pub fn shared(profile: &Path) -> Option<Arc<CheckpointStore>> {
    static STORES: OnceLock<Mutex<HashMap<PathBuf, Arc<CheckpointStore>>>> = OnceLock::new();
    let root = profile.join(CHECKPOINT_FOLDER);
    let mut stores = STORES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(store) = stores.get(&root) {
        return Some(Arc::clone(store));
    }
    let store = Arc::new(CheckpointStore::open(&root, CheckpointLimits::default()).ok()?);
    stores.insert(root, Arc::clone(&store));
    Some(store)
}

/// The runtime's own folders, which no record holds: its profile and its
/// state.
pub fn protected_folders(profile: Option<&Path>) -> Vec<PathBuf> {
    profile
        .map(Path::to_path_buf)
        .into_iter()
        .chain(crate::state_root::state_directory())
        .collect()
}

/// Folders inside a run's folder that a record leaves out: the runtime's
/// own profile and state.
pub fn excluded_folders(app: &impl PiLaunchBoundaries, folder: &Path) -> Vec<PathBuf> {
    let profile = app
        .pi_session_root()
        .ok()
        .and_then(|root| root.parent().map(Path::to_path_buf));
    protected_folders(profile.as_deref())
        .into_iter()
        .filter(|path| path.starts_with(folder) && path != folder)
        .collect()
}

/// Records a run's folder as the run ends, once the run recorded it as it
/// started.
pub struct RunCheckpoints {
    store: Arc<CheckpointStore>,
    thread_id: String,
    run_id: String,
    folder: PathBuf,
    excluded: Vec<PathBuf>,
}

impl RunCheckpoints {
    /// Records the folder and the thread's saved conversation before a new
    /// reply. A failed record leaves the run as it is: the turn then offers
    /// no rewind.
    pub fn begin(
        app: &impl PiLaunchBoundaries,
        journal: &SharedStorage,
        run_id: &str,
        subject: Option<&str>,
        folder: Option<&Path>,
    ) -> Option<Self> {
        let checkpoints = Self::prepare(app, run_id, folder)?;
        let session_root = app.pi_session_root().ok();
        let conversation = session_root.as_ref().and_then(|root| {
            crate::pi_execution::previous_thread_session(journal, run_id, subject, root)
                .ok()
                .flatten()
                .map(|locator| (locator.as_str().to_owned(), root.join(locator.as_str())))
        });
        let result = checkpoints.store.capture(&CaptureRequest {
            thread_id: &checkpoints.thread_id,
            run_id,
            kind: CheckpointKind::Before,
            folder: &checkpoints.folder,
            excluded: &checkpoints.excluded,
            conversation: conversation
                .as_ref()
                .map(|(locator, path)| (locator.as_str(), path.as_path())),
        });
        match result {
            Ok(_) => Some(checkpoints),
            Err(error) => {
                eprintln!(
                    "muniment-runtime: run_id={run_id} checkpoint_before failed error={error}"
                );
                None
            }
        }
    }

    /// Records the folder as a resumed reply ends, when the reply recorded
    /// it as it first started.
    pub fn resume(
        app: &impl PiLaunchBoundaries,
        run_id: &str,
        folder: Option<&Path>,
    ) -> Option<Self> {
        let checkpoints = Self::prepare(app, run_id, folder)?;
        checkpoints
            .store
            .record(&checkpoints.thread_id, run_id, CheckpointKind::Before)
            .ok()
            .flatten()
            .map(|_| checkpoints)
    }

    fn prepare(app: &impl PiLaunchBoundaries, run_id: &str, folder: Option<&Path>) -> Option<Self> {
        let store = app.checkpoint_store()?;
        let thread_id = app.extension_thread_id()?;
        let folder = folder?.to_owned();
        let excluded = excluded_folders(app, &folder);
        Some(Self {
            store,
            thread_id,
            run_id: run_id.to_owned(),
            folder,
            excluded,
        })
    }
}

impl Drop for RunCheckpoints {
    fn drop(&mut self) {
        if let Err(error) = self.store.capture(&CaptureRequest {
            thread_id: &self.thread_id,
            run_id: &self.run_id,
            kind: CheckpointKind::After,
            folder: &self.folder,
            excluded: &self.excluded,
            conversation: None,
        }) {
            eprintln!(
                "muniment-runtime: run_id={} checkpoint_after failed error={error}",
                self.run_id
            );
        }
    }
}
