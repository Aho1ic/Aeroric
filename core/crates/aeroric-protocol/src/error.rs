//! The protocol error model (plan §2.3).

use serde::{Deserialize, Serialize};

/// Wire codes. A client must never crash on a code it does not know: an older
/// client talking to a newer host degrades to `Other`, and the text is kept so the
/// user can still be told what the host actually said. Adding or renaming a code
/// is a protocol change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    Unauthenticated,
    UnsupportedCapability,
    RevisionConflict,
    NotFound,
    Busy,
    StreamGap,
    SnapshotRequired,
    HostRestarted,
    InteractiveUiRequired,
    IoError,
    /// Not a code to emit: what a decoder produces for text it has never seen.
    Other(String),
}

impl ErrorCode {
    /// Codes a host may put on the wire. `Other` is deliberately absent: it is the
    /// client-side fallback, never something to send.
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::InvalidArgument,
        ErrorCode::Unauthenticated,
        ErrorCode::UnsupportedCapability,
        ErrorCode::RevisionConflict,
        ErrorCode::NotFound,
        ErrorCode::Busy,
        ErrorCode::StreamGap,
        ErrorCode::SnapshotRequired,
        ErrorCode::HostRestarted,
        ErrorCode::InteractiveUiRequired,
        ErrorCode::IoError,
    ];

    /// The wire spelling of a known code. This match is exhaustive on purpose:
    /// growing the enum without naming the new code here fails to compile, which is
    /// what keeps `ALL`, `contracts/native/v1/` and the decoders in step.
    fn known_name(code: &ErrorCode) -> Option<&'static str> {
        Some(match code {
            ErrorCode::InvalidArgument => "INVALID_ARGUMENT",
            ErrorCode::Unauthenticated => "UNAUTHENTICATED",
            ErrorCode::UnsupportedCapability => "UNSUPPORTED_CAPABILITY",
            ErrorCode::RevisionConflict => "REVISION_CONFLICT",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::Busy => "BUSY",
            ErrorCode::StreamGap => "STREAM_GAP",
            ErrorCode::SnapshotRequired => "SNAPSHOT_REQUIRED",
            ErrorCode::HostRestarted => "HOST_RESTARTED",
            ErrorCode::InteractiveUiRequired => "INTERACTIVE_UI_REQUIRED",
            ErrorCode::IoError => "IO_ERROR",
            ErrorCode::Other(_) => return None,
        })
    }

    pub fn as_str(&self) -> String {
        match self {
            ErrorCode::Other(text) => text.clone(),
            known => Self::known_name(known).unwrap_or_default().to_string(),
        }
    }

    pub fn from_wire(text: &str) -> Self {
        Self::ALL
            .iter()
            .find(|code| Self::known_name(code) == Some(text))
            .cloned()
            .unwrap_or_else(|| ErrorCode::Other(text.to_string()))
    }

    pub fn is_retryable_by_default(&self) -> bool {
        matches!(
            self,
            ErrorCode::Busy | ErrorCode::StreamGap | ErrorCode::SnapshotRequired
        )
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from_wire(&String::deserialize(d)?))
    }
}

/// The only shape a host may put into `RpcError.detail` (Q12).
///
/// `detail` exists to carry diagnostics, and diagnostics are exactly what gets
/// pasted into review evidence and `.artifacts` logs — from P7-04 onward real
/// DBX connection strings live nearby. So the field is a *closed struct*, not a
/// free map: an unregistered key cannot reach the wire without changing this type,
/// which makes the omission visible in a diff. `contracts/native/v1/envelope.schema.json`
/// publishes the same key list, and the round-trip test pins the two together.
///
/// Note the deliberate asymmetry: frames keep unknown content (§2.3 forward
/// compatibility), `detail` does not. A diagnostic we cannot name is not a
/// diagnostic, it is a leak with a plausible shape.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ErrorDetail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_holder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<crate::envelope::Quantity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_seq: Option<crate::envelope::Quantity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub through_seq: Option<crate::envelope::Quantity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<crate::envelope::Quantity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_revision: Option<crate::envelope::Quantity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ErrorDetail {
    /// Published key list, in the order the contract prints it. Derived from the
    /// struct's own serializer so it cannot drift from the type.
    pub fn allowed_keys() -> Vec<String> {
        let sample = ErrorDetail {
            lock_holder: Some(String::new()),
            task_id: Some(String::new()),
            generation: Some(crate::envelope::Quantity::from_u64(0)),
            epoch: Some(String::new()),
            from_seq: Some(crate::envelope::Quantity::from_u64(0)),
            through_seq: Some(crate::envelope::Quantity::from_u64(0)),
            expected_revision: Some(crate::envelope::Quantity::from_u64(0)),
            actual_revision: Some(crate::envelope::Quantity::from_u64(0)),
            field: Some(String::new()),
            reason: Some(String::new()),
        };
        match serde_json::to_value(&sample) {
            Ok(serde_json::Value::Object(map)) => map.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }
}

