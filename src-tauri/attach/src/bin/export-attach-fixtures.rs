use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use muniment_attach::*;
use serde::Serialize;
use serde_json::json;

const VERSION_DIRECTORY: &str = "muniment.attach/1";

fn id(n: u128) -> Id {
    Id::new(format!("{n:032x}")).expect("fixture IDs are valid UUIDs")
}

fn insert<T: Serialize>(fixtures: &mut BTreeMap<String, Vec<u8>>, name: &str, value: &T) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("fixture serialization succeeds");
    bytes.push(b'\n');
    fixtures.insert(format!("{name}.json"), bytes);
}

fn fixtures() -> BTreeMap<String, Vec<u8>> {
    let mut fixtures = BTreeMap::new();
    insert(
        &mut fixtures,
        "negotiation-hello",
        &Hello {
            protocol: Protocol,
            client: Client {
                kind: "editor-extension".into(),
                version: "0.0.1".into(),
            },
            supported: VersionRange { min: 1, max: 1 },
            client_nonce: "fixture-client-nonce".into(),
        },
    );
    insert(
        &mut fixtures,
        "negotiation-welcome",
        &welcome(1, "0.0.1", "fixture-server-nonce", "fixture-challenge"),
    );
    insert(
        &mut fixtures,
        "authorization-authorized",
        &authorized(
            "fixture-capability",
            3600,
            900,
            [("workspace-1".into(), BTreeSet::from(["thread.read".into()]))]
                .into_iter()
                .collect(),
        ),
    );

    let operations = [
        ("thread-list", Operation::ThreadList),
        ("thread-open", Operation::ThreadOpen),
        ("run-open", Operation::RunOpen),
        ("run-start", Operation::RunStart),
        ("run-stream", Operation::RunStream),
        ("run-cursor-ack", Operation::RunCursorAck),
        ("run-steer", Operation::RunSteer),
        ("run-follow-up", Operation::RunFollowUp),
        ("run-cancel", Operation::RunCancel),
        ("permission-answer", Operation::PermissionAnswer),
        ("artifact-fetch", Operation::ArtifactFetch),
        ("artifact-window", Operation::ArtifactWindow),
        ("request-cancel", Operation::RequestCancel),
    ];
    for (index, (name, operation)) in operations.into_iter().enumerate() {
        insert(
            &mut fixtures,
            &format!("request-{name}"),
            &Request {
                protocol: Protocol,
                request_id: id(100 + index as u128),
                operation,
                capability: "fixture-capability".into(),
                idempotency_key: operation
                    .requires_idempotency_key()
                    .then(|| id(200 + index as u128)),
                body: json!({"fixture": operation.as_str()}),
            },
        );
    }
    insert(
        &mut fixtures,
        "response-success",
        &Response {
            protocol: Protocol,
            request_id: id(100),
            ok: Success,
            body: json!({"accepted": true}),
        },
    );

    let errors = [
        (
            "protocol-incompatible",
            ProtocolError::protocol_incompatible(
                VersionRange { min: 1, max: 1 },
                ErrorAction::UpgradeCompanion,
            ),
        ),
        ("payload-too-large", ProtocolError::payload_too_large()),
        ("malformed-frame", ProtocolError::malformed_frame()),
        (
            "idempotency-key-required",
            ProtocolError::idempotency_key_required(),
        ),
        (
            "idempotency-key-forbidden",
            ProtocolError::idempotency_key_forbidden(),
        ),
        (
            "idempotency-conflict",
            ProtocolError::idempotency_conflict(),
        ),
        ("persistence-failed", ProtocolError::persistence_failed()),
        ("invalid-cursor", ProtocolError::invalid_cursor()),
        (
            "invalid-artifact-cursor",
            ProtocolError::invalid_artifact_cursor(),
        ),
        ("invalid-request", ProtocolError::invalid_request()),
        ("unauthorized", ProtocolError::unauthorized()),
        (
            "unsupported-operation",
            ProtocolError::unsupported_operation(),
        ),
    ];
    for (index, (name, error)) in errors.into_iter().enumerate() {
        insert(
            &mut fixtures,
            &format!("error-{name}"),
            &ErrorEnvelope {
                protocol: Protocol,
                request_id: Some(id(300 + index as u128)),
                ok: Failure,
                error,
            },
        );
    }

    let events = [
        ("run-event", EventName::RunEvent),
        ("subscription-caught-up", EventName::SubscriptionCaughtUp),
        ("permission-pending", EventName::PermissionPending),
        ("artifact-chunk", EventName::ArtifactChunk),
        ("artifact-complete", EventName::ArtifactComplete),
        ("request-cancelled", EventName::RequestCancelled),
        ("capability-revoked", EventName::CapabilityRevoked),
        ("stream-closed", EventName::StreamClosed),
        (
            "unknown",
            serde_json::from_value(json!("future.optional")).expect("valid bounded event name"),
        ),
    ];
    for (index, (name, event)) in events.into_iter().enumerate() {
        insert(
            &mut fixtures,
            &format!("event-{name}"),
            &Event {
                protocol: Protocol,
                subscription_id: id(400),
                event,
                run_id: Some(id(401)),
                run_seq: Some(index as u64 + 1),
                body: json!({"fixture": name}),
            },
        );
    }
    fixtures
}

