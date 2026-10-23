//! MCP OAuth sign-ins as leases. Pi keeps each remote server's sign-in in
//! `<agent>/mcp-auth.json`. The member that signed in holds the refresh
//! token and refreshes it; the other members get the access token alone,
//! sealed in `mcp:<owner>`, and Pi on a member without a refresh token uses
//! the token it has and never refreshes it.
//!
//! The owner refreshes under the lock Pi takes for the same refresh, and Pi
//! skips its refresh when the access token changed meanwhile, so a rotating
//! refresh token is spent once.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::shared_models::LEASE_MARGIN_MS;

const AUTH_FILE: &str = "mcp-auth.json";
const REFRESH_TIMEOUT: Duration = Duration::from_secs(15);
/// The state fields that stay with the machine that signed in.
const PRIVATE_FIELDS: [&str; 2] = ["codeVerifier", "oauthState"];

fn read(agent: &Path) -> Map<String, Value> {
    muniment_models::pi_models::read_json(&agent.join(AUTH_FILE)).unwrap_or_default()
}

pub(super) fn digest(value: &Value) -> String {
    hex::encode(&Sha256::digest(value.to_string().as_bytes())[..16])
}

fn refresh_token(state: &Value) -> Option<&str> {
    state["tokens"]["refresh_token"]
        .as_str()
        .filter(|token| !token.is_empty())
}

/// Whether a sign-in's access token has less than the lease margin, or half
/// its lifetime, left.
fn expiring(state: &Value, now_ms: i64) -> bool {
    let Some(expires) = state["tokensExpireAt"].as_i64() else {
        return false;
    };
    let lifetime = state["tokens"]["expires_in"]
        .as_i64()
        .map_or(LEASE_MARGIN_MS, |seconds| seconds * 1000);
    expires - now_ms < LEASE_MARGIN_MS.min(lifetime / 2)
}

/// The lock Pi takes before it refreshes the sign-in stored under `key`.
fn refresh_lock(agent: &Path, key: &str) -> std::io::Result<crate::pi_settings::SettingsLock> {
    let hash = hex::encode(Sha256::digest(key.as_bytes()));
    crate::pi_settings::lock_mcp_refresh(&agent.join(format!("mcp-auth-refresh-{}", &hash[..16])))
}

/// Asks the authorization server for fresh tokens for `state`.
fn request(state: &Value, refresh: &str) -> Result<Value, String> {
    let discovery = &state["discovery"];
    let endpoint = discovery["authorizationServerMetadata"]["token_endpoint"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            discovery["authorizationServerUrl"]
                .as_str()
                .map(|base| format!("{}/token", base.trim_end_matches('/')))
        })
        .ok_or("The sign-in names no token endpoint.")?;
    let client = &state["clientInformation"];
    let mut form = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh.to_owned()),
    ];
    if let Some(resource) = discovery["resourceMetadata"]["resource"].as_str() {
        form.push(("resource", resource.to_owned()));
    }
    let client_id = client["client_id"].as_str().unwrap_or_default();
    let secret = client["client_secret"].as_str();
    let basic = secret.is_some()
        && discovery["authorizationServerMetadata"]["token_endpoint_auth_methods_supported"]
            .as_array()
            .is_some_and(|methods| {
                methods.iter().any(|method| method == "client_secret_basic")
                    && !methods.iter().any(|method| method == "client_secret_post")
            });
    let mut call = ureq::AgentBuilder::new()
        .timeout(REFRESH_TIMEOUT)
        .build()
        .post(&endpoint)
        .set("Accept", "application/json");
    if basic {
        use base64::Engine as _;
        let pair = format!("{client_id}:{}", secret.unwrap_or_default());
        call = call.set(
            "Authorization",
            &format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(pair)
            ),
        );
    } else {
        if !client_id.is_empty() {
            form.push(("client_id", client_id.to_owned()));
        }
        if let Some(secret) = secret {
            form.push(("client_secret", secret.to_owned()));
        }
    }
    let pairs: Vec<(&str, &str)> = form
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let answer: Value = match call.send_form(&pairs) {
        Ok(response) => response.into_json().map_err(|error| error.to_string())?,
        Err(ureq::Error::Status(status, _)) => {
            return Err(format!(
                "The authorization server refused the refresh ({status})."
            ))
        }
        Err(error) => return Err(error.to_string()),
    };
    if answer["access_token"].as_str().is_none_or(str::is_empty) {
        return Err("The refresh answered no access token.".into());
    }
    Ok(answer)
}

