#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use muniment_core::attach::linux::{CompanionProvenance, ThreadListService};
use muniment_core::attach::{
    save_client_credentials, ClientCredential, Id, ProtocolError, RuntimeActivityRegistry,
    SignedWorkspaceApproval, COMPANION_CREDENTIAL_FILE_NAME,
};
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_execution::PiRuntime;
use muniment_core::session_thread::SessionThread;
use muniment_runtime::{
    compose_attach_service, open_companion_registry, open_profile_storage, revoke_companion,
    RuntimeAttachBoundaries,
};

mod common;
use common::TemporaryProfile;

fn boundaries(profile: &TemporaryProfile) -> RuntimeAttachBoundaries {
    RuntimeAttachBoundaries::new(
        open_profile_storage(&profile.profile).unwrap(),
        Arc::new(Mutex::new(None)),
        profile.profile.clone(),
        profile.config.clone(),
        Arc::new(Mutex::new(None::<PiRuntime>)),
        Arc::new(ApplicationMemoryRuntime::new(
            profile.config.clone(),
            profile.profile.join("memory"),
        )),
        RuntimeActivityRegistry::new(),
        SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
        open_companion_registry(&profile.profile).unwrap(),
    )
}

#[test]
fn recorded_home_wins_when_the_runtime_composes_the_service() {
    let profile = TemporaryProfile::new("attach-service-home", true);
    let registry = open_companion_registry(&profile.profile).unwrap();

    let service = compose_attach_service(
        boundaries(&profile),
        &registry,
        &profile.profile,
        &profile.config,
    )
    .unwrap();

    assert_eq!(service.home, profile.root.join("home"));
    assert!(profile.profile.join("attach-idempotency.sqlite3").is_file());
    assert_eq!(
        service.credential_path,
        Some(profile.profile.join(COMPANION_CREDENTIAL_FILE_NAME))
    );
    assert!(service.client_identity.is_none());
}

#[test]
fn service_credential_map_sees_a_registry_revoke() {
    let profile = TemporaryProfile::new("attach-service-revoke", true);
    let identity = "018f0000-0000-7000-8000-000000000001";
    let credential = ClientCredential {
        credential: "ab".repeat(32),
        claimed_kind: "cli".into(),
        claimed_version: "1.0.0".into(),
        approved_at: Some("2026-08-12T00:00:00Z".into()),
    };
    save_client_credentials(
        &profile.profile.join(COMPANION_CREDENTIAL_FILE_NAME),
        &HashMap::from([(identity.into(), credential)]),
    )
    .unwrap();
    let registry = open_companion_registry(&profile.profile).unwrap();
    let service = compose_attach_service(
        boundaries(&profile),
        &registry,
        &profile.profile,
        &profile.config,
    )
    .unwrap();

    assert!(service
        .client_credentials
        .lock()
        .unwrap()
        .contains_key(identity));
    revoke_companion(&registry, identity).unwrap();
    assert!(!service
        .client_credentials
        .lock()
        .unwrap()
        .contains_key(identity));
}

#[test]
fn service_lists_and_idempotently_revokes_companions() {
    let profile = TemporaryProfile::new("attach-service-companions", true);
    let identity = "018f0000-0000-7000-8000-000000000001";
    let credential = ClientCredential {
        credential: "ab".repeat(32),
        claimed_kind: "cli".into(),
        claimed_version: "1.0.0".into(),
        approved_at: Some("2026-08-12T00:00:00Z".into()),
    };
    save_client_credentials(
        &profile.profile.join(COMPANION_CREDENTIAL_FILE_NAME),
        &HashMap::from([(identity.into(), credential)]),
    )
    .unwrap();
    let registry = open_companion_registry(&profile.profile).unwrap();
    let boundaries = RuntimeAttachBoundaries::new(
        open_profile_storage(&profile.profile).unwrap(),
        Arc::new(Mutex::new(None)),
        profile.profile.clone(),
        profile.config.clone(),
        Arc::new(Mutex::new(None::<PiRuntime>)),
        Arc::new(ApplicationMemoryRuntime::new(
            profile.config.clone(),
            profile.profile.join("memory"),
        )),
        RuntimeActivityRegistry::new(),
        SignedWorkspaceApproval::default(),
        Arc::new(SessionThread::default()),
        registry.clone(),
    );
    let mut service =
        compose_attach_service(boundaries, &registry, &profile.profile, &profile.config).unwrap();
    let provenance = CompanionProvenance {
        profile: "default".into(),
        companion_kind: "cli".into(),
        companion_version: "1.0.0".into(),
        peer_uid: 1000,
        peer_pid: 42,
    };
    let key = Id::new("018f0000-0000-7000-8000-000000000010").unwrap();

    let listed = service.list_companions().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].identity, identity);
    for request_id in [
        "018f0000-0000-7000-8000-000000000011",
        "018f0000-0000-7000-8000-000000000012",
    ] {
        service
            .revoke_companion(
                identity,
                &Id::new(request_id).unwrap(),
                &key,
                provenance.clone(),
            )
            .unwrap();
    }
    assert!(service.list_companions().unwrap().is_empty());

    assert_eq!(
        service
            .revoke_companion(
                "unknown",
                &Id::new("018f0000-0000-7000-8000-000000000013").unwrap(),
                &Id::new("018f0000-0000-7000-8000-000000000014").unwrap(),
                provenance,
            )
            .unwrap_err(),
        ProtocolError::unauthorized()
    );
}
