//! 外部 Agent 能力适配面(AP7,P9-01 契约)。
//!
//! ## 为什么是「探测 + 拒绝」而不是「猜」
//!
//! `aeroric_protocol::capability` 的设计前提是 `v == 1` 从不蕴含能力:握手返回的
//! 是**版本交集 + 显式能力表**,`UNSUPPORTED_CAPABILITY` 必须是服务端拒绝,而不是
//! 客户端对宿主「大概有什么」的猜测。本模块把该原则落到外部族:每个 advertised 项
//! 都必须由**具体适配器 + 版本 + 方法表**探测得出,探不到就是 `false` 并附 `note`。
//!
//! ## 明确禁止的做法(P9-01 原文)
//!
//! **不得从 TUI 文本抓取来伪造协议能力**。本模块只读结构化事实:
//! [`crate::agents::dsh_host::api`] 的方法表与版本,以及(未来的)其他适配器显式
//! 声明的清单。任何「解析终端输出推断出 fork 可用」的实现都违背本契约。
//!
//! ## 落点顺序(P9-01 契约 2)
//!
//! [`SessionControls`] 的每个入口都先查 [`SessionCapability::advertised`],未广告
//! 即 [`ControlError::Unsupported`];**宿主也必须独立拒绝**,UI 隐藏只是体验层的
//! 第一道,不是安全边界。

pub mod capability_probe;
pub mod external_protocols;
pub mod session_controls;
pub mod work_graph;
pub mod workflow;

pub use capability_probe::{probe_agent_capabilities, CapabilityProbe};
pub use external_protocols::{
    admit_attachments, answer_webview_credential_request, apply_login, AcpAttachment,
    AcpNegotiation, AcpRefusal, AcpSession, AttachmentKind, AttachmentRefusal, MicPermission,
    OAuthError, OAuthFlow, OAuthLaunch, OAuthStatus, SandboxError, SandboxReport, SandboxState,
    ToolActivityNotice, ToolPermissionGrant, ToolPermissionHandoff, ToolPermissionKind, VoiceError,
    VoicePhase, VoiceSession, WebviewCredentialRequest,
};
pub use session_controls::{
    ControlError, SessionCapability, SessionControlRequest, SessionControlResponse,
    SessionControls, ShareExport, ShareRedaction,
};
pub use work_graph::{
    CheckpointOutcome, CheckpointWriter, ExternalRef, GoalRecord, GoalStatus, GoalStore,
    MemoryItem, MemorySource, NodeKey, NodeStatus, PersistenceKind, SubagentRecord,
    SubagentRegistry, TaskNode, TaskTree, WorkGraphError, WorkGraphWarning,
};
pub use workflow::{
    MaintenanceArtifact, MaintenanceArtifactState, MaintenanceError, MaintenanceKind,
    MaintenanceStore, MemoryMaintenanceConfig, ResumeRefusal, RetryDecision, RetryPolicy,
    StepRecord, StepStatus, WorkflowJournal,
};
