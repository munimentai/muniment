//! Exchanges between in-process members, without a network: thread clock
//! deltas at scale and the fallback to an older member's full state.

use std::sync::Arc;

use rusqlite::params;
use serde_json::json;

use super::*;

struct TestHost;

impl SwarmHost for TestHost {
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

struct Node {
    swarm: Swarm,
    id: String,
    root: PathBuf,
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Node {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("swarm-exchange-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let storage = Arc::new(Mutex::new(crate::run_events::ChatStorage {
            journal: crate::journal::RunJournal::open(root.join("runs.sqlite3")).unwrap(),
            cas: crate::cas::LocalCas::open(&root.join("cas")).unwrap(),
        }));
        let swarm = Swarm::open(&root, storage, Arc::new(TestHost)).unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        *lock(&swarm.inner.identity) = Some(Identity {
            access: None,
            runtime_id: id.clone(),
            runtime_name: name.into(),
            runtime_version: "1.0.0".into(),
            form_factor: None,
        });
        swarm.inner.store.set_meta("runtime_id", Some(&id)).unwrap();
        Self { swarm, id, root }
    }

    fn inner(&self) -> &Inner {
        &self.swarm.inner
    }

    fn legacy(self) -> Self {
        self.swarm.set_legacy_wire(true);
        self
    }

    /// Registers `count` agent threads with three events each from this
    /// member, as a busy member holds them.
    fn seed_threads(&self, count: usize) -> Vec<String> {
        let threads: Vec<String> = (0..count)
            .map(|_| uuid::Uuid::now_v7().to_string())
            .collect();
        let agent = uuid::Uuid::now_v7().to_string();
        let mut connection = self.inner().store.connect().unwrap();
        let transaction = connection.transaction().unwrap();
        for thread in &threads {
            let registration = ThreadRegistration {
                thread_id: thread.clone(),
                kind: "agent".into(),
                agent_id: Some(agent.clone()),
                group_id: None,
                updated_at: "2026-01-01T00:00:00Z".into(),
            };
            transaction
                .execute(
                    "INSERT INTO threads(thread_id, record) VALUES(?1, ?2)",
                    params![thread, serde_json::to_string(&registration).unwrap()],
                )
                .unwrap();
            for seq in 1..=3 {
                transaction
                    .execute(
                        "INSERT INTO log(thread_id, origin, seq, kind, event_id, event_type) \
                         VALUES(?1, ?2, ?3, 'thread', ?4, 'thread.title.renamed')",
                        params![thread, self.id, seq, uuid::Uuid::now_v7().to_string()],
                    )
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
        sync::migrate(self.inner()).unwrap();
        threads
    }

    /// Copies another member's replication index, as a member that already
    /// replicated its threads holds it.
    fn copy_log(&self, from: &Node) {
        let connection = self.inner().store.connect().unwrap();
        connection
            .execute(
                "ATTACH DATABASE ?1 AS other",
                [from.root.join(store::STORE_FILE).to_string_lossy().as_ref()],
            )
            .unwrap();
        connection
            .execute("INSERT INTO log SELECT * FROM other.log", [])
            .unwrap();
        connection.execute("DETACH DATABASE other", []).unwrap();
    }

    fn add_event(&self, thread: &str, seq: u64) {
        self.inner()
            .store
            .connect()
            .unwrap()
            .execute(
                "INSERT INTO log(thread_id, origin, seq, kind, event_id, event_type) \
                 VALUES(?1, ?2, ?3, 'thread', ?4, 'thread.title.renamed')",
                params![thread, self.id, seq, uuid::Uuid::now_v7().to_string()],
            )
            .unwrap();
    }

    fn registered(&self) -> usize {
        sync::registrations(self.inner()).unwrap().len()
    }

    fn agent(&self, name: &str) -> DirectoryEntry {
        let entry = DirectoryEntry {
            agent_id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            label: String::new(),
            avatar: None,
            primary_runtime: self.id.clone(),
            epoch: 0,
            updated_at: String::new(),
            deleted: false,
            primary_thread: None,
            definition: Some(json!({"name": name})),
            activity: None,
        };
        assert!(directory::put_entry(self.inner(), &entry, None).unwrap());
        entry
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = directory::entries(self.inner(), false)
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        names.sort();
        names
    }

    fn group_names(&self) -> Vec<String> {
        let mut names: Vec<String> = groups::all(self.inner())
            .unwrap()
            .into_iter()
            .filter(|group| !group.deleted)
            .map(|group| group.name)
            .collect();
        names.sort();
        names
    }
}

/// The sizes of one round: the request and the response.
struct Round {
    request: Exchange,
    request_bytes: usize,
    response: Exchange,
    response_bytes: usize,
}

/// One exchange from `dialer` to `answerer` and back, through JSON as a
/// link carries it.
fn round(dialer: &Node, answerer: &Node) -> Round {
    let wire = |exchange: &Exchange| -> (Exchange, usize) {
        let bytes = serde_json::to_vec(exchange).unwrap();
        (serde_json::from_slice(&bytes).unwrap(), bytes.len())
    };
    let (request, request_bytes) = wire(&dialer.inner().build_exchange(&answerer.id).unwrap());
    answerer
        .inner()
        .merge_exchange(&dialer.id, request.clone())
        .unwrap();
    let (response, response_bytes) = wire(&answerer.inner().build_exchange(&dialer.id).unwrap());
    dialer
        .inner()
        .merge_exchange(&answerer.id, response.clone())
        .unwrap();
    Round {
        request,
        request_bytes,
        response,
        response_bytes,
    }
}

/// Exchanges until neither side leaves anything out. Answers the rounds.
fn settle(dialer: &Node, answerer: &Node) -> Vec<Round> {
    let mut rounds = Vec::new();
    for _ in 0..40 {
        let round = round(dialer, answerer);
        let more = round.request.more || round.response.more;
        rounds.push(round);
        if !more {
            return rounds;
        }
    }
    panic!("the exchange never settled");
}

#[test]
fn a_changed_thread_among_5000_sends_one_clock() {
    let a = Node::new("a");
    let b = Node::new("b");
    let threads = a.seed_threads(5000);
    b.copy_log(&a);

    // B joins: the registrations and both members' clocks arrive in pages.
    let started = std::time::Instant::now();
    let join = settle(&b, &a);
    let join_time = started.elapsed();
    let largest = join
        .iter()
        .map(|round| round.request_bytes.max(round.response_bytes))
        .max()
        .unwrap();
    assert!(largest <= FRAME_BUDGET, "a frame of {largest} bytes");
    assert_eq!(b.registered(), 5000);
    assert!(join[0].request.watermarks.is_some());
    // The first request does not know A's wire, so it carries both forms;
    // A answers with records only.
    assert!(join[0].request.record_vv.is_some());
    assert!(join[0].response.directory.is_empty() && join[0].response.threads.is_empty());

    // Idle: nothing changed, so the frames carry no records or clocks.
    let idle = round(&b, &a);
    assert!(idle.request.records.is_empty() && idle.response.records.is_empty());
    assert!(idle.request.clocks.is_empty() && idle.response.clocks.is_empty());

    // One thread changes on A: A's next exchange carries its clock alone.
    a.add_event(&threads[42], 4);
    let started = std::time::Instant::now();
    let changed = round(&b, &a);
    let changed_time = started.elapsed();
    assert_eq!(changed.response.clocks.len(), 1);
    assert_eq!(changed.response.clocks[&threads[42]][&a.id], 4);
    assert!(
        changed.response_bytes < 4 * 1024,
        "{}",
        changed.response_bytes
    );
    let held = lock(&b.inner().peers)[&a.id].clocks[&threads[42]][&a.id];
    assert_eq!(held, 4);

    // The same state as an older member's full exchange.
    let old = Node::new("old").legacy();
    round(&old, &a);
    let full = a.inner().build_exchange(&old.id).unwrap();
    let full_bytes = serde_json::to_vec(&full).unwrap().len();
    let mut unbounded = full.clone();
    unbounded.threads = sync::registrations(a.inner()).unwrap();
    unbounded.clocks = sync::clocks(a.inner()).unwrap();
    let unbounded_bytes = serde_json::to_vec(&unbounded).unwrap().len();

    eprintln!(
        "swarm delta exchange, 5000 threads: join {} rounds in {:?}, largest frame {} bytes, \
         frames {:?}; idle {} + {} bytes; one changed thread {} + {} bytes in {:?}; \
         full state {} bytes ({} bytes before the frame cap)",
        join.len(),
        join_time,
        largest,
        join.iter()
            .map(|round| (round.request_bytes, round.response_bytes))
            .collect::<Vec<_>>(),
        idle.request_bytes,
        idle.response_bytes,
        changed.request_bytes,
        changed.response_bytes,
        changed_time,
        full_bytes,
        unbounded_bytes,
    );
}

#[test]
fn a_restarted_peer_and_a_drifted_digest_get_every_clock_again() {
    let a = Node::new("a");
    let b = Node::new("b");
    let threads = a.seed_threads(50);
    settle(&b, &a);
    assert_eq!(lock(&b.inner().peers)[&a.id].clocks.len(), 50);

    // B forgets what it knew of A, as a restart does: its next exchange
    // acknowledges nothing, and A sends every clock again.
    lock(&b.inner().peers).clear();
    settle(&b, &a);
    assert_eq!(lock(&b.inner().peers)[&a.id].clocks.len(), 50);

    // B's copy drifts. A's digest exposes it and B asks for every clock.
    lock(&b.inner().peers)
        .get_mut(&a.id)
        .unwrap()
        .clocks
        .remove(&threads[0]);
    lock(&a.inner().peers).get_mut(&b.id).unwrap().digest_at = None;
    settle(&b, &a);
    assert!(!lock(&b.inner().peers)[&a.id]
        .clocks
        .contains_key(&threads[0]));
    settle(&b, &a);
    assert!(lock(&b.inner().peers)[&a.id]
        .clocks
        .contains_key(&threads[0]));
}

#[test]
fn an_older_member_links_through_the_full_state() {
    let a = Node::new("a");
    let old = Node::new("old").legacy();
    let c = Node::new("c");
    let scout = a.agent("Scout");
    let writer = old.agent("Writer");
    let critic = c.agent("Critic");
    let thread = uuid::Uuid::now_v7().to_string();
    let group = Group {
        group_id: uuid::Uuid::now_v7().to_string(),
        name: "Launch room".into(),
        members: vec![scout.agent_id.clone(), writer.agent_id.clone()],
        lead: scout.agent_id.clone(),
        meeting_mode: false,
        debate_rounds: 2,
        owner_runtime: old.id.clone(),
        thread_id: thread,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: String::new(),
        deleted: false,
    };
    assert!(groups::put(old.inner(), &group).unwrap());

    // The older member dials A and C dials A: a chain through A.
    let first = round(&old, &a);
    assert!(first.request.record_vv.is_none() && first.request.records.is_empty());
    assert!(first.request.from.capabilities.is_empty());
    assert!(first.response.record_vv.is_none() && first.response.records.is_empty());
    assert!(!first.response.directory.is_empty());
    for _ in 0..3 {
        settle(&old, &a);
        settle(&c, &a);
    }
    let all = vec!["Critic".to_owned(), "Scout".to_owned(), "Writer".to_owned()];
    for node in [&a, &old, &c] {
        assert_eq!(node.names(), all);
        assert_eq!(node.group_names(), vec!["Launch room".to_owned()]);
    }
    // C speaks records to A; A speaks the full state to the older member.
    assert!(lock(&a.inner().peers)[&c.id].stats.store);
    assert!(!lock(&a.inner().peers)[&old.id].stats.store);

    // An edit on C reaches the older member through A, and the older
    // member's rename reaches C. The full state keeps times in seconds, so
    // each edit lands in a later second.
    let next_second = || std::thread::sleep(std::time::Duration::from_millis(1100));
    next_second();
    let mut renamed = critic.clone();
    renamed.name = "Lead critic".into();
    directory::put_entry(c.inner(), &renamed, None).unwrap();
    let mut moved = group.clone();
    moved.name = "War room".into();
    groups::put(old.inner(), &moved).unwrap();
    for _ in 0..3 {
        settle(&c, &a);
        settle(&old, &a);
    }
    for node in [&a, &old, &c] {
        assert!(
            node.names().contains(&"Lead critic".to_owned()),
            "{:?}",
            node.names()
        );
        assert_eq!(node.group_names(), vec!["War room".to_owned()]);
    }
    // A deletion on C leaves every directory.
    next_second();
    let mut gone = renamed;
    gone.deleted = true;
    directory::put_entry(c.inner(), &gone, None).unwrap();
    for _ in 0..3 {
        settle(&c, &a);
        settle(&old, &a);
    }
    for node in [&a, &old, &c] {
        assert!(!node.names().contains(&"Lead critic".to_owned()));
    }
}
