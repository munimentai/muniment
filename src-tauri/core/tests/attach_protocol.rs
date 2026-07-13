use muniment_core::attach::*;
use serde_json::{json, Value};

fn id(n: u128) -> Id {
    Id::new(format!("{n:032x}")).unwrap()
}

fn round_trip(value: Envelope) {
    let encoded = encode_frame(&value).unwrap();
    let (decoded, consumed) = decode_frame::<Envelope>(&encoded).unwrap().unwrap();
    assert_eq!(decoded, value);
    assert_eq!(consumed, encoded.len());
    assert_eq!(encode_frame(&decoded).unwrap(), encoded);
}

#[test]
fn all_envelope_kinds_round_trip() {
    round_trip(Envelope::Request(Request {
        protocol: Protocol,
        request_id: id(1),
        operation: Operation::RunStart,
        capability: "connection-capability".into(),
        idempotency_key: Some(id(2)),
        body: json!({"text":"hello"}),
    }));
    round_trip(Envelope::Response(Response {
        protocol: Protocol,
        request_id: id(1),
        ok: Success,
        body: json!({"accepted":true}),
    }));
    round_trip(Envelope::Error(ErrorEnvelope {
        protocol: Protocol,
        request_id: Some(id(1)),
        ok: Failure,
        error: ProtocolError::malformed_frame(),
    }));
    round_trip(Envelope::Event(Event {
        protocol: Protocol,
        subscription_id: id(3),
        event: EventName::RunEvent,
        run_id: Some(id(4)),
        run_seq: Some(7),
        body: json!({"kind":"message"}),
    }));
}

#[test]
fn envelope_discriminants_are_fixed_on_encode_and_decode() {
    assert_eq!(serde_json::to_value(Success).unwrap(), true);
    assert_eq!(serde_json::to_value(Failure).unwrap(), false);
    assert!(serde_json::from_value::<Success>(json!(false)).is_err());
    assert!(serde_json::from_value::<Failure>(json!(true)).is_err());

    let response = Response {
        protocol: Protocol,
        request_id: id(1),
        ok: Success,
        body: json!({}),
    };
    let error = ErrorEnvelope {
        protocol: Protocol,
        request_id: None,
        ok: Failure,
        error: ProtocolError::malformed_frame(),
    };
    assert_eq!(serde_json::to_value(response).unwrap()["ok"], true);
    assert_eq!(serde_json::to_value(error).unwrap()["ok"], false);
}

#[test]
fn frame_errors_and_incremental_decode_are_bounded() {
    let oversized = ((MAX_FRAME_LENGTH as u32) + 1).to_be_bytes();
    assert!(matches!(
        decode_frame::<Value>(&oversized),
        Err(FrameError::PayloadTooLarge)
    ));

    let valid = encode_frame(&json!({"hello":"world"})).unwrap();
    for split in 0..valid.len() {
        assert!(decode_frame::<Value>(&valid[..split]).unwrap().is_none());
    }
    assert_eq!(
        decode_frame::<Value>(&valid).unwrap().unwrap().0,
        json!({"hello":"world"})
    );

    let truncated = [&(10u32.to_be_bytes())[..], b"{}"].concat();
    assert!(decode_frame::<Value>(&truncated).unwrap().is_none());
    let invalid_utf8 = [&(1u32.to_be_bytes())[..], &[0xff]].concat();
    assert!(matches!(
        decode_frame::<Value>(&invalid_utf8),
        Err(FrameError::InvalidUtf8)
    ));
    let invalid_json = [&(1u32.to_be_bytes())[..], b"{"].concat();
    assert!(matches!(
        decode_frame::<Value>(&invalid_json),
        Err(FrameError::InvalidJson)
    ));
}

#[test]
fn envelope_ids_are_bounded_uuid_strings() {
    let bad = json!({
        "protocol": PROTOCOL, "request_id": "x".repeat(MAX_ID_LENGTH + 1),
        "operation": "thread.list", "capability": "c", "body": {}
    });
    let frame = encode_frame(&bad).unwrap();
    assert!(matches!(
        decode_frame::<Envelope>(&frame),
        Err(FrameError::InvalidJson)
    ));
}

