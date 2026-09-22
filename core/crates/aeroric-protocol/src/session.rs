//! Session ownership. The owner of a session is what was actually recorded on
//! disk, not what the UI dropdown currently says — after a failed agent switch
//! those two differ, and using the wrong one resumes against another agent's home.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionSource {
    /// Produced by the agent this task is configured with.
    #[default]
    Native,
    /// Produced by a different agent and adopted for reading.
    Adopted,
    /// Produced by a cross-protocol handoff, which starts a new session and keeps
    /// the origin so a resume can never silently claim native continuity.
    Handoff,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOwner {
    pub family: crate::task::ProtocolFamily,
    pub session_id: String,
    /// Absolute path of the transcript file, as recorded at discovery time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub source: SessionSource,
    /// Present when the session did not come from the task's current agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_by: Option<String>,
    /// A handoff names the session it came from, so history stays openable while
    /// resume is refused on the new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_session_id: Option<String>,
}

/// Content this build cannot interpret: an unknown tool call, an unrecognized
/// content block, a message from an agent whose format we only partly know.
/// It is preserved with its source id rather than dropped, because dropping is
/// indistinguishable from "the agent never said that".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueContent {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    /// Which family produced it, so a later parser can claim it without guessing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_family: Option<crate::task::ProtocolFamily>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

/// One page of history. Cursor is opaque to clients; `has_more` plus a stable
/// message id is what lets pagination avoid jumping when new content arrives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    pub session_id: String,
    pub family: crate::task::ProtocolFamily,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub id: String,
    pub role: MessageRole,
    #[serde(default)]
    pub blocks: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentBlock {
    Text { kind: String, text: String },
    Unknown(OpaqueContent),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handoff_session_keeps_its_origin_visible() {
        let owner: SessionOwner = serde_json::from_str(
            r#"{"family":"dsh","sessionId":"s2","source":"handoff","originSessionId":"s1"}"#,
        )
        .unwrap();
        assert_eq!(owner.source, SessionSource::Handoff);
        assert_eq!(owner.origin_session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn unknown_content_is_preserved_not_dropped() {
        let page: SessionPage = serde_json::from_str(
            r#"{"sessionId":"s","family":"omp","hasMore":true,
               "messages":[{"id":"m1","role":"assistant","blocks":[
                 {"kind":"text","text":"hi"},
                 {"kind":"mystery","sourceId":"b7","payload":{"raw":1},"originFamily":"claude"}]}]}"#,
        )
        .unwrap();
        assert_eq!(page.messages[0].blocks.len(), 2);
        match &page.messages[0].blocks[1] {
            ContentBlock::Unknown(opaque) => {
                assert_eq!(opaque.kind, "mystery");
                assert_eq!(
                    opaque.origin_family,
                    Some(crate::task::ProtocolFamily::Claude)
                );
            }
            other => panic!("expected the unrecognised block to survive as opaque, got {other:?}"),
        }
    }
}
