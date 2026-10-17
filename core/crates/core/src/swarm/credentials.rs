//! The secrets members seal to each other, by class:
//!
//! - **models** (`models:<owner>`): the owner's model connections, the
//!   bundle a desktop's `models.share` sends. Each full member applies it
//!   under the device `swarm:<owner>`. A rotating sign-in travels as a lease
//!   without its refresh token, so the owner stays its one refresher and
//!   reseals a fresh lease before [`shared_models::LEASE_MARGIN_MS`].
//! - **vault** (`vault:<issuer>:<member>`): a model vault machine token that
//!   a desktop with its own account session issued for one full member.
//! - **mcp** (`mcp:<owner>`): the MCP bearer tokens the owner saved. The
//!   owner keeps refreshing what it signed in to.
//! - **handover** (`handover:<owner>:<sign-in>`): a sign-in with its refresh
//!   token, which `fleet.hold_sign_in` moves to a new refresher.
//! - **lease** (`lease:<owner>:<worker>`): for a member the roster makes a
//!   worker, the owner's connections that the agents it runs name, each
//!   without a refresh token, sealed to that worker alone.
//!
//! A removal in the roster makes every member reseal without the removed
//! member, drop its bundle, revoke the vault token issued for it, and log
//! each connection it held so the owner can rotate it at the provider.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::fleet::{self, Keys, Role, View};
use super::store::sql;
use super::{lock, Inner, MemberRecord, Message, SealedSecret};
use crate::shared_models::{self, Bundle, CloudLink};

/// How often the worker reseals this runtime's own secrets.
const PASS_INTERVAL: Duration = Duration::from_secs(60);
/// How long a member the model vault refused a token waits before this
/// runtime asks for one again.
const VAULT_RETRY: chrono::Duration = chrono::Duration::hours(1);

/// When the last reseal pass ran, and whether one runs or is due.
#[derive(Default)]
pub(super) struct PassClock {
    last: Option<Instant>,
    running: bool,
    dirty: bool,
    interval: Option<Duration>,
}

fn agent_directory(inner: &Inner) -> std::path::PathBuf {
    crate::state_root::agent_directory(&inner.profile)
}

/// The device a member's sealed connections apply under.
fn device(owner: &str) -> String {
    format!("swarm:{owner}")
}

fn sharing(inner: &Inner) -> Result<bool, String> {
    Ok(inner.store.meta("fleet_share_models")?.as_deref() == Some("1"))
}

pub(super) fn set_interval(inner: &Inner, interval: Duration) {
    lock(&inner.fleet.pass).interval = Some(interval);
}

/// Asks for a reseal pass soon.
fn mark_dirty(inner: &Inner) {
    lock(&inner.fleet.pass).dirty = true;
    inner.nudge();
}

/// Starts a reseal pass on its own thread when one is due, so a sign-in
/// refresh never holds up the swarm worker.
pub(super) fn maybe_pass(inner: &Arc<Inner>) {
    {
        let mut clock = lock(&inner.fleet.pass);
        let interval = clock.interval.unwrap_or(PASS_INTERVAL);
        let due = clock.dirty || clock.last.is_none_or(|last| last.elapsed() >= interval);
        if clock.running || !due {
            return;
        }
        clock.running = true;
        clock.dirty = false;
    }
    let inner = Arc::clone(inner);
    std::thread::spawn(move || {
        if let Err(error) = pass(&inner) {
            crate::runtime_eprintln!("muniment-runtime: swarm fleet pass failed: {error}");
        }
        let mut clock = lock(&inner.fleet.pass);
        clock.running = false;
        clock.last = Some(Instant::now());
    });
}

fn fleet_pub(view: &View, runtime_id: &str) -> Option<(String, String)> {
    view.members
        .get(runtime_id)
        .map(|member| (runtime_id.to_owned(), member.card.fleet_pub.clone()))
}

