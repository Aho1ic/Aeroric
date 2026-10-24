//! AP7 适配器服务面(P9-01..P9-04 的宿主接线)。
//!
//! ## 为什么这一层必须存在
//!
//! 勘察发现:适配逻辑(`DshApiClient` 的 `session.fork` / `goals.*` / `subagents.*`
//! 等)早就在 core 里,但**唯一**的消费者是待退休的 Tauri 前端
//! (`src-tauri/src/dsh_webui/commands.rs`),而宿主 UDS 面几乎不注册这些方法
//! (`grep -oE '"(dsh|session)\.[a-zA-Z_]+"' core/crates/aeroric-host/src/*.rs` = 6)。
//! 也就是说「能力存在但产品拿不到」。本文件把它接上,并保证:
//!
//! 1. 只有**探测到**的能力才广告(`handshake` 的 `capabilities.agents.*`)与暴露;
//! 2. 未广告的能力即使被直接调用也**服务端拒绝**([`ControlError::Unsupported`]);
//! 3. **没有会话控制 adapter** 时,这组方法根本不注册——客户端拿到
//!    method-not-found,而不是一个会假装工作的空壳(与 `HostWiring::maintenance`
//!    同口径)。
//!
//! ## 这一层不做的事
//!
//! 不解析终端文本推断能力(契约在 `aeroric_core::adapters`);不在原生端 `eval`
//! 任何 workflow 脚本(journal 只在 core,[`WorkflowJournal`] 是纯逻辑)。

use std::sync::Arc;

use aeroric_core::adapters::{
    probe_agent_capabilities, CapabilityProbe, ControlError, SessionCapability,
    SessionControlRequest, SessionControls,
};
use aeroric_protocol::capability::{AgentCapabilities, Capability};
use aeroric_protocol::envelope::{Frame, Success};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use crate::dispatcher::{failure, ServiceSet};

/// 宿主探测到的适配器事实。宿主用真实运行时信息构造(方法表是否存在、
/// 上游上报的版本、是否具备结构化提示通道)。
#[derive(Clone, Debug)]
pub struct AdapterInjection {
    pub probe: CapabilityProbe,
    /// 队列权限:任务**入队那一刻**写进记录的 `permissionMode`。
    pub queued_permission_mode: aeroric_protocol::task::PermissionMode,
    /// 可达的适配器数量(0 = 只有内置 PTY 族)。
    pub reachable_adapters: usize,
}

impl Default for AdapterInjection {
    fn default() -> Self {
        Self {
            probe: CapabilityProbe::default(),
            queued_permission_mode: aeroric_protocol::task::PermissionMode::Ask,
            reachable_adapters: 0,
        }
    }
}

impl AdapterInjection {
    pub fn capabilities(&self) -> AgentCapabilities {
        probe_agent_capabilities(&self.probe)
    }
}

