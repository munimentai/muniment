//! Fleet keys, the roster and sealed secrets between in-process members,
//! without a network: forged members, relays, revocation, the model vault
//! token path and a sign-in handover.

use std::sync::Arc;

use serde_json::{json, Value};

use super::fleet::{self, Role};
use super::*;
use crate::attach::IdentitySigner;
use crate::model_router::config::{self, Account, Credential};
use crate::shared_models::{self, CloudSession};
use muniment_models::pi_models;

pub(super) struct FleetHost {
    pub(super) session: Option<CloudSession>,
    /// With a scope, replica threads land in the member's journal, and every
    /// thread has a live run that takes steering input.
    pub(super) scope: Option<ReplicaScope>,
    /// The steering input the live runs took, as (thread, text).
    pub(super) steered: Mutex<Vec<(String, String)>>,
}

impl SwarmHost for FleetHost {
    fn replica_scope(&self) -> Option<ReplicaScope> {
        self.scope.clone()
    }

    fn steer(&self, thread_id: &str, text: &str) -> Option<String> {
        self.scope.as_ref()?;
        let mut steered = lock(&self.steered);
        steered.push((thread_id.to_owned(), text.to_owned()));
        Some(format!("run-steered-{}", steered.len()))
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

    fn cloud_session(&self) -> Option<CloudSession> {
        self.session.clone()
    }
}

pub(super) struct Node {
    pub(super) swarm: Swarm,
    pub(super) id: String,
    pub(super) root: PathBuf,
    pub(super) cert: String,
    pub(super) host: Arc<FleetHost>,
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Node {
    pub(super) fn new(name: &str) -> Self {
        Self::with_session(name, None)
    }

    pub(super) fn with_session(name: &str, session: Option<CloudSession>) -> Self {
        Self::with_host(
            name,
            FleetHost {
                session,
                scope: None,
                steered: Mutex::default(),
            },
        )
    }

    /// A member whose journal takes replica threads and whose threads each
    /// have a live run, as a desktop or a runtime that runs Pi holds them.
    pub(super) fn scoped(name: &str) -> Self {
        Self::with_host(
            name,
            FleetHost {
                session: None,
                scope: Some(ReplicaScope {
                    workspace: "/workspace".into(),
                    attach_profile: "test-profile".into(),
                    subject: None,
                }),
                steered: Mutex::default(),
            },
        )
    }

    fn with_host(name: &str, host: FleetHost) -> Self {
        let root =
            std::env::temp_dir().join(format!("swarm-fleet-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let storage = Arc::new(Mutex::new(crate::run_events::ChatStorage {
            journal: crate::journal::RunJournal::open(root.join("runs.sqlite3")).unwrap(),
            cas: crate::cas::LocalCas::open(&root.join("cas")).unwrap(),
        }));
        let host = Arc::new(host);
        let swarm = Swarm::open(&root, storage, Arc::clone(&host) as Arc<dyn SwarmHost>).unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        *lock(&swarm.inner.identity) = Some(Identity {
            access: None,
            runtime_id: id.clone(),
            runtime_name: name.into(),
            runtime_version: "1.0.0".into(),
            form_factor: None,
        });
        swarm.inner.store.set_meta("runtime_id", Some(&id)).unwrap();
        let signer = IdentitySigner::generate();
        let cert = signer.certificate_sha256().to_owned();
        swarm.set_identity_signer(signer);
        Self {
            swarm,
            id,
            root,
            cert,
            host,
        }
    }

    pub(super) fn inner(&self) -> &Inner {
        &self.swarm.inner
    }

    pub(super) fn agent(&self) -> PathBuf {
        crate::state_root::agent_directory(&self.root)
    }

    pub(super) fn auth(&self) -> serde_json::Map<String, Value> {
        pi_models::read_json(&self.agent().join("auth.json")).unwrap()
    }

    pub(super) fn write_auth(&self, auth: Value) {
        std::fs::create_dir_all(self.agent()).unwrap();
        pi_models::write_json(&self.agent().join("auth.json"), auth.as_object().unwrap()).unwrap();
    }

    pub(super) fn share(&self) {
        credentials::credentials_operation(self.inner(), json!({"share_models": true})).unwrap();
    }

    pub(super) fn pass(&self) {
        self.swarm.fleet_pass().unwrap();
    }

    pub(super) fn view(&self) -> Arc<fleet::View> {
        fleet::view(self.inner()).unwrap()
    }

    pub(super) fn sealed(&self, secret_id: &str) -> Option<SealedSecret> {
        fleet::sealed(self.inner(), secret_id).unwrap()
    }

    pub(super) fn open(&self, secret: &SealedSecret) -> Result<Option<Value>, String> {
        let keys = fleet::keys(self.inner()).unwrap();
        fleet::open(&keys, &self.id, secret)
            .map(|bytes| bytes.map(|bytes| serde_json::from_slice(&bytes).unwrap()))
    }

