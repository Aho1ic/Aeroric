//! Terminal plane. Kept separate from business events on purpose: raw PTY bytes
//! have their own offset and generation, and conflating them with `seq` would make
//! a replayed byte block look like a missed state change.

use serde::{Deserialize, Serialize};

/// Identity of one PTY stream. `(task_id, generation)` is the only valid key: a
/// restarted process must never inherit the previous stream's offset.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalIdentity {
    pub task_id: String,
    pub generation: crate::envelope::Quantity,
}

/// Raw bytes as base64. The plan fixes 64 KiB of *raw* bytes per chunk; base64
/// inflates by 4/3, so the frame limit is checked after encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BytesBase64(pub String);

/// Maximum raw bytes one terminal chunk may carry.
pub const MAX_TERMINAL_CHUNK_BYTES: usize = 64 * 1024;

impl BytesBase64 {
    /// Rejects anything that is not valid base64 rather than emitting replacement
    /// bytes: a silently mangled PTY chunk can destroy a screen the user cannot see.
    pub fn decode(&self) -> Option<Vec<u8>> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        if self.0.is_empty() {
            return Some(Vec::new());
        }
        let body = self.0.as_bytes();
        if !body.len().is_multiple_of(4) {
            return None;
        }
        let mut padding = 0usize;
        while padding < 2 && body[body.len() - 1 - padding] == b'=' {
            padding += 1;
        }
        let mut acc: u32 = 0;
        let mut bits: u32 = 0;
        let mut out: Vec<u8> = Vec::new();
        for byte in &body[..body.len() - padding] {
            let value = ALPHABET.iter().position(|c| c == byte)? as u32;
            acc = (acc << 6) | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(((acc >> bits) & 0xFF) as u8);
            }
        }
        if bits >= 6 || acc & ((1u32 << bits) - 1) != 0 {
            return None;
        }
        if out.len() != body.len() / 4 * 3 - padding {
            return None;
        }
        Some(out)
    }
}

impl<'de> Deserialize<'de> for BytesBase64 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        let value = Self(text);
        if value.decode().is_none() {
            return Err(serde::de::Error::custom("bytesBase64 is not valid base64"));
        }
        Ok(value)
    }
}

impl Serialize for BytesBase64 {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutput {
    #[serde(flatten)]
    pub identity: TerminalIdentity,
    pub offset: crate::envelope::Quantity,
    pub next_offset: crate::envelope::Quantity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// True when the buffer was compacted: the renderer must reset rather than
    /// continue feeding bytes into an unknown escape-sequence position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_base64: Option<BytesBase64>,
}

/// The terminal plane's own reset notice. Deliberately not `GapNotice`: that one
/// counts control-stream `seq` numbers, while this one says "the byte buffer was
/// compacted, clear the screen before feeding more". A renderer that treated the
/// two alike would resume mid escape sequence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalGap {
    #[serde(flatten)]
    pub identity: TerminalIdentity,
    pub reason: crate::event::GapReason,
    /// True means the consumer must reset, not continue.
    pub reset: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// Lowest offset the host can still replay from, when it can say so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_offset: Option<crate::envelope::Quantity>,
}

/// One control lease per stream: exactly one writer may send input or resize.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalLease {
    #[serde(flatten)]
    pub identity: TerminalIdentity,
    pub holder: String,
    pub acquired_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalResize {
    pub cols: u16,
    pub rows: u16,
}

impl TerminalResize {
    /// A hidden view reporting 0x0 must not overwrite the size the user is looking at.
    pub fn is_meaningful(&self) -> bool {
        self.cols > 0 && self.rows > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_base64_decodes_to_the_exact_bytes() {
        assert_eq!(
            BytesBase64("YWJj".into()).decode().unwrap(),
            b"abc".to_vec()
        );
        assert_eq!(
            BytesBase64("+P4=".into()).decode().unwrap(),
            vec![0xf8, 0xfe]
        );
        assert_eq!(BytesBase64("".into()).decode().unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn malformed_base64_is_rejected_not_replaced() {
        for bad in ["!!!", "YWJ", "YWJj=", "a=bc", "YWJj==", "===="] {
            assert_eq!(
                BytesBase64(bad.into()).decode(),
                None,
                "{bad} must not decode"
            );
        }
    }

    #[test]
    fn a_hidden_view_cannot_resize_the_terminal_to_nothing() {
        assert!(!TerminalResize { cols: 0, rows: 24 }.is_meaningful());
        assert!(TerminalResize { cols: 80, rows: 24 }.is_meaningful());
    }

    #[test]
    fn offsets_stay_decimal_strings_on_the_wire() {
        let value: TerminalOutput = serde_json::from_str(
            r#"{"taskId":"t","generation":"3","offset":"0","nextOffset":"3","bytesBase64":"YWJj"}"#,
        )
        .unwrap();
        assert_eq!(value.identity.generation.as_u64(), 3);
        assert_eq!(value.bytes_base64.unwrap().decode().unwrap(), b"abc");
    }
}