fn existing_entries(directory: &Path) -> Result<BTreeSet<String>, String> {
    if !directory.is_dir() {
        return Ok(BTreeSet::new());
    }
    fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
        .map(|entry| {
            entry
                .map_err(|error| format!("cannot read {}: {error}", directory.display()))
                .and_then(|entry| {
                    entry
                        .file_name()
                        .into_string()
                        .map_err(|_| format!("non-UTF-8 fixture path in {}", directory.display()))
                })
        })
        .collect()
}

fn check(directory: &Path, fixtures: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let existing = existing_entries(directory)?;
    let expected: BTreeSet<_> = fixtures.keys().cloned().collect();
    let mut drift = Vec::new();
    for name in expected.difference(&existing) {
        drift.push(format!("missing: {}", directory.join(name).display()));
    }
    for name in existing.difference(&expected) {
        drift.push(format!("extra: {}", directory.join(name).display()));
    }
    for name in expected.intersection(&existing) {
        let path = directory.join(name);
        match fs::read(&path) {
            Ok(bytes) if bytes == fixtures[name] => {}
            Ok(_) => drift.push(format!("stale: {}", path.display())),
            Err(error) => drift.push(format!("cannot read {}: {error}", path.display())),
        }
    }
    if drift.is_empty() {
        Ok(())
    } else {
        Err(format!("attach fixture drift detected:\n{}\nregenerate with: cargo run -p muniment-attach --bin export-attach-fixtures -- ../protocol-fixtures", drift.join("\n")))
    }
}

fn write(directory: &Path, fixtures: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    let expected: BTreeSet<_> = fixtures.keys().cloned().collect();
    for extra in existing_entries(directory)?.difference(&expected) {
        let path = directory.join(extra);
        if path.is_file() {
            fs::remove_file(&path)
                .map_err(|error| format!("cannot remove {}: {error}", path.display()))?;
        } else {
            return Err(format!(
                "cannot replace extra non-file fixture: {}",
                path.display()
            ));
        }
    }
    for (name, bytes) in fixtures {
        let path = directory.join(name);
        fs::write(&path, bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let root = args.next().map(PathBuf::from).ok_or_else(|| {
        "usage: export-attach-fixtures <protocol-fixtures-directory> [--check]".to_owned()
    })?;
    let check_only = match args.next() {
        None => false,
        Some(value) if value == "--check" => true,
        Some(value) => return Err(format!("unexpected argument: {}", value.to_string_lossy())),
    };
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let directory = root.join(VERSION_DIRECTORY);
    let fixtures = fixtures();
    if check_only {
        check(&directory, &fixtures)
    } else {
        write(&directory, &fixtures)
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
