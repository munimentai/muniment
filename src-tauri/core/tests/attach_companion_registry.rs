#![cfg(target_os = "linux")]

use muniment_core::attach::linux::LiveConnectionRegistry;
use muniment_core::attach::{
    load_client_credentials, ClientCredential, CompanionRecord, CompanionRegistry, ErrorCode,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

fn path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "muniment-companion-registry-{name}-{}",
        Uuid::now_v7()
    ))
}

fn entry(
    credential: &str,
    kind: &str,
    version: &str,
    approved_at: Option<&str>,
) -> ClientCredential {
    ClientCredential {
        credential: credential.repeat(32),
        claimed_kind: kind.to_owned(),
        claimed_version: version.to_owned(),
        approved_at: approved_at.map(str::to_owned),
    }
}

fn registry(
    credentials: HashMap<String, ClientCredential>,
    path: &PathBuf,
) -> (
    CompanionRegistry,
    Arc<Mutex<HashMap<String, ClientCredential>>>,
) {
    let credentials = Arc::new(Mutex::new(credentials));
    (
        CompanionRegistry::new(credentials.clone(), path, LiveConnectionRegistry::default()),
        credentials,
    )
}

#[test]
fn revoke_persists_the_removal_before_it_finishes() {
    let path = path("revoke").join("credentials.json");
    let identity = "018f0000-0000-7000-8000-000000000001";
    let (registry, _) = registry(
        HashMap::from([(identity.to_owned(), entry("ab", "cli", "1.2.3", None))]),
        &path,
    );

    registry.revoke(identity).unwrap();

    assert!(!load_client_credentials(&path)
        .unwrap()
        .contains_key(identity));
    assert!(registry.list().unwrap().is_empty());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn failed_persist_restores_the_credential() {
    let blocker = path("failed-persist");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let path = blocker.join("credentials.json");
    let identity = "018f0000-0000-7000-8000-000000000001";
    let (registry, credentials) = registry(
        HashMap::from([(identity.to_owned(), entry("ab", "cli", "1.2.3", None))]),
        &path,
    );

    assert_eq!(
        registry.revoke(identity).unwrap_err().code(),
        ErrorCode::PersistenceFailed
    );
    assert!(credentials.lock().unwrap().contains_key(identity));
    std::fs::remove_file(blocker).unwrap();
}

#[test]
fn unknown_identity_is_unauthorized_without_changing_the_store() {
    let path = path("unknown").join("credentials.json");
    let identity = "018f0000-0000-7000-8000-000000000001";
    let (registry, credentials) = registry(
        HashMap::from([(identity.to_owned(), entry("ab", "cli", "1.2.3", None))]),
        &path,
    );

    assert_eq!(
        registry
            .revoke("018f0000-0000-7000-8000-000000000099")
            .unwrap_err()
            .code(),
        ErrorCode::Unauthorized
    );
    assert!(credentials.lock().unwrap().contains_key(identity));
    assert!(!path.exists());
}

#[test]
fn list_returns_claims_and_approval_times_sorted_by_identity() {
    let path = path("list").join("credentials.json");
    let first = "018f0000-0000-7000-8000-000000000001";
    let second = "018f0000-0000-7000-8000-000000000002";
    let (registry, _) = registry(
        HashMap::from([
            (second.to_owned(), entry("cd", "mobile", "2.0.0", None)),
            (
                first.to_owned(),
                entry("ab", "cli", "1.2.3", Some("2026-08-04T12:00:00Z")),
            ),
        ]),
        &path,
    );

    assert_eq!(
        registry.list().unwrap(),
        vec![
            CompanionRecord {
                identity: first.to_owned(),
                claimed_kind: "cli".to_owned(),
                claimed_version: "1.2.3".to_owned(),
                approved_at: Some("2026-08-04T12:00:00Z".to_owned()),
            },
            CompanionRecord {
                identity: second.to_owned(),
                claimed_kind: "mobile".to_owned(),
                claimed_version: "2.0.0".to_owned(),
                approved_at: None,
            },
        ]
    );
}
