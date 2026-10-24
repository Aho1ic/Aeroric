//! 能力探测:把「适配器真实拥有的结构化事实」翻译成 `AgentCapabilities`。
//!
//! 探测只使用**已存在**的接口面:
//! - [`crate::agents::dsh_host::api::DshApiClient`] 的方法表(65 条 `dsh.*` 的客户端投影),
//!   以及其中真正与会话控制相关的子集;
//! - 适配器自报的版本字符串(`HostCapabilities::build_version` 的口径)。
//!
//! 探测**不**读终端、不读会话文件、不读 UI 状态。

use std::collections::BTreeMap;

use aeroric_protocol::capability::AgentCapabilities;
use serde::{Deserialize, Serialize};

/// 探测输入:调用方(宿主)收集的适配器事实。
///
/// `supports` 是**适配器显式声明**的方法名集合(例如 `session.fork`),不是从
/// 终端文本推断的字符串集合。`version` 供 `since`/`note` 记录出处。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityProbe {
    /// 适配器标识(如 `dsh`、`omp`、`claude-code`)。
    pub adapter: String,
    /// 适配器版本,未声明则 `None`(此时只按方法表判定)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 适配器显式声明支持的方法/能力名。
    #[serde(default)]
    pub supports: Vec<String>,
    /// 该族是否能接受结构化提示(`native_prompt`)。
    #[serde(default)]
    pub native_prompt: bool,
    /// 该族是否只有终端输入通道(`terminal_input`)。
    #[serde(default)]
    pub terminal_input: bool,
    /// 该族是否支持附件/图片输入。
    #[serde(default)]
    pub attachments: bool,
}

impl CapabilityProbe {
    pub fn new(adapter: impl Into<String>) -> Self {
        Self {
            adapter: adapter.into(),
            ..Self::default()
        }
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    pub fn supports(mut self, names: &[&str]) -> Self {
        self.supports = names.iter().map(|name| (*name).to_string()).collect();
        self
    }

    pub fn with(mut self, name: &str) -> Self {
        self.supports.push(name.to_string());
        self
    }

    pub fn native_prompt(mut self, value: bool) -> Self {
        self.native_prompt = value;
        self
    }

    pub fn terminal_input(mut self, value: bool) -> Self {
        self.terminal_input = value;
        self
    }

    pub fn attachments(mut self, value: bool) -> Self {
        self.attachments = value;
        self
    }

    /// 是否声明了某能力。`session.*` 的两种写法都认,避免调用点各写一套。
    pub fn has(&self, name: &str) -> bool {
        let bare = name.strip_prefix("session.").unwrap_or(name);
        self.supports
            .iter()
            .any(|supported| supported == name || supported == bare)
    }
}

/// 会话控制类能力的方法名。`false` 时 **宿主必须拒绝**,不能只靠 UI 隐藏。
pub const SESSION_CONTROL_METHODS: &[(&str, &str)] = &[
    ("plan", "session.plan"),
    ("compact", "session.compact"),
    ("fork", "session.fork"),
    ("revert", "session.revert"),
    ("share", "session.share"),
];

/// 把探测结果翻译成协议能力表。
///
/// 规则:
/// - 会话控制五项逐个按方法表判定;
/// - `permission_respond`/`questions_respond` 只有在 `native_prompt` 为真时才有意义
///   (PTY-only 族有终端,但不能因此被广告成能答结构化提问);这一条正是
///   `AgentCapabilities` 文档注释里点名的错误;
/// - `usage`/`thinking_levels` 只在适配器显式声明时给出,`None` != 空。
pub fn probe_agent_capabilities(probe: &CapabilityProbe) -> AgentCapabilities {
    let mut extra = BTreeMap::new();
    extra.insert(
        "adapter".to_string(),
        serde_json::Value::String(probe.adapter.clone()),
    );
    if let Some(version) = probe.version.as_deref() {
        extra.insert(
            "adapterVersion".to_string(),
            serde_json::Value::String(version.to_string()),
        );
    }
    // 探不到的项要能解释自己(「needs a Windows host」同款用法)。
    let mut unsupported: Vec<serde_json::Value> = Vec::new();
    for (_, method) in SESSION_CONTROL_METHODS {
        if !probe.has(method) {
            unsupported.push(serde_json::Value::String((*method).to_string()));
        }
    }
    if !unsupported.is_empty() {
        extra.insert(
            "unsupported".to_string(),
            serde_json::Value::Array(unsupported),
        );
    }

    let structured = probe.native_prompt;
    AgentCapabilities {
        native_prompt: structured,
        terminal_input: probe.terminal_input,
        permission_respond: structured && probe.has("permission.respond"),
        questions_respond: structured && probe.has("questions.respond"),
        plan_mode: probe.has("session.plan"),
        compact: probe.has("session.compact"),
        fork: probe.has("session.fork"),
        revert: probe.has("session.revert"),
        share: probe.has("session.share"),
        usage: probe.has("usage"),
        attachments: probe.attachments,
        thinking_levels: None,
        extra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_only_family_is_never_advertised_as_structured() {
        // 有终端 != 能答结构化提问(AgentCapabilities 文档注释点名的错误)。
        let probe = CapabilityProbe::new("pty-only")
            .terminal_input(true)
            .with("permission.respond");
        let caps = probe_agent_capabilities(&probe);
        assert!(caps.terminal_input);
        assert!(!caps.native_prompt);
        assert!(!caps.permission_respond);
        assert!(!caps.questions_respond);
    }

    #[test]
    fn only_declared_session_controls_are_advertised_and_rest_is_explained() {
        let probe = CapabilityProbe::new("dsh")
            .version("0.8.1")
            .native_prompt(true)
            .supports(&["session.plan", "session.fork"]);
        let caps = probe_agent_capabilities(&probe);
        assert!(caps.plan_mode);
        assert!(caps.fork);
        assert!(!caps.compact);
        assert!(!caps.revert);
        assert!(!caps.share);
        assert_eq!(caps.extra["adapter"], "dsh");
        assert_eq!(caps.extra["adapterVersion"], "0.8.1");
        let unsupported = caps.extra["unsupported"].as_array().unwrap();
        assert_eq!(unsupported.len(), 3);
        assert!(unsupported
            .iter()
            .any(|value| value == "session.compact"));
    }

    #[test]
    fn bare_names_and_prefixed_names_are_the_same_capability() {
        let probe = CapabilityProbe::new("dsh").supports(&["plan"]);
        assert!(probe_agent_capabilities(&probe).plan_mode);
        let probe = CapabilityProbe::new("dsh").supports(&["session.plan"]);
        assert!(probe_agent_capabilities(&probe).plan_mode);
    }

    #[test]
    fn unknown_thinking_levels_stay_unknown_instead_of_empty() {
        let caps = probe_agent_capabilities(&CapabilityProbe::new("dsh"));
        assert!(caps.thinking_levels.is_none());
        assert!(!caps.usage);
    }
}