/// Reseals this runtime's own secrets whose content or recipients changed:
/// its model connections and MCP tokens while sharing is on, and the model
/// vault tokens it issues. Holds the refresh lock throughout, so a sign-in
/// it hands over is never refreshed here at the same time.
pub(super) fn pass(inner: &Inner) -> Result<(), String> {
    let _refresh = lock(&inner.fleet.refresh);
    let keys = fleet::keys(inner)?;
    let view = fleet::view(inner)?;
    if view.self_removed() || view.role(&view.self_id) != Some(Role::Full) {
        return Ok(());
    }
    let recipients: Vec<(String, String)> = view
        .full_recipients()
        .iter()
        .filter_map(|id| fleet_pub(&view, id))
        .collect();
    let agent = agent_directory(inner);
    let self_id = view.self_id.clone();
    let models_id = format!("models:{self_id}");
    let mcp_id = format!("mcp:{self_id}");
    if !sharing(inner)? {
        withdraw(inner, &keys, &models_id, "models")?;
        withdraw(inner, &keys, &mcp_id, "mcp")?;
        seal_leases(inner, &keys, &view, &Bundle::default())?;
        return Ok(());
    }
    let now_ms = inner.now().timestamp_millis();
    let bundle = shared_models::bundle(&agent, now_ms)?;
    let names = shared_models::connection_names(&bundle);
    let plaintext = Zeroizing::new(
        serde_json::to_vec(&json!({"bundle": bundle})).map_err(|error| error.to_string())?,
    );
    reseal(
        inner,
        &keys,
        &models_id,
        "models",
        &recipients,
        &plaintext,
        names,
    )?;
    let tokens = own_mcp_tokens(inner)?;
    let names: Vec<String> = tokens.keys().map(|key| format!("mcp:{key}")).collect();
    let plaintext = Zeroizing::new(
        serde_json::to_vec(&json!({"tokens": tokens})).map_err(|error| error.to_string())?,
    );
    reseal(inner, &keys, &mcp_id, "mcp", &recipients, &plaintext, names)?;
    seal_leases(inner, &keys, &view, &bundle)?;
    issue_vault_tokens(inner, &keys, &view, &agent)
}

/// The Pi providers the agents name, and whether one runs on the router or
/// on no model of its own.
fn providers_of(inner: &Inner, agents: &[String]) -> (BTreeSet<String>, bool) {
    let mut providers = BTreeSet::new();
    let mut router = false;
    for agent in agents {
        let provider = super::directory::entry(inner, agent)
            .and_then(|entry| entry.definition)
            .and_then(|definition| definition["model"]["provider"].as_str().map(str::to_owned));
        match provider {
            Some(provider) if provider != muniment_models::registry::ROUTER_PROVIDER => {
                providers.insert(provider);
            }
            _ => router = true,
        }
    }
    (providers, router)
}

/// Seals each worker a lease of the connections the agents it runs name,
/// and withdraws the lease of a member that no longer is a worker.
fn seal_leases(inner: &Inner, keys: &Keys, view: &View, bundle: &Bundle) -> Result<(), String> {
    let agents: Vec<String> = super::directory::all(inner)?
        .into_iter()
        .filter(|entry| !entry.deleted)
        .map(|entry| entry.agent_id)
        .collect();
    let workers = fleet::recipient_set(inner, &agents)?.workers;
    let prefix = format!("lease:{}:", view.self_id);
    for worker in &workers {
        let Some(recipient) = fleet_pub(view, worker) else {
            continue;
        };
        let runs: Vec<String> = agents
            .iter()
            .filter(|agent| {
                super::directory::primary_of(inner, agent).as_deref() == Some(worker.as_str())
            })
            .cloned()
            .collect();
        let (providers, router) = providers_of(inner, &runs);
        let lease = shared_models::lease(bundle.clone(), &providers, router);
        let names = shared_models::connection_names(&lease);
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&json!({"bundle": lease})).map_err(|error| error.to_string())?,
        );
        reseal(
            inner,
            keys,
            &format!("{prefix}{worker}"),
            "lease",
            &[recipient],
            &plaintext,
            names,
        )?;
    }
    let held: Vec<String> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare("SELECT secret_id FROM fleet_own WHERE secret_id LIKE 'lease:%'")
            .map_err(sql)?;
        let rows = statement.query_map([], |row| row.get(0)).map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    for secret_id in held {
        let worker = secret_id.strip_prefix(&prefix).unwrap_or_default();
        if !workers.iter().any(|held| held == worker) {
            withdraw(inner, keys, &secret_id, "lease")?;
        }
    }
    Ok(())
}

