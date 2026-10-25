//! Stable local-protocol v1 DTOs: envelope, projects, tasks, sessions, capability,
//! errors, events and the terminal plane.
//!
//! This crate is the wire contract shared by `aeroric-core`, `aeroric-host`,
//! `aeroric-cli`, the SwiftUI client and the Tauri embedded adapter. It must stay
//! free of Tauri, React, Swift and storage concerns (plan INV-02), and every type
//! here is generated into `contracts/native/v1/` by `bin/export-schema.rs`; the
//! Swift and TypeScript decoders read the same fixtures.

pub mod capability;
pub mod envelope;
pub mod error;
pub mod event;
pub mod legacy_cleanup;
pub mod project;
pub mod session;
pub mod session_live;
pub mod submission;
pub mod task;
pub mod terminal;

pub use capability::{AgentCapabilities, Capability, HostCapabilities};
pub use envelope::{
    decode_line, encode_line, EventFrame, Failure, Frame, FrameError, Quantity, Request, Success,
    MAX_FRAME_BYTES,
};
pub use error::{DispatchError, ErrorCode, ErrorDetail, RpcError, StorageRejection};
pub use event::{GapNotice, GapReason, SubscriptionHandshake};
pub use project::{Project, ProjectTarget};
pub use session::{
    ContentBlock, MessageRole, OpaqueContent, SessionMessage, SessionOwner, SessionPage,
    SessionSource,
};
pub use task::{AgentId, BuiltInAgent, PermissionMode, ProtocolFamily, TaskSnapshot, TaskStatus};
pub use terminal::{
    BytesBase64, TerminalGap, TerminalIdentity, TerminalLease, TerminalOutput, TerminalResize,
    MAX_TERMINAL_CHUNK_BYTES,
};

/// Local protocol version carried in every frame. Distinct from the relay protocol
/// version, and never a proxy for capability support.
pub const PROTOCOL_VERSION: u8 = 1;

/// Wire names for a vocabulary list, spelled by `serde` itself so the published
/// contract can never disagree with what the deserializers accept.
fn wire_names<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values
        .iter()
        .map(|value| {
            serde_json::to_value(value)
                .ok()
                .and_then(|name| name.as_str().map(str::to_string))
                .expect("a vocabulary variant must serialize as a JSON string")
        })
        .collect()
}

pub fn task_statuses() -> Vec<String> {
    wire_names(task::TaskStatus::ALL)
}
pub fn protocol_families() -> Vec<String> {
    wire_names(task::ProtocolFamily::ALL)
}
pub fn permission_modes() -> Vec<String> {
    wire_names(task::PermissionMode::ALL)
}
pub fn built_in_agents() -> Vec<String> {
    wire_names(task::BuiltInAgent::ALL)
}
pub fn error_codes() -> Vec<String> {
    wire_names(error::ErrorCode::ALL)
}

/// ADR-037 的意图词汇表：13 条具名意图，每条按 ADR-026 归入 `metadata`（冲突可自动重放
/// 一次）或 `state`（永不自动重放）。权威枚举住 core（`tasks::service::TaskIntent`），
/// 这份名单是它的**线上事实**——契约、TS 与宿主都从这里拼写，core 侧有一条对齐用例钉住
/// "名单与 serde 判别式不许各长一份"。
pub fn task_intents() -> Vec<(&'static str, &'static str)> {
    vec![
        ("rename", "metadata"),
        ("toggleStar", "metadata"),
        ("archive", "metadata"),
        ("unarchive", "metadata"),
        ("attachSession", "metadata"),
        ("setWorktree", "metadata"),
        ("discardWorktree", "metadata"),
        ("recordDiffStats", "metadata"),
        ("normalizeAfterBoot", "metadata"),
        ("editTodo", "metadata"),
        ("create", "state"),
        ("delete", "state"),
        ("switchConfig", "state"),
        ("resumeWithSession", "state"),
    ]
}

pub fn task_intent_ops() -> Vec<String> {
    task_intents()
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect()
}

pub fn task_intent_ops_of(kind: &str) -> Vec<String> {
    task_intents()
        .into_iter()
        .filter(|(_, k)| *k == kind)
        .map(|(name, _)| name.to_string())
        .collect()
}

/// The only keys a host may put into `RpcError.detail` (Q12). Derived from the
/// struct so the published contract and the type cannot disagree.
pub fn detail_allowed_keys() -> Vec<String> {
    error::ErrorDetail::allowed_keys()
}

/// Local protocol version policy (Q13): the major version must match exactly.
/// Compatibility inside v1 is carried by named capabilities, never inferred from a
/// version number, and a v2 requires its own ADR before it can exist.
pub fn handshake_allows_peer(version: u8) -> bool {
    version == PROTOCOL_VERSION
}

/// Crate version string reported during the host handshake.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn protocol_version_is_pinned_to_one() {
        assert_eq!(super::PROTOCOL_VERSION, 1);
        assert!(!super::version().is_empty());
    }

    #[test]
    fn published_vocabularies_have_the_documented_size_and_spelling() {
        assert_eq!(
            super::task_statuses(),
            [
                "todo",
                "pending",
                "running",
                "input_required",
                "detached",
                "interrupted",
                "done",
                "failed",
                "cancelled"
            ]
        );
        assert_eq!(
            super::protocol_families(),
            ["claude", "codex", "dsh", "omp"]
        );
        assert_eq!(
            super::permission_modes(),
            ["ask", "auto_edit", "full_access"]
        );
        assert_eq!(
            super::built_in_agents(),
            ["claude", "claude_gpt55", "codex", "dsh", "omp"]
        );
        assert_eq!(super::error_codes().len(), 11);
        assert_eq!(super::error_codes()[9], "INTERACTIVE_UI_REQUIRED");
        // ADR-037/026：词汇表的账——14 条、10/4、名字唯一。
        let intents = super::task_intents();
        assert_eq!(intents.len(), 14);
        assert_eq!(super::task_intent_ops_of("metadata").len(), 10);
        assert_eq!(super::task_intent_ops_of("state").len(), 4);
        let mut names: Vec<&str> = intents.iter().map(|(n, _)| *n).collect();
        let before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), before, "op 名不得有重");
        assert!(
            intents
                .iter()
                .all(|(_, kind)| matches!(*kind, "metadata" | "state")),
            "分类只有两个合法取值"
        );
    }
}