/// Refreshes the sign-in under `key` when it is about to expire, under
/// Pi's refresh lock, and answers the state as it stands after.
fn refresh(agent: &Path, key: &str, now_ms: i64) -> Result<Option<Value>, String> {
    let _refresh = refresh_lock(agent, key).map_err(|error| error.to_string())?;
    let path = agent.join(AUTH_FILE);
    let state = {
        let _lock = muniment_models::pi_models::lock(&path).map_err(|error| error.to_string())?;
        read(agent).get(key).cloned()
    };
    let Some(mut state) = state else {
        return Ok(None);
    };
    let Some(refresh) = refresh_token(&state).map(str::to_owned) else {
        return Ok(Some(state));
    };
    if !expiring(&state, now_ms) {
        return Ok(Some(state));
    }
    let mut tokens = request(&state, &refresh)?;
    // A server that does not rotate keeps the refresh token it had.
    if tokens["refresh_token"].as_str().is_none_or(str::is_empty) {
        tokens["refresh_token"] = refresh.into();
    }
    match tokens["expires_in"].as_i64() {
        Some(seconds) => state["tokensExpireAt"] = (now_ms + seconds * 1000).into(),
        None => {
            if let Some(object) = state.as_object_mut() {
                object.remove("tokensExpireAt");
            }
        }
    }
    state["tokens"] = tokens;
    let _lock = muniment_models::pi_models::lock(&path).map_err(|error| error.to_string())?;
    let mut states = read(agent);
    states.insert(key.to_owned(), state.clone());
    muniment_models::pi_models::write_json(&path, &states).map_err(|error| error.to_string())?;
    Ok(Some(state))
}

/// The leases of this machine's own MCP sign-ins, refreshed first when
/// about to expire: each state without its refresh token, client secret or
/// sign-in flow. `received` names what other members' leases wrote here.
pub(super) fn own_leases(
    agent: &Path,
    received: &BTreeMap<String, String>,
    now_ms: i64,
) -> Map<String, Value> {
    let mut leases = Map::new();
    for (key, state) in read(agent) {
        if refresh_token(&state).is_none() || received.get(&key) == Some(&digest(&state)) {
            continue;
        }
        let state = match refresh(agent, &key, now_ms) {
            Ok(Some(state)) => state,
            Ok(None) => continue,
            Err(error) => {
                crate::runtime_eprintln!("muniment-runtime: MCP sign-in {key} refresh: {error}");
                state
            }
        };
        if state["tokensExpireAt"]
            .as_i64()
            .is_some_and(|expires| expires <= now_ms)
        {
            continue;
        }
        let mut lease = state;
        if let Some(object) = lease.as_object_mut() {
            for field in PRIVATE_FIELDS {
                object.remove(field);
            }
        }
        if let Some(tokens) = lease["tokens"].as_object_mut() {
            tokens.remove("refresh_token");
        }
        if let Some(client) = lease["clientInformation"].as_object_mut() {
            client.remove("client_secret");
        }
        leases.insert(key, lease);
    }
    leases
}

/// Writes the leases one owner shares in place of those it shared before:
/// a lease lands where this machine holds no sign-in of its own, and a
/// lease it wrote that no one changed goes when the owner stops sharing it.
/// Answers the digest of each lease written.
pub(super) fn apply(
    agent: &Path,
    previous: &BTreeMap<String, String>,
    leases: &Map<String, Value>,
) -> Result<BTreeMap<String, String>, String> {
    let path = agent.join(AUTH_FILE);
    if leases.is_empty() && previous.is_empty() {
        return Ok(BTreeMap::new());
    }
    std::fs::create_dir_all(agent).map_err(|error| error.to_string())?;
    let _lock = muniment_models::pi_models::lock(&path).map_err(|error| error.to_string())?;
    let mut states = read(agent);
    let before = states.clone();
    for (key, written) in previous {
        if states.get(key).map(digest).as_ref() == Some(written) {
            states.remove(key);
        }
    }
    let mut written = BTreeMap::new();
    for (key, lease) in leases {
        if states
            .get(key)
            .is_some_and(|held| refresh_token(held).is_some())
        {
            continue;
        }
        states.insert(key.clone(), lease.clone());
        written.insert(key.clone(), digest(lease));
    }
    if states != before {
        muniment_models::pi_models::write_json(&path, &states)
            .map_err(|error| error.to_string())?;
    }
    Ok(written)
}