fn own_digest(inner: &Inner, secret_id: &str) -> Result<Option<String>, String> {
    inner
        .store
        .connect()?
        .query_row(
            "SELECT digest FROM fleet_own WHERE secret_id=?1",
            [secret_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)
}

/// Seals a new version of one of this runtime's secrets when its content or
/// its recipients changed. Answers whether it sealed one.
fn reseal(
    inner: &Inner,
    keys: &Keys,
    secret_id: &str,
    class: &str,
    recipients: &[(String, String)],
    plaintext: &[u8],
    names: Vec<String>,
) -> Result<bool, String> {
    let mut ids: Vec<&str> = recipients.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort_unstable();
    let mut hasher = Sha256::new();
    hasher.update(plaintext);
    hasher.update(b"\n");
    hasher.update(ids.join(",").as_bytes());
    let digest = hex::encode(hasher.finalize());
    if own_digest(inner, secret_id)?.as_deref() == Some(digest.as_str()) {
        return Ok(false);
    }
    let version = fleet::sealed(inner, secret_id)?.map_or(0, |held| held.version) + 1;
    let secret = fleet::seal(
        inner, keys, secret_id, class, version, recipients, plaintext,
    )?;
    fleet::put_sealed(inner, &secret)?;
    inner
        .store
        .connect()?
        .execute(
            "INSERT INTO fleet_own(secret_id, digest) VALUES(?1, ?2) \
             ON CONFLICT(secret_id) DO UPDATE SET digest=excluded.digest",
            params![secret_id, digest],
        )
        .map_err(sql)?;
    fleet::audit(
        inner,
        "sealed",
        None,
        json!({"secret_id": secret_id, "version": version, "recipients": ids, "keys": names}),
    )?;
    inner.nudge();
    Ok(true)
}

/// Seals one empty version with no recipients of a secret this runtime no
/// longer shares, so every member drops what it applied.
fn withdraw(inner: &Inner, keys: &Keys, secret_id: &str, class: &str) -> Result<(), String> {
    if own_digest(inner, secret_id)?.is_none() {
        return Ok(());
    }
    reseal(inner, keys, secret_id, class, &[], b"{}", Vec::new())?;
    inner
        .store
        .connect()?
        .execute("DELETE FROM fleet_own WHERE secret_id=?1", [secret_id])
        .map(drop)
        .map_err(sql)
}

/// The MCP tokens this runtime saved itself, without those other members
/// share here.
fn own_mcp_tokens(inner: &Inner) -> Result<serde_json::Map<String, Value>, String> {
    let mut tokens = crate::extend::mcp_tokens(&inner.profile);
    for state in opened_states(inner, "mcp:")? {
        if let Some(written) = state["written"].as_object() {
            for (key, digest) in written {
                if tokens.get(key).map(crate::extend::token_digest).as_deref() == digest.as_str() {
                    tokens.remove(key);
                }
            }
        }
    }
    Ok(tokens)
}

fn member_record(inner: &Inner, runtime_id: &str) -> Option<MemberRecord> {
    inner
        .store
        .record("members", "runtime_id", "record", runtime_id)
        .ok()
        .flatten()
}

/// The machine a model vault machine token request names.
fn machine(inner: &Inner, view: &View, runtime_id: &str) -> Value {
    let record = member_record(inner, runtime_id);
    let name = record
        .as_ref()
        .map(|record| record.runtime_name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| crate::attach::DEFAULT_RUNTIME_NAME.to_owned());
    let mut endpoints: Vec<Value> = record
        .iter()
        .flat_map(|record| record.endpoints.iter())
        .filter_map(|endpoint| {
            let (host, port) = endpoint.rsplit_once(':')?;
            let host = host.trim_start_matches('[').trim_end_matches(']');
            Some(json!({"host": host, "port": port.parse::<u16>().ok()?}))
        })
        .take(16)
        .collect();
    // The contract needs one endpoint. A member that only dials out has none
    // and registers as loopback.
    if endpoints.is_empty() {
        endpoints.push(json!({"host": "localhost", "port": 1}));
    }
    json!({
        "runtime_id": runtime_id,
        "runtime_name": name,
        "certificate_sha256": view.members.get(runtime_id).map(|member| member.card.cert_sha256.clone()),
        "endpoints": endpoints,
    })
}

/// Issues a model vault machine token for each full member that lacks one,
/// when this runtime holds the vault with its own account session.
fn issue_vault_tokens(
    inner: &Inner,
    keys: &Keys,
    view: &View,
    agent: &std::path::Path,
) -> Result<(), String> {
    if !shared_models::holds_native_vault(agent)? {
        return Ok(());
    }
    let Some(session) = inner.host.cloud_session() else {
        return Ok(());
    };
    for member in view.full_recipients() {
        let issued: Option<String> = inner
            .store
            .connect()?
            .query_row(
                "SELECT pairing_id FROM fleet_vault WHERE runtime_id=?1",
                [&member],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)?;
        if issued.is_some() {
            continue;
        }
        let Some(recipient) = fleet_pub(view, &member) else {
            continue;
        };
        let refused_key = format!("fleet_vault_refused:{member}");
        let refused: Option<Value> = inner
            .store
            .meta(&refused_key)?
            .and_then(|text| serde_json::from_str(&text).ok());
        let retry_due = refused.as_ref().is_none_or(|refused| {
            refused["at"]
                .as_str()
                .and_then(super::parse_time)
                .is_none_or(|at| inner.now() - at >= VAULT_RETRY)
        });
        if !retry_due {
            continue;
        }
        let (link, pairing_id) =
            match shared_models::issue_machine_token(&session, &machine(inner, view, &member)) {
                Ok(issued) => issued,
                Err(error) => {
                    // The account's pairing limit, or another refusal: the
                    // member keeps the leases this runtime seals it, and
                    // the token is asked for again later.
                    crate::runtime_eprintln!(
                        "muniment-runtime: model vault token for {member} failed: {error}"
                    );
                    if refused.is_none() {
                        fleet::audit(
                            inner,
                            "vault_token_refused",
                            Some(&member),
                            json!({"error": error}),
                        )?;
                    }
                    inner.store.set_meta(
                        &refused_key,
                        Some(&json!({"at": inner.now_text(), "error": error}).to_string()),
                    )?;
                    continue;
                }
            };
        inner.store.set_meta(&refused_key, None)?;
        inner
            .store
            .connect()?
            .execute(
                "INSERT OR REPLACE INTO fleet_vault(runtime_id, pairing_id, issued_at) VALUES(?1, ?2, ?3)",
                params![member, pairing_id, inner.now_text()],
            )
            .map_err(sql)?;
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&json!({"cloud": link})).map_err(|error| error.to_string())?,
        );
        reseal(
            inner,
            keys,
            &format!("vault:{}:{member}", view.self_id),
            "vault",
            &[recipient],
            &plaintext,
            vec![format!("vault-token:{member}")],
        )?;
        fleet::audit(
            inner,
            "vault_token_issued",
            Some(&member),
            json!({"pairing_id": pairing_id}),
        )?;
    }
    Ok(())
}

