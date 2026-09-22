//! Envelope types for the local protocol v1 (plan §2.3): Unix domain socket,
//! UTF-8 JSONL, one frame per line.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum bytes of one JSONL frame, counted in raw UTF-8 bytes.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// A decimal-string quantity (`seq`, `revision`, `generation`, byte offsets).
///
/// Kept as a string on the wire because JS numbers lose precision past 2^53-1;
/// an index that silently wraps is worse than one that fails to parse.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Quantity(String);

impl Default for Quantity {
    /// Revision 0 means "no revision assigned yet", which is what an absent field
    /// has always meant for these counters.
    fn default() -> Self {
        Self("0".to_string())
    }
}

impl Quantity {
    pub fn new(value: &str) -> Option<Self> {
        if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
            Some(Self(value.to_string()))
        } else {
            None
        }
    }

    pub fn from_u64(value: u64) -> Self {
        Self(value.to_string())
    }

    pub fn as_u64(&self) -> u64 {
        self.0.parse().unwrap_or(u64::MAX)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A client → host call. Every envelope struct speaks camelCase on the wire
/// (plan §2.3): a snake_case field name would be parked in `extra` instead of the
/// typed field, which is a silent contract break rather than a parse error.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub v: u8,
    pub id: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub params: Value,
    /// Fields this version of the decoder does not know about. They are carried
    /// forward untouched so a newer host's addition is not a parse failure here.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Success {
    pub v: u8,
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub v: u8,
    pub id: String,
    pub ok: bool,
    pub error: crate::error::RpcError,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventFrame {
    pub v: u8,
    pub event: String,
    pub epoch: String,
    pub seq: Quantity,
    #[serde(default)]
    pub data: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Frame {
    Event(EventFrame),
    Failure(Failure),
    Success(Success),
    Request(Request),
}

/// A JSONL transport error that is *not* a protocol-level `RpcError`: the bytes
/// never formed a decodable frame, so the caller must decide whether to fail the
/// connection or resync.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    Empty,
    TooLarge { bytes: usize, limit: usize },
    NotUtf8,
    Malformed(String),
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::Empty => write!(f, "frame is empty"),
            FrameError::TooLarge { bytes, limit } => {
                write!(f, "frame of {bytes} B exceeds the {limit} B limit")
            }
            FrameError::NotUtf8 => write!(f, "frame is not valid UTF-8"),
            FrameError::Malformed(reason) => {
                write!(f, "frame is not a valid protocol frame: {reason}")
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// Decode one line. Order matters: oversize and non-UTF-8 are transport faults
/// and must not be laundered into a generic JSON error.
pub fn decode_line(line: &[u8]) -> Result<Frame, FrameError> {
    // The size limit is checked before anything else: an oversized line is a
    // transport fault whatever its content, and a blank megabyte must not be
    // reported as "nothing happened" (or scanned for whitespace) first.
    if line.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge {
            bytes: line.len(),
            limit: MAX_FRAME_BYTES,
        });
    }
    if line.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(FrameError::Empty);
    }
    let text = std::str::from_utf8(line).map_err(|_| FrameError::NotUtf8)?;
    let value: Value =
        serde_json::from_str(text).map_err(|e| FrameError::Malformed(e.to_string()))?;
    serde_json::from_value(value).map_err(|e| FrameError::Malformed(e.to_string()))
}

pub fn encode_line(frame: &Frame) -> Result<String, serde_json::Error> {
    let mut text = serde_json::to_string(frame)?;
    if text.len() > MAX_FRAME_BYTES {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "encoded frame is {} B, over the {MAX_FRAME_BYTES} B limit",
                text.len()
            ),
        )));
    }
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three decoders are pinned on the same verdict here: an oversized line is
    /// a transport fault even when every byte of it is whitespace.
    #[test]
    fn a_blank_oversized_line_is_a_transport_fault() {
        let blank = vec![b' '; MAX_FRAME_BYTES + 1];
        assert_eq!(
            decode_line(&blank).unwrap_err(),
            FrameError::TooLarge {
                bytes: MAX_FRAME_BYTES + 1,
                limit: MAX_FRAME_BYTES
            }
        );
        assert_eq!(decode_line(b"   \n").unwrap_err(), FrameError::Empty);
        assert_eq!(
            decode_line(&vec![b' '; MAX_FRAME_BYTES]).unwrap_err(),
            FrameError::Empty
        );
    }

    #[test]
    fn quantity_rejects_non_decimal_forms() {
        assert!(Quantity::new("18").is_some());
        assert!(Quantity::new("-1").is_none());
        assert!(Quantity::new("1.0").is_none());
        assert!(Quantity::new("").is_none());
        assert!(Quantity::new("1e3").is_none());
    }

    /// `extra` swallows unknown keys, so a misspelled field is indistinguishable
    /// from a forward-compatibility addition unless the binding is asserted here.
    #[test]
    fn idempotency_key_binds_to_its_field_not_to_extra() {
        let request: Request = serde_json::from_str(
            r#"{"v":1,"id":"r1","method":"task.start","idempotencyKey":"op-1","params":{},"x":1}"#,
        )
        .unwrap();
        assert_eq!(request.idempotency_key.as_deref(), Some("op-1"));
        assert_eq!(request.extra.keys().collect::<Vec<_>>(), vec!["x"]);
        let text = serde_json::to_string(&request).unwrap();
        assert!(text.contains("\"idempotencyKey\""), "{text}");
        assert!(!text.contains("idempotency_key"), "{text}");
    }
}
