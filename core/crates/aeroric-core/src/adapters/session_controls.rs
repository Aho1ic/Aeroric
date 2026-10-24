//! 会话控制策略(AP7 / P9-01):plan、compact、fork、revert、share。
//!
//! 三条不可协商的规则,这里是它们的**唯一**实现点:
//!
//! 1. **未广告即拒绝**。每个入口先查 [`SessionCapability::advertised`];宿主必须在
//!    服务端拒绝(`ControlError::Unsupported`),UI 隐藏不是安全边界。
//! 2. **入队时权限固定**。队列里的任务带着**入队那一刻**的 `permissionMode`,
//!    消费时不得按当前 UI 设置重新解释([`ControlError::PermissionDrift`]),控制命令
//!    也不得把权限抬高到入队值之上([`ControlError::PermissionEscalation`])。
//! 3. **破坏性操作先给证据**。`revert` 若影响工作区,必须先有 diff 预览被确认
//!    ([`ControlError::PreviewRequired`]);`share` 默认本地脱敏导出,公开上传需要显式
//!    endpoint + 用户确认([`ControlError::PublicShareNeedsEndpoint`] / `ConfirmationRequired`)。
//!
//! session revert 与 git revert 是**两件事**,分别用 [`RevertTarget::Session`] 与
//! [`RevertTarget::Workspace`] 表达,后者必须走预览门。

use aeroric_protocol::capability::AgentCapabilities;
use aeroric_protocol::task::PermissionMode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 会话控制能力。字面量与 iOS `CompanionSessionControl` 对齐(`session.plan` 等),
/// `share` 是本阶段新增的第五项。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionCapability {
    Plan,
    Compact,
    Fork,
    Revert,
    Share,
}

impl SessionCapability {
    pub const ALL: &'static [SessionCapability] = &[
        SessionCapability::Plan,
        SessionCapability::Compact,
        SessionCapability::Fork,
        SessionCapability::Revert,
        SessionCapability::Share,
    ];

    /// 与 iOS 端既有字面量一致的控制方法名。
    pub fn method(&self) -> &'static str {
        match self {
            SessionCapability::Plan => "session.plan",
            SessionCapability::Compact => "session.compact",
            SessionCapability::Fork => "session.fork",
            SessionCapability::Revert => "session.revert",
            SessionCapability::Share => "session.share",
        }
    }

    /// 该能力是否被适配器广告。未广告的能力**宿主必须拒绝**。
    pub fn advertised(&self, capabilities: &AgentCapabilities) -> bool {
        match self {
            SessionCapability::Plan => capabilities.plan_mode,
            SessionCapability::Compact => capabilities.compact,
            SessionCapability::Fork => capabilities.fork,
            SessionCapability::Revert => capabilities.revert,
            SessionCapability::Share => capabilities.share,
        }
    }
}

/// revert 的作用对象:会话历史与工作区**分开**,后者才有破坏性。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevertTarget {
    /// 只回退会话历史指针。
    Session,
    /// 回退工作区文件,必须带确认过的 diff 预览。
    Workspace { affected_paths: Vec<String> },
}

