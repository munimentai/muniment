//! Exchanges between in-process members, without a network: thread clock
//! deltas at scale, the fallback to an older member's full state, ordinary
//! threads and their offline window, bootstrap progress, tombstone
//! collection and a member that comes back after 100 days.

use std::sync::Arc;

use rusqlite::params;
use serde_json::json;

use super::*;

/// A host with no runs. With a scope, replica threads land in its journal.
struct TestHost(Option<ReplicaScope>);

impl SwarmHost for TestHost {
    fn replica_scope(&self) -> Option<ReplicaScope> {
        self.0.clone()
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
        Self::with_host(name, TestHost(None))
    }

    /// A member whose journal takes replica threads.
    fn scoped(name: &str) -> Self {
        Self::with_host(
            name,
            TestHost(Some(ReplicaScope {
                workspace: "/workspace".into(),
                attach_profile: "test-profile".into(),
                subject: None,
            })),
        )
    }

    fn with_host(name: &str, host: TestHost) -> Self {
        let root =
            std::env::temp_dir().join(format!("swarm-exchange-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let storage = Arc::new(Mutex::new(crate::run_events::ChatStorage {
            journal: crate::journal::RunJournal::open(root.join("runs.sqlite3")).unwrap(),
            cas: crate::cas::LocalCas::open(&root.join("cas")).unwrap(),
        }));
        let swarm = Swarm::open(&root, storage, Arc::new(host)).unwrap();
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
                owner_runtime: None,
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
            move_suggested: false,
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

fn provenance() -> crate::journal::Provenance {
    let mut extra = std::collections::BTreeMap::new();
    extra.insert("attach_profile".into(), json!("test-profile"));
    crate::journal::Provenance {
        source: "test".into(),
        source_version: "1".into(),
        actor_id: None,
        device_id: None,
        rpc_request_id: None,
        capability_versions: None,
        extra,
    }
}

impl Node {
    /// Creates an ordinary thread this member owns, titled, with `posts`
    /// posted in it, and indexes it.
    fn plain_thread(&self, title: &str, posts: &[&str]) -> String {
        let thread = uuid::Uuid::now_v7().to_string();
        {
            let mut storage = self.inner().storage.lock().unwrap();
            let at = self.inner().now_text();
            storage
                .journal
                .create_thread_with_id(&thread, "/workspace", &at, provenance())
                .unwrap();
            storage
                .journal
                .append_thread_title_renamed(1, &thread, title, &at, &provenance())
                .unwrap();
        }
        assert!(sync::ensure_registration(
            self.inner(),
            &ThreadRegistration {
                thread_id: thread.clone(),
                kind: sync::PLAIN.into(),
                agent_id: None,
                group_id: None,
                updated_at: self.inner().now_text(),
                owner_runtime: Some(self.id.clone()),
            },
        )
        .unwrap());
        // Agent posts, which the owner keeps without running them.
        for post in posts {
            sync::post_as(self.inner(), &thread, json!({"kind": "agent"}), post).unwrap();
        }
        sync::scan_local(self.inner()).unwrap();
        thread
    }

