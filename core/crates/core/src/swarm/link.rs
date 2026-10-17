//! Links between members: invitations, joins, the dialing side's reconnect
//! loop, the answering side's exchange, and the member list.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use muniment_attach::{
    connect_remote_runtime, pair_with_link, DesktopClient, Operation, PairingLink,
    RemoteClientOptions, RemoteConnectError, RemoteRuntimeRecord, SCOPE_SWARM_PEER,
};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use super::store::sql;
use super::{lock, parse_time, Exchange, Inner, MemberRecord, ONLINE_SECONDS};
use crate::attach::{PairedDevice, PairingLinkRequest, RemoteAccess};

/// The client kind a linked runtime names itself with.
pub const PEER_KIND: &str = "swarm-peer";
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

pub(super) struct LinkHandle {
    stop: Arc<AtomicBool>,
    exchanges: Arc<AtomicU64>,
    connected: Arc<AtomicBool>,
}

fn links_directory(inner: &Inner) -> PathBuf {
    inner.profile.join("swarm").join("links")
}

fn record_path(inner: &Inner, runtime_id: &str) -> PathBuf {
    links_directory(inner).join(format!("{runtime_id}.json"))
}

fn options(inner: &Inner) -> Result<RemoteClientOptions, String> {
    let identity = lock(&inner.identity);
    let identity = identity
        .as_ref()
        .ok_or("Turn on remote access before linking runtimes.")?;
    Ok(RemoteClientOptions {
        kind: PEER_KIND.into(),
        version: identity.runtime_version.clone(),
        device_name: Some(identity.runtime_name.clone()),
        scopes: Some(BTreeSet::from([SCOPE_SWARM_PEER.to_owned()])),
        io_timeout: IO_TIMEOUT,
    })
}

fn set_peer(
    inner: &Inner,
    runtime_id: &str,
    direction: &str,
    device_id: Option<&str>,
    runtime_name: Option<&str>,
    state: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let now = inner.now_text();
    let seen = (state == "connected").then_some(now.clone());
    inner
        .store
        .connect()?
        .execute(
            "INSERT INTO peers(runtime_id, direction, device_id, runtime_name, state, last_seen_at, last_error, created_at) \
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(runtime_id, direction) DO UPDATE SET \
             device_id=COALESCE(excluded.device_id, peers.device_id), \
             runtime_name=COALESCE(excluded.runtime_name, peers.runtime_name), state=excluded.state, \
             last_seen_at=COALESCE(excluded.last_seen_at, peers.last_seen_at), last_error=excluded.last_error",
            params![runtime_id, direction, device_id, runtime_name, state, seen, error, now],
        )
        .map(drop)
        .map_err(sql)
}

/// One row of the peers table.
struct Peer {
    runtime_id: String,
    direction: String,
    device_id: Option<String>,
    runtime_name: Option<String>,
    state: String,
    last_seen_at: Option<String>,
    last_error: Option<String>,
    created_at: String,
}

fn peers(inner: &Inner) -> Result<Vec<Peer>, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare(
            "SELECT runtime_id, direction, device_id, runtime_name, state, last_seen_at, last_error, created_at FROM peers",
        )
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok(Peer {
                runtime_id: row.get(0)?,
                direction: row.get(1)?,
                device_id: row.get(2)?,
                runtime_name: row.get(3)?,
                state: row.get(4)?,
                last_seen_at: row.get(5)?,
                last_error: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql)
}

/// Starts a link thread for each saved link record.
pub(super) fn start_saved(inner: &Arc<Inner>) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(links_directory(inner)) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(runtime_id) = name.strip_suffix(".json") {
            start(inner, runtime_id, None);
        }
    }
    Ok(())
}

