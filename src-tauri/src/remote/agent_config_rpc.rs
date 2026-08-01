//! Agent 配置的远程读写 RPC(手机端「Agent 配置」页)。
//!
//! ⚠️ 安全说明:本模块**刻意**放宽了 `tasks_rpc::agents_list` /
//! `agents_models` 的「绝不回传 base_url / api_key」约定 —— 这是用户明确要求的
//! 能力(手机上远程改 agent 接入配置)。凭据只在 E2EE 通道内传输,且只对已配对
//! 并通过 auth 的设备可见;手机端展示层默认对 API Key 掩码。
//!
//! 写入统一走 `crate::app_settings::save_custom_agent_profile`,由它负责
//! 校验、codex wrapper 脚本重写与 `atomic_write_private`,本层不自己写盘。
//! 内置 claude / codex 没有这些字段,只做只读列举。

use serde_json::{json, Value};

use super::rpc::str_param;
use crate::app_settings::CustomAgentProfile;

const MAX_ID_LEN: usize = 64;
const MAX_BASE_URL_LEN: usize = 512;
const MAX_API_KEY_LEN: usize = 512;
const MAX_MODELS: usize = 64;
const MAX_MODEL_LEN: usize = 128;

fn profile_view(profile: &CustomAgentProfile) -> Value {
    let label = if profile.label.trim().is_empty() {
        profile.id.clone()
    } else {
        profile.label.clone()
    };
    json!({
        "id": profile.id,
        "label": label,
        "codexLike": profile.codex_like,
        "editable": true,
        "baseUrl": profile.base_url,
        "apiKey": profile.api_key,
        "models": profile.models,
        "enable1mContext": profile.enable_1m_context,
        "enableChatCompletionsProxy": profile.enable_chat_completions_proxy,
    })
}

/// RPC `agentConfig.list`:内置 agent 只读 + 自定义 profile 全字段。
pub(crate) async fn agent_config_list() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let settings = crate::app_settings::load_settings_internal();
        let mut agents = vec![
            json!({ "id": "claude", "label": "Claude Code", "codexLike": false, "editable": false }),
            json!({ "id": "codex", "label": "Codex", "codexLike": true, "editable": false }),
        ];
        for profile in &settings.custom_agents {
            if profile.id.is_empty() {
                continue;
            }
            agents.push(profile_view(profile));
        }
        Ok(json!({ "agents": agents }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 参数里的 `models`:必须是字符串数组,逐项长度受限。缺省 = 不改。
fn models_param(params: &Value) -> Result<Option<Vec<String>>, String> {
    let Some(value) = params.get("models") else {
        return Ok(None);
    };
    let items = value.as_array().ok_or("Invalid models")?;
    if items.len() > MAX_MODELS {
        return Err("Too many models".to_string());
    }
    let mut models = Vec::with_capacity(items.len());
    for item in items {
        let model = item.as_str().ok_or("Invalid models")?.trim();
        if model.is_empty() {
            continue;
        }
        if model.len() > MAX_MODEL_LEN {
            return Err("Model name too long".to_string());
        }
        models.push(model.to_string());
    }
    Ok(Some(models))
}

fn text_param(params: &Value, key: &str, max_len: usize) -> Result<Option<String>, String> {
    let Some(value) = params.get(key) else {
        return Ok(None);
    };
    let text = value.as_str().ok_or_else(|| format!("Invalid {key}"))?;
    if text.len() > max_len {
        return Err(format!("{key} too long"));
    }
    Ok(Some(text.trim().to_string()))
}

/// RPC `agentConfig.save { id, baseUrl?, apiKey?, models? }`:合并进已存在的
/// 自定义 profile 后交给桌面端同一条保存路径。不新建、不删除 agent。
pub(crate) async fn agent_config_save(params: Value) -> Result<Value, String> {
    let id = str_param(&params, "id")?;
    if id.len() > MAX_ID_LEN {
        return Err("Invalid id".to_string());
    }
    let base_url = text_param(&params, "baseUrl", MAX_BASE_URL_LEN)?;
    let api_key = text_param(&params, "apiKey", MAX_API_KEY_LEN)?;
    let models = models_param(&params)?;

    let settings =
        tauri::async_runtime::spawn_blocking(crate::app_settings::load_settings_internal)
            .await
            .map_err(|e| e.to_string())?;
    let mut profile = settings
        .custom_agents
        .into_iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| format!("Agent not found: {id}"))?;

    if let Some(base_url) = base_url {
        profile.base_url = base_url;
    }
    if let Some(api_key) = api_key {
        profile.api_key = api_key;
    }
    if let Some(models) = models {
        profile.models = models;
    }

    crate::app_settings::save_custom_agent_profile(profile).await?;
    super::audit::log("agent-config-saved", json!({ "id": id }));
    Ok(json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_param_absent_means_no_change() {
        let params = json!({});
        assert_eq!(text_param(&params, "baseUrl", 16).unwrap(), None);
    }

    #[test]
    fn text_param_rejects_wrong_type_and_overlong() {
        let params = json!({ "baseUrl": 1 });
        assert!(text_param(&params, "baseUrl", 16).is_err());
        let params = json!({ "baseUrl": "0123456789abcdefg" });
        assert!(text_param(&params, "baseUrl", 16).is_err());
    }

    #[test]
    fn models_param_filters_blank_and_rejects_bad_shape() {
        let params = json!({ "models": ["a", "  ", " b "] });
        assert_eq!(
            models_param(&params).unwrap(),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert!(models_param(&json!({ "models": "a" })).is_err());
        assert!(models_param(&json!({ "models": [1] })).is_err());
        let too_many: Vec<String> = (0..MAX_MODELS + 1).map(|i| i.to_string()).collect();
        assert!(models_param(&json!({ "models": too_many })).is_err());
    }

    #[test]
    fn models_param_rejects_overlong_model_name() {
        let long = "m".repeat(MAX_MODEL_LEN + 1);
        assert!(models_param(&json!({ "models": [long] })).is_err());
    }
}