/// The three storage-layer refusals a client has to distinguish (ADR-011). They are
/// declared *here* because the mapping is protocol: a client that cannot tell "retry after
/// the other Aeroric exits" apart from "move your data directory off NFS" will retry
/// forever, or show a spinner for a problem that never fixes itself.
///
/// The core owns the conditions; this enum owns the spelling. `detail_keys` must stay
/// inside ADR-008's closed allowlist, and `export-schema` refuses to publish the contract
/// if it does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageRejection {
    /// Another live process holds the data directory (INV-03).
    LeaseHeld,
    /// This location may not host a writer at all (ADR-010), including "nobody probed".
    LocationUnfit,
    /// The bytes on disk are not the ones this writer last produced (ADR-012).
    StaleWrite,
}

impl StorageRejection {
    pub const ALL: &'static [StorageRejection] = &[
        StorageRejection::LeaseHeld,
        StorageRejection::LocationUnfit,
        StorageRejection::StaleWrite,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            StorageRejection::LeaseHeld => "leaseHeld",
            StorageRejection::LocationUnfit => "locationUnfit",
            StorageRejection::StaleWrite => "staleWrite",
        }
    }

    /// No new codes: adding one would be a protocol change (ADR-009), so each refusal is
    /// mapped onto an existing one.
    pub fn error_code(self) -> ErrorCode {
        match self {
            StorageRejection::LeaseHeld => ErrorCode::Busy,
            StorageRejection::LocationUnfit => ErrorCode::IoError,
            StorageRejection::StaleWrite => ErrorCode::RevisionConflict,
        }
    }

    /// Which `ErrorDetail` keys carry the information a client needs to act.
    pub fn detail_keys(self) -> Vec<&'static str> {
        match self {
            StorageRejection::LeaseHeld => vec!["lockHolder", "epoch"],
            StorageRejection::LocationUnfit => vec!["reason"],
            StorageRejection::StaleWrite => vec!["expectedRevision", "actualRevision", "epoch"],
        }
    }

    /// Whether a retry could ever succeed. `LocationUnfit` never can: the fix is a human
    /// moving the directory, and a client that retries it just spins.
    pub fn retryable(self) -> bool {
        matches!(self, StorageRejection::LeaseHeld)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    /// Machine-readable detail. Built from [`ErrorDetail`]; see that type for why
    /// the key set is closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl RpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let message = message.into();
        let retryable = code.is_retryable_by_default();
        Self {
            code,
            message,
            retryable,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }

    /// The sanctioned way to attach diagnostics.
    pub fn with_error_detail(self, detail: &ErrorDetail) -> Self {
        match serde_json::to_value(detail) {
            Ok(value) => self.with_detail(value),
            Err(_) => self,
        }
    }
}

impl core::fmt::Display for RpcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:?} (retryable={}): {}",
            self.code, self.retryable, self.message
        )
    }
}

impl std::error::Error for RpcError {}