    /// The audit entries of `kind` this member wrote, oldest first.
    pub(super) fn audit(&self, kind: &str) -> Vec<Value> {
        let mut entries: Vec<Value> = audit::entries(self.inner())
            .unwrap()
            .into_iter()
            .filter(|entry| entry.kind == kind && entry.actor_runtime == self.id)
            .map(|entry| entry.record)
            .collect();
        entries.reverse();
        entries
    }
}

/// Carries an exchange through JSON, as a link does.
pub(super) fn wire(exchange: &Exchange) -> Exchange {
    serde_json::from_slice(&serde_json::to_vec(exchange).unwrap()).unwrap()
}

/// One exchange from `dialer` to `answerer` and back. The dialer pinned the
/// answerer's certificate, so it vouches for the card the answer carries.
/// Answers what the answerer sent back.
pub(super) fn round(dialer: &Node, answerer: &Node) -> Exchange {
    fleet::note_pinned(&answerer.id);
    let request = wire(&dialer.inner().build_exchange(&answerer.id).unwrap());
    fleet::note_device(&dialer.id, &format!("device-{}", dialer.id));
    answerer
        .inner()
        .merge_exchange(&dialer.id, request)
        .unwrap();
    let answer = wire(&answerer.inner().build_exchange(&dialer.id).unwrap());
    fleet::note_pinned(&answerer.id);
    fleet::vouch_for_link(dialer.inner(), &answerer.id, &answer.from, &answerer.cert).unwrap();
    dialer
        .inner()
        .merge_exchange(&answerer.id, answer.clone())
        .unwrap();
    answer
}

/// Runs a few rounds over each link so everything relays end to end.
pub(super) fn settle_links(links: &[(&Node, &Node)]) {
    for _ in 0..4 {
        for (dialer, answerer) in links {
            round(dialer, answerer);
        }
        for (dialer, _) in links {
            dialer.swarm.tick().unwrap();
        }
    }
}

/// `joiner` redeems an invitation `inviter` issued with `role`.
pub(super) fn join(joiner: &Node, inviter: &Node, role: Role) {
    let secret = uuid::Uuid::new_v4().simple().to_string();
    fleet::note_invite(inviter.inner(), &secret, role, "2999-01-01T00:00:00Z").unwrap();
    fleet::note_join(joiner.inner(), &inviter.id, &secret).unwrap();
    round(joiner, inviter);
    round(joiner, inviter);
}

pub(super) fn api_key(key: &str) -> Value {
    json!({"type": "api_key", "key": key})
}

#[test]
fn a_join_vouches_both_ways_and_records_the_role() {
    let a = Node::new("a");
    let b = Node::new("b");
    let w = Node::new("w");
    join(&b, &a, Role::Full);
    join(&w, &a, Role::Worker);
    round(&b, &a);
    let view = b.view();
    assert_eq!(view.role(&a.id), Some(Role::Full));
    assert_eq!(view.role(&w.id), Some(Role::Worker));
    assert_eq!(a.view().role(&b.id), Some(Role::Full));
    // The worker's own view takes its role from the inviter's vouch.
    assert_eq!(w.view().role(&w.id), Some(Role::Worker));
    assert!(!fleet::may_invite(w.inner()).unwrap());
    // An admission is spent: the joiner stops presenting it.
    assert!(b
        .inner()
        .store
        .meta(&format!("fleet_admission:{}", a.id))
        .unwrap()
        .is_none());
    // A secret for agents a worker runs may reach it; sealing waits for S5.
    let set = fleet::recipient_set(a.inner(), &[]).unwrap();
    assert_eq!(set.full, vec![b.id.clone()]);
    assert!(set.workers.is_empty());
}

#[test]
fn a_forged_member_gets_no_ciphertext_and_a_relay_cannot_open_another_copy() {
    let a = Node::new("a");
    let b = Node::new("b");
    let c = Node::new("c");
    let m = Node::new("m");
    join(&b, &a, Role::Full);
    join(&c, &b, Role::Full);
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    a.share();

    // The forged member links to A without an invitation and claims C's id
    // with its own card, a vouch it signed itself and one it says A signed.
    let mut forged = wire(&m.inner().build_exchange(&a.id).unwrap());
    let mut claim = forged.from.clone();
    claim.runtime_id = c.id.clone();
    claim.updated_at = "2999-01-01T00:00:00Z".into();
    forged.members.push(claim);
    let m_keys = fleet::keys(m.inner()).unwrap();
    let fake = fleet::sign_entry(
        &m_keys,
        RosterEntry {
            entry_id: String::new(),
            kind: "add".into(),
            runtime_id: m.id.clone(),
            card: Some(m_keys.card.clone()),
            role: Some("full".into()),
            signer: a.id.clone(),
            clock: Hlc::at(i64::MAX / 4),
            sig: String::new(),
        },
    )
    .unwrap();
    forged.roster.push(fake);
    a.inner().merge_exchange(&m.id, forged).unwrap();

    settle_links(&[(&b, &a), (&c, &b)]);
    a.pass();
    settle_links(&[(&b, &a), (&c, &b)]);

    let models = a.sealed(&format!("models:{}", a.id)).unwrap();
    let recipients: Vec<&String> = models.recipients.keys().collect();
    let mut expected = vec![&b.id, &c.id];
    expected.sort();
    assert_eq!(recipients, expected);
    assert!(!a.view().trusts(&m.id));
    // A sends the forged member no sealed secret at all.
    let answer = wire(&a.inner().build_exchange(&m.id).unwrap());
    assert!(answer.sealed.is_empty());
    assert!(m.open(&models).unwrap().is_none());

    // The forged member's device claims C's id and replays C's card and
    // record: A still sends that device no sealed secret.
    let mut replay = wire(&c.inner().build_exchange(&a.id).unwrap());
    replay.sealed_have = Some(Default::default());
    fleet::note_device(&c.id, &format!("device-{}", m.id));
    a.inner().merge_exchange(&c.id, replay).unwrap();
    assert!(wire(&a.inner().build_exchange(&c.id).unwrap())
        .sealed
        .is_empty());

    // C got A's connections through B, which relayed them.
    assert_eq!(c.auth()["openai"]["key"], "sk-from-a");
    assert_eq!(b.auth()["openai"]["key"], "sk-from-a");
    let relayed = b.sealed(&format!("models:{}", a.id)).unwrap();
    assert_eq!(relayed, models);
    // B holds C's copy and cannot open it.
    let keys = fleet::keys(b.inner()).unwrap();
    assert!(fleet::open(&keys, &c.id, &relayed).is_err());

    // A copy moved to another recipient's slot or a changed header fails.
    let mut swapped = models.clone();
    let copy = swapped.recipients[&c.id].clone();
    swapped.recipients.insert(b.id.clone(), copy);
    assert!(b.open(&swapped).is_err());
    let mut bumped = models.clone();
    bumped.version += 1;
    assert!(c.open(&bumped).is_err());
    // A tampered version does not verify, so no member keeps it.
    let mut carried = wire(&b.inner().build_exchange(&c.id).unwrap());
    carried.sealed = vec![bumped];
    c.inner().merge_exchange(&b.id, carried).unwrap();
    assert_eq!(c.sealed(&models.secret_id).unwrap().version, models.version);
}

#[test]
fn a_worker_vouch_and_a_removed_signer_count_for_nothing() {
    let a = Node::new("a");
    let b = Node::new("b");
    let w = Node::new("w");
    let x = Node::new("x");
    join(&b, &a, Role::Full);
    join(&w, &a, Role::Worker);
    // The worker vouches for X; no one else does.
    let card = x.inner().build_exchange(&w.id).unwrap().from.fleet.unwrap();
    let secret = uuid::Uuid::new_v4().simple().to_string();
    fleet::note_invite(w.inner(), &secret, Role::Full, "2999-01-01T00:00:00Z").unwrap();
    fleet::note_join(x.inner(), &w.id, &secret).unwrap();
    round(&x, &w);
    round(&w, &a);
    round(&b, &a);
    assert!(fleet::entries(a.inner())
        .unwrap()
        .iter()
        .any(|entry| entry.runtime_id == x.id && entry.card.as_ref() == Some(&card)));
    assert!(!a.view().trusts(&x.id));
    assert!(!b.view().trusts(&x.id));

    // A removes B. A removal B signs afterwards does not count.
    assert!(fleet::remove_member(a.inner(), &b.id).unwrap());
    round(&b, &a);
    assert!(b.view().self_removed());
    assert!(fleet::remove_member(b.inner(), &a.id).unwrap());
    round(&b, &a);
    assert!(a.view().removed.contains(&b.id));
    assert!(!a.view().removed.contains(&a.id));
}

#[test]
fn a_revocation_reseals_without_the_member_and_its_copies_go_stale() {
    let a = Node::new("a");
    let b = Node::new("b");
    let c = Node::new("c");
    let d = Node::new("d");
    join(&b, &a, Role::Full);
    join(&c, &a, Role::Full);
    // C alone vouches for D.
    join(&d, &c, Role::Full);
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    a.share();
    let links = [(&b, &a), (&c, &a), (&d, &c)];
    settle_links(&links);
    a.pass();
    settle_links(&links);
    let models_id = format!("models:{}", a.id);
    assert_eq!(a.sealed(&models_id).unwrap().version, 1);
    assert_eq!(c.sealed(&models_id).unwrap().version, 1);
    assert_eq!(c.auth()["openai"]["key"], "sk-from-a");
    assert!(a.view().trusts(&d.id));

    assert!(fleet::remove_member(a.inner(), &c.id).unwrap());
    a.swarm.tick().unwrap();
    a.pass();
    settle_links(&links);

    let current = a.sealed(&models_id).unwrap();
    assert_eq!(current.version, 2);
    assert!(!current.recipients.contains_key(&c.id));
    // D keeps its place through A's fresh vouch, so the new version names
    // it, though its one link ran through C.
    assert!(a.view().trusts(&d.id));
    assert!(b.view().trusts(&d.id));
    assert!(fleet::entries(a.inner())
        .unwrap()
        .iter()
        .any(|entry| { entry.kind == "add" && entry.signer == a.id && entry.runtime_id == d.id }));
    assert!(current.recipients.contains_key(&d.id));
    assert_eq!(b.sealed(&models_id).unwrap().version, 2);
    // C never receives version 2; it learns of its removal and drops what
    // it held.
    assert!(c.view().self_removed());
    assert!(c.sealed(&models_id).is_none_or(|held| held.version < 2));
    assert!(!c.auth().contains_key("openai"));
    assert!(c.open(&current).unwrap().is_none());
    // Each member logs what C held, to rotate at the provider.
    let revoked = a.audit("revoked");
    assert_eq!(revoked.len(), 1);
    assert!(revoked[0]["keys"]
        .as_array()
        .unwrap()
        .contains(&json!("pi:openai")));
    assert!(!b.audit("revoked").is_empty());
}

#[test]
fn a_desktop_issues_each_full_member_a_vault_token_and_revokes_it_with_the_member() {
    let token = format!("mvt_{}", "b".repeat(64));
    let (base, heard) = shared_models::tests::vault_stub_recording(vec![
        (
            "PUT /v1/model-vault",
            json!({"contract_version": shared_models::VAULT_CONTRACT, "connections": 1, "held": []}),
        ),
        (
            "POST /v1/model-vault/machine-tokens",
            json!({"contract_version": shared_models::VAULT_CONTRACT, "pairing_id": "pairing-b", "token": token, "limit": 5, "count": 2}),
        ),
        (
            "POST /v1/model-vault/leases",
            json!({"contract_version": shared_models::VAULT_CONTRACT, "bundle": {"pi": {"openai": api_key("vault-key")}, "router": []}, "expires_at": null}),
        ),
        (
            "DELETE /v1/runtime-pairings",
            json!({"contract_version": shared_models::RUNTIME_PAIRING_CONTRACT, "revoked": true}),
        ),
    ]);
    let session = CloudSession {
        base_url: base,
        bearer: "native".into(),
    };
    let a = Node::with_session("desktop", Some(session.clone()));
    let b = Node::new("b");
    let w = Node::new("w");
    join(&b, &a, Role::Full);
    join(&w, &a, Role::Worker);
    // B uses connections the vault manages, so it takes a token.
    credentials::credentials_operation(b.inner(), json!({"use_vault": true})).unwrap();
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    shared_models::upload_to_vault(&a.agent(), &session, 0).unwrap();
    a.share();
    let links = [(&b, &a), (&w, &a)];
    settle_links(&links);
    a.pass();
    settle_links(&links);

    let issued: Vec<Value> = heard
        .lock()
        .unwrap()
        .iter()
        .filter(|(route, _)| route == "POST /v1/model-vault/machine-tokens")
        .map(|(_, body)| body.clone())
        .collect();
    // One token, for the full member alone; the worker gets leases through a
    // full member.
    assert_eq!(issued.len(), 1);
    assert_eq!(issued[0]["runtime_id"], b.id.as_str());
    assert_eq!(issued[0]["certificate_sha256"], b.cert.as_str());
    let vault = b.sealed(&format!("vault:{}:{}", a.id, b.id)).unwrap();
    assert_eq!(vault.recipients.keys().collect::<Vec<_>>(), vec![&b.id]);
    assert_eq!(
        b.open(&vault).unwrap().unwrap()["cloud"]["token"],
        token.as_str()
    );
    let record = std::fs::read_to_string(b.agent().join(shared_models::RECORD_FILE)).unwrap();
    assert!(record.contains(&format!("swarm:{}", a.id)) && record.contains(&token));
    assert!(w.sealed(&format!("vault:{}:{}", a.id, b.id)).is_none());

    assert!(fleet::remove_member(a.inner(), &b.id).unwrap());
    a.swarm.tick().unwrap();
    let revoked: Vec<Value> = heard
        .lock()
        .unwrap()
        .iter()
        .filter(|(route, _)| route == "DELETE /v1/runtime-pairings")
        .map(|(_, body)| body.clone())
        .collect();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0]["pairing_id"], "pairing-b");
    assert!(a.sealed(&format!("vault:{}:{}", a.id, b.id)).is_none());
    assert_eq!(a.audit("vault_token_revoked").len(), 1);
}

