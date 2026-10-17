//! In-process members for the tests of classes other parts of core keep on
//! records: each has its own profile, and two members exchange through the
//! same frames a link carries, without a network.

use std::path::PathBuf;
use std::sync::Arc;

use super::*;

struct QuietHost;

impl SwarmHost for QuietHost {
    fn replica_scope(&self) -> Option<ReplicaScope> {
        None
    }

    fn steer(&self, _thread_id: &str, _text: &str) -> Option<String> {
        None
    }

    fn answer_permission(
        &self,
        _run_id: &str,
        _gate_id: &str,
        _answer: ChatPermissionAnswer,
    ) -> Result<(), String> {
        Err("No runs here.".into())
    }

    fn kick_runs(&self) {}
}

/// One member with its own profile directory.
pub(crate) struct TestMember {
    pub(crate) swarm: Swarm,
    pub(crate) id: String,
    pub(crate) profile: PathBuf,
}

impl Drop for TestMember {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

impl TestMember {
    pub(crate) fn new(name: &str) -> Self {
        let profile =
            std::env::temp_dir().join(format!("swarm-member-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&profile).unwrap();
        let storage = Arc::new(Mutex::new(crate::run_events::ChatStorage {
            journal: crate::journal::RunJournal::open(profile.join("runs.sqlite3")).unwrap(),
            cas: crate::cas::LocalCas::open(&profile.join("cas")).unwrap(),
        }));
        let swarm = Swarm::open(&profile, storage, Arc::new(QuietHost)).unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        *lock(&swarm.inner.identity) = Some(Identity {
            access: None,
            runtime_id: id.clone(),
            runtime_name: name.into(),
            runtime_version: "1.0.0".into(),
            form_factor: None,
        });
        swarm.inner.store.set_meta("runtime_id", Some(&id)).unwrap();
        Self { swarm, id, profile }
    }

    pub(crate) fn records(&self) -> Records {
        self.swarm.records()
    }

    /// How many record rows this member wrote itself.
    pub(crate) fn own_writes(&self) -> u64 {
        self.swarm
            .inner
            .records
            .version_vector()
            .unwrap()
            .get(&self.id)
            .copied()
            .unwrap_or(0)
    }
}

/// Exchanges between `dialer` and `answerer` until neither leaves anything
/// out.
pub(crate) fn exchange(dialer: &TestMember, answerer: &TestMember) {
    for _ in 0..40 {
        let wire = |exchange: &Exchange| -> Exchange {
            serde_json::from_slice(&serde_json::to_vec(exchange).unwrap()).unwrap()
        };
        let request = wire(&dialer.swarm.inner.build_exchange(&answerer.id).unwrap());
        answerer
            .swarm
            .inner
            .merge_exchange(&dialer.id, request.clone())
            .unwrap();
        let response = wire(&answerer.swarm.inner.build_exchange(&dialer.id).unwrap());
        dialer
            .swarm
            .inner
            .merge_exchange(&answerer.id, response.clone())
            .unwrap();
        if !request.more && !response.more {
            return;
        }
    }
    panic!("the exchange never settled");
}
