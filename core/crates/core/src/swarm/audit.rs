//! The fleet's audit log: the append-only record class `audit`, which full
//! members replicate and workers never receive. Each member appends what it
//! did: each secret it sealed with its recipients, each action another
//! member forwarded to it with the actor's device, its origin and its
//! result, each role change and removal it signed or acted on with the
//! connections the removed member held, and each snapshot it served.
//!
//! A record's key starts with the member that wrote it, and a member never
//! replaces or deletes a record it holds, so no member can rewrite another's
//! entries.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::store::sql;
use super::{Inner, RecordRow};

/// The record class of the audit log.
pub(super) const CLASS: &str = "audit";
/// The one field of an audit record.
const FIELD: &str = "entry";
/// The most entries one `fleet.audit` answers.
pub const MAX_AUDIT_LIMIT: usize = 500;
const DEFAULT_LIMIT: usize = 100;

/// One audit entry as `fleet.audit` answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub at: String,
    pub kind: String,
    /// The member that wrote the entry.
    pub actor_runtime: String,
    /// The member the entry is about, if one.
    #[serde(default)]
    pub runtime_id: Option<String>,
    pub record: Value,
}

/// Whether a row is an audit entry `runtime_id` wrote: its key names that
/// member, and it sets the entry rather than removing it.
pub(super) fn written_by(row: &RecordRow, runtime_id: &str) -> bool {
    row.field == FIELD
        && !row.deleted
        && row
            .key
            .strip_prefix(runtime_id)
            .is_some_and(|rest| rest.starts_with(':'))
}

/// Appends one entry this runtime writes.
pub(super) fn append(
    inner: &Inner,
    kind: &str,
    runtime_id: Option<&str>,
    record: Value,
) -> Result<(), String> {
    let origin = inner.origin()?;
    migrate(inner, &origin)?;
    write(inner, &origin, &inner.now_text(), kind, runtime_id, record)
}

fn write(
    inner: &Inner,
    origin: &str,
    at: &str,
    kind: &str,
    runtime_id: Option<&str>,
    record: Value,
) -> Result<(), String> {
    let key = format!("{origin}:{}", uuid::Uuid::now_v7());
    let value = json!({"at": at, "kind": kind, "runtime_id": runtime_id, "record": record});
    inner
        .records
        .put(origin, inner.now(), CLASS, &key, FIELD, value, 0)?;
    inner.nudge();
    Ok(())
}

/// Moves the entries of the local fleet log a runtime kept before the audit
/// class onto audit records, oldest first, once.
pub(super) fn migrate(inner: &Inner, origin: &str) -> Result<(), String> {
    if inner.store.meta("audit_migrated")?.is_some() {
        return Ok(());
    }
    let rows: Vec<(i64, String, String, Option<String>, String)> = {
        let connection = inner.store.connect()?;
        let mut statement = connection
            .prepare("SELECT seq, at, kind, runtime_id, record FROM fleet_audit ORDER BY seq")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    for (seq, at, kind, runtime_id, record) in rows {
        let record = serde_json::from_str(&record).unwrap_or(Value::Null);
        write(inner, origin, &at, &kind, runtime_id.as_deref(), record)?;
        inner
            .store
            .connect()?
            .execute("DELETE FROM fleet_audit WHERE seq=?1", params![seq])
            .map_err(sql)?;
    }
    inner.store.set_meta("audit_migrated", Some("1"))
}

/// Every audit entry this runtime holds, newest first.
pub(super) fn entries(inner: &Inner) -> Result<Vec<AuditEntry>, String> {
    let mut entries: Vec<AuditEntry> = inner
        .records
        .scan(CLASS, None, usize::MAX)?
        .into_iter()
        .filter_map(|record| {
            let field = record.fields.get(FIELD)?;
            let value = &field.value;
            Some(AuditEntry {
                id: record.key.clone(),
                at: value["at"].as_str()?.to_owned(),
                kind: value["kind"].as_str()?.to_owned(),
                actor_runtime: field.origin.clone(),
                runtime_id: value["runtime_id"].as_str().map(str::to_owned),
                record: value["record"].clone(),
            })
        })
        .collect();
    entries.sort_by(|a, b| (&b.at, &b.id).cmp(&(&a.at, &a.id)));
    Ok(entries)
}

/// `fleet.audit {since?, limit?}`: the audit entries after `since`, newest
/// first, at most `limit`.
pub(super) fn operation(inner: &Inner, body: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        since: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    }
    let body: Body = serde_json::from_value(body).map_err(|_| "The request is invalid.")?;
    if let Some(since) = &body.since {
        super::parse_time(since).ok_or("Give since as an RFC 3339 time.")?;
    }
    let limit = body.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_AUDIT_LIMIT {
        return Err(format!("Give a limit from 1 to {MAX_AUDIT_LIMIT}."));
    }
    let mut entries = entries(inner)?;
    if let Some(since) = body.since.as_deref().and_then(super::parse_time) {
        entries.retain(|entry| super::parse_time(&entry.at).is_some_and(|at| at > since));
    }
    let more = entries.len() > limit;
    entries.truncate(limit);
    Ok(json!({"entries": entries, "more": more}))
}