fn subscription(access: &str, refresh: Option<&str>) -> Account {
    Account {
        id: "work".into(),
        family: "openai".into(),
        label: "Work".into(),
        credential: Credential::Subscription {
            provider: "openai-codex".into(),
            access: access.into(),
            refresh: refresh.map(str::to_owned),
            expires_ms: Some(i64::MAX / 2),
            account_id: None,
            email: None,
            plan: None,
            renews_at_ms: None,
        },
        base_url: None,
        models: Vec::new(),
        enabled: true,
        weight: 1,
        overflow: false,
    }
}

fn accounts(node: &Node) -> Vec<Account> {
    config::load(&node.agent()).unwrap().accounts
}

#[test]
fn hold_sign_in_moves_the_refresh_token_to_the_new_holder_alone() {
    let a = Node::new("a");
    let b = Node::new("b");
    join(&b, &a, Role::Full);
    std::fs::create_dir_all(a.agent()).unwrap();
    let mut settings = config::load(&a.agent()).unwrap();
    settings.accounts.push(subscription("a1", Some("r1")));
    settings.enabled = true;
    config::save(&a.agent(), &settings).unwrap();
    a.share();
    settle_links(&[(&b, &a)]);
    a.pass();
    settle_links(&[(&b, &a)]);

    // B holds a lease without the refresh token.
    let lease = accounts(&b);
    assert_eq!(lease.len(), 1);
    assert!(lease[0].id.ends_with(":work"));
    assert!(matches!(
        &lease[0].credential,
        Credential::Subscription { refresh: None, .. }
    ));

    let answer = credentials::hold_operation(b.inner(), json!({"account": "router:work"})).unwrap();
    assert_eq!(answer["status"], "requested");
    assert_eq!(answer["holder"], a.id.as_str());
    settle_links(&[(&b, &a)]);
    a.pass();
    b.pass();
    settle_links(&[(&b, &a)]);

    let held = accounts(&b);
    let own = held.iter().find(|account| account.id == "work").unwrap();
    assert!(matches!(&own.credential,
        Credential::Subscription { refresh: Some(refresh), .. } if refresh == "r1"));
    assert_eq!(held.len(), 1, "the lease from A left: {held:?}");
    // A keeps a lease from B and no refresh token.
    let leased = accounts(&a);
    assert_eq!(leased.len(), 1);
    assert!(leased[0].id.ends_with(":work") && leased[0].id.contains(&b.id));
    assert!(matches!(
        &leased[0].credential,
        Credential::Subscription { refresh: None, .. }
    ));
    assert_eq!(
        credentials::hold_operation(b.inner(), json!({"account": "router:work"})).unwrap()
            ["status"],
        "held"
    );
    assert_eq!(a.audit("hold_moved").len(), 1);
}

