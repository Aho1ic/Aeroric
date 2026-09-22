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
pub mod project;
pub mod session;
pub mod task;
pub mod terminal;

pub use capability::{AgentCapabilities, Capability, HostCapabilities};
pub use envelope::{
    decode_line, encode_line, EventFrame, Failure, Frame, FrameError, Quantity, Request, Success,
    MAX_FRAME_BYTES,
};
pub use error::{DispatchError, ErrorCode, RpcError};
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
    }
}