fn start(inner: &Arc<Inner>, runtime_id: &str, client: Option<DesktopClient>) {
    let mut links = lock(&inner.links);
    if links.contains_key(runtime_id) {
        return;
    }
    let handle = LinkHandle {
        stop: Arc::new(AtomicBool::new(false)),
        exchanges: Arc::new(AtomicU64::new(0)),
        connected: Arc::new(AtomicBool::new(false)),
    };
    let weak = Arc::downgrade(inner);
    let runtime_id_owned = runtime_id.to_owned();
    let (stop, exchanges, connected) = (
        Arc::clone(&handle.stop),
        Arc::clone(&handle.exchanges),
        Arc::clone(&handle.connected),
    );
    std::thread::spawn(move || {
        run(weak, runtime_id_owned, client, stop, exchanges, connected);
    });
    links.insert(runtime_id.to_owned(), handle);
}

/// Stops every link thread.
pub(super) fn stop_all(inner: &Inner) {
    for (_, handle) in std::mem::take(&mut *lock(&inner.links)) {
        handle.stop.store(true, Ordering::SeqCst);
    }
}

fn stop_link(inner: &Inner, runtime_id: &str) {
    if let Some(handle) = lock(&inner.links).remove(runtime_id) {
        handle.stop.store(true, Ordering::SeqCst);
    }
    inner.nudge();
}

/// Waits on the swarm's wake signal for up to `timeout`, or until `stop`.
fn wait(weak: &Weak<Inner>, timeout: Duration, stop: &AtomicBool) {
    let deadline = Instant::now() + timeout;
    while !stop.load(Ordering::SeqCst) {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        let guard = lock(&inner.wake);
        let generation = *guard;
        let (guard, _) = inner
            .woken
            .wait_timeout(guard, remaining.min(Duration::from_millis(250)))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *guard != generation {
            return;
        }
    }
}

