//! Runtime-owned boundaries for desktop attach reads.

use std::sync::{Arc, Mutex};

use muniment_core::active_run::queue_permission_answer_with_commit;
use muniment_core::attach::linux::{
    RunStreamPage, ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage,
    ThreadOpenRequest,
};
use muniment_core::attach::ProtocolError;
use muniment_core::journal::thread_mutation::create_thread_now;
use muniment_core::journal::{JournalCommitHint, Provenance};
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::run_events::SharedStorage;
use muniment_core::run_start::{ActiveRun, RunAttachBoundaries, RunStartError};

/// Supplies attach reads from runtime-owned state.
pub struct RuntimeAttachBoundaries {
    storage: SharedStorage,
    active: Arc<Mutex<Option<ActiveRun>>>,
}

impl RuntimeAttachBoundaries {
    pub fn new(storage: SharedStorage, active: Arc<Mutex<Option<ActiveRun>>>) -> Self {
        Self { storage, active }
    }
}

impl RunAttachBoundaries for RuntimeAttachBoundaries {
    fn list_threads(
        &self,
        workspace: &str,
        request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::list_threads(&mut storage.journal, workspace, request)
    }

    fn open_thread(
        &self,
        workspace: &str,
        request: ThreadOpenRequest,
    ) -> Result<ThreadOpenPage, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::open_thread(&mut storage.journal, workspace, request)
    }

    fn create_thread(
        &self,
        workspace: &str,
        mut provenance: Provenance,
    ) -> Result<String, ProtocolError> {
        provenance.source = "muniment-runtime".into();
        provenance.source_version = env!("CARGO_PKG_VERSION").into();
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        create_thread_now(&mut storage.journal, workspace, provenance)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn stream_run(
        &self,
        workspace: &str,
        run_id: &str,
        after_run_seq: u64,
    ) -> Result<RunStreamPage, ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        ThreadListService::stream_run(&mut storage.journal, workspace, run_id, after_run_seq)
    }

    fn subscribe_run_commits(
        &self,
        run_id: &str,
    ) -> Result<(u64, std::sync::mpsc::Receiver<JournalCommitHint>), ProtocolError> {
        let mut storage = self
            .storage
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        if storage
            .journal
            .run_thread_id(run_id)
            .map_err(|_| ProtocolError::persistence_failed())?
            .is_none()
        {
            return Err(ProtocolError::thread_not_found());
        }
        storage
            .journal
            .subscribe_commits(run_id)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn queue_attach_permission_answer(
        &self,
        workspace: &str,
        run_id: &str,
        gate_id: &str,
        answer: ChatPermissionAnswer,
    ) -> Result<std::sync::mpsc::Receiver<Option<u64>>, RunStartError> {
        queue_permission_answer_with_commit(
            &self.active,
            Some(workspace),
            run_id.to_owned(),
            gate_id.to_owned(),
            answer,
        )
        .map_err(RunStartError::InvalidRequest)
    }
}
