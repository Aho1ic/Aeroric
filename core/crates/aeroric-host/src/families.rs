//! CLI host 的 `AgentFamilies`（b1.2）：数据源＝`storage::load_settings`，
//! 判定＝core `agents::families`（b1.0）——**与宿主 `SettingsFamilies` 同一份
//! 判定、不同注入点**，不留第二份真相。
//!
//! 每次判定现读 settings（CLI host 的调用频度是 per-launch，不是 webview 的
//! per-render；缓存是宿主全局器的职责，这里没有可失效的缓存可守）。

use std::sync::Arc;

use aeroric_core::agents::families::{
    built_in_native_resume_family, built_in_session_family, custom_session_family, is_builtin,
    launch_family, unknown_session_family, FamilyProfile,
};
use aeroric_core::context::CoreContext;
use aeroric_core::tasks::{AgentFamilies, ProtocolFamily};
use serde_json::Value;

pub struct SettingsFamiliesCli {
    context: Arc<CoreContext>,
}

impl SettingsFamiliesCli {
    pub fn new(context: Arc<CoreContext>) -> Self {
        Self { context }
    }

    fn profile(&self, agent: &str) -> Option<(String, bool)> {
        let fields = aeroric_core::storage::load_settings(&self.context)
            .ok()
            .and_then(|outcome| outcome.found())?;
        let profile = fields
            .get("custom_agents")
            .and_then(Value::as_array)?
            .iter()
            .find(|candidate| candidate["id"].as_str() == Some(agent))?
            .clone();
        Some((
            profile["family"].as_str().unwrap_or_default().to_string(),
            profile["codex_like"].as_bool().unwrap_or(false),
        ))
    }
}

impl AgentFamilies for SettingsFamiliesCli {
    fn launch_family(&self, agent: &str) -> ProtocolFamily {
        match self.profile(agent) {
            Some((family, codex_like)) => launch_family(FamilyProfile {
                family: &family,
                codex_like,
            }),
            None => {
                // 内建与未知都走名字：内建表给准确答案，unknown 落
                // claude/codex 的镜像回退（与宿主同一条链，b1.0）。
                match built_in_session_family(agent) {
                    Some(ProtocolFamily::Dsh) => ProtocolFamily::Dsh,
                    Some(ProtocolFamily::Omp) => ProtocolFamily::Omp,
                    Some(ProtocolFamily::Codex) => ProtocolFamily::Codex,
                    _ => launch_family(FamilyProfile {
                        family: "",
                        codex_like: agent != "claude",
                    }),
                }
            }
        }
    }

    fn session_family(&self, agent: &str) -> ProtocolFamily {
        if let Some(built) = built_in_session_family(agent) {
            return built;
        }
        match self.profile(agent) {
            Some((family, codex_like)) => custom_session_family(FamilyProfile {
                family: &family,
                codex_like,
            }),
            None => unknown_session_family(agent),
        }
    }

    fn is_builtin(&self, agent: &str) -> bool {
        is_builtin(agent)
    }

    fn native_resume_family(&self, agent: &str) -> ProtocolFamily {
        built_in_native_resume_family(agent).unwrap_or_else(|| self.session_family(agent))
    }
}