/// A mutating method the host has no handler for. Returning "not found" would
/// let a client believe the operation happened elsewhere; refusing is required
/// because an unknown mutation may otherwise be replayed against a different
/// data directory.
#[derive(thiserror::Error, Debug)]
pub enum DispatchError {
    #[error("unknown mutating method {0}")]
    UnknownMutatingMethod(String),
    #[error("method {0} is not permitted by this client's capability allow-list")]
    MethodNotPermitted(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_wire_code_degrades_but_keeps_its_text() {
        let raw = r#"{"code":"SOMETHING_NEW","message":"future host","retryable":false}"#;
        let parsed: RpcError = serde_json::from_str(raw).expect("unknown code must decode");
        assert_eq!(parsed.code, ErrorCode::Other("SOMETHING_NEW".to_string()));
        assert_eq!(parsed.code.as_str(), "SOMETHING_NEW");
        assert!(!parsed.code.is_retryable_by_default());
        assert_eq!(parsed.message, "future host");
        // Displaying and re-sending it must not invent a known code.
        assert_eq!(
            serde_json::to_string(&parsed).unwrap(),
            r#"{"code":"SOMETHING_NEW","message":"future host","retryable":false}"#
        );
    }

    #[test]
    fn codes_use_the_documented_screaming_snake_spelling() {
        let json = serde_json::to_string(&RpcError::new(
            ErrorCode::InteractiveUiRequired,
            "needs a GUI",
        ))
        .unwrap();
        assert!(json.contains("\"INTERACTIVE_UI_REQUIRED\""), "{json}");
    }

    /// A key the allowlist does not contain must not survive into a detail object.
    #[test]
    fn the_detail_allowlist_rejects_an_unregistered_key() {
        let error = RpcError::new(ErrorCode::Busy, "try again");
        let detail = ErrorDetail {
            lock_holder: Some("pid-4242".into()),
            ..Default::default()
        };
        let built = error.with_error_detail(&detail);
        let detail_value = built.detail.clone().expect("a detail must be attached");
        assert_eq!(detail_value["lockHolder"], serde_json::json!("pid-4242"));
        assert_eq!(
            detail_value.as_object().map(|m| m.len()),
            Some(1),
            "an unset optional must not appear as null on the wire"
        );
        let unregistered: Result<ErrorDetail, _> = serde_json::from_value(serde_json::json!({
            "lockHolder": "pid-4242",
            "connectionString": "postgres://u:p@h/d"
        }));
        assert!(
            unregistered.is_err(),
            "an unregistered detail key must be refused, not carried"
        );
        let keys = ErrorDetail::allowed_keys();
        assert_eq!(keys.len(), 10, "{keys:?}");
        for secret in ["token", "password", "connectionString", "apiKey"] {
            assert!(
                !keys.contains(&secret.to_string()),
                "{secret} must not be allowed"
            );
        }
    }

    /// `ALL` is published into `contracts/native/v1/envelope.schema.json`. The match
    /// in `known_name` is exhaustive, so a new code cannot be added without naming
    /// its wire spelling here — and this pins that every emittable code really has
    /// one, uniquely, and decodes back to itself.
    #[test]
    fn every_emittable_code_has_a_unique_wire_name() {
        assert_eq!(ErrorCode::ALL.len(), 11);
        let mut names: Vec<String> = ErrorCode::ALL
            .iter()
            .map(|code| {
                let name = ErrorCode::known_name(code).expect("an emittable code must be named");
                assert_eq!(serde_json::to_value(code).unwrap(), serde_json::json!(name));
                assert_eq!(ErrorCode::from_wire(name), code.clone(), "{name}");
                name.to_string()
            })
            .collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 11, "two codes share a wire name");
        assert!(names.iter().all(|n| n == &n.to_uppercase()));
        assert_eq!(
            ErrorCode::known_name(&ErrorCode::Other("X".into())),
            None,
            "the fallback is never emittable"
        );
    }

    #[test]
    fn each_storage_refusal_uses_an_existing_code_and_allowlisted_detail() {
        let allowed = ErrorDetail::allowed_keys();
        for rejection in StorageRejection::ALL {
            assert!(
                ErrorCode::ALL.contains(&rejection.error_code()),
                "{} maps to a code no host may emit",
                rejection.as_str()
            );
            for key in rejection.detail_keys() {
                assert!(
                    allowed.iter().any(|k| k == key),
                    "{:?} is not in the ADR-008 allowlist",
                    key
                );
            }
        }
        assert_eq!(
            StorageRejection::LocationUnfit.error_code(),
            ErrorCode::IoError,
            "an unfit location is not a capability the client can ask for again"
        );
        assert!(
            !StorageRejection::LocationUnfit.retryable(),
            "and retrying it must be sayably pointless"
        );
        assert!(StorageRejection::LeaseHeld.retryable());
        assert!(!StorageRejection::StaleWrite.retryable());
    }
}
