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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    /// Machine-readable detail (lock holder, gap cursor, field name). Never carries
    /// secrets, tokens, connection strings or raw session content.
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
}