/// How each full member reaches the model vault through this runtime: its
/// own machine token, or leases from this runtime while the account's
/// pairing limit or another refusal keeps it from one.
fn vault_states(inner: &Inner, view: &View) -> Result<Vec<Value>, String> {
    let mut states = Vec::new();
    for member in view.full_recipients() {
        let issued: Option<String> = inner
            .store
            .connect()?
            .query_row(
                "SELECT issued_at FROM fleet_vault WHERE runtime_id=?1",
                [&member],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)?;
        let refused: Option<Value> = inner
            .store
            .meta(&format!("fleet_vault_refused:{member}"))?
            .and_then(|text| serde_json::from_str(&text).ok());
        let state = match (&issued, &refused) {
            (Some(at), _) => json!({"runtime_id": member, "state": "token", "issued_at": at}),
            (None, Some(refused)) => json!({
                "runtime_id": member,
                "state": "leases",
                "reason": refused["error"],
                "since": refused["at"],
            }),
            (None, None) => continue,
        };
        states.push(state);
    }
    Ok(states)
}

/// What this runtime applied from each sealed secret: its version and state.
fn opened(inner: &Inner) -> Result<BTreeMap<String, (u64, Value)>, String> {
    let connection = inner.store.connect()?;
    let mut statement = connection
        .prepare("SELECT secret_id, version, state FROM fleet_opened")
        .map_err(sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(sql)?;
    let mut opened = BTreeMap::new();
    for row in rows {
        let (id, version, state) = row.map_err(sql)?;
        opened.insert(
            id,
            (version, serde_json::from_str(&state).unwrap_or(Value::Null)),
        );
    }
    Ok(opened)
}

fn opened_states(inner: &Inner, prefix: &str) -> Result<Vec<Value>, String> {
    Ok(opened(inner)?
        .into_iter()
        .filter(|(id, _)| id.starts_with(prefix))
        .map(|(_, (_, state))| state)
        .collect())
}

fn mark_opened(
    inner: &Inner,
    secret_id: &str,
    version: u64,
    owner: &str,
    state: &Value,
) -> Result<(), String> {
    inner
        .store
        .connect()?
        .execute(
            "INSERT INTO fleet_opened(secret_id, version, owner, state) VALUES(?1, ?2, ?3, ?4) \
             ON CONFLICT(secret_id) DO UPDATE SET version=excluded.version, owner=excluded.owner, state=excluded.state",
            params![secret_id, version, owner, state.to_string()],
        )
        .map(drop)
        .map_err(sql)
}

fn open_json(keys: &Keys, self_id: &str, secret: &SealedSecret) -> Result<Option<Value>, String> {
    Ok(match fleet::open(keys, self_id, secret)? {
        Some(bytes) => Some(
            serde_json::from_slice(&bytes)
                .map_err(|_| "A sealed secret holds content this version cannot read.")?,
        ),
        None => None,
    })
}

/// Brings this runtime in line with the roster and the sealed secrets: the
/// removals it has not acted on yet, then each new version it holds a copy
/// of, or lost its copy in.
pub(super) fn settle(inner: &Inner) -> Result<(), String> {
    let (Ok(keys), Ok(view)) = (fleet::keys(inner), fleet::view(inner)) else {
        return Ok(());
    };
    act_on_removals(inner, &view)?;
    act_on_roles(inner, &view)?;
    let self_id = view.self_id.clone();
    let opened = opened(inner)?;
    let mut models = BTreeSet::new();
    let mut mcp = BTreeSet::new();
    for secret in fleet::sealed_rows(inner)? {
        if secret.owner == self_id
            || !view.trusts(&secret.owner)
            || opened.get(&secret.secret_id).map(|(version, _)| *version) == Some(secret.version)
        {
            continue;
        }
        match secret.class.as_str() {
            "models" => {
                models.insert(secret.owner.clone());
            }
            "vault" if secret.secret_id == format!("vault:{}:{self_id}", secret.owner) => {
                models.insert(secret.owner.clone());
            }
            "lease" if secret.secret_id == format!("lease:{}:{self_id}", secret.owner) => {
                models.insert(secret.owner.clone());
            }
            "mcp" => {
                mcp.insert(secret.owner.clone());
            }
            "handover" => receive_handover(inner, &keys, &self_id, &secret)?,
            // Another member's vault token, or a class a later stage opens
            // where it is used.
            _ => mark_opened(
                inner,
                &secret.secret_id,
                secret.version,
                &secret.owner,
                &Value::Null,
            )?,
        }
    }
    for owner in models {
        if let Err(error) = apply_models(inner, &keys, &view, &owner) {
            crate::runtime_eprintln!(
                "muniment-runtime: model connections from {owner} stayed out: {error}"
            );
        }
    }
    for owner in mcp {
        if let Err(error) = apply_mcp(inner, &keys, &self_id, &owner) {
            crate::runtime_eprintln!(
                "muniment-runtime: MCP tokens from {owner} stayed out: {error}"
            );
        }
    }
    Ok(())
}

/// Applies one owner's model connections and the vault token it issued for
/// this runtime, together, as the device `swarm:<owner>`.
fn apply_models(inner: &Inner, keys: &Keys, view: &View, owner: &str) -> Result<(), String> {
    let self_id = &view.self_id;
    let models = fleet::sealed(inner, &format!("models:{owner}"))?;
    let vault = fleet::sealed(inner, &format!("vault:{owner}:{self_id}"))?;
    let lease = fleet::sealed(inner, &format!("lease:{owner}:{self_id}"))?;
    let mut bundle = Bundle::default();
    // A full member takes the owner's connections; a worker its lease.
    for secret in models.iter().chain(lease.iter()) {
        if let Some(value) = open_json(keys, self_id, secret)? {
            bundle = serde_json::from_value(value["bundle"].clone())
                .map_err(|_| "The shared connections are not valid.")?;
            break;
        }
    }
    if let Some(secret) = &vault {
        if let Some(value) = open_json(keys, self_id, secret)? {
            bundle.cloud = serde_json::from_value::<CloudLink>(value["cloud"].clone()).ok();
        }
    }
    let mut names = shared_models::connection_names(&bundle);
    let linked = bundle.cloud.is_some();
    if linked {
        names.push(format!("vault-token:{self_id}"));
    }
    let agent = agent_directory(inner);
    let name = member_record(inner, owner)
        .map(|record| record.runtime_name)
        .unwrap_or_default();
    let applied = shared_models::apply(&agent, &device(owner), &name, bundle);
    let state = match &applied {
        Ok(_) => json!({"keys": names}),
        Err(error) => json!({"keys": names, "error": error}),
    };
    for secret in models.iter().chain(vault.iter()).chain(lease.iter()) {
        mark_opened(inner, &secret.secret_id, secret.version, owner, &state)?;
        fleet::audit(
            inner,
            "opened",
            Some(owner),
            json!({
                "secret_id": secret.secret_id,
                "version": secret.version,
                "recipients": secret.recipients.keys().collect::<Vec<_>>(),
                "keys": names,
            }),
        )?;
    }
    applied?;
    if linked {
        std::thread::spawn(move || {
            if let Err(error) = shared_models::refresh_vault_leases(&agent, || None) {
                crate::runtime_eprintln!("muniment-runtime: model vault leases: {error}");
            }
        });
    }
    Ok(())
}

/// Applies the MCP tokens one owner shares, in place of those it shared
/// before.
fn apply_mcp(inner: &Inner, keys: &Keys, self_id: &str, owner: &str) -> Result<(), String> {
    let secret_id = format!("mcp:{owner}");
    let Some(secret) = fleet::sealed(inner, &secret_id)? else {
        return Ok(());
    };
    let tokens = open_json(keys, self_id, &secret)?
        .and_then(|value| value["tokens"].as_object().cloned())
        .unwrap_or_default();
    let written = share_mcp(inner, &secret_id, &tokens)?;
    let names: Vec<String> = tokens.keys().map(|key| format!("mcp:{key}")).collect();
    mark_opened(
        inner,
        &secret_id,
        secret.version,
        owner,
        &json!({"keys": names, "written": written}),
    )?;
    fleet::audit(
        inner,
        "opened",
        Some(owner),
        json!({
            "secret_id": secret_id,
            "version": secret.version,
            "recipients": secret.recipients.keys().collect::<Vec<_>>(),
            "keys": names,
        }),
    )
}

/// Replaces the MCP tokens `secret_id` wrote here with `tokens`.
fn share_mcp(
    inner: &Inner,
    secret_id: &str,
    tokens: &serde_json::Map<String, Value>,
) -> Result<BTreeMap<String, String>, String> {
    let previous: BTreeMap<String, String> = opened(inner)?
        .remove(secret_id)
        .and_then(|(_, state)| serde_json::from_value(state["written"].clone()).ok())
        .unwrap_or_default();
    crate::extend::share_mcp_tokens(&inner.profile, &previous, tokens)
}

/// Keeps a sign-in another member handed to this runtime as its own, so
/// this runtime becomes its one refresher.
fn receive_handover(
    inner: &Inner,
    keys: &Keys,
    self_id: &str,
    secret: &SealedSecret,
) -> Result<(), String> {
    let Some(value) = open_json(keys, self_id, secret)? else {
        return mark_opened(
            inner,
            &secret.secret_id,
            secret.version,
            &secret.owner,
            &Value::Null,
        );
    };
    let name = value["account"].as_str().unwrap_or_default().to_owned();
    let installed = shared_models::install_sign_in(&agent_directory(inner), &name, &value["entry"]);
    mark_opened(
        inner,
        &secret.secret_id,
        secret.version,
        &secret.owner,
        &json!({"keys": [name]}),
    )?;
    installed?;
    fleet::audit(
        inner,
        "hold_received",
        Some(&secret.owner),
        json!({"account": name}),
    )?;
    mark_dirty(inner);
    Ok(())
}

/// Acts once on each member the roster removed since the last time: drops
/// what it shared here, revokes the vault token issued for it, and logs each
/// connection it held.
fn act_on_removals(inner: &Inner, view: &View) -> Result<(), String> {
    let known: BTreeSet<String> = inner
        .store
        .meta("fleet_removed")?
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if known == view.removed {
        return Ok(());
    }
    // A member that joined again leaves the list, so a later removal acts.
    let fresh: Vec<String> = view.removed.difference(&known).cloned().collect();
    for removed in &fresh {
        if *removed == view.self_id {
            // This runtime left the fleet: what others shared here goes.
            let owners: BTreeSet<String> = opened(inner)?
                .keys()
                .filter_map(|id| id.split(':').nth(1).map(str::to_owned))
                .collect();
            for owner in owners {
                drop_owner(inner, &owner)?;
            }
            continue;
        }
        drop_owner(inner, removed)?;
        revoke_vault_token(inner, &view.self_id, removed)?;
        fleet::audit(
            inner,
            "revoked",
            Some(removed),
            json!({"keys": held_keys(inner, removed)?}),
        )?;
    }
    inner.store.set_meta(
        "fleet_removed",
        Some(&serde_json::to_string(&view.removed).map_err(|error| error.to_string())?),
    )?;
    mark_dirty(inner);
    Ok(())
}

/// Acts once on each role change the roster shows: the member's sync
/// starts over, so what it may hold now travels to it, and this runtime
/// reseals. A change of this runtime's own role takes the records the new
/// role holds afresh and drops what it may no longer hold.
fn act_on_roles(inner: &Inner, view: &View) -> Result<(), String> {
    let current: BTreeMap<String, String> = view
        .members
        .iter()
        .map(|(id, member)| (id.clone(), member.role.as_str().to_owned()))
        .collect();
    let known: Option<BTreeMap<String, String>> = inner
        .store
        .meta("fleet_roles")?
        .and_then(|text| serde_json::from_str(&text).ok());
    if known.as_ref() == Some(&current) {
        return Ok(());
    }
    for (id, role) in &current {
        let before = known.as_ref().and_then(|known| known.get(id));
        if before.is_none_or(|before| before == role) {
            continue;
        }
        if *id == view.self_id {
            let role = Role::parse(role).unwrap_or(Role::Worker);
            super::roles::change_own_role(inner, role)?;
        } else {
            inner.forget_peer(id);
        }
    }
    inner.store.set_meta(
        "fleet_roles",
        Some(&serde_json::to_string(&current).map_err(|error| error.to_string())?),
    )?;
    mark_dirty(inner);
    Ok(())
}

/// Removes what `owner` shared here and the secrets it sealed.
fn drop_owner(inner: &Inner, owner: &str) -> Result<(), String> {
    let agent = agent_directory(inner);
    if let Err(error) = shared_models::drop_device(&agent, &device(owner)) {
        crate::runtime_eprintln!(
            "muniment-runtime: shared connections from {owner} stayed: {error}"
        );
    }
    share_mcp(inner, &format!("mcp:{owner}"), &serde_json::Map::new())?;
    let connection = inner.store.connect()?;
    connection
        .execute("DELETE FROM fleet_opened WHERE owner=?1", [owner])
        .map_err(sql)?;
    connection
        .execute("DELETE FROM fleet_sealed WHERE owner=?1", [owner])
        .map_err(sql)?;
    Ok(())
}

/// Revokes the model vault token this runtime issued for `member`.
fn revoke_vault_token(inner: &Inner, self_id: &str, member: &str) -> Result<(), String> {
    let pairing: Option<String> = inner
        .store
        .connect()?
        .query_row(
            "SELECT pairing_id FROM fleet_vault WHERE runtime_id=?1",
            [member],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    let Some(pairing_id) = pairing else {
        return Ok(());
    };
    let secret_id = format!("vault:{self_id}:{member}");
    let revoked = match inner.host.cloud_session() {
        Some(session) => shared_models::revoke_machine_pairing(&session, &pairing_id),
        None => Err("No one is signed in to revoke the model vault token.".into()),
    };
    let connection = inner.store.connect()?;
    connection
        .execute("DELETE FROM fleet_vault WHERE runtime_id=?1", [member])
        .map_err(sql)?;
    connection
        .execute("DELETE FROM fleet_sealed WHERE secret_id=?1", [&secret_id])
        .map_err(sql)?;
    connection
        .execute("DELETE FROM fleet_own WHERE secret_id=?1", [&secret_id])
        .map_err(sql)?;
    fleet::audit(
        inner,
        "vault_token_revoked",
        Some(member),
        json!({"pairing_id": pairing_id, "error": revoked.err()}),
    )
}

/// Every connection a member held: the names in each logged seal or open
/// whose recipients named it, and the secrets this runtime cannot read that
/// named it.
fn held_keys(inner: &Inner, member: &str) -> Result<Vec<String>, String> {
    let mut keys = BTreeSet::new();
    for entry in super::audit::entries(inner)? {
        if !matches!(entry.kind.as_str(), "sealed" | "opened") {
            continue;
        }
        let record = entry.record;
        let named = record["recipients"]
            .as_array()
            .is_some_and(|recipients| recipients.iter().any(|id| id == member));
        if named {
            for key in record["keys"].as_array().into_iter().flatten() {
                if let Some(key) = key.as_str() {
                    keys.insert(key.to_owned());
                }
            }
        }
    }
    Ok(keys.into_iter().collect())
}

/// The member whose model connections name `account`, as this runtime
/// applied them.
fn holder_of(inner: &Inner, account: &str) -> Result<Option<String>, String> {
    Ok(opened(inner)?.into_iter().find_map(|(id, (_, state))| {
        let owner = id.strip_prefix("models:")?;
        state["keys"]
            .as_array()?
            .iter()
            .any(|key| key == account)
            .then(|| owner.to_owned())
    }))
}

fn valid_account(account: &str) -> bool {
    let id = account
        .strip_prefix("pi:")
        .or_else(|| account.strip_prefix("router:"))
        .unwrap_or_default();
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// `fleet.hold_sign_in {account}`: asks the member that refreshes a shared
/// sign-in to hand it to this runtime, and turns sharing on here so the
/// other members keep their leases.
pub(super) fn hold_operation(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        account: String,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    if !valid_account(&body.account) {
        return Err("Name the sign-in as pi:<provider> or router:<account id>.".into());
    }
    let view = fleet::view(inner)?;
    if view.role(&view.self_id) != Some(Role::Full) {
        return Err("A worker cannot hold a sign-in.".into());
    }
    if shared_models::holds_sign_in(&agent_directory(inner), &body.account)? {
        return Ok(json!({"status": "held", "holder": view.self_id}));
    }
    let holder = holder_of(inner, &body.account)?
        .filter(|holder| view.trusts(holder))
        .ok_or("No fleet member shares that sign-in.")?;
    // The new holder shares the sign-in on, so the members keep their leases.
    inner.store.set_meta("fleet_share_models", Some("1"))?;
    let message = super::messaging::new_message(
        inner,
        "hold_request",
        &holder,
        None,
        None,
        0,
        json!({"account": body.account}),
    );
    super::messaging::send(inner, message)?;
    Ok(json!({"status": "requested", "holder": holder}))
}

/// A member asked to refresh one of this runtime's sign-ins: hands the
/// sign-in over, sealed to that member alone, and stops refreshing it.
pub(super) fn on_hold_request(inner: &Inner, message: &Message) -> Result<(), String> {
    let account = message.body["account"]
        .as_str()
        .filter(|account| valid_account(account))
        .ok_or("The request is invalid.")?
        .to_owned();
    let keys = fleet::keys(inner)?;
    let view = fleet::view(inner)?;
    let to = message.from_runtime.clone();
    let recipient = fleet_pub(&view, &to)
        .filter(|_| to != view.self_id && view.role(&to) == Some(Role::Full))
        .ok_or("Only a full fleet member can hold a sign-in.")?;
    let agent = agent_directory(inner);
    let entry = {
        let _refresh = lock(&inner.fleet.refresh);
        crate::model_router::native_auth::without_refresh(|| {
            shared_models::take_sign_in(&agent, &account)
        })?
    };
    let plaintext = Zeroizing::new(
        serde_json::to_vec(&json!({"account": account, "entry": entry}))
            .map_err(|error| error.to_string())?,
    );
    let secret_id = format!("handover:{}:{account}", view.self_id);
    let version = fleet::sealed(inner, &secret_id)?.map_or(0, |held| held.version) + 1;
    let secret = fleet::seal(
        inner,
        &keys,
        &secret_id,
        "handover",
        version,
        &[recipient],
        &plaintext,
    )?;
    fleet::put_sealed(inner, &secret)?;
    fleet::audit(
        inner,
        "sealed",
        None,
        json!({"secret_id": secret_id, "version": version, "recipients": [to], "keys": [account]}),
    )?;
    fleet::audit(inner, "hold_moved", Some(&to), json!({"account": account}))?;
    mark_dirty(inner);
    Ok(())
}

/// `fleet.credentials {share_models?}`: turns sharing this runtime's model
/// connections with the fleet on or off, and shows the sealed secrets this
/// runtime holds and its recent fleet log.
pub(super) fn credentials_operation(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        share_models: Option<bool>,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    if let Some(share) = body.share_models {
        inner
            .store
            .set_meta("fleet_share_models", Some(if share { "1" } else { "0" }))?;
        mark_dirty(inner);
    }
    let view = fleet::view(inner)?;
    let opened = opened(inner)?;
    let secrets: Vec<Value> = fleet::sealed_rows(inner)?
        .into_iter()
        .map(|secret| {
            let keys = opened
                .get(&secret.secret_id)
                .map(|(_, state)| state["keys"].clone())
                .filter(Value::is_array)
                .unwrap_or_else(|| json!([]));
            json!({
                "secret_id": secret.secret_id,
                "class": secret.class,
                "owner": secret.owner,
                "version": secret.version,
                "for_this_runtime": secret.recipients.contains_key(&view.self_id),
                "keys": keys,
            })
        })
        .collect();
    let audit: Vec<Value> = super::audit::entries(inner)?
        .into_iter()
        .take(50)
        .map(|entry| {
            json!({
                "at": entry.at,
                "kind": entry.kind,
                "runtime_id": entry.runtime_id,
                "actor_runtime": entry.actor_runtime,
                "record": entry.record,
            })
        })
        .collect();
    let vault = vault_states(inner, &view)?;
    Ok(json!({
        "share_models": sharing(inner)?,
        "role": view.role(&view.self_id).map(Role::as_str),
        "removed": view.self_removed(),
        "secrets": secrets,
        "audit": audit,
        "vault": vault,
    }))
}