fn run(
    weak: Weak<Inner>,
    runtime_id: String,
    mut client: Option<DesktopClient>,
    stop: Arc<AtomicBool>,
    exchanges: Arc<AtomicU64>,
    connected: Arc<AtomicBool>,
) {
    let mut backoff = BACKOFF_MIN;
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let Ok(record) = RemoteRuntimeRecord::load(&record_path(&inner, &runtime_id)) else {
            return;
        };
        let connection = match client.take() {
            Some(client) => Ok(client),
            None => match options(&inner) {
                Ok(options) => connect_remote_runtime(&record, &options),
                Err(_) => Err(RemoteConnectError::Unreachable),
            },
        };
        let mut active = match connection {
            Ok(active) => active,
            Err(RemoteConnectError::Unauthorized) => {
                // The other runtime revoked this link. It stays listed as
                // revoked until the owner removes it.
                let _ = set_peer(
                    &inner,
                    &runtime_id,
                    "outbound",
                    None,
                    None,
                    "revoked",
                    Some("The other runtime revoked this link."),
                );
                inner.changed("members");
                lock(&inner.links).remove(&runtime_id);
                return;
            }
            Err(error) => {
                let _ = set_peer(
                    &inner,
                    &runtime_id,
                    "outbound",
                    None,
                    None,
                    "backoff",
                    Some(&error.to_string()),
                );
                drop(inner);
                wait(&weak, backoff, &stop);
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        backoff = BACKOFF_MIN;
        let _ = set_peer(
            &inner,
            &runtime_id,
            "outbound",
            Some(record.credential.device_id.as_str()),
            Some(&record.runtime_name),
            "connected",
            None,
        );
        connected.store(true, Ordering::SeqCst);
        drop(inner);
        loop {
            if stop.load(Ordering::SeqCst) {
                connected.store(false, Ordering::SeqCst);
                return;
            }
            let Some(inner) = weak.upgrade() else {
                return;
            };
            match exchange_once(&inner, &runtime_id, &mut active) {
                Ok(more) => {
                    exchanges.fetch_add(1, Ordering::SeqCst);
                    let poll = *lock(&inner.poll);
                    drop(inner);
                    if !more {
                        wait(&weak, poll, &stop);
                    }
                }
                Err(error) => {
                    connected.store(false, Ordering::SeqCst);
                    let _ = set_peer(
                        &inner,
                        &runtime_id,
                        "outbound",
                        None,
                        None,
                        "backoff",
                        Some(&error),
                    );
                    drop(inner);
                    wait(&weak, backoff, &stop);
                    break;
                }
            }
        }
    }
}

/// Runs one exchange over `client` and answers whether either side left
/// items out.
fn exchange_once(inner: &Inner, peer: &str, client: &mut DesktopClient) -> Result<bool, String> {
    super::fleet::note_pinned(peer);
    let request = inner.build_exchange(peer)?;
    let sent_more = request.more;
    let body = serde_json::to_value(&request).map_err(|error| error.to_string())?;
    let response = client
        .request(Operation::SwarmExchange, None, body)
        .map_err(|error| {
            client.last_request_error().map_or_else(
                || format!("The exchange failed: {error:?}"),
                |error| format!("The exchange failed: {error}"),
            )
        })?;
    let answer: Exchange = serde_json::from_value(response.body)
        .map_err(|error| format!("The exchange answer is invalid: {error}"))?;
    if answer.from.runtime_id != peer {
        return Err("A different runtime answered the link.".into());
    }
    // The answer came over this link's pinned certificate, so this runtime
    // vouches for the card it carries.
    if answer.from.fleet.is_some() {
        if let Ok(record) = RemoteRuntimeRecord::load(&record_path(inner, peer)) {
            super::fleet::vouch_for_link(inner, peer, &answer.from, &record.fingerprint.to_hex())?;
        }
    }
    let more = answer.more;
    let name = answer.from.runtime_name.clone();
    super::ops::note_handed(inner, &request.messages);
    let kick = inner.merge_exchange(peer, answer)?;
    set_peer(
        inner,
        peer,
        "outbound",
        None,
        Some(&name),
        "connected",
        None,
    )?;
    if kick {
        inner.host.kick_runs();
    }
    Ok(sent_more || more)
}

/// Waits until every connected link ran two more exchanges, for tests.
pub(super) fn exchange_all_now(inner: &Inner) {
    let counts: HashMap<String, (Arc<AtomicU64>, u64)> = lock(&inner.links)
        .iter()
        .filter(|(_, handle)| handle.connected.load(Ordering::SeqCst))
        .map(|(id, handle)| {
            (
                id.clone(),
                (
                    Arc::clone(&handle.exchanges),
                    handle.exchanges.load(Ordering::SeqCst),
                ),
            )
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    for (counter, start) in counts.values() {
        while counter.load(Ordering::SeqCst) < start + 2 && Instant::now() < deadline {
            inner.nudge();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Answers a linked runtime's exchange: binds its device to its runtime id
/// on first use, takes in what it sent and answers what it lacks.
pub(super) fn answer_exchange(
    inner: &Inner,
    device_id: &str,
    body: Value,
) -> Result<Value, String> {
    let self_id = inner
        .runtime_id()
        .ok_or("This runtime is not part of a swarm.")?;
    let exchange: Exchange = serde_json::from_value(body)
        .map_err(|error| format!("The exchange is invalid: {error}"))?;
    let peer = exchange.from.runtime_id.clone();
    if peer == self_id || muniment_attach::Id::new(peer.as_str()).is_err() {
        return Err("The exchange names an invalid runtime.".into());
    }
    let bound: Option<String> = inner
        .store
        .connect()?
        .query_row(
            "SELECT runtime_id FROM peers WHERE direction='inbound' AND device_id=?1",
            [device_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    if bound.as_deref().is_some_and(|bound| bound != peer) {
        return Err("This link belongs to another runtime.".into());
    }
    super::fleet::note_device(&peer, device_id);
    let name = exchange.from.runtime_name.clone();
    set_peer(
        inner,
        &peer,
        "inbound",
        Some(device_id),
        Some(&name),
        "connected",
        None,
    )?;
    if inner.merge_exchange(&peer, exchange)? {
        inner.host.kick_runs();
    }
    let answer = inner.build_exchange(&peer)?;
    super::ops::note_handed(inner, &answer.messages);
    serde_json::to_value(answer).map_err(|error| error.to_string())
}

/// This runtime's own member record.
pub(super) fn self_record(inner: &Inner) -> Result<MemberRecord, String> {
    let fleet = super::fleet::card(inner);
    let identity = lock(&inner.identity);
    let identity = identity
        .as_ref()
        .ok_or("This runtime is not part of a swarm.")?;
    let now = inner.now();
    let fresh = |seen: &Option<String>| {
        seen.as_deref()
            .and_then(parse_time)
            .is_some_and(|seen| (now - seen).num_seconds() <= ONLINE_SECONDS)
    };
    let mut links: Vec<String> = peers(inner)?
        .into_iter()
        .filter(|peer| peer.state == "connected" && fresh(&peer.last_seen_at))
        .map(|peer| peer.runtime_id)
        .collect();
    links.sort();
    links.dedup();
    let mut capabilities = if inner.legacy() {
        Vec::new()
    } else {
        vec![
            super::STORE_CAPABILITY.to_owned(),
            super::OPS_CAPABILITY.to_owned(),
        ]
    };
    if fleet.is_some() {
        capabilities.push(super::fleet::CAPABILITY.to_owned());
        if super::credentials::uses_vault(inner)? {
            capabilities.push(super::credentials::VAULT_CAPABILITY.to_owned());
        }
    }
    Ok(MemberRecord {
        runtime_id: identity.runtime_id.clone(),
        runtime_name: identity.runtime_name.clone(),
        form_factor: identity.form_factor.clone(),
        heartbeat_at: super::timestamp(now),
        last_client_at: inner.store.meta("last_client_at")?,
        links,
        updated_at: super::timestamp(now),
        capabilities,
        fleet,
        endpoints: identity
            .access
            .as_ref()
            .map(RemoteAccess::advertised_endpoints)
            .unwrap_or_default(),
        offline_days: Some(super::bootstrap::offline(inner).thread_days),
        pins: super::sync::own_pins(inner)?,
        record_vv: if inner.legacy() {
            None
        } else {
            Some(inner.records.version_vector()?)
        },
    })
}

/// Whether a member was last heard of longer ago than a tombstone lasts, so
/// it must take a fresh snapshot when it returns.
fn stale(inner: &Inner, record: &MemberRecord) -> bool {
    parse_time(&record.heartbeat_at)
        .is_some_and(|seen| (inner.now() - seen).num_days() >= super::TOMBSTONE_DAYS)
}

/// Every member record this runtime knows, its own first.
pub(super) fn member_records(inner: &Inner) -> Result<Vec<MemberRecord>, String> {
    let self_record = self_record(inner)?;
    let mut records: Vec<MemberRecord> = inner
        .store
        .records::<MemberRecord>("members", "record")?
        .into_iter()
        .filter(|record| record.runtime_id != self_record.runtime_id)
        .collect();
    records.sort_by(|a, b| a.runtime_name.cmp(&b.runtime_name));
    records.insert(0, self_record);
    Ok(records)
}

/// Keeps the newest record of each other member.
pub(super) fn merge_members(
    inner: &Inner,
    from: &MemberRecord,
    members: &[MemberRecord],
) -> Result<(), String> {
    let self_id = inner.runtime_id().unwrap_or_default();
    let mut added = false;
    for record in std::iter::once(from).chain(members) {
        if record.runtime_id == self_id
            || muniment_attach::Id::new(record.runtime_id.as_str()).is_err()
        {
            continue;
        }
        let existing: Option<MemberRecord> =
            inner
                .store
                .record("members", "runtime_id", "record", &record.runtime_id)?;
        // A record rewritten within the same second, as a new pin makes it,
        // still replaces the held one.
        if existing.as_ref().is_none_or(|existing| {
            record.updated_at > existing.updated_at
                || (record.updated_at == existing.updated_at && record != existing)
        }) {
            added |= existing.is_none();
            // A member's new pins or window change which threads it takes.
            if existing.as_ref().is_none_or(|existing| {
                existing.pins != record.pins || existing.offline_days != record.offline_days
            }) {
                inner.refresh_exports();
            }
            inner.store.put_record(
                "members",
                "runtime_id",
                "record",
                &record.runtime_id,
                record,
            )?;
        }
    }
    if added {
        inner.changed("members");
    }
    Ok(())
}

/// Whether a member was heard from within [`ONLINE_SECONDS`].
pub(super) fn online(inner: &Inner, runtime_id: &str) -> bool {
    if inner.runtime_id().as_deref() == Some(runtime_id) {
        return true;
    }
    let now = inner.now();
    inner
        .store
        .record::<MemberRecord>("members", "runtime_id", "record", runtime_id)
        .ok()
        .flatten()
        .and_then(|record| parse_time(&record.heartbeat_at))
        .is_some_and(|seen| (now - seen).num_seconds() <= ONLINE_SECONDS)
}

/// Whether `runtime_id` names this runtime or a member this runtime knows.
pub(super) fn known_member(inner: &Inner, runtime_id: &str) -> bool {
    inner.runtime_id().as_deref() == Some(runtime_id)
        || inner
            .store
            .record::<MemberRecord>("members", "runtime_id", "record", runtime_id)
            .ok()
            .flatten()
            .is_some()
}

/// The answer of `swarm.members`.
pub(super) fn members_view(inner: &Inner) -> Result<Value, String> {
    let self_id = inner
        .runtime_id()
        .ok_or("Turn on remote access to see the swarm.")?;
    let peers = peers(inner)?;
    let mut members = Vec::new();
    for record in member_records(inner)? {
        let this = record.runtime_id == self_id;
        let link = peers
            .iter()
            .filter(|peer| peer.runtime_id == record.runtime_id)
            .max_by_key(|peer| (peer.state == "connected", peer.direction == "outbound"))
            .map(|peer| {
                json!({
                    "direction": peer.direction,
                    "state": peer.state,
                    "device_id": peer.device_id,
                    "last_seen_at": peer.last_seen_at,
                    "last_error": peer.last_error,
                })
            });
        members.push(json!({
            "runtime_id": record.runtime_id,
            "runtime_name": record.runtime_name,
            "form_factor": record.form_factor,
            "self": this,
            "online": online(inner, &record.runtime_id),
            "stale": !this && stale(inner, &record),
            "heartbeat_at": record.heartbeat_at,
            "last_client_at": record.last_client_at,
            "links": record.links,
            "link": link,
            "fleet": super::fleet::member_view(inner, &record.runtime_id),
        }));
    }
    // A link whose other end never answered still shows.
    for peer in &peers {
        if peer.runtime_id != self_id
            && !members
                .iter()
                .any(|member| member["runtime_id"] == peer.runtime_id.as_str())
        {
            members.push(json!({
                "runtime_id": peer.runtime_id,
                "runtime_name": peer.runtime_name,
                "form_factor": null,
                "self": false,
                "online": false,
                "stale": false,
                "heartbeat_at": null,
                "last_client_at": null,
                "links": [],
                "link": {
                    "direction": peer.direction,
                    "state": peer.state,
                    "device_id": peer.device_id,
                    "last_seen_at": peer.last_seen_at,
                    "last_error": peer.last_error,
                },
            }));
        }
    }
    Ok(json!({
        "self": self_id,
        "members": members,
        "absence": super::absence::view(inner)?,
        "sync": inner.sync_view()?,
    }))
}

/// `swarm.invite`: issues a `swarm.peer` pairing link.
pub(super) fn invite(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        endpoints: Option<Vec<String>>,
        #[serde(default)]
        role: Option<String>,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let role = match body.role.as_deref() {
        None => super::fleet::Role::Full,
        Some(role) => super::fleet::Role::parse(role).ok_or("The role is full or worker.")?,
    };
    if !super::fleet::may_invite(inner)? {
        return Err("A worker cannot invite runtimes.".into());
    }
    let access = inner
        .access()
        .ok_or("Turn on remote access before linking runtimes.")?;
    let created = access
        .create_pairing_link(PairingLinkRequest {
            scopes: Some(BTreeSet::from([SCOPE_SWARM_PEER.to_owned()])),
            endpoints: body.endpoints,
        })
        .map_err(|error| error.to_string())?;
    super::fleet::note_invite(inner, &created.link.secret, role, &created.expires_at)?;
    Ok(json!({
        "link": created.link.to_uri(),
        "expires_at": created.expires_at,
        "runtime_id": access.runtime_info().runtime_id,
        "role": role.as_str(),
    }))
}

/// `swarm.set_role {runtime_id, role}`: gives a fleet member a new role
/// with a signed roster entry. Every member enforces it once the entry
/// reaches it.
pub(super) fn set_role(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        runtime_id: String,
        role: String,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let role = super::fleet::Role::parse(&body.role).ok_or("The role is full or worker.")?;
    let previous = super::fleet::set_role(inner, &body.runtime_id, role)?;
    if let Some(previous) = previous {
        super::audit::append(
            inner,
            "role_changed",
            Some(&body.runtime_id),
            json!({"from": previous.as_str(), "to": role.as_str()}),
        )?;
        super::credentials::settle(inner)?;
        inner.nudge();
    }
    Ok(json!({
        "runtime_id": body.runtime_id,
        "role": role.as_str(),
        "previous": previous.map(super::fleet::Role::as_str),
        "changed": previous.is_some(),
    }))
}

/// `swarm.join`: redeems another runtime's invitation, keeps the link and
/// runs the first exchange before it answers.
pub(super) fn join(inner: &Arc<Inner>, body: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        link: String,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    let link = PairingLink::parse(&body.link).map_err(|error| error.to_string())?;
    let self_id = inner
        .runtime_id()
        .ok_or("Turn on remote access before linking runtimes.")?;
    if link.runtime_id.as_str() == self_id {
        return Err("This invitation is from this runtime.".into());
    }
    let (mut client, record) = pair_with_link(&link, &options(inner)?)
        .map_err(|error| format!("The other runtime could not be linked: {error}"))?;
    if !record.scopes.contains(SCOPE_SWARM_PEER) {
        return Err("The invitation does not link runtimes.".into());
    }
    let runtime_id = record.runtime_id.as_str().to_owned();
    super::fleet::note_join(inner, &runtime_id, &link.secret)?;
    std::fs::create_dir_all(links_directory(inner))
        .map_err(|error| format!("The link could not be saved: {error}"))?;
    record
        .save(&record_path(inner, &runtime_id))
        .map_err(|error| format!("The link could not be saved: {error}"))?;
    set_peer(
        inner,
        &runtime_id,
        "outbound",
        Some(record.credential.device_id.as_str()),
        Some(&record.runtime_name),
        "connected",
        None,
    )?;
    exchange_once(inner, &runtime_id, &mut client)?;
    start(inner, &runtime_id, Some(client));
    inner.changed("members");
    let member = members_view(inner)?["members"]
        .as_array()
        .and_then(|members| {
            members
                .iter()
                .find(|member| member["runtime_id"] == runtime_id.as_str())
                .cloned()
        })
        .unwrap_or_else(|| json!({"runtime_id": runtime_id, "runtime_name": record.runtime_name}));
    Ok(json!({"member": member}))
}

/// Drops every link to a member. An outbound link asks the other runtime to
/// revoke this runtime's device first.
pub(super) fn leave_runtime(inner: &Inner, runtime_id: &str) -> Result<bool, String> {
    let mut removed = false;
    stop_link(inner, runtime_id);
    let path = record_path(inner, runtime_id);
    if let Ok(record) = RemoteRuntimeRecord::load(&path) {
        if let Ok(options) = options(inner) {
            if let Ok(mut client) = connect_remote_runtime(&record, &options) {
                let _ = client.revoke_remote_device(record.credential.device_id.as_str());
            }
        }
    }
    if std::fs::remove_file(&path).is_ok() {
        removed = true;
    }
    for peer in peers(inner)?
        .into_iter()
        .filter(|peer| peer.runtime_id == runtime_id)
    {
        if peer.direction == "inbound" {
            if let (Some(access), Some(device)) = (inner.access(), &peer.device_id) {
                let _ = access.revoke_device(device);
            }
        }
        removed = true;
    }
    let connection = inner.store.connect()?;
    connection
        .execute("DELETE FROM peers WHERE runtime_id=?1", [runtime_id])
        .map_err(sql)?;
    connection
        .execute("DELETE FROM members WHERE runtime_id=?1", [runtime_id])
        .map_err(sql)?;
    lock(&inner.peers).remove(runtime_id);
    inner
        .store
        .set_meta(&format!("peer_store:{runtime_id}"), None)?;
    end_membership(inner, runtime_id);
    inner.changed("members");
    Ok(removed)
}

/// Appends a signed roster removal of a member this runtime dropped, and
/// carries out what it means here: resealed secrets, a dropped bundle and
/// a revoked model vault token.
fn end_membership(inner: &Inner, runtime_id: &str) {
    if super::fleet::keys(inner).is_err() {
        return;
    }
    let ended = super::fleet::remove_member(inner, runtime_id)
        .and_then(|_| super::credentials::settle(inner));
    if let Err(error) = ended {
        crate::runtime_eprintln!(
            "muniment-runtime: swarm member {runtime_id} stayed in the fleet roster: {error}"
        );
    }
}

/// `swarm.leave`.
pub(super) fn leave(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        runtime_id: String,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    Ok(json!({"removed": leave_runtime(inner, &body.runtime_id)?}))
}

/// Shows outbound links in the remote device list and revokes them there.
pub(super) struct OutboundHook(pub(super) Weak<Inner>);

impl crate::attach::OutboundLinks for OutboundHook {
    fn list(&self) -> Vec<PairedDevice> {
        let Some(inner) = self.0.upgrade() else {
            return Vec::new();
        };
        peers(&inner)
            .unwrap_or_default()
            .into_iter()
            .filter(|peer| peer.direction == "outbound")
            .filter_map(|peer| {
                Some(PairedDevice {
                    device_id: peer.device_id?,
                    name: peer.runtime_name.unwrap_or_else(|| peer.runtime_id.clone()),
                    kind: PEER_KIND.into(),
                    scopes: BTreeSet::from([SCOPE_SWARM_PEER.to_owned()]),
                    created_at: peer.created_at,
                    last_seen_at: peer.last_seen_at,
                    direction: Some("outbound".into()),
                    runtime_id: Some(peer.runtime_id),
                })
            })
            .collect()
    }

    fn revoke(&self, device_id: &str) -> bool {
        let Some(inner) = self.0.upgrade() else {
            return false;
        };
        let runtime = peers(&inner).ok().and_then(|peers| {
            peers
                .into_iter()
                .find(|peer| {
                    peer.direction == "outbound" && peer.device_id.as_deref() == Some(device_id)
                })
                .map(|peer| peer.runtime_id)
        });
        runtime.is_some_and(|runtime| leave_runtime(&inner, &runtime).unwrap_or(false))
    }

    fn revoked_inbound(&self, device_id: &str) {
        let Some(inner) = self.0.upgrade() else {
            return;
        };
        let runtime = peers(&inner).ok().and_then(|peers| {
            peers
                .into_iter()
                .find(|peer| {
                    peer.direction == "inbound" && peer.device_id.as_deref() == Some(device_id)
                })
                .map(|peer| peer.runtime_id)
        });
        if let Some(runtime) = runtime {
            end_membership(&inner, &runtime);
        }
    }
}