/// An op message `from` forwards to `to` for its actor.
fn op_message(from: &Node, to: &Node, operation: &str, body: Value) -> Message {
    Message {
        message_id: uuid::Uuid::now_v7().to_string(),
        kind: "op".into(),
        from_runtime: from.id.clone(),
        to_runtime: Some(to.id.clone()),
        from_agent: None,
        to_agent: None,
        hop: 0,
        created_at: to.inner().now_text(),
        body: json!({
            "operation": operation,
            "body": body,
            "actor_device": "local",
            "actor_scopes": ["thread.read", "run.write", "agents.manage", "runtime.admin"],
            "idempotency_key": uuid::Uuid::now_v7().to_string(),
        }),
    }
}

#[test]
fn a_thread_for_a_named_machine_never_starts_on_a_runtime_outside_a_fleet() {
    let a = Node::new("alone");
    *lock(&a.inner().identity) = None;
    let named = json!({"runtime_id": uuid::Uuid::now_v7().to_string(), "text": "Hi."});
    let refused = a
        .swarm
        .route(
            Operation::ThreadCreate,
            &named,
            &FleetActor::phone(),
            Some("req_create_0001"),
        )
        .unwrap_err();
    assert!(refused.contains("not in a fleet"), "{refused}");
    // A thread without a named machine starts here.
    assert_eq!(
        a.swarm
            .route(
                Operation::ThreadCreate,
                &json!({"text": "Hi."}),
                &FleetActor::phone(),
                Some("req_create_0002"),
            )
            .unwrap(),
        None
    );
}