/// 由探测结果构造握手用能力清单。
///
/// **每一个 advertised 项都必须有证据**:`session-controls` 只在至少一个
/// advertised 控制/工具项存在时出现;否则它带 `note` 说明为什么没有。这直接落实
/// `capability.rs` 的注释——`v == 1` 从不蕴含能力。
pub fn adapter_capabilities(injection: &AdapterInjection) -> Vec<Capability> {
    let capabilities = injection.capabilities();
    let controls = [
        SessionCapability::Plan,
        SessionCapability::Compact,
        SessionCapability::Fork,
        SessionCapability::Revert,
        SessionCapability::Share,
    ];
    let advertised: Vec<&str> = controls
        .iter()
        .filter(|capability| capability.advertised(&capabilities))
        .map(|capability| capability.method())
        .collect();

    let work_graph = capabilities.native_prompt
        && injection.probe.has("subagents.list")
        && injection.probe.has("goals.get");
    let workflows = injection.probe.has("goals.get") && capabilities.native_prompt;
    let external_protocols = capabilities.attachments || capabilities.native_prompt;

    let mut list = Vec::new();
    list.push(Capability {
        name: "session-controls".to_string(),
        enabled: !advertised.is_empty(),
        since: Some(1),
        note: if advertised.is_empty() {
            Some(format!(
                "adapter {:?} advertises no session controls ({} methods probed)",
                injection.probe.adapter,
                injection.probe.supports.len()
            ))
        } else {
            Some(format!(
                "adapter {:?} {}: {}",
                injection.probe.adapter,
                injection.probe.version.as_deref().unwrap_or("version unknown"),
                advertised.join(", ")
            ))
        },
    });
    list.push(Capability {
        name: "agent-work-graph".to_string(),
        enabled: work_graph,
        since: Some(1),
        note: (!work_graph).then(|| {
            format!(
                "adapter {:?} has no structured subagent/goal surface",
                injection.probe.adapter
            )
        }),
    });
    list.push(Capability {
        name: "workflows".to_string(),
        enabled: workflows,
        since: Some(1),
        note: (!workflows)
            .then(|| "workflow journaling needs a structured agent surface".to_string()),
    });
    list.push(Capability {
        name: "external-protocols".to_string(),
        enabled: external_protocols,
        since: Some(1),
        note: (!external_protocols).then(|| "no structured attachment surface".to_string()),
    });
    list
}

/// 该注入是否具备注册会话控制面的条件。
pub fn registers_session_controls(injection: &AdapterInjection) -> bool {
    adapter_capabilities(injection)
        .iter()
        .any(|capability| capability.name == "session-controls" && capability.enabled)
}

fn parse_capability(value: &Value) -> Result<SessionCapability, String> {
    match value.as_str() {
        Some("plan") => Ok(SessionCapability::Plan),
        Some("compact") => Ok(SessionCapability::Compact),
        Some("fork") => Ok(SessionCapability::Fork),
        Some("revert") => Ok(SessionCapability::Revert),
        Some("share") => Ok(SessionCapability::Share),
        other => Err(format!("unknown session capability {other:?}")),
    }
}

fn parse_permission(value: &Value) -> Result<aeroric_protocol::task::PermissionMode, String> {
    serde_json::from_value(value.clone())
        .map_err(|_| format!("unknown permission mode {value:?}"))
}

