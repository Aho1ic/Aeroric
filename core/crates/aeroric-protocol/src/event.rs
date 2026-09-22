//! Event payloads. `seq` is assigned at one ordered publish point, so a client can
//! tell "nothing happened" apart from "I missed something".

use serde::{Deserialize, Serialize};

/// Snapshot handshake returned when a client subscribes. Registration and the
/// returned watermark are one atomic step, otherwise an event between the two RPCs
/// is lost with no gap to report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionHandshake {
    pub epoch: String,
    pub high_watermark: crate::envelope::Quantity,
    /// False when the backlog fell out of the replay window; the client must then
    /// re-read state instead of pretending it caught up.
    #[serde(default)]
    pub replayed_from_log: bool,
}

/// Why a subscriber got a gap instead of events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GapReason {
    /// Replay window (4096 events / 8 MiB by default) was exceeded.
    SnapshotRequired,
    /// A slow control consumer was dropped rather than blocking the publisher.
    Backpressure,
    /// The host restarted; old sequence numbers do not carry over.
    EpochChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GapNotice {
    pub epoch: String,
    pub from: crate::envelope::Quantity,
    pub through: crate::envelope::Quantity,
    pub reason: GapReason,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handshake_can_say_the_backlog_is_gone() {
        let value: SubscriptionHandshake = serde_json::from_str(
            r#"{"epoch":"e1","highWatermark":"4096","replayedFromLog":false}"#,
        )
        .unwrap();
        assert_eq!(value.high_watermark.as_u64(), 4096);
        assert!(!value.replayed_from_log);
    }

    #[test]
    fn a_gap_names_its_reason() {
        let gap: GapNotice = serde_json::from_str(
            r#"{"epoch":"e1","from":"10","through":"41","reason":"SNAPSHOT_REQUIRED"}"#,
        )
        .unwrap();
        assert_eq!(gap.reason, GapReason::SnapshotRequired);
        assert_eq!(gap.through.as_u64(), 41);
    }
}