#[test]
fn an_owner_refuses_admin_actions_from_a_worker_and_logs_every_action() {
    let a = Node::new("a");
    let b = Node::new("b");
    let w = Node::new("w");
    join(&b, &a, Role::Full);
    join(&w, &a, Role::Worker);
    let agent = uuid::Uuid::now_v7().to_string();
    let routine = json!({"agent_id": agent, "routine": {"name": "Nightly"}});
    let from_worker = op_message(&w, &a, "routine.save", routine.clone());
    let post_from_worker = op_message(
        &w,
        &a,
        "thread.post",
        json!({"thread_id": uuid::Uuid::now_v7().to_string(), "text": "Hi."}),
    );
    let from_full = op_message(&b, &a, "routine.save", routine);
    {
        let _merge = lock(&a.inner().merge);
        messaging::receive(
            a.inner(),
            vec![
                from_worker.clone(),
                post_from_worker.clone(),
                from_full.clone(),
            ],
        )
        .unwrap();
    }
    ops::execute_pending(a.inner()).unwrap();
    let ack = |message: &Message| -> Ack {
        a.inner()
            .store
            .record("acks", "message_id", "ack", &message.message_id)
            .unwrap()
            .unwrap()
    };
    // The worker's role lacks agents.manage, whatever scopes its actor claims.
    let refused = ack(&from_worker);
    assert_eq!(refused.status, "refused");
    assert!(refused.note.unwrap().contains("agents.manage"));
    // A post needs run.write, which a worker passes on; this host runs no
    // forwarded actions, so it fails there rather than at the scope check.
    assert!(!ack(&post_from_worker)
        .note
        .unwrap_or_default()
        .contains("scope"));
    assert!(!ack(&from_full).note.unwrap_or_default().contains("scope"));

    // Every action is in A's audit log with its actor, origin and result.
    let logged = a.audit("op");
    assert_eq!(logged.len(), 3);
    let worker_entry = logged
        .iter()
        .find(|entry| entry["op_id"] == from_worker.message_id.as_str())
        .unwrap();
    assert_eq!(worker_entry["origin"], w.id.as_str());
    assert_eq!(worker_entry["actor_device"], "local");
    assert_eq!(worker_entry["result"], "refused");

    // The log replicates to the full member and never to the worker.
    settle_links(&[(&b, &a), (&w, &a)]);
    let entries = |node: &Node| audit::entries(node.inner()).unwrap();
    assert!(entries(&b)
        .iter()
        .any(|entry| entry.actor_runtime == a.id && entry.kind == "op"));
    assert!(entries(&w).iter().all(|entry| entry.actor_runtime == w.id));

    // No member rewrites another's entry: a row that replaces one of A's,
    // or claims A's key under another origin, changes nothing on B.
    let held = entries(&b)
        .into_iter()
        .find(|entry| entry.actor_runtime == a.id)
        .unwrap();
    let row = |origin: &str, seq: u64| RecordRow {
        class: audit::CLASS.into(),
        key: held.id.clone(),
        field: "entry".into(),
        value: Some(json!({"at": held.at, "kind": "forged", "runtime_id": null, "record": {}})),
        clock: Hlc::at(i64::MAX / 4),
        origin: origin.to_owned(),
        seq,
        rank: 0,
        deleted: false,
    };
    let changed = b
        .inner()
        .records
        .merge(
            b.inner().now(),
            vec![row(&a.id, 1_000_000), row(&w.id, 1_000_000)],
        )
        .unwrap();
    assert!(changed.is_empty());
    let after = entries(&b)
        .into_iter()
        .find(|entry| entry.id == held.id)
        .unwrap();
    assert_eq!(after.kind, held.kind);
}

