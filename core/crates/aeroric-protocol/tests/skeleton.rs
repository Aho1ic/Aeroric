//! Skeleton smoke suite: proves the local-protocol v1 envelope contract holds in
//! Rust the way AeroricProtocol proves it in Swift. P2-01 replaces these literals
//! with the shared fixtures under `contracts/native/v1/`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Envelope {
    v: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(flatten)]
    unknown: Map<String, Value>,
}

/// Byte-for-byte the same input AeroricProtocolTests feeds the Swift decoder.
const SHARED_REQUEST: &str = r#"{"v":1,"id":"req-1","method":"task.start","futureField":{"a":1}}"#;

#[test]
fn preserves_unknown_members_across_round_trip() {
    let decoded: Envelope = serde_json::from_str(SHARED_REQUEST).expect("frame must decode");
    assert_eq!(decoded.v, 1);
    assert_eq!(decoded.id.as_deref(), Some("req-1"));
    assert_eq!(decoded.method.as_deref(), Some("task.start"));
    assert_eq!(decoded.unknown.len(), 1, "exactly one unmodelled member");
    assert_eq!(decoded.unknown["futureField"]["a"], Value::from(1));

    let reencoded = serde_json::to_string(&decoded).expect("encode");
    assert!(
        reencoded.contains("futureField"),
        "unknown member must be re-emitted"
    );
    let again: Envelope = serde_json::from_str(&reencoded).expect("re-decode");
    assert_eq!(decoded, again);
}

#[test]
fn response_frame_and_absent_optionals_stay_absent() {
    let decoded: Envelope =
        serde_json::from_str(r#"{"v":1,"id":"req-1","ok":true}"#).expect("response decodes");
    assert_eq!(decoded.ok, Some(true));
    assert!(decoded.method.is_none());
    let text = serde_json::to_string(&decoded).expect("encode");
    assert!(
        !text.contains("method"),
        "absent optional must not serialise as null"
    );
    assert!(!text.contains("result"));
}

#[test]
fn oversized_ids_stay_decimal_strings_not_floats() {
    // seq / revision / generation / byte offsets are strings in v1 precisely so a
    // JS or float path cannot silently round them.
    let raw = r#"{"v":1,"seq":"9007199254740993","offset":"18446744073709551615"}"#;
    let decoded: Envelope = serde_json::from_str(raw).expect("frame decodes");
    assert_eq!(decoded.unknown["seq"], Value::from("9007199254740993"));
    assert_eq!(
        decoded.unknown["offset"],
        Value::from("18446744073709551615")
    );
    let again = serde_json::to_string(&decoded).expect("encode");
    assert!(
        again.contains("18446744073709551615"),
        "u64::MAX must not be reformatted"
    );
}

#[test]
fn malformed_and_versionless_frames_error_without_panicking() {
    assert!(serde_json::from_str::<Envelope>("not json at all").is_err());
    assert!(serde_json::from_str::<Envelope>(r#"{"id":"x"}"#).is_err());
    assert!(serde_json::from_str::<Envelope>("").is_err());
    assert!(
        serde_json::from_str::<Envelope>(r#"{"v":2}"#).is_ok_and(|e| e.v == 2),
        "a newer version must decode rather than be rejected here; negotiation is P2-01's job"
    );
}

#[test]
fn unknown_mutating_method_is_not_guessed_at() {
    // The skeleton has no method registry yet; assert the decoder keeps the
    // method verbatim so the dispatcher, not the parser, decides rejection.
    let decoded: Envelope =
        serde_json::from_str(r#"{"v":1,"id":"r","method":"not.a.real.method"}"#).expect("decodes");
    assert_eq!(decoded.method.as_deref(), Some("not.a.real.method"));
}