fn raw_json_frame(payload: &[u8]) -> Vec<u8> {
    [&(payload.len() as u32).to_be_bytes()[..], payload].concat()
}

#[test]
fn frame_structural_limits_are_enforced_independently() {
    let nested = format!(
        "{}0{}",
        "[".repeat(MAX_JSON_DEPTH),
        "]".repeat(MAX_JSON_DEPTH)
    );
    assert!(matches!(
        decode_frame::<Value>(&raw_json_frame(nested.as_bytes())),
        Err(FrameError::StructureLimit)
    ));

    let long_string = serde_json::to_string(&"x".repeat(MAX_TEXT_LENGTH + 1)).unwrap();
    assert!(matches!(
        decode_frame::<Value>(&raw_json_frame(long_string.as_bytes())),
        Err(FrameError::StructureLimit)
    ));

    // Each object entry contributes its key and string value to the string count.
    let too_many_strings = Value::Object(
        (0..(MAX_JSON_STRINGS / 2 + 1))
            .map(|i| (format!("k{i}"), json!("v")))
            .collect(),
    );
    let payload = serde_json::to_vec(&too_many_strings).unwrap();
    assert!(matches!(
        decode_frame::<Value>(&raw_json_frame(&payload)),
        Err(FrameError::StructureLimit)
    ));

    let too_many_entries = Value::Array(vec![Value::Null; MAX_JSON_COLLECTION_ENTRIES + 1]);
    let payload = serde_json::to_vec(&too_many_entries).unwrap();
    assert!(matches!(
        decode_frame::<Value>(&raw_json_frame(&payload)),
        Err(FrameError::StructureLimit)
    ));
}

#[test]
fn hello_welcome_and_version_overlap() {
    let hello = Hello {
        protocol: Protocol,
        client: Client {
            kind: "cli".into(),
            version: "1.0.0".into(),
        },
        supported: VersionRange { min: 1, max: 2 },
        client_nonce: "client-nonce".into(),
    };
    let selected =
        negotiate_first(FirstMessage::Hello(hello), VersionRange { min: 1, max: 1 }).unwrap();
    let welcome = welcome(selected, "0.1.0", "server-nonce", "challenge");
    assert_eq!(welcome.selected, 1);
    assert_eq!(welcome.authorization, Authorization::PairingRequired);
    assert_eq!(
        serde_json::to_value(welcome).unwrap()["authorization"],
        "pairing_required"
    );
}

#[test]
fn incompatibility_is_actionable_and_discloses_no_runtime_state() {
    let error = negotiate_version(
        VersionRange { min: 2, max: 2 },
        VersionRange { min: 1, max: 1 },
    )
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::ProtocolIncompatible);
    assert_eq!(error.action(), Some(ErrorAction::UpgradeDesktop));
    let serialized = serde_json::to_string(&error).unwrap();
    for forbidden in ["profile", "session", "workspace", "entitlement", "runtime"] {
        assert!(!serialized.contains(forbidden));
    }
}

#[test]
fn error_schema_rejects_arbitrary_messages_and_mismatched_details() {
    let approved = [
        ProtocolError::malformed_frame(),
        ProtocolError::payload_too_large(),
        ProtocolError::protocol_incompatible(
            VersionRange { min: 1, max: 1 },
            ErrorAction::UpgradeCompanion,
        ),
    ];
    let serialized = serde_json::to_string(&approved).unwrap();
    assert!(!serialized.contains("/home/user/.env"));

    let injected = json!({
        "code": "malformed_frame",
        "message": "/home/user/.env contains TOKEN=secret",
        "retryable": false
    });
    assert!(serde_json::from_value::<ProtocolError>(injected).is_err());

    let mismatched = json!({
        "code": "payload_too_large",
        "message": "The frame is malformed.",
        "retryable": false
    });
    assert!(serde_json::from_value::<ProtocolError>(mismatched).is_err());
}