#[test]
fn seven_full_members_sync_and_seal_without_the_model_vault() {
    let token = format!("mvt_{}", "c".repeat(64));
    let (base, heard) = shared_models::tests::vault_stub_recording(vec![
        (
            "PUT /v1/model-vault",
            json!({"contract_version": shared_models::VAULT_CONTRACT, "connections": 1, "held": []}),
        ),
        (
            "POST /v1/model-vault/machine-tokens",
            json!({"contract_version": shared_models::VAULT_CONTRACT, "pairing_id": "p", "token": token, "limit": 5, "count": 5}),
        ),
    ]);
    let session = CloudSession {
        base_url: base,
        bearer: "native".into(),
    };
    // A desktop that holds the vault, and six more full members in a chain
    // and a star, none of which uses a connection the vault manages.
    let a = Node::with_session("desktop", Some(session.clone()));
    let others: Vec<Node> = (0..6)
        .map(|index| Node::new(&format!("m{index}")))
        .collect();
    join(&others[0], &a, Role::Full);
    for index in 1..6 {
        join(&others[index], &others[index / 2], Role::Full);
    }
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    shared_models::upload_to_vault(&a.agent(), &session, 0).unwrap();
    a.share();
    let links: Vec<(&Node, &Node)> = std::iter::once((&others[0], &a))
        .chain((1..6).map(|index| (&others[index], &others[index / 2])))
        .collect();
    settle_links(&links);
    a.pass();
    settle_links(&links);
    settle_links(&links);

    let models = a.sealed(&format!("models:{}", a.id)).unwrap();
    assert_eq!(models.recipients.len(), 6);
    for node in &others {
        assert_eq!(node.view().members.len(), 7, "{} trusts too few", node.id);
        assert_eq!(node.auth()["openai"]["key"], "sk-from-a");
    }
    // No member asked for a vault token, so none counts toward the limit.
    assert!(!heard
        .lock()
        .unwrap()
        .iter()
        .any(|(route, _)| route == "POST /v1/model-vault/machine-tokens"));
}