/// 分享目的地。默认本地,公开上传是显式动作。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShareDestination {
    /// 本地导出到用户选定的目录(默认路径)。
    Local { path: String },
    /// 公开上传:必须显式 endpoint。
    Public { endpoint: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareRedaction {
    /// 默认:剥凭据/密钥/环境变量值/主目录绝对路径。
    #[default]
    Strict,
    /// 用户显式要求原样导出;UI 必须明示风险。
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionControlRequest {
    pub capability: SessionCapability,
    pub session_id: String,
    /// 入队那一刻写进任务记录的权限模式(P9-01:队列消费按它解释)。
    pub queued_permission_mode: PermissionMode,
    /// 队列消费时 UI 当前的权限模式。与入队值不同即拒绝,而不是「取新的」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_permission_mode: Option<PermissionMode>,
    /// 该控制命令自身要求的权限模式(如 plan 命令)。高于入队值即拒绝。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_permission_mode: Option<PermissionMode>,
    /// `Plan` 能力的目标状态。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert_target: Option<RevertTarget>,
    /// 用户是否已经看过并确认了 diff 预览。
    #[serde(default)]
    pub preview_confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_destination: Option<ShareDestination>,
    #[serde(default)]
    pub share_redaction: ShareRedaction,
    /// 公开上传/原样导出这类不可逆动作的用户确认。
    #[serde(default)]
    pub confirmed: bool,
}

impl SessionControlRequest {
    pub fn new(capability: SessionCapability, session_id: impl Into<String>) -> Self {
        Self {
            capability,
            session_id: session_id.into(),
            queued_permission_mode: PermissionMode::Ask,
            current_permission_mode: None,
            requested_permission_mode: None,
            plan_enabled: None,
            revert_target: None,
            preview_confirmed: false,
            share_destination: None,
            share_redaction: ShareRedaction::Strict,
            confirmed: false,
        }
    }

    pub fn queued_permission(mut self, mode: PermissionMode) -> Self {
        self.queued_permission_mode = mode;
        self
    }

    pub fn current_permission(mut self, mode: PermissionMode) -> Self {
        self.current_permission_mode = Some(mode);
        self
    }

    pub fn requested_permission(mut self, mode: PermissionMode) -> Self {
        self.requested_permission_mode = Some(mode);
        self
    }

    pub fn plan_enabled(mut self, enabled: bool) -> Self {
        self.plan_enabled = Some(enabled);
        self
    }

    pub fn revert_target(mut self, target: RevertTarget) -> Self {
        self.revert_target = Some(target);
        self
    }

    pub fn preview_confirmed(mut self, confirmed: bool) -> Self {
        self.preview_confirmed = confirmed;
        self
    }

    pub fn share_destination(mut self, destination: ShareDestination) -> Self {
        self.share_destination = Some(destination);
        self
    }

    pub fn share_redaction(mut self, redaction: ShareRedaction) -> Self {
        self.share_redaction = redaction;
        self
    }

    pub fn confirmed(mut self, confirmed: bool) -> Self {
        self.confirmed = confirmed;
        self
    }
}

/// 拒绝原因。全部是**服务端拒绝**,不是 UI 提示。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ControlError {
    /// 适配器未广告该能力(附上未支持清单的出处)。
    Unsupported {
        capability: SessionCapability,
        method: String,
        note: Option<String>,
    },
    /// 队列消费时的权限模式与入队值不一致。
    PermissionDrift {
        queued: PermissionMode,
        current: PermissionMode,
    },
    /// 控制命令试图把权限抬到入队值之上。
    PermissionEscalation {
        queued: PermissionMode,
        requested: PermissionMode,
    },
    /// 影响工作区的 revert 缺少确认过的预览。
    PreviewRequired { affected_paths: Vec<String> },
    /// 公开上传缺显式 endpoint。
    PublicShareNeedsEndpoint,
    /// 不可逆动作缺用户确认(公开上传 / 原样导出)。
    ConfirmationRequired { action: String },
    /// revert/share 请求缺少必要字段。
    MissingTarget { capability: SessionCapability },
}

/// 分享导出的产物:脱敏后的 payload + 被改动过的路径清单(给 UI 展示)。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareExport {
    pub payload: Value,
    pub redacted: Vec<String>,
    pub destination: ShareDestination,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionControlResponse {
    pub capability: SessionCapability,
    pub method: String,
    /// `Plan` 的目标状态回显(供 UI 与队列核对)。
    pub plan_enabled: Option<bool>,
    /// `Revert` 已确认的目标。
    pub revert_target: Option<RevertTarget>,
    /// `Share` 的脱敏导出;本地导出在授权阶段就返回,公开上传在确认后才返回。
    pub share: Option<ShareExport>,
}

/// 会话控制策略持有者:一个(适配器能力, 入队权限)对。
#[derive(Clone, Debug)]
pub struct SessionControls {
    capabilities: AgentCapabilities,
    queued_permission_mode: PermissionMode,
}

impl SessionControls {
    pub fn new(capabilities: AgentCapabilities, queued_permission_mode: PermissionMode) -> Self {
        Self {
            capabilities,
            queued_permission_mode,
        }
    }

    pub fn capabilities(&self) -> &AgentCapabilities {
        &self.capabilities
    }

    fn note_for(&self, capability: SessionCapability) -> Option<String> {
        let method = capability.method();
        let unsupported = self.capabilities.extra.get("unsupported")?.as_array()?;
        unsupported
            .iter()
            .any(|value| value.as_str() == Some(method))
            .then(|| format!("adapter does not implement {method}"))
    }

    /// 队列消费时的权限检查:入队值必须与当前值一致,且控制命令不得抬高权限。
    fn check_permissions(&self, request: &SessionControlRequest) -> Result<(), ControlError> {
        if let Some(current) = request.current_permission_mode {
            if current != self.queued_permission_mode {
                return Err(ControlError::PermissionDrift {
                    queued: self.queued_permission_mode,
                    current,
                });
            }
        }
        if let Some(requested) = request.requested_permission_mode {
            if requested.rank() > self.queued_permission_mode.rank() {
                return Err(ControlError::PermissionEscalation {
                    queued: self.queued_permission_mode,
                    requested,
                });
            }
        }
        Ok(())
    }

    /// 授权并产出控制响应。所有拒绝都在这里,调用方不得绕开。
    pub fn authorize(
        &self,
        request: &SessionControlRequest,
        payload: &Value,
    ) -> Result<SessionControlResponse, ControlError> {
        let capability = request.capability;
        if !capability.advertised(&self.capabilities) {
            return Err(ControlError::Unsupported {
                capability,
                method: capability.method().to_string(),
                note: self.note_for(capability),
            });
        }
        self.check_permissions(request)?;

        let mut response = SessionControlResponse {
            capability,
            method: capability.method().to_string(),
            plan_enabled: None,
            revert_target: None,
            share: None,
        };

        match capability {
            SessionCapability::Plan => {
                response.plan_enabled = request.plan_enabled;
            }
            SessionCapability::Compact | SessionCapability::Fork => {}
            SessionCapability::Revert => {
                let Some(target) = request.revert_target.clone() else {
                    return Err(ControlError::MissingTarget { capability });
                };
                if let RevertTarget::Workspace { affected_paths } = &target {
                    // 破坏性:必须先看 diff。
                    if !request.preview_confirmed {
                        return Err(ControlError::PreviewRequired {
                            affected_paths: affected_paths.clone(),
                        });
                    }
                }
                response.revert_target = Some(target);
            }
            SessionCapability::Share => {
                let Some(destination) = request.share_destination.clone() else {
                    return Err(ControlError::MissingTarget { capability });
                };
                if let ShareDestination::Public { endpoint } = &destination {
                    if endpoint.trim().is_empty() {
                        return Err(ControlError::PublicShareNeedsEndpoint);
                    }
                    // 公开上传不可逆:必须显式确认。
                    if !request.confirmed {
                        return Err(ControlError::ConfirmationRequired {
                            action: "public share".to_string(),
                        });
                    }
                }
                if request.share_redaction == ShareRedaction::None && !request.confirmed {
                    return Err(ControlError::ConfirmationRequired {
                        action: "unredacted share".to_string(),
                    });
                }
                let export = export_share(payload, destination, request.share_redaction);
                response.share = Some(export);
            }
        }
        Ok(response)
    }
}

const REDACTED_KEYS: &[&str] = &[
    "apikey",
    "api_key",
    "authorization",
    "credential",
    "credentials",
    "env",
    "password",
    "secret",
    "token",
    "privatekey",
    "private_key",
];

/// 默认脱敏:删掉凭据类键,并把主目录绝对路径折叠成 `~`。
///
/// 这是**默认**,不是可选优化:`ShareDestination::Local` 也走这里。
pub fn export_share(
    payload: &Value,
    destination: ShareDestination,
    redaction: ShareRedaction,
) -> ShareExport {
    if redaction == ShareRedaction::None {
        return ShareExport {
            payload: payload.clone(),
            redacted: Vec::new(),
            destination,
        };
    }
    let mut redacted = Vec::new();
    let payload = redact_value(payload, "", &mut redacted);
    ShareExport {
        payload,
        redacted,
        destination,
    }
}

fn redact_value(value: &Value, pointer: &str, redacted: &mut Vec<String>) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, child) in map {
                let lowered = key.to_ascii_lowercase();
                let child_pointer = format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
                if REDACTED_KEYS.contains(&lowered.as_str()) {
                    redacted.push(child_pointer);
                    continue;
                }
                out.insert(key.clone(), redact_value(child, &child_pointer, redacted));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    let child_pointer = format!("{pointer}/{index}");
                    redact_value(child, &child_pointer, redacted)
                })
                .collect(),
        ),
        Value::String(text) => {
            // 折叠主目录组件、保留尾段:`/Users/macbook/a/b` → `/Users/~/a/b`。
            // 只删用户名,不删路径结构——否则分享出来的记录会失去可读性。
            let scrub = |prefix: &str, rest: &str| match rest.split_once('/') {
                Some((_home, tail)) => Some(format!("{prefix}~/{tail}")),
                None => Some(format!("{prefix}~")),
            };
            let cleaned = text
                .strip_prefix("/Users/")
                .and_then(|rest| scrub("/Users/", rest))
                .or_else(|| {
                    text.strip_prefix("/home/")
                        .and_then(|rest| scrub("/home/", rest))
                });
            match cleaned {
                Some(cleaned) => {
                    redacted.push(pointer.to_string());
                    Value::String(cleaned)
                }
                None => Value::String(text.clone()),
            }
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dsh_caps() -> AgentCapabilities {
        crate::adapters::probe_agent_capabilities(
            &crate::adapters::CapabilityProbe::new("dsh")
                .native_prompt(true)
                .supports(&["session.plan", "session.compact", "session.fork", "session.revert", "session.share"]),
        )
    }

    #[test]
    fn unadvertised_capability_is_refused_by_the_server() {
        let caps = crate::adapters::probe_agent_capabilities(
            &crate::adapters::CapabilityProbe::new("dsh")
                .native_prompt(true)
                .supports(&["session.fork"]),
        );
        let controls = SessionControls::new(caps, PermissionMode::Ask);
        let request = SessionControlRequest::new(SessionCapability::Compact, "s1");
        let error = controls.authorize(&request, &json!({})).unwrap_err();
        match error {
            ControlError::Unsupported { capability, method, note } => {
                assert_eq!(capability, SessionCapability::Compact);
                assert_eq!(method, "session.compact");
                assert!(note.unwrap().contains("session.compact"));
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn queued_permission_is_authoritative_not_the_current_ui_setting() {
        let controls = SessionControls::new(dsh_caps(), PermissionMode::Ask);
        let request = SessionControlRequest::new(SessionCapability::Compact, "s1")
            .current_permission(PermissionMode::FullAccess);
        let error = controls.authorize(&request, &json!({})).unwrap_err();
        assert_eq!(
            error,
            ControlError::PermissionDrift {
                queued: PermissionMode::Ask,
                current: PermissionMode::FullAccess,
            }
        );
    }

    #[test]
    fn control_command_cannot_escalate_permissions_beyond_the_queue_value() {
        let controls = SessionControls::new(dsh_caps(), PermissionMode::AutoEdit);
        let request = SessionControlRequest::new(SessionCapability::Plan, "s1")
            .requested_permission(PermissionMode::FullAccess);
        let error = controls.authorize(&request, &json!({})).unwrap_err();
        assert_eq!(
            error,
            ControlError::PermissionEscalation {
                queued: PermissionMode::AutoEdit,
                requested: PermissionMode::FullAccess,
            }
        );
        // 同级或更低是允许的。
        let ok = SessionControlRequest::new(SessionCapability::Plan, "s1")
            .requested_permission(PermissionMode::AutoEdit)
            .plan_enabled(true);
        assert_eq!(
            controls.authorize(&ok, &json!({})).unwrap().plan_enabled,
            Some(true)
        );
    }

    #[test]
    fn session_revert_needs_no_preview_but_workspace_revert_does() {
        let controls = SessionControls::new(dsh_caps(), PermissionMode::Ask);
        let session = SessionControlRequest::new(SessionCapability::Revert, "s1")
            .revert_target(RevertTarget::Session);
        assert_eq!(
            controls.authorize(&session, &json!({})).unwrap().revert_target,
            Some(RevertTarget::Session)
        );

        let workspace = SessionControlRequest::new(SessionCapability::Revert, "s1")
            .revert_target(RevertTarget::Workspace {
                affected_paths: vec!["src/main.rs".to_string()],
            });
        let error = controls.authorize(&workspace, &json!({})).unwrap_err();
        assert_eq!(
            error,
            ControlError::PreviewRequired {
                affected_paths: vec!["src/main.rs".to_string()],
            }
        );

        let confirmed = workspace.clone().preview_confirmed(true);
        assert!(controls.authorize(&confirmed, &json!({})).is_ok());
    }

    #[test]
    fn public_share_needs_an_endpoint_and_confirmation() {
        let controls = SessionControls::new(dsh_caps(), PermissionMode::Ask);
        let no_endpoint = SessionControlRequest::new(SessionCapability::Share, "s1")
            .share_destination(ShareDestination::Public {
                endpoint: "  ".to_string(),
            })
            .confirmed(true);
        assert_eq!(
            controls.authorize(&no_endpoint, &json!({})).unwrap_err(),
            ControlError::PublicShareNeedsEndpoint
        );

        let unconfirmed = SessionControlRequest::new(SessionCapability::Share, "s1")
            .share_destination(ShareDestination::Public {
                endpoint: "https://example.com/upload".to_string(),
            });
        assert_eq!(
            controls.authorize(&unconfirmed, &json!({})).unwrap_err(),
            ControlError::ConfirmationRequired {
                action: "public share".to_string(),
            }
        );

        let ok = unconfirmed.clone().confirmed(true);
        assert!(controls.authorize(&ok, &json!({})).is_ok());
    }

    #[test]
    fn local_share_is_redacted_by_default() {
        let controls = SessionControls::new(dsh_caps(), PermissionMode::Ask);
        let payload = json!({
            "sessionId": "s1",
            "cwd": "/Users/macbook/Downloads/project",
            "apiKey": "sk-live-123",
            "env": { "TOKEN": "t" },
            "messages": [{ "role": "user", "text": "hi" }],
        });
        let request = SessionControlRequest::new(SessionCapability::Share, "s1")
            .share_destination(ShareDestination::Local {
                path: "~/Desktop".to_string(),
            });
        let response = controls.authorize(&request, &payload).unwrap();
        let export = response.share.unwrap();
        assert!(export.payload.get("apiKey").is_none());
        assert!(export.payload.get("env").is_none());
        assert_eq!(export.payload["cwd"], json!("/Users/~/Downloads/project"));
        assert!(export.redacted.iter().any(|pointer| pointer == "/apiKey"));
        assert!(export.redacted.iter().any(|pointer| pointer == "/cwd"));
        // 内容本身保留,脱敏不等于清空。
        assert_eq!(export.payload["messages"][0]["text"], json!("hi"));
    }

    #[test]
    fn unredacted_share_requires_explicit_confirmation() {
        let controls = SessionControls::new(dsh_caps(), PermissionMode::Ask);
        let request = SessionControlRequest::new(SessionCapability::Share, "s1")
            .share_destination(ShareDestination::Local {
                path: "/tmp/a.json".to_string(),
            })
            .share_redaction(ShareRedaction::None);
        assert_eq!(
            controls.authorize(&request, &json!({})).unwrap_err(),
            ControlError::ConfirmationRequired {
                action: "unredacted share".to_string(),
            }
        );
        assert!(controls
            .authorize(&request.confirmed(true), &json!({"apiKey": "x"}))
            .is_ok());
    }
}