    fn posts(&self, thread: &str) -> Vec<String> {
        sync::posted_messages(&self.inner().storage, thread, 100)
            .unwrap_or_default()
            .into_iter()
            .map(|post| post["text"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// The titles of the threads this member's journal lists.
    fn listed(&self) -> Vec<String> {
        let mut storage = self.inner().storage.lock().unwrap();
        let mut titles: Vec<String> = storage
            .journal
            .thread_summaries(100, None)
            .unwrap()
            .summaries
            .into_iter()
            .map(|summary| summary.title)
            .collect();
        titles.sort();
        titles
    }

    fn op(&self, operation: Operation, body: Value) -> Value {
        self.swarm.operation(operation, body, None).unwrap()
    }

    fn note(&self, key: &str, value: Value) {
        self.swarm
            .records()
            .put("note", key, "text", value)
            .unwrap();
    }

    fn note_live(&self, key: &str) -> bool {
        self.swarm
            .records()
            .get("note", key)
            .unwrap()
            .is_some_and(|record| record.is_live())
    }

    fn tombstones(&self, class: &str) -> u64 {
        self.inner()
            .records
            .counts()
            .unwrap()
            .get(class)
            .map_or(0, |count| count.tombstones)
    }

    /// Forgets what it knew of its peers, as a restart does.
    fn restart(&self) {
        lock(&self.inner().peers).clear();
    }
}

fn percent(status: &Value, class: &str) -> u64 {
    status["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["class"] == class)
        .and_then(|entry| entry["percent"].as_u64())
        .unwrap_or(100)
}

#[test]
fn an_ordinary_thread_older_than_the_window_arrives_on_demand() {
    let a = Node::scoped("a");
    let b = Node::scoped("b");
    // An old thread from 40 days ago, then a recent one.
    a.swarm.advance_clock(chrono::Duration::days(-40));
    let old = a.plain_thread("Trip plan", &["Book the train.", "Pack light."]);
    a.swarm.advance_clock(chrono::Duration::days(40));
    let recent = a.plain_thread("Weekly review", &["Draft the summary."]);
    settle(&b, &a);
    settle(&b, &a);

    // B lists both threads with their titles and owner, and holds the
    // history of the recent one only.
    assert_eq!(
        b.listed(),
        vec!["Trip plan".to_owned(), "Weekly review".to_owned()]
    );
    assert_eq!(b.swarm.thread_owner(&old), Some(a.id.clone()));
    assert_eq!(b.swarm.is_replica_thread(&old), Some(a.id.clone()));
    assert_eq!(b.posts(&recent), vec!["Draft the summary.".to_owned()]);
    assert!(b.posts(&old).is_empty());
    let summary = sync::summary(b.inner(), &old).unwrap();
    assert_eq!(summary.title, "Trip plan");
    assert_eq!(summary.last_post.unwrap().text, "Pack light.");

    // A post on B reaches A, which holds it under B's origin.
    sync::post(b.inner(), &recent, json!({"kind": "agent"}), "From B.").unwrap();
    settle(&b, &a);
    assert_eq!(
        a.posts(&recent),
        vec!["Draft the summary.".to_owned(), "From B.".to_owned()]
    );

    // A fetch pins the old thread: B says it is fetching, then holds it.
    let fetch = json!({"thread_id": old});
    let first = b.op(Operation::ThreadHistoryFetch, fetch.clone());
    assert_eq!(first["status"], "fetching");
    assert_eq!(first["owner_runtime"], a.id.as_str());
    settle(&b, &a);
    settle(&b, &a);
    assert_eq!(
        b.posts(&old),
        vec!["Book the train.".to_owned(), "Pack light.".to_owned()]
    );
    assert_eq!(b.op(Operation::ThreadHistoryFetch, fetch)["status"], "held");
    // The owner holds its own thread.
    assert_eq!(
        a.op(Operation::ThreadHistoryFetch, json!({"thread_id": recent}))["status"],
        "held"
    );

    // A wider window takes every thread without a fetch.
    let c = Node::scoped("c");
    c.op(Operation::FleetOfflineSet, json!({"thread_days": 0}));
    settle(&c, &a);
    settle(&c, &a);
    assert_eq!(c.posts(&old).len(), 2);
}

#[test]
fn a_fresh_member_bootstraps_with_progress_per_class() {
    let a = Node::scoped("a");
    for index in 0..4000 {
        a.note(&format!("note-{index:05}"), json!("x".repeat(120)));
    }
    let threads: Vec<String> = (0..40)
        .map(|index| a.plain_thread(&format!("Thread {index}"), &["Hello."]))
        .collect();
    a.agent("Scout");

    let b = Node::scoped("b");
    let events = b.swarm.subscribe();
    let mut statuses = Vec::new();
    for round in 0..60 {
        let round = round_once(&b, &a, round);
        statuses.push(b.op(Operation::FleetSyncStatus, json!({})));
        if !round {
            break;
        }
    }
    // The first status shows the records part way, by class.
    let first = &statuses[0];
    assert_eq!(first["state"], "bootstrapping", "{first}");
    assert!(percent(first, "note") < 100, "{first}");
    let last = statuses.last().unwrap();
    for class in ["note", "thread", "agent", "history"] {
        assert_eq!(percent(last, class), 100, "{class}: {last}");
    }
    assert_eq!(last["state"], "synced", "{last}");
    assert_eq!(b.posts(&threads[7]), vec!["Hello.".to_owned()]);
    // The events: progress while it ran, and the end once the throttle let
    // the last one through.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    b.inner().tick().unwrap();
    let changes: Vec<Value> = events
        .try_iter()
        .filter(|event| event.name == muniment_attach::EventName::FleetSyncChanged)
        .map(|event| event.body)
        .collect();
    assert!(changes
        .first()
        .is_some_and(|change| change["state"] == "bootstrapping"));
    assert_eq!(changes.last().unwrap()["state"], "synced", "{changes:?}");
    eprintln!(
        "bootstrap: {} rounds; statuses {:?}",
        statuses.len(),
        statuses
            .iter()
            .map(|status| (
                percent(status, "note"),
                percent(status, "thread"),
                percent(status, "history")
            ))
            .collect::<Vec<_>>()
    );
}

/// Moves every clock in `nodes` on by `days`, with the `links` exchanging
/// every 30 days, as members in touch do.
fn pass_days(nodes: &[&Node], links: &[(&Node, &Node)], days: i64) {
    let mut left = days;
    while left > 0 {
        let step = left.min(30);
        for node in nodes {
            node.swarm.advance_clock(chrono::Duration::days(step));
        }
        for (dialer, answerer) in links {
            settle(dialer, answerer);
        }
        left -= step;
    }
}

/// One round between two members. Answers whether either left items out.
fn round_once(dialer: &Node, answerer: &Node, _round: usize) -> bool {
    let round = round(dialer, answerer);
    round.request.more || round.response.more
}

#[test]
fn covered_tombstones_older_than_90_days_go() {
    let a = Node::scoped("a");
    let b = Node::scoped("b");
    a.note("gone", json!("soon"));
    a.note("kept", json!("stays"));
    settle(&b, &a);
    a.swarm.records().delete("note", "gone").unwrap();
    settle(&b, &a);
    assert_eq!(a.tombstones("note"), 1);
    // Young tombstones stay.
    assert_eq!(a.swarm.collect_now().unwrap(), 0);

    // 91 days later B is still in touch, but a member A heard from lately
    // has not seen the delete: the tombstone stays.
    pass_days(&[&a, &b], &[(&b, &a)], 91);
    let lagging = MemberRecord {
        runtime_id: uuid::Uuid::now_v7().to_string(),
        runtime_name: "lagging".into(),
        heartbeat_at: a.inner().now_text(),
        updated_at: a.inner().now_text(),
        record_vv: Some(VersionVector::new()),
        ..MemberRecord::default()
    };
    a.inner()
        .store
        .put_record(
            "members",
            "runtime_id",
            "record",
            &lagging.runtime_id,
            &lagging,
        )
        .unwrap();
    assert_eq!(a.swarm.collect_now().unwrap(), 0);
    assert_eq!(a.tombstones("note"), 1);

    // Once it holds every write, the tombstone goes, and so does B's.
    let mut covering = lagging.clone();
    covering.record_vv = Some(a.inner().records.version_vector().unwrap());
    a.inner()
        .store
        .put_record(
            "members",
            "runtime_id",
            "record",
            &covering.runtime_id,
            &covering,
        )
        .unwrap();
    assert_eq!(a.swarm.collect_now().unwrap(), 1);
    assert_eq!(a.tombstones("note"), 0);
    assert!(!a.note_live("gone") && a.note_live("kept"));
    settle(&b, &a);
    assert_eq!(b.swarm.collect_now().unwrap(), 1);
    assert!(!b.note_live("gone"));
    // A write older than the horizon for a field the store no longer holds
    // is refused.
    let old_row = RecordRow {
        class: "note".into(),
        key: "gone".into(),
        field: "text".into(),
        value: Some(json!("soon")),
        clock: Hlc::at(0),
        origin: b.id.clone(),
        seq: 1,
        rank: 0,
        deleted: false,
    };
    a.inner()
        .records
        .merge(a.inner().now(), vec![old_row])
        .unwrap();
    assert!(!a.note_live("gone"));
}

#[test]
fn a_member_back_after_100_days_resurrects_nothing() {
    let a = Node::scoped("a");
    let b = Node::scoped("b");
    let d = Node::scoped("d");
    d.note("x", json!("written on d"));
    a.note("y", json!("written on a"));
    let thread = a.plain_thread("Old notes", &["Remember this."]);
    for _ in 0..2 {
        settle(&d, &a);
        settle(&b, &a);
    }
    assert!(b.note_live("x") && d.note_live("y"));
    assert!(d.listed().contains(&"Old notes".to_owned()));

    // D goes away. A deletes D's note and its own thread.
    a.swarm.records().delete("note", "x").unwrap();
    {
        let mut storage = a.inner().storage.lock().unwrap();
        let last = storage.journal.last_thread_seq(&thread).unwrap();
        storage
            .journal
            .append_thread_deleted(last, &thread, &a.inner().now_text(), &provenance())
            .unwrap();
    }
    sync::scan_local(a.inner()).unwrap();
    settle(&b, &a);
    assert!(!b.note_live("x"));
    assert!(sync::registration(b.inner(), &thread).is_none());
    assert!(!b.listed().contains(&"Old notes".to_owned()));

    // 100 days pass. A and B stay in touch and drop the tombstones, since D
    // is stale.
    pass_days(&[&a, &b, &d], &[(&b, &a)], 100);
    let members = a.op(Operation::SwarmMembers, json!({}));
    let d_member = members["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["runtime_id"] == d.id.as_str())
        .cloned()
        .unwrap();
    assert_eq!(d_member["stale"], true);
    assert_eq!(a.swarm.collect_now().unwrap(), 2);
    assert_eq!(b.swarm.collect_now().unwrap(), 2);
    assert_eq!(a.tombstones("note") + a.tombstones(sync::CLASS), 0);

    // D writes while still away, restarts and comes back.
    d.note("z", json!("written away"));
    d.restart();
    for _ in 0..3 {
        settle(&d, &a);
        settle(&b, &a);
    }
    for node in [&a, &b, &d] {
        assert!(!node.note_live("x"), "{} holds x", node.root.display());
        assert!(node.note_live("y") && node.note_live("z"));
        assert!(sync::registration(node.inner(), &thread).is_none());
        assert!(!node.listed().contains(&"Old notes".to_owned()));
    }
    let status = d.op(Operation::FleetSyncStatus, json!({}));
    assert!(status["bootstrapped_at"].is_string(), "{status}");
    assert!(status["snapshot_needed_at"].is_null(), "{status}");
}

impl Node {
    /// Writes `count` ordinary threads this member owns, each with
    /// `posts` agent posts, straight into the journal, and registers them.
    fn seed_journal_threads(&self, count: usize, posts: usize) -> Vec<String> {
        let threads: Vec<String> = (0..count)
            .map(|_| uuid::Uuid::now_v7().to_string())
            .collect();
        let at = self.inner().now_text();
        let envelope = |thread: &str, seq: u64, event_type: &str, payload: Value| {
            let id = uuid::Uuid::now_v7().to_string();
            let mut value = serde_json::to_value(crate::journal::ThreadEventEnvelope::new(
                &id,
                thread,
                event_type,
                &at,
                payload,
                provenance(),
            ))
            .unwrap();
            value["thread_seq"] = json!(seq);
            (id, value.to_string())
        };
        let mut journal = rusqlite::Connection::open(self.root.join("runs.sqlite3")).unwrap();
        let transaction = journal.transaction().unwrap();
        for thread in &threads {
            let mut rows = vec![(
                1,
                "thread.created",
                envelope(
                    thread,
                    1,
                    "thread.created",
                    json!({"migration_backfill": false, "workspace": "/workspace"}),
                ),
            )];
            for index in 0..posts {
                let message = uuid::Uuid::now_v7().to_string();
                rows.push((
                    index as u64 + 2,
                    THREAD_MESSAGE_POSTED_TYPE,
                    envelope(
                        thread,
                        index as u64 + 2,
                        THREAD_MESSAGE_POSTED_TYPE,
                        json!({
                            "message_id": message,
                            "author": {"kind": "agent"},
                            "text": format!("Step {index} of the plan."),
                            "deps": {},
                        }),
                    ),
                ));
            }
            for (seq, event_type, (id, json)) in rows {
                transaction
                    .execute(
                        "INSERT INTO thread_events(event_id,thread_id,thread_seq,event_type,\
                         event_version,envelope_version,recorded_at,envelope_json) \
                         VALUES(?1,?2,?3,?4,1,1,?5,?6)",
                        params![id, thread, seq, event_type, at, json],
                    )
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
        let mut store = self.inner().store.connect().unwrap();
        let transaction = store.transaction().unwrap();
        for thread in &threads {
            let registration = ThreadRegistration {
                thread_id: thread.clone(),
                kind: sync::PLAIN.into(),
                agent_id: None,
                group_id: None,
                updated_at: at.clone(),
                owner_runtime: Some(self.id.clone()),
            };
            transaction
                .execute(
                    "INSERT INTO threads(thread_id, record) VALUES(?1, ?2)",
                    params![thread, serde_json::to_string(&registration).unwrap()],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        sync::migrate(self.inner()).unwrap();
        threads
    }
}

const THREAD_MESSAGE_POSTED_TYPE: &str = crate::journal::replica::THREAD_MESSAGE_POSTED;

fn timed<T>(work: impl FnOnce() -> T) -> (T, std::time::Duration) {
    let started = std::time::Instant::now();
    let value = work();
    (value, started.elapsed())
}

#[test]
fn ordinary_threads_at_5000_with_50000_events_cost_little_per_tick() {
    let a = Node::scoped("a");
    let count = 5000;
    let threads = a.seed_journal_threads(count, 10);
    // The first pass indexes every event and writes every summary.
    let ((), first) = timed(|| a.inner().tick().unwrap());
    let events: i64 = a
        .inner()
        .store
        .connect()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(events as usize, count * 10);
    assert_eq!(sync::summaries(a.inner()).unwrap().len(), count);
    // Idle passes read only what changed.
    let idle: Vec<std::time::Duration> = (0..5)
        .map(|_| timed(|| a.inner().tick().unwrap()).1)
        .collect();
    // A full pass, as every pass was before.
    sync::rescan(a.inner());
    let ((), full) = timed(|| a.inner().tick().unwrap());

    // B holds A's events already and joins: records and clocks in pages.
    let b = Node::new("b");
    b.copy_log(&a);
    let (join, join_time) = timed(|| settle(&b, &a));
    let largest = join
        .iter()
        .map(|round| round.request_bytes.max(round.response_bytes))
        .max()
        .unwrap();
    let total: usize = join
        .iter()
        .map(|round| round.request_bytes + round.response_bytes)
        .sum();
    assert!(largest <= FRAME_BUDGET, "a frame of {largest} bytes");
    assert_eq!(sync::registrations(b.inner()).unwrap().len(), count);

    // Idle exchanges carry no records, clocks or events.
    let (idle_round, idle_time) = timed(|| round(&b, &a));
    assert!(idle_round.request.records.is_empty() && idle_round.response.records.is_empty());
    assert!(idle_round.response.clocks.is_empty() && idle_round.response.events.is_empty());

    // One new post on A: one clock and one event cross.
    {
        let mut storage = a.inner().storage.lock().unwrap();
        let message = uuid::Uuid::now_v7().to_string();
        storage
            .journal
            .append_thread_event_with_id(crate::journal::ThreadEventEnvelope::new(
                &message,
                &threads[42],
                THREAD_MESSAGE_POSTED_TYPE,
                &a.inner().now_text(),
                json!({"message_id": message, "author": {"kind": "agent"}, "text": "One more.", "deps": {}}),
                provenance(),
            ))
            .unwrap();
    }
    let ((), changed_tick) = timed(|| a.inner().tick().unwrap());
    let (changed, changed_time) = timed(|| round(&b, &a));
    assert_eq!(changed.response.clocks.len(), 1);
    assert_eq!(changed.response.events.len(), 1);

    eprintln!(
        "fleet scale, 5000 ordinary threads and 50000 events: first pass {first:?}; \
         idle passes {idle:?}; full pass {full:?}; pass after one post {changed_tick:?}; \
         join {} rounds in {join_time:?}, largest frame {largest} bytes, {total} bytes in all; \
         idle round {idle_time:?}, {} + {} bytes; one post {changed_time:?}, {} + {} bytes",
        join.len(),
        idle_round.request_bytes,
        idle_round.response_bytes,
        changed.request_bytes,
        changed.response_bytes,
    );
}
