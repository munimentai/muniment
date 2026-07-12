use muniment_core::attach::{codec::*, protocol::*};
use serde_json::json;

const ID: &str = "123e4567-e89b-12d3-a456-426614174000";

fn framed(value: serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(&value).unwrap();
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend(body);
    out
}

#[test]
fn frame_prefix_is_unsigned_big_endian() {
    assert_eq!(&encode_frame(&json!({"x": 1})).unwrap()[..4], &[0, 0, 0, 7]);
}

#[test]
fn all_closed_operations_decode_and_apply_idempotency_rules() {
    let effectful = [
        "run.start",
        "run.steer",
        "run.follow_up",
        "run.cancel",
        "permission.answer",
    ];
    let operations = [
        "thread.list",
        "thread.open",
        "run.open",
        "run.start",
        "run.stream",
        "run.cursor_ack",
        "run.steer",
        "run.follow_up",
        "run.cancel",
        "permission.answer",
        "artifact.fetch",
        "artifact.window",
    ];
    for operation in operations {
        let key = effectful.contains(&operation).then_some("stable-key");
        let value = json!({"protocol":PROTOCOL,"request_id":ID,"operation":operation,"capability":"cap","idempotency_key":key,"body":{}});
        assert!(decode_request(&framed(value)).is_ok(), "{operation}");
    }
    let cancel = json!({"protocol":PROTOCOL,"request_id":ID,"operation":"request.cancel","capability":"cap","body":{"kind":"request","request_id":ID}});
    assert!(decode_request(&framed(cancel)).is_ok());
}

#[test]
fn unknown_operation_and_wrong_protocol_are_rejected() {
    for (protocol, operation) in [(PROTOCOL, "future.op"), ("other/1", "thread.list")] {
        let value = json!({"protocol":protocol,"request_id":ID,"operation":operation,"capability":"cap","body":{}});
        assert!(decode_request(&framed(value)).is_err());
    }
}

#[test]
fn known_and_future_events_decode() {
    for event in [
        "run.event",
        "subscription.caught_up",
        "permission.pending",
        "artifact.chunk",
        "artifact.complete",
        "request.cancelled",
        "capability.revoked",
        "stream.closed",
        "future.event",
    ] {
        let value = json!({"protocol":PROTOCOL,"subscription_id":ID,"event":event,"body":{},"future_optional":true});
        assert!(decode_frame::<Event>(&framed(value)).is_ok(), "{event}");
    }
}

#[test]
fn framing_and_resource_limits_fail_closed() {
    assert!(matches!(
        decode_frame::<serde_json::Value>(&[0, 0, 0, 0]),
        Err(CodecError::ZeroLength)
    ));
    assert!(matches!(
        decode_frame::<serde_json::Value>(&[0, 0, 0, 4, b'{']),
        Err(CodecError::Truncated)
    ));
    assert!(matches!(
        decode_frame::<serde_json::Value>(&[0, 16, 0, 1]),
        Err(CodecError::FrameTooLarge)
    ));
    let mut nested = json!(null);
    for _ in 0..MAX_JSON_DEPTH {
        nested = json!([nested]);
    }
    assert!(matches!(
        decode_frame::<serde_json::Value>(&framed(nested)),
        Err(CodecError::LimitExceeded)
    ));
    let chunk = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        vec![0; MAX_ARTIFACT_CHUNK_BYTES + 1],
    );
    let event = json!({"protocol":PROTOCOL,"subscription_id":ID,"event":"artifact.chunk","body":{"data":chunk}});
    assert!(matches!(
        decode_frame::<Event>(&framed(event)),
        Err(CodecError::ArtifactChunkTooLarge)
    ));
}

#[test]
fn negotiation_is_minimal_and_actionable() {
    assert_eq!(
        negotiate_version(VersionRange { min: 1, max: 2 }).unwrap(),
        1
    );
    let old = negotiate_version(VersionRange { min: 0, max: 0 }).unwrap_err();
    assert_eq!(old.action, Some(ErrorAction::UpgradeCompanion));
    let new = negotiate_version(VersionRange { min: 2, max: 3 }).unwrap_err();
    assert_eq!(new.action, Some(ErrorAction::UpgradeDesktop));
    let encoded = serde_json::to_string(&new).unwrap();
    for secret in ["profile", "session", "workspace", "entitlement", "runtime"] {
        assert!(!encoded.contains(secret));
    }
}