#[test]
fn a_member_the_vault_refuses_takes_the_vaults_connections_as_leases() {
    let (base, heard) = shared_models::tests::vault_stub_recording(vec![
        (
            "PUT /v1/model-vault",
            json!({"contract_version": shared_models::VAULT_CONTRACT, "connections": 1, "held": []}),
        ),
        (
            "POST /v1/model-vault/machine-tokens",
            json!({"error": "The account reached its limit of 5 paired machines."}),
        ),
    ]);
    let session = CloudSession {
        base_url: base,
        bearer: "native".into(),
    };
    let a = Node::with_session("desktop", Some(session.clone()));
    let b = Node::new("b");
    join(&b, &a, Role::Full);
    credentials::credentials_operation(b.inner(), json!({"use_vault": true})).unwrap();
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    shared_models::upload_to_vault(&a.agent(), &session, 0).unwrap();
    // The vault leases A a sign-in it manages.
    shared_models::apply(
        &a.agent(),
        shared_models::CLOUD_DEVICE,
        "MuniCloud",
        shared_models::Bundle {
            pi: json!({"anthropic": {"type": "oauth", "access": "vault-access", "refresh": "vault-refresh", "expires": i64::MAX / 2}})
                .as_object()
                .unwrap()
                .clone(),
            ..Default::default()
        },
    )
    .unwrap();
    a.share();
    settle_links(&[(&b, &a)]);
    a.pass();
    settle_links(&[(&b, &a)]);
    a.pass();
    settle_links(&[(&b, &a)]);

    assert!(heard
        .lock()
        .unwrap()
        .iter()
        .any(|(route, _)| route == "POST /v1/model-vault/machine-tokens"));
    let answer = credentials::credentials_operation(a.inner(), json!({})).unwrap();
    assert_eq!(answer["vault"][0]["runtime_id"], b.id.as_str());
    assert_eq!(answer["vault"][0]["state"], "leases");
    assert_eq!(a.audit("vault_token_refused").len(), 1);
    // B holds the swarm's connection and the vault's sign-in as a lease,
    // without its refresh token.
    let auth = b.auth();
    assert_eq!(auth["openai"]["key"], "sk-from-a");
    assert_eq!(auth["anthropic"]["access"], "vault-access");
    assert!(auth["anthropic"].get("refresh").is_none());
}

fn journal_provenance() -> crate::journal::Provenance {
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
    /// Creates an ordinary thread here, titled, with one run of `prompt`,
    /// and lets the scan register it as this member's own. Answers the
    /// thread and the run.
    fn ordinary_thread(&self, title: &str, prompt: &str) -> (String, String) {
        let thread = uuid::Uuid::now_v7().to_string();
        let run = uuid::Uuid::now_v7().to_string();
        {
            let mut storage = self.inner().storage.lock().unwrap();
            let at = self.inner().now_text();
            let journal = &mut storage.journal;
            journal
                .create_thread_with_id(&thread, "/workspace", &at, journal_provenance())
                .unwrap();
            journal
                .append_thread_title_renamed(1, &thread, title, &at, &journal_provenance())
                .unwrap();
            journal
                .append_new_run_in_thread(
                    "/workspace",
                    &thread,
                    &crate::journal::EventEnvelope {
                        event_id: uuid::Uuid::now_v7().to_string(),
                        run_id: run.clone(),
                        run_seq: 1,
                        event_type: "user.prompt.submitted".into(),
                        event_version: 1,
                        envelope_version: 1,
                        recorded_at: at.clone(),
                        occurred_at: None,
                        correlation_id: None,
                        causation_id: None,
                        payload: crate::journal::EventPayload::Inline {
                            payload_json: json!({"prompt": prompt}),
                        },
                        provenance: journal_provenance(),
                        extra: std::collections::BTreeMap::new(),
                    },
                )
                .unwrap();
        }
        sync::scan_local(self.inner()).unwrap();
        (thread, run)
    }

    fn holds_thread(&self, thread: &str) -> bool {
        self.inner()
            .storage
            .lock()
            .unwrap()
            .journal
            .thread_exists(thread)
            .unwrap()
    }

    fn run_thread(&self, run: &str) -> Option<String> {
        self.inner()
            .storage
            .lock()
            .unwrap()
            .journal
            .run_thread_id(run)
            .unwrap()
    }

    fn title(&self, thread: &str) -> Option<String> {
        self.inner()
            .storage
            .lock()
            .unwrap()
            .journal
            .summaries_of_threads(&[thread.to_owned()])
            .unwrap()
            .into_iter()
            .next()
            .map(|summary| summary.title)
    }

    fn posts(&self, thread: &str) -> Vec<Value> {
        sync::posted_messages(&self.inner().storage, thread, 100).unwrap_or_default()
    }
}