fn parse_revert_target(value: &Value) -> Result<aeroric_core::adapters::session_controls::RevertTarget, String> {
    use aeroric_core::adapters::session_controls::RevertTarget;
    match value.get("target").and_then(Value::as_str) {
        Some("session") => Ok(RevertTarget::Session),
        Some("workspace") => Ok(RevertTarget::Workspace {
            affected_paths: value
                .get("affectedPaths")
                .and_then(Value::as_array)
                .map(|paths| {
                    paths
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        }),
        other => Err(format!("unknown revert target {other:?}")),
    }
}

fn parse_share_destination(
    value: &Value,
) -> Result<aeroric_core::adapters::session_controls::ShareDestination, String> {
    use aeroric_core::adapters::session_controls::ShareDestination;
    match value.get("target").and_then(Value::as_str) {
        Some("local") => Ok(ShareDestination::Local {
            path: value
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        Some("public") => Ok(ShareDestination::Public {
            endpoint: value
                .get("endpoint")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        other => Err(format!("unknown share destination {other:?}")),
    }
}

/// 解析请求并从 params 里挑出要参与脱敏导出的 payload。
fn build_request(params: &Value) -> Result<(SessionControlRequest, Value), String> {
    let capability = parse_capability(
        params
            .get("capability")
            .ok_or_else(|| "session.control requires a capability".to_string())?,
    )?;
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| "session.control requires a sessionId".to_string())?
        .to_string();

    let mut request = SessionControlRequest::new(capability, session_id);
    if let Some(mode) = params.get("currentPermissionMode") {
        request = request.current_permission(parse_permission(mode)?);
    }
    if let Some(mode) = params.get("requestedPermissionMode") {
        request = request.requested_permission(parse_permission(mode)?);
    }
    if let Some(enabled) = params.get("planEnabled").and_then(Value::as_bool) {
        request = request.plan_enabled(enabled);
    }
    if let Some(target) = params.get("revertTarget") {
        request = request.revert_target(parse_revert_target(target)?);
    }
    if let Some(confirmed) = params.get("previewConfirmed").and_then(Value::as_bool) {
        request = request.preview_confirmed(confirmed);
    }
    if let Some(destination) = params.get("shareDestination") {
        request = request.share_destination(parse_share_destination(destination)?);
    }
    if params.get("shareRedaction").and_then(Value::as_str) == Some("none") {
        request = request.share_redaction(aeroric_core::adapters::session_controls::ShareRedaction::None);
    }
    if let Some(confirmed) = params.get("confirmed").and_then(Value::as_bool) {
        request = request.confirmed(confirmed);
    }

    let payload = params.get("payload").cloned().unwrap_or(Value::Null);
    Ok((request, payload))
}

fn control_error(error: ControlError) -> (ErrorCode, String) {
    match &error {
        ControlError::Unsupported { .. } => (
            ErrorCode::UnsupportedCapability,
            format!("{error:?}"),
        ),
        ControlError::PermissionDrift { .. }
        | ControlError::PermissionEscalation { .. }
        | ControlError::PreviewRequired { .. }
        | ControlError::PublicShareNeedsEndpoint
        | ControlError::ConfirmationRequired { .. }
        | ControlError::MissingTarget { .. } => {
            (ErrorCode::InvalidArgument, format!("{error:?}"))
        }
    }
}

/// 注册会话控制面。意图:`session.control` 是**策略入口**,真正的 adapter 调用由外部
/// 运行时执行;授权通过后响应里带回已裁决的目标/脱敏导出,宿主据此执行。
pub fn register_session_control_services(set: &mut ServiceSet, injection: Arc<AdapterInjection>) {
    if !registers_session_controls(&injection) {
        return;
    }
    // 每个会话的能力探测在注册时冻结一次:方法表不会在宿主生命周期内变。
    let controls = SessionControls::new(
        injection.capabilities(),
        injection.queued_permission_mode,
    );
    set.register(
        "session.control",
        Arc::new(move |request, _context| {
            let controls = controls.clone();
            Box::pin(async move {
                let (control_request, payload) = match build_request(&request.params) {
                    Ok(parsed) => parsed,
                    Err(message) => {
                        return failure(request.id.clone(), ErrorCode::InvalidArgument, &message)
                    }
                };
                match controls.authorize(&control_request, &payload) {
                    Ok(response) => Frame::Success(Success {
                        v: 1,
                        id: request.id.clone(),
                        ok: true,
                        result: serde_json::to_value(response).unwrap_or(Value::Null),
                        extra: Default::default(),
                    }),
                    Err(error) => {
                        let (code, message) = control_error(error);
                        failure(request.id.clone(), code, &message)
                    }
                }
            })
        }),
    );
}

/// 注册适配器能力查询面(客户端不必重握手即可读探测结果)。
pub fn register_adapter_services(set: &mut ServiceSet, injection: Arc<AdapterInjection>) {
    let injection_for_capabilities = Arc::clone(&injection);
    set.register(
        "adapters.capabilities",
        Arc::new(move |request, _context| {
            let injection = Arc::clone(&injection_for_capabilities);
            Box::pin(async move {
                let payload = json!({
                    "adapter": injection.probe.adapter,
                    "version": injection.probe.version,
                    "reachableAdapters": injection.reachable_adapters,
                    "agent": injection.capabilities(),
                    "host": adapter_capabilities(&injection),
                });
                Frame::Success(Success {
                    v: 1,
                    id: request.id.clone(),
                    ok: true,
                    result: payload,
                    extra: Default::default(),
                })
            })
        }),
    );
}
