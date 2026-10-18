//! Fleet keys, the signed roster and sealed secrets.
//!
//! - **Keys.** Each member keeps an X25519 fleet key in its credential store.
//!   Its member record carries a card with the public half, signed with the
//!   member's P-256 identity key, the key behind its pinned TLS certificate.
//! - **Roster.** A join adds two signed entries: the inviter vouches for the
//!   joiner with the invitation's role, and the joiner vouches for the
//!   inviter whose certificate it pinned. A member trusts itself and each
//!   member an entry from a trusted full member names. A signed `remove`
//!   ends a member and every vouch it signed.
//! - **Sealed secrets.** An owner seals each version of a secret to each
//!   recipient's fleet key (X25519, HKDF-SHA256, AES-256-GCM) and signs it.
//!   Members keep the newest version, pass it only to roster members, and
//!   open only their own copy. A relay holds ciphertext it cannot open.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::hlc::Hlc;
use super::store::sql;
use super::{lock, FleetCard, Inner, MemberRecord, RosterEntry, SealedCopy, SealedSecret};
use crate::attach::{verify_identity_signature, IdentitySigner};
use crate::model_router::credentials::{CredentialStore, FileCredentialStore};
use crate::relay_encryption::{self, AgreementKeyPair};

/// The capability a member with a fleet card names.
pub(super) const CAPABILITY: &str = "fleet/1";
const KEY_SERVICE: &str = "ai.muniment.fleet";
const KEY_NAME: &str = "fleet-key";
const CARD_LABEL: &str = "muniment.fleet-card/1";
const ROSTER_LABEL: &str = "muniment.fleet-roster/1";
const SEAL_LABEL: &str = "muniment.fleet-seal/1";
const SECRET_LABEL: &str = "muniment.fleet-secret/1";
/// The most roster entries a member keeps.
const MAX_ENTRIES: usize = 4096;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS fleet_roster(entry_id TEXT PRIMARY KEY, entry TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS fleet_sealed(
  secret_id TEXT PRIMARY KEY, version INTEGER NOT NULL, owner TEXT NOT NULL, record TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS fleet_invites(
  invite TEXT PRIMARY KEY, role TEXT NOT NULL, expires_at TEXT NOT NULL, used_by TEXT);
CREATE TABLE IF NOT EXISTS fleet_opened(
  secret_id TEXT PRIMARY KEY, version INTEGER NOT NULL, owner TEXT NOT NULL, state TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS fleet_own(secret_id TEXT PRIMARY KEY, digest TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS fleet_vault(
  runtime_id TEXT PRIMARY KEY, pairing_id TEXT NOT NULL, issued_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS fleet_audit(
  seq INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL, kind TEXT NOT NULL,
  runtime_id TEXT, record TEXT NOT NULL);
";

/// A member's role in the roster. Stage S5 enforces it; the roster records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Full,
    Worker,
}

impl Role {
    pub(super) fn parse(text: &str) -> Option<Self> {
        match text {
            "full" => Some(Self::Full),
            "worker" => Some(Self::Worker),
            _ => None,
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Worker => "worker",
        }
    }
}

/// This runtime's identity signer, fleet key and the card that states both.
pub(super) struct Keys {
    pub(super) signer: Arc<IdentitySigner>,
    fleet: AgreementKeyPair,
    pub(super) card: FleetCard,
}

/// What one member knows of a peer's roster and sealed secrets.
#[derive(Default)]
struct FleetPeer {
    roster_digest: Option<String>,
    sealed_have: Option<BTreeMap<String, u64>>,
}

/// How this runtime reaches a peer in the current exchange.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Channel {
    #[default]
    Unknown,
    /// This runtime's outbound link, pinned to the peer's certificate.
    Pinned,
    /// A device paired to this runtime, which only claims the peer's id.
    Device(String),
}

/// The fleet state of one swarm.
#[derive(Default)]
pub(super) struct FleetState {
    signer: Mutex<Option<Arc<IdentitySigner>>>,
    keys: Mutex<Option<Arc<Keys>>>,
    view: Mutex<Option<(String, Arc<View>)>>,
    peers: Mutex<HashMap<String, FleetPeer>>,
    /// Held while this runtime refreshes the sign-ins it seals or hands one
    /// over, so a handed-over refresh token is never also refreshed here.
    pub(super) refresh: Mutex<()>,
    /// When the last reseal pass ran, and whether one runs now.
    pub(super) pass: Mutex<super::credentials::PassClock>,
}

/// One trusted member as this runtime's roster view holds it.
#[derive(Clone, Debug)]
pub(super) struct Member {
    pub(super) card: FleetCard,
    pub(super) role: Role,
    identity: Vec<u8>,
}

/// The members this runtime trusts, from its own roster entries.
#[derive(Debug)]
pub(super) struct View {
    pub(super) self_id: String,
    /// Every trusted member, this runtime among them.
    pub(super) members: BTreeMap<String, Member>,
    /// The members a trusted remove entry ended.
    pub(super) removed: BTreeSet<String>,
}

impl View {
    pub(super) fn trusts(&self, runtime_id: &str) -> bool {
        self.members.contains_key(runtime_id)
    }

    pub(super) fn role(&self, runtime_id: &str) -> Option<Role> {
        self.members.get(runtime_id).map(|member| member.role)
    }

    pub(super) fn self_removed(&self) -> bool {
        self.removed.contains(&self.self_id)
    }

    /// The full members other than this runtime, which its secrets go to.
    /// Workers wait for stage S5's scoping.
    pub(super) fn full_recipients(&self) -> Vec<String> {
        self.members
            .iter()
            .filter(|(id, member)| **id != self.self_id && member.role == Role::Full)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// The members one secret may reach, by role: every full member, and the
/// workers that run an agent the secret serves. Sealing takes the full
/// members alone until stage S5 enforces worker scoping.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecipientSet {
    pub full: Vec<String>,
    pub workers: Vec<String>,
}

pub(super) fn initialize(inner: &Inner) -> Result<(), String> {
    let connection = inner.store.connect()?;
    connection.execute_batch(SCHEMA).map_err(sql)?;
    // Links made before fleet keys keep working: the member each inbound
    // link belongs to is admitted as full once its card arrives over that
    // link's device.
    if inner.store.meta("fleet_initialized")?.is_none() {
        let inbound: Vec<(String, String)> = {
            let mut statement = connection
                .prepare(
                    "SELECT runtime_id, device_id FROM peers WHERE direction='inbound' AND device_id IS NOT NULL",
                )
                .map_err(sql)?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(sql)?;
            rows.collect::<Result<_, _>>().map_err(sql)?
        };
        for (runtime_id, device_id) in inbound {
            inner
                .store
                .set_meta(&format!("fleet_legacy:{runtime_id}"), Some(&device_id))?;
        }
        inner.store.set_meta("fleet_initialized", Some("1"))?;
    }
    Ok(())
}

thread_local! {
    /// The peer the exchange on this thread is with, and how it arrived.
    /// Exchanges with one peer may run on several threads at once, so the
    /// channel belongs to the call, not to the peer.
    static CHANNEL: std::cell::RefCell<Option<(String, Channel)>> =
        const { std::cell::RefCell::new(None) };
}

/// Notes that the exchange on this thread with `peer` runs over this
/// runtime's outbound link, pinned to the peer's certificate.
pub(super) fn note_pinned(peer: &str) {
    CHANNEL.with(|held| *held.borrow_mut() = Some((peer.to_owned(), Channel::Pinned)));
}

/// Notes that the exchange on this thread with `peer` came from the paired
/// device `device_id`.
pub(super) fn note_device(peer: &str, device_id: &str) {
    CHANNEL.with(|held| {
        *held.borrow_mut() = Some((peer.to_owned(), Channel::Device(device_id.to_owned())))
    });
}

fn channel(peer: &str) -> Channel {
    CHANNEL.with(|held| match held.borrow().as_ref() {
        Some((current, channel)) if current == peer => channel.clone(),
        _ => Channel::Unknown,
    })
}

/// Whether the current exchange with `peer` provably reaches it: this
/// runtime's pinned link, or the device the peer was admitted through. A
/// device that only claims a member's id gets no sealed secret.
pub(super) fn proven(inner: &Inner, peer: &str) -> Result<bool, String> {
    Ok(match channel(peer) {
        Channel::Pinned => true,
        Channel::Device(device) => {
            inner
                .store
                .meta(&format!("fleet_device:{peer}"))?
                .as_deref()
                == Some(device.as_str())
        }
        Channel::Unknown => false,
    })
}

/// Whether the current exchange with `peer`, which the roster does not
/// name, runs over this runtime's pinned link or over the device a link
/// made before fleet keys bound to it.
pub(super) fn legacy_channel(inner: &Inner, peer: &str) -> Result<bool, String> {
    Ok(match channel(peer) {
        Channel::Pinned => true,
        Channel::Device(device) => {
            inner
                .store
                .meta(&format!("fleet_legacy:{peer}"))?
                .as_deref()
                == Some(device.as_str())
        }
        Channel::Unknown => false,
    })
}

/// Gives the fleet this runtime's identity signer.
pub(super) fn set_signer(inner: &Inner, signer: Option<Arc<IdentitySigner>>) {
    *lock(&inner.fleet.signer) = signer;
    *lock(&inner.fleet.keys) = None;
    *lock(&inner.fleet.view) = None;
}

fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text).ok()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The store the fleet key lives in: the host's, or owner-only files under
/// the profile.
fn credential_store(inner: &Inner) -> Arc<dyn CredentialStore> {
    inner.host.credentials().unwrap_or_else(|| {
        Arc::new(FileCredentialStore::new(
            inner.profile.join("swarm").join("credentials"),
        ))
    })
}

/// This runtime's keys, created on first use.
pub(super) fn keys(inner: &Inner) -> Result<Arc<Keys>, String> {
    let mut held = lock(&inner.fleet.keys);
    if let Some(keys) = held.as_ref() {
        return Ok(Arc::clone(keys));
    }
    let signer = lock(&inner.fleet.signer)
        .clone()
        .ok_or("Turn on remote access to hold fleet keys.")?;
    let store = credential_store(inner);
    let stored = store
        .read(KEY_SERVICE, KEY_NAME)
        .and_then(|text| decode(text.trim()))
        .map(Zeroizing::new)
        .and_then(|bytes| <[u8; 32]>::try_from(bytes.as_slice()).ok());
    let fleet = match stored {
        Some(bytes) => AgreementKeyPair::from_private_bytes(bytes),
        None => {
            let mut bytes = Zeroizing::new([0u8; 32]);
            getrandom::fill(bytes.as_mut()).map_err(|_| "The system random source failed.")?;
            let text = Zeroizing::new(encode(bytes.as_ref()));
            store
                .write(KEY_SERVICE, KEY_NAME, &text)
                .map_err(|error| format!("The fleet key could not be saved: {error}"))?;
            AgreementKeyPair::from_private_bytes(*bytes)
        }
    };
    let card = sign_card(&signer, &fleet.public_key())?;
    let keys = Arc::new(Keys {
        signer,
        fleet,
        card,
    });
    *held = Some(Arc::clone(&keys));
    Ok(keys)
}

/// This runtime's card, once it has an identity.
pub(super) fn card(inner: &Inner) -> Option<FleetCard> {
    keys(inner).ok().map(|keys| keys.card.clone())
}

fn card_statement(card: &FleetCard) -> String {
    format!(
        "{CARD_LABEL}\n{}\n{}\n{}",
        card.identity_pub, card.cert_sha256, card.fleet_pub
    )
}

fn sign_card(signer: &IdentitySigner, fleet_pub: &str) -> Result<FleetCard, String> {
    let mut card = FleetCard {
        identity_pub: encode(signer.public_key()),
        cert_sha256: signer.certificate_sha256().to_owned(),
        fleet_pub: fleet_pub.to_owned(),
        sig: String::new(),
    };
    card.sig = encode(&signer.sign(card_statement(&card).as_bytes())?);
    Ok(card)
}

/// Whether a card's identity key signed it.
pub(super) fn card_verifies(card: &FleetCard) -> bool {
    let (Some(identity), Some(sig)) = (decode(&card.identity_pub), decode(&card.sig)) else {
        return false;
    };
    decode(&card.fleet_pub).is_some_and(|key| key.len() == 32)
        && verify_identity_signature(&identity, card_statement(card).as_bytes(), &sig)
}

fn statement_of(value: Value, drop: &[&str]) -> String {
    let mut value = value;
    if let Some(object) = value.as_object_mut() {
        for key in drop {
            object.remove(*key);
        }
    }
    relay_encryption::canonical_json(&value)
}

fn entry_statement(entry: &RosterEntry) -> String {
    let value = serde_json::to_value(entry).unwrap_or(Value::Null);
    format!(
        "{ROSTER_LABEL}\n{}",
        statement_of(value, &["entry_id", "sig"])
    )
}

pub(super) fn sign_entry(keys: &Keys, mut entry: RosterEntry) -> Result<RosterEntry, String> {
    let statement = entry_statement(&entry);
    entry.entry_id = sha256_hex(statement.as_bytes());
    entry.sig = encode(&keys.signer.sign(statement.as_bytes())?);
    Ok(entry)
}

fn entry_verifies(entry: &RosterEntry, identity: &[u8]) -> bool {
    let statement = entry_statement(entry);
    entry.entry_id == sha256_hex(statement.as_bytes())
        && decode(&entry.sig)
            .is_some_and(|sig| verify_identity_signature(identity, statement.as_bytes(), &sig))
}

/// A clock for a new entry about `subject`, later than every entry about it
/// that a trusted member signed. Received entries never move the record
/// clock, so a forged far-future entry cannot skew it.
fn next_clock(inner: &Inner, subject: &str) -> Result<Hlc, String> {
    let mut clock = inner.records.tick(inner.now());
    let view = view(inner)?;
    for entry in entries(inner)? {
        if entry.runtime_id == subject && view.trusts(&entry.signer) && entry.clock >= clock {
            clock = match entry.clock.counter.checked_add(1) {
                Some(counter) => Hlc {
                    millis: entry.clock.millis,
                    counter,
                },
                None => Hlc {
                    millis: entry.clock.millis + 1,
                    counter: 0,
                },
            };
        }
    }
    Ok(clock)
}

/// Every roster entry this runtime holds.
pub(super) fn entries(inner: &Inner) -> Result<Vec<RosterEntry>, String> {
    inner.store.records("fleet_roster", "entry")
}

fn digest_of(entries: &[RosterEntry]) -> String {
    let mut ids: Vec<&str> = entries
        .iter()
        .map(|entry| entry.entry_id.as_str())
        .collect();
    ids.sort_unstable();
    sha256_hex(ids.join("\n").as_bytes())
}

/// Keeps roster entries whose ids match their statements. Signatures are
/// checked when the view is built, since a signer may arrive later.
fn store_entries(inner: &Inner, incoming: Vec<RosterEntry>) -> Result<bool, String> {
    if incoming.is_empty() {
        return Ok(false);
    }
    let mut connection = inner.store.connect()?;
    let count = |connection: &rusqlite::Connection| -> Result<i64, String> {
        connection
            .query_row("SELECT COUNT(*) FROM fleet_roster", [], |row| row.get(0))
            .map_err(sql)
    };
    let mut held = count(&connection)?;
    if held.max(0) as usize >= MAX_ENTRIES {
        // A full roster sheds the entries no trusted member signed.
        if let Ok(view) = view(inner) {
            for entry in entries(inner)? {
                if !view.trusts(&entry.signer) {
                    connection
                        .execute(
                            "DELETE FROM fleet_roster WHERE entry_id=?1",
                            [&entry.entry_id],
                        )
                        .map_err(sql)?;
                }
            }
            *lock(&inner.fleet.view) = None;
            held = count(&connection)?;
        }
    }
    let mut room = MAX_ENTRIES.saturating_sub(held.max(0) as usize);
    let transaction = connection.transaction().map_err(sql)?;
    let mut added = false;
    for entry in incoming {
        if room == 0 {
            break;
        }
        if !matches!(entry.kind.as_str(), "add" | "remove")
            || entry.entry_id != sha256_hex(entry_statement(&entry).as_bytes())
        {
            continue;
        }
        let json = serde_json::to_string(&entry).map_err(|error| error.to_string())?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO fleet_roster(entry_id, entry) VALUES(?1, ?2)",
                params![entry.entry_id, json],
            )
            .map_err(sql)?;
        if inserted > 0 {
            room -= 1;
            added = true;
        }
    }
    transaction.commit().map_err(sql)?;
    if added {
        *lock(&inner.fleet.view) = None;
    }
    Ok(added)
}

/// Builds the members `self_id` trusts from `entries`.
fn compute(self_id: &str, own: &Keys, entries: &[RosterEntry]) -> View {
    let verified: std::cell::RefCell<HashMap<(String, Vec<u8>), bool>> = Default::default();
    let verifies = |entry: &RosterEntry, identity: &[u8]| -> bool {
        *verified
            .borrow_mut()
            .entry((entry.entry_id.clone(), identity.to_vec()))
            .or_insert_with(|| entry_verifies(entry, identity))
    };
    let own_identity = own.signer.public_key().to_vec();
    // The members reachable from this runtime through adds that members
    // outside `excluded` signed.
    let closure = |excluded: &BTreeSet<String>| -> BTreeMap<String, Member> {
        let mut members = BTreeMap::new();
        members.insert(
            self_id.to_owned(),
            Member {
                card: own.card.clone(),
                role: Role::Full,
                identity: own_identity.clone(),
            },
        );
        for _ in 0..=entries.len() {
            let mut best: BTreeMap<&str, &RosterEntry> = BTreeMap::new();
            for entry in entries.iter().filter(|entry| entry.kind == "add") {
                if entry.signer == entry.runtime_id
                    || excluded.contains(&entry.runtime_id)
                    || excluded.contains(&entry.signer)
                {
                    continue;
                }
                let Some(signer) = members.get(&entry.signer) else {
                    continue;
                };
                if entry.signer != self_id && signer.role != Role::Full {
                    continue;
                }
                let valid = entry.role.as_deref().and_then(Role::parse).is_some()
                    && entry.card.as_ref().is_some_and(card_verifies)
                    && verifies(entry, &signer.identity);
                if !valid {
                    continue;
                }
                let newer = best.get(entry.runtime_id.as_str()).is_none_or(|held| {
                    (held.clock, &held.entry_id) < (entry.clock, &entry.entry_id)
                });
                if newer {
                    best.insert(&entry.runtime_id, entry);
                }
            }
            let mut next = BTreeMap::new();
            next.insert(self_id.to_owned(), members[self_id].clone());
            for (subject, entry) in best {
                let card = entry.card.clone().unwrap_or_default();
                let role = entry
                    .role
                    .as_deref()
                    .and_then(Role::parse)
                    .unwrap_or(Role::Worker);
                let Some(identity) = decode(&card.identity_pub) else {
                    continue;
                };
                if subject == self_id {
                    // This runtime's role is the one the newest vouch names.
                    if let Some(own) = next.get_mut(self_id) {
                        own.role = role;
                    }
                    continue;
                }
                next.insert(
                    subject.to_owned(),
                    Member {
                        card,
                        role,
                        identity,
                    },
                );
            }
            let same = next.len() == members.len()
                && next.iter().all(|(id, member)| {
                    members.get(id).is_some_and(|held| held.role == member.role)
                });
            members = next;
            if same {
                break;
            }
        }
        members
    };
    let reachable = closure(&BTreeSet::new());
    // A removal counts when a trusted full member signed it before anyone
    // removed that signer, and it is newer than the member's newest vouch.
    let newest_add = |runtime_id: &str| -> Option<Hlc> {
        entries
            .iter()
            .filter(|entry| entry.kind == "add" && entry.runtime_id == runtime_id)
            .filter(|entry| reachable.contains_key(&entry.signer))
            .map(|entry| entry.clock)
            .max()
    };
    let mut removals: Vec<&RosterEntry> = entries
        .iter()
        .filter(|entry| entry.kind == "remove")
        .collect();
    removals.sort_by(|a, b| (a.clock, &a.entry_id).cmp(&(b.clock, &b.entry_id)));
    let mut removed = BTreeSet::new();
    for entry in removals {
        let Some(signer) = reachable.get(&entry.signer) else {
            continue;
        };
        if removed.contains(&entry.signer)
            || (entry.signer != self_id && signer.role != Role::Full)
            || !verifies(entry, &signer.identity)
        {
            continue;
        }
        if newest_add(&entry.runtime_id).is_none_or(|added| entry.clock > added) {
            removed.insert(entry.runtime_id.clone());
        }
    }
    let members = closure(&removed);
    View {
        self_id: self_id.to_owned(),
        members,
        removed,
    }
}

/// The members this runtime trusts now.
pub(super) fn view(inner: &Inner) -> Result<Arc<View>, String> {
    let keys = keys(inner)?;
    let self_id = inner.origin()?;
    let entries = entries(inner)?;
    let digest = digest_of(&entries);
    let mut held = lock(&inner.fleet.view);
    if let Some((cached, view)) = held.as_ref() {
        if *cached == digest && view.self_id == self_id {
            return Ok(Arc::clone(view));
        }
    }
    let view = Arc::new(compute(&self_id, &keys, &entries));
    *held = Some((digest, Arc::clone(&view)));
    Ok(view)
}

fn add_entry(
    inner: &Inner,
    keys: &Keys,
    runtime_id: &str,
    card: &FleetCard,
    role: Role,
) -> Result<RosterEntry, String> {
    sign_entry(
        keys,
        RosterEntry {
            entry_id: String::new(),
            kind: "add".into(),
            runtime_id: runtime_id.to_owned(),
            card: Some(card.clone()),
            role: Some(role.as_str().into()),
            signer: inner.origin()?,
            clock: next_clock(inner, runtime_id)?,
            sig: String::new(),
        },
    )
}

/// Whether this runtime already vouched for `runtime_id` with `card`.
fn vouched(inner: &Inner, runtime_id: &str, card: &FleetCard) -> Result<bool, String> {
    let self_id = inner.origin()?;
    Ok(entries(inner)?.iter().any(|entry| {
        entry.kind == "add"
            && entry.signer == self_id
            && entry.runtime_id == runtime_id
            && entry.card.as_ref() == Some(card)
    }))
}

/// Vouches for a member whose certificate this runtime pinned: the other
/// end of an outbound link, which answered over that link.
pub(super) fn vouch_for_link(
    inner: &Inner,
    peer: &str,
    from: &MemberRecord,
    pinned_sha256: &str,
) -> Result<(), String> {
    let Some(card) = from.fleet.as_ref() else {
        return Ok(());
    };
    if from.runtime_id != peer
        || !card.cert_sha256.eq_ignore_ascii_case(pinned_sha256)
        || !card_verifies(card)
    {
        return Ok(());
    }
    let Ok(keys) = keys(inner) else {
        return Ok(());
    };
    if vouched(inner, peer, card)? {
        return Ok(());
    }
    let entry = add_entry(inner, &keys, peer, card, Role::Full)?;
    store_entries(inner, vec![entry])?;
    inner.changed("members");
    Ok(())
}

/// Records an invitation's secret and role, so the joiner that redeems it
/// is vouched for with that role.
pub(super) fn note_invite(
    inner: &Inner,
    secret: &str,
    role: Role,
    expires_at: &str,
) -> Result<(), String> {
    inner
        .store
        .connect()?
        .execute(
            "INSERT OR REPLACE INTO fleet_invites(invite, role, expires_at, used_by) VALUES(?1, ?2, ?3, NULL)",
            params![sha256_hex(secret.as_bytes()), role.as_str(), expires_at],
        )
        .map(drop)
        .map_err(sql)
}

/// Keeps what a join needs until the inviter vouched for this runtime: the
/// invitation's hash, which the next exchanges with the inviter carry.
pub(super) fn note_join(inner: &Inner, inviter: &str, secret: &str) -> Result<(), String> {
    inner.store.set_meta(
        &format!("fleet_admission:{inviter}"),
        Some(&sha256_hex(secret.as_bytes())),
    )
}

/// Whether this runtime may invite: a worker may not.
pub(super) fn may_invite(inner: &Inner) -> Result<bool, String> {
    Ok(view(inner).map_or(true, |view| {
        view.role(&view.self_id) == Some(Role::Full) && !view.self_removed()
    }))
}

/// Vouches for a joiner that presents an invitation this runtime issued, or
/// for a member whose link predates fleet keys.
fn admit(
    inner: &Inner,
    peer: &str,
    from: &MemberRecord,
    admission: Option<&str>,
) -> Result<(), String> {
    let Some(card) = from.fleet.as_ref() else {
        return Ok(());
    };
    if from.runtime_id != peer || !card_verifies(card) {
        return Ok(());
    }
    let role = match admission {
        Some(invite) => {
            let row: Option<(String, String, Option<String>)> = inner
                .store
                .connect()?
                .query_row(
                    "SELECT role, expires_at, used_by FROM fleet_invites WHERE invite=?1",
                    [invite],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(sql)?;
            match row {
                Some((role, expires_at, used_by)) => {
                    let open = used_by.as_deref() == Some(peer)
                        || (used_by.is_none() && expires_at.as_str() > inner.now_text().as_str());
                    if !open {
                        return Ok(());
                    }
                    inner
                        .store
                        .connect()?
                        .execute(
                            "UPDATE fleet_invites SET used_by=?1 WHERE invite=?2",
                            params![peer, invite],
                        )
                        .map_err(sql)?;
                    Role::parse(&role).unwrap_or(Role::Worker)
                }
                None => return Ok(()),
            }
        }
        None => {
            let legacy = inner.store.meta(&format!("fleet_legacy:{peer}"))?;
            if legacy.is_none() || channel(peer) != Channel::Device(legacy.unwrap_or_default()) {
                return Ok(());
            }
            Role::Full
        }
    };
    // The device that presented the admission is the peer's from now on.
    if let Channel::Device(device) = channel(peer) {
        inner
            .store
            .set_meta(&format!("fleet_device:{peer}"), Some(&device))?;
    }
    let Ok(keys) = keys(inner) else {
        return Ok(());
    };
    if vouched(inner, peer, card)? {
        return Ok(());
    }
    let entry = add_entry(inner, &keys, peer, card, role)?;
    store_entries(inner, vec![entry])?;
    inner.changed("members");
    Ok(())
}

/// Appends a signed entry that gives the trusted member `runtime_id` a new
/// role. Answers the role it held before, or `None` when it held `role`.
pub(super) fn set_role(
    inner: &Inner,
    runtime_id: &str,
    role: Role,
) -> Result<Option<Role>, String> {
    let keys = keys(inner)?;
    let view = view(inner)?;
    if view.role(&view.self_id) != Some(Role::Full) || view.self_removed() {
        return Err("Only a full member can change a role.".into());
    }
    if runtime_id == view.self_id {
        return Err("A runtime cannot change its own role.".into());
    }
    let member = view
        .members
        .get(runtime_id)
        .filter(|_| !view.removed.contains(runtime_id))
        .ok_or("The fleet has no such member.")?;
    if member.role == role {
        return Ok(None);
    }
    let previous = member.role;
    let entry = add_entry(inner, &keys, runtime_id, &member.card.clone(), role)?;
    store_entries(inner, vec![entry])?;
    inner.changed("members");
    Ok(Some(previous))
}

/// Appends a signed removal of `runtime_id`. Members only it vouched for
/// keep their place through a fresh vouch from this runtime. Answers
/// whether the member was not already removed.
pub(super) fn remove_member(inner: &Inner, runtime_id: &str) -> Result<bool, String> {
    let keys = keys(inner)?;
    let self_id = inner.origin()?;
    if runtime_id == self_id {
        return Err("A runtime cannot remove itself from the fleet.".into());
    }
    let before = view(inner)?;
    if before.removed.contains(runtime_id) {
        return Ok(false);
    }
    let removal = sign_entry(
        &keys,
        RosterEntry {
            entry_id: String::new(),
            kind: "remove".into(),
            runtime_id: runtime_id.to_owned(),
            card: None,
            role: None,
            signer: self_id.clone(),
            clock: next_clock(inner, runtime_id)?,
            sig: String::new(),
        },
    )?;
    let mut all = entries(inner)?;
    all.push(removal.clone());
    let after = compute(&self_id, &keys, &all);
    let mut added = vec![removal];
    for (id, member) in &before.members {
        if *id != self_id && id != runtime_id && !after.members.contains_key(id) {
            added.push(add_entry(inner, &keys, id, &member.card, member.role)?);
        }
    }
    store_entries(inner, added)?;
    inner.changed("members");
    Ok(true)
}

/// The fields of one exchange the fleet fills.
#[derive(Default)]
pub(super) struct Outgoing {
    pub(super) roster: Vec<RosterEntry>,
    pub(super) roster_digest: Option<String>,
    pub(super) sealed: Vec<SealedSecret>,
    pub(super) sealed_have: Option<BTreeMap<String, u64>>,
    pub(super) admission: Option<String>,
    pub(super) more: bool,
}

/// What this runtime sends `peer`: its roster when the peer's differs, the
/// sealed secrets a trusted peer lacks within `budget`, and a pending
/// admission.
pub(super) fn outgoing(
    inner: &Inner,
    peer: &str,
    access: &super::roles::Access,
    budget: &mut usize,
) -> Result<Outgoing, String> {
    if keys(inner).is_err() {
        return Ok(Outgoing::default());
    }
    // Only entries a trusted member signed travel on, so entries no member
    // can verify stop at the first member that holds them.
    let view = view(inner)?;
    let roster: Vec<RosterEntry> = entries(inner)?
        .into_iter()
        .filter(|entry| view.trusts(&entry.signer))
        .collect();
    let digest = digest_of(&roster);
    let (peer_digest, peer_have) = {
        let peers = lock(&inner.fleet.peers);
        let held = peers.get(peer);
        (
            held.and_then(|held| held.roster_digest.clone()),
            held.and_then(|held| held.sealed_have.clone()),
        )
    };
    let mut out = Outgoing {
        roster_digest: Some(digest.clone()),
        admission: inner.store.meta(&format!("fleet_admission:{peer}"))?,
        ..Outgoing::default()
    };
    if peer_digest.as_deref() != Some(digest.as_str()) {
        out.roster = roster;
        *budget = budget.saturating_sub(super::budget_cost(&out.roster));
    }
    let held = sealed_rows(inner)?;
    out.sealed_have = Some(
        held.iter()
            .map(|secret| (secret.secret_id.clone(), secret.version))
            .collect(),
    );
    // A full member takes every secret, a worker only those sealed to it.
    if view.trusts(peer) && !view.self_removed() && proven(inner, peer)? {
        for secret in held {
            if !access.holds_sealed(&secret) {
                continue;
            }
            let lacks = peer_have.as_ref().is_none_or(|have| {
                have.get(&secret.secret_id).copied().unwrap_or(0) < secret.version
            });
            if !lacks {
                continue;
            }
            let bytes = super::budget_cost(&secret);
            if bytes > *budget {
                out.more = true;
                break;
            }
            *budget -= bytes;
            out.sealed.push(secret);
        }
    }
    Ok(out)
}

/// The fleet fields of one exchange a peer sent.
pub(super) struct Incoming {
    pub(super) roster: Vec<RosterEntry>,
    pub(super) roster_digest: Option<String>,
    pub(super) sealed: Vec<SealedSecret>,
    pub(super) sealed_have: Option<BTreeMap<String, u64>>,
    pub(super) admission: Option<String>,
}

/// Takes in a peer's fleet fields: roster entries, an admission it
/// presents, then the sealed secrets that verify. Answers whether the
/// roster or the sealed secrets changed.
pub(super) fn merge(
    inner: &Inner,
    peer: &str,
    from: &MemberRecord,
    incoming: Incoming,
) -> Result<bool, String> {
    {
        let mut peers = lock(&inner.fleet.peers);
        let held = peers.entry(peer.to_owned()).or_default();
        if incoming.roster_digest.is_some() {
            held.roster_digest = incoming.roster_digest;
        }
        if incoming.sealed_have.is_some() {
            held.sealed_have = incoming.sealed_have;
        }
    }
    if keys(inner).is_err() {
        return Ok(false);
    }
    let mut changed = store_entries(inner, incoming.roster)?;
    admit(inner, peer, from, incoming.admission.as_deref())?;
    // The inviter vouched for this runtime: the admission is done.
    let self_id = inner.origin()?;
    if inner
        .store
        .meta(&format!("fleet_admission:{peer}"))?
        .is_some()
        && entries(inner)?
            .iter()
            .any(|entry| entry.kind == "add" && entry.runtime_id == self_id && entry.signer == peer)
    {
        inner
            .store
            .set_meta(&format!("fleet_admission:{peer}"), None)?;
    }
    if !incoming.sealed.is_empty() {
        let view = view(inner)?;
        for secret in incoming.sealed {
            changed |= store_sealed(inner, &view, secret)?;
        }
    }
    Ok(changed)
}

/// Whether a secret's id names its class and owner: `<class>:<owner>`,
/// then optionally `:` and more.
fn id_matches(secret: &SealedSecret) -> bool {
    let prefix = format!("{}:{}", secret.class, secret.owner);
    !secret.class.is_empty()
        && secret
            .class
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        && (secret.secret_id == prefix
            || secret
                .secret_id
                .strip_prefix(&prefix)
                .is_some_and(|rest| rest.starts_with(':')))
}

fn secret_statement(secret: &SealedSecret) -> String {
    let value = serde_json::to_value(secret).unwrap_or(Value::Null);
    format!("{SECRET_LABEL}\n{}", statement_of(value, &["sig"]))
}

/// Keeps a sealed secret a trusted full member signed when it is newer
/// than the held version.
fn store_sealed(inner: &Inner, view: &View, secret: SealedSecret) -> Result<bool, String> {
    if secret.owner == view.self_id || !id_matches(&secret) {
        return Ok(false);
    }
    let Some(owner) = view.members.get(&secret.owner) else {
        return Ok(false);
    };
    if owner.role != Role::Full {
        return Ok(false);
    }
    let signed = decode(&secret.sig).is_some_and(|sig| {
        verify_identity_signature(&owner.identity, secret_statement(&secret).as_bytes(), &sig)
    });
    if !signed {
        return Ok(false);
    }
    put_sealed(inner, &secret)
}

/// Stores a sealed secret when it is newer than the held version.
pub(super) fn put_sealed(inner: &Inner, secret: &SealedSecret) -> Result<bool, String> {
    let held: Option<u64> = inner
        .store
        .connect()?
        .query_row(
            "SELECT version FROM fleet_sealed WHERE secret_id=?1",
            [&secret.secret_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    if held.is_some_and(|held| held >= secret.version) {
        return Ok(false);
    }
    inner
        .store
        .connect()?
        .execute(
            "INSERT INTO fleet_sealed(secret_id, version, owner, record) VALUES(?1, ?2, ?3, ?4) \
             ON CONFLICT(secret_id) DO UPDATE SET version=excluded.version, owner=excluded.owner, record=excluded.record",
            params![
                secret.secret_id,
                secret.version,
                secret.owner,
                serde_json::to_string(secret).map_err(|error| error.to_string())?
            ],
        )
        .map_err(sql)?;
    Ok(true)
}

/// Every sealed secret this runtime holds.
pub(super) fn sealed_rows(inner: &Inner) -> Result<Vec<SealedSecret>, String> {
    let mut rows: Vec<SealedSecret> = inner.store.records("fleet_sealed", "record")?;
    rows.sort_by(|a, b| a.secret_id.cmp(&b.secret_id));
    Ok(rows)
}

pub(super) fn sealed(inner: &Inner, secret_id: &str) -> Result<Option<SealedSecret>, String> {
    inner
        .store
        .record("fleet_sealed", "secret_id", "record", secret_id)
}

/// The header each recipient's copy authenticates: everything that names
/// the secret, its version, its owner and the recipient.
#[derive(Clone, serde::Serialize)]
struct SealHeader {
    label: &'static str,
    secret_id: String,
    version: u64,
    owner: String,
    class: String,
    recipient: String,
    ephemeral: String,
}

fn header(secret: &SealedSecret, recipient: &str) -> SealHeader {
    SealHeader {
        label: SEAL_LABEL,
        secret_id: secret.secret_id.clone(),
        version: secret.version,
        owner: secret.owner.clone(),
        class: secret.class.clone(),
        recipient: recipient.to_owned(),
        ephemeral: secret.ephemeral.clone(),
    }
}

fn copy_key(
    shared: &[u8; 32],
    header: &SealHeader,
) -> Result<relay_encryption::DirectionKey, String> {
    relay_encryption::derive_key(
        shared,
        SEAL_LABEL.as_bytes(),
        relay_encryption::associated_data(header).as_bytes(),
    )
    .map_err(|error| error.to_string())
}

/// Seals `plaintext` as version `version` of `secret_id` to each recipient
/// in `recipients` (runtime id and fleet key), and signs it.
pub(super) fn seal(
    inner: &Inner,
    keys: &Keys,
    secret_id: &str,
    class: &str,
    version: u64,
    recipients: &[(String, String)],
    plaintext: &[u8],
) -> Result<SealedSecret, String> {
    let ephemeral = AgreementKeyPair::generate().map_err(|error| error.to_string())?;
    let mut secret = SealedSecret {
        secret_id: secret_id.to_owned(),
        version,
        owner: inner.origin()?,
        class: class.to_owned(),
        ephemeral: ephemeral.public_key(),
        recipients: BTreeMap::new(),
        sealed_at: inner.now_text(),
        sig: String::new(),
    };
    for (recipient, fleet_pub) in recipients {
        let shared = ephemeral
            .agree(fleet_pub)
            .map_err(|error| error.to_string())?;
        let header = header(&secret, recipient);
        let key = copy_key(&shared, &header)?;
        let sealed = relay_encryption::seal_fresh(&key, &header, plaintext)
            .map_err(|error| error.to_string())?;
        secret.recipients.insert(
            recipient.clone(),
            SealedCopy {
                nonce: sealed.nonce,
                ciphertext: sealed.ciphertext,
            },
        );
    }
    secret.sig = encode(&keys.signer.sign(secret_statement(&secret).as_bytes())?);
    Ok(secret)
}

/// Opens this runtime's copy of a sealed secret. Answers `None` when the
/// secret holds no copy for it.
pub(super) fn open(
    keys: &Keys,
    self_id: &str,
    secret: &SealedSecret,
) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    let Some(copy) = secret.recipients.get(self_id) else {
        return Ok(None);
    };
    let shared = keys
        .fleet
        .agree(&secret.ephemeral)
        .map_err(|error| error.to_string())?;
    let header = header(secret, self_id);
    let key = copy_key(&shared, &header)?;
    let sealed = relay_encryption::Sealed {
        contract_version: relay_encryption::CONTRACT_VERSION.to_owned(),
        header,
        nonce: copy.nonce.clone(),
        ciphertext: copy.ciphertext.clone(),
    };
    relay_encryption::open_sealed(&key, &sealed)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// The full members and the workers a secret may reach. `agents` names the
/// agents the secret serves; a worker that runs one of them may hold it.
pub(super) fn recipient_set(inner: &Inner, agents: &[String]) -> Result<RecipientSet, String> {
    let view = view(inner)?;
    let mut set = RecipientSet {
        full: view.full_recipients(),
        workers: Vec::new(),
    };
    for (id, member) in &view.members {
        if *id == view.self_id || member.role != Role::Worker {
            continue;
        }
        let runs_one = agents.iter().any(|agent| {
            super::directory::primary_of(inner, agent).as_deref() == Some(id.as_str())
        });
        if runs_one {
            set.workers.push(id.clone());
        }
    }
    Ok(set)
}

/// The fleet facts `swarm.members` shows for one member.
pub(super) fn member_view(inner: &Inner, runtime_id: &str) -> Value {
    let Ok(view) = view(inner) else {
        return Value::Null;
    };
    serde_json::json!({
        "trusted": view.trusts(runtime_id),
        "role": view.role(runtime_id).map(Role::as_str),
        "removed": view.removed.contains(runtime_id),
    })
}

/// Appends one entry to the fleet's audit log.
pub(super) fn audit(
    inner: &Inner,
    kind: &str,
    runtime_id: Option<&str>,
    record: Value,
) -> Result<(), String> {
    super::audit::append(inner, kind, runtime_id, record)
}