#[test]
fn a_worker_owned_ordinary_thread_reaches_a_full_member_and_a_post_steers_it() {
    let a = Node::scoped("desktop");
    let w = Node::scoped("worker");
    let v = Node::scoped("other-worker");
    join(&w, &a, Role::Worker);
    join(&v, &a, Role::Worker);
    let links = [(&w, &a), (&v, &a)];
    settle_links(&links);

    let (thread, run) = w.ordinary_thread("Parser fix", "Fix the parser.");
    sync::post_as(w.inner(), &thread, json!({"kind": "agent"}), "Tests pass.").unwrap();
    let (own, _) = a.ordinary_thread("Desktop notes", "Plan the week.");
    let (other, _) = v.ordinary_thread("Other task", "Write docs.");
    settle_links(&links);

    // The full member lists the worker's thread with its title, owner, run
    // and posts, and holds its summary.
    let registration = sync::registration(a.inner(), &thread).unwrap();
    assert_eq!(registration.kind, sync::PLAIN);
    assert_eq!(registration.owner_runtime.as_deref(), Some(w.id.as_str()));
    assert_eq!(a.swarm.thread_owner(&thread), Some(w.id.clone()));
    assert_eq!(a.swarm.is_replica_thread(&thread), Some(w.id.clone()));
    assert_eq!(a.title(&thread).as_deref(), Some("Parser fix"));
    assert_eq!(a.run_thread(&run), Some(thread.clone()));
    assert_eq!(a.posts(&thread)[0]["text"], "Tests pass.");
    assert_eq!(
        sync::summary(a.inner(), &thread).unwrap().title,
        "Parser fix"
    );
    assert!(a.holds_thread(&other));

    // A worker holds neither the full member's thread nor another worker's.
    for (worker, foreign) in [(&w, &own), (&w, &other), (&v, &thread), (&v, &own)] {
        assert!(sync::registration(worker.inner(), foreign).is_none());
        assert!(!worker.holds_thread(foreign));
    }

    // A post from the desktop steers the worker's live run, and the worker's
    // consumption names that run on the desktop.
    sync::post_operation(
        a.inner(),
        json!({"thread_id": thread, "text": "Keep the old API."}),
    )
    .unwrap();
    settle_links(&links);
    assert_eq!(
        *lock(&w.host.steered),
        vec![(thread.clone(), "Keep the old API.".to_owned())]
    );
    assert!(lock(&a.host.steered).is_empty());
    let posted = a.posts(&thread);
    let post = posted
        .iter()
        .find(|post| post["text"] == "Keep the old API.")
        .unwrap();
    assert_eq!(post["consumed"], true, "{post}");
    assert_eq!(post["run_id"], "run-steered-1");
    assert_eq!(post["origin_runtime"], a.id.as_str());

    // A worker cannot claim the full member's thread as its own: a forged
    // registration and summary change nothing there.
    let mut forged = wire(&w.inner().build_exchange(&a.id).unwrap());
    let clock = Hlc::at(i64::MAX / 4);
    let row = |field: &str, value: Value| RecordRow {
        class: sync::CLASS.into(),
        key: own.clone(),
        field: field.into(),
        value: Some(value),
        clock,
        origin: w.id.clone(),
        seq: 1_000_000,
        rank: 0,
        deleted: false,
    };
    forged.records = vec![
        row(
            sync::REGISTRATION,
            json!({"thread_id": own, "kind": sync::PLAIN, "updated_at": "2999-01-01T00:00:00Z",
                   "owner_runtime": w.id}),
        ),
        row(
            "summary",
            json!({"title": "Taken", "owner_runtime": w.id, "last_at": "2999-01-01T00:00:00Z"}),
        ),
    ];
    a.inner().merge_exchange(&w.id, forged).unwrap();
    assert_eq!(a.swarm.thread_owner(&own), Some(a.id.clone()));
    assert_eq!(
        sync::summary(a.inner(), &own).unwrap().title,
        "Desktop notes"
    );
}

#[test]
fn a_removed_worker_threads_stay_readable() {
    let a = Node::scoped("desktop");
    let w = Node::scoped("worker");
    join(&w, &a, Role::Worker);
    settle_links(&[(&w, &a)]);
    let (thread, run) = w.ordinary_thread("Release notes", "Draft the notes.");
    sync::post_as(w.inner(), &thread, json!({"kind": "agent"}), "Drafted.").unwrap();
    settle_links(&[(&w, &a)]);
    assert_eq!(a.title(&thread).as_deref(), Some("Release notes"));

    // The desktop ends the worker's membership, as a finished run leaves.
    link::leave_runtime(a.inner(), &w.id).unwrap();
    assert!(a.view().removed.contains(&w.id));
    a.swarm.tick().unwrap();
    settle_links(&[(&w, &a)]);
    assert!(w.view().self_removed());

    // What the worker writes afterwards never arrives.
    sync::post_as(w.inner(), &thread, json!({"kind": "agent"}), "Too late.").unwrap();
    settle_links(&[(&w, &a)]);

    // The desktop still lists and reads the thread: title, owner, run and
    // posts, up to the removal.
    assert!(a.holds_thread(&thread));
    assert_eq!(a.title(&thread).as_deref(), Some("Release notes"));
    assert_eq!(a.swarm.thread_owner(&thread), Some(w.id.clone()));
    assert_eq!(
        sync::summary(a.inner(), &thread).unwrap().title,
        "Release notes"
    );
    assert_eq!(a.run_thread(&run), Some(thread.clone()));
    let texts: Vec<Value> = a
        .posts(&thread)
        .into_iter()
        .map(|post| post["text"].clone())
        .collect();
    assert_eq!(texts, vec![json!("Drafted.")]);
    // The worker keeps its own copy.
    assert_eq!(w.posts(&thread).len(), 2);
}
