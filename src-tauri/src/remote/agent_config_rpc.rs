//! Agent 配置的远程读写 RPC(手机端「Agent 配置」页)。
//!
//! ⚠️ 安全说明:本模块**刻意**放宽了 `tasks_rpc::agents_list` /
//! `agents_models` 的「绝不回传 base_url / api_key」约定 —— 这是用户明确要求的
//! 能力(手机上远程改 agent 接入配置)。凭据只在 E2EE 通道内传输,且只对已配对
//! 并通过 auth 的设备可见;手机端展示层默认对 API Key 掩码。
//!
//! 自定义 profile 写入统一走
//! `crate::app_settings::save_custom_agent_profile`,由它负责校验、codex
//! wrapper 脚本重写与 `atomic_write_private`;内置 agent 则通过统一设置保存链路
//! 更新凭据、模型和代理开关。

use serde_json::{json, Value};

use super::rpc::str_param;
use crate::app_settings::{
    AgentSetupDraft, AgentSetupKind, BuiltInAgentCredentials, CustomAgentProfile,
};

const MAX_ID_LEN: usize = 64;
const MAX_BASE_URL_LEN: usize = 512;
const MAX_API_KEY_LEN: usize = 512;
const MAX_MODELS: usize = 64;
const MAX_MODEL_LEN: usize = 128;

fn profile_view(profile: &CustomAgentProfile, proxy_enabled: bool) -> Value {
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
        "proxyEnabled": proxy_enabled,
    })
}

/// RPC `agentConfig.list`:内置和自定义 profile 都回传可编辑的连接字段。
pub(crate) async fn agent_config_list() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let settings = crate::app_settings::load_settings_internal();
        let builtin = |id: &str, label: &str, codex_like: bool| {
            let credentials = settings
                .builtin_agent_credentials
                .get(id)
                .cloned()
                .unwrap_or_default();
            json!({
                "id": id,
                "label": label,
                "codexLike": codex_like,
                "editable": true,
                "baseUrl": credentials.base_url,
                "apiKey": credentials.api_key,
                "models": credentials.models,
                "enable1mContext": credentials.enable_1m_context,
                "proxyEnabled": settings.agent_proxy_enabled.get(id).copied().unwrap_or(false),
            })
        };
        let mut agents = vec![
            builtin("claude", "Claude Code", false),
            builtin("codex", "Codex", true),
        ];
        for profile in &settings.custom_agents {
            if profile.id.is_empty() {
                continue;
            }
            agents.push(profile_view(
                profile,
                settings
                    .agent_proxy_enabled
                    .get(&profile.id)
                    .copied()
                    .unwrap_or(false),
            ));
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

fn bool_param(params: &Value, key: &str) -> Result<Option<bool>, String> {
    let Some(value) = params.get(key) else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| format!("Invalid {key}"))
}

fn kind_param(params: &Value) -> Result<AgentSetupKind, String> {
    serde_json::from_value(
        params
            .get("kind")
            .cloned()
            .ok_or_else(|| "Missing param: kind".to_string())?,
    )
    .map_err(|_| "Invalid kind".to_string())
}

fn setup_id(label: &str, requested: Option<String>, kind: &AgentSetupKind) -> String {
    let source = requested
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| label.to_string());
    let suffix = if matches!(kind, AgentSetupKind::Codex) {
        "codex"
    } else {
        "claude"
    };
    let mut id = source
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    id = id.trim_matches(['_', '-']).to_string();
    if id.is_empty() {
        id = "agent".to_string();
    }
    format!("{id}_{suffix}")
}

/// RPC `agentConfig.save`:更新内置或自定义 agent 的接入字段、代理和模型列表。
pub(crate) async fn agent_config_save(params: Value) -> Result<Value, String> {
    let id = str_param(&params, "id")?;
    if id.len() > MAX_ID_LEN {
        return Err("Invalid id".to_string());
    }
    let base_url = text_param(&params, "baseUrl", MAX_BASE_URL_LEN)?;
    let api_key = text_param(&params, "apiKey", MAX_API_KEY_LEN)?;
    let models = models_param(&params)?;
    let enable_1m_context = bool_param(&params, "enable1mContext")?;
    let enable_chat_completions_proxy = bool_param(&params, "enableChatCompletionsProxy")?;
    let proxy_enabled = bool_param(&params, "proxyEnabled")?;

    if matches!(id.as_str(), "claude" | "codex") {
        let mut settings =
            tauri::async_runtime::spawn_blocking(crate::app_settings::load_settings_internal)
                .await
                .map_err(|e| e.to_string())?;
        let mut credentials = settings
            .builtin_agent_credentials
            .get(&id)
            .cloned()
            .unwrap_or_else(BuiltInAgentCredentials::default);
        if let Some(value) = base_url {
            credentials.base_url = value;
        }
        if let Some(value) = api_key {
            credentials.api_key = value;
        }
        if let Some(value) = models {
            credentials.models = value;
        }
        if let Some(value) = enable_1m_context {
            credentials.enable_1m_context = value;
        }
        settings
            .builtin_agent_credentials
            .insert(id.clone(), credentials);
        if let Some(enabled) = proxy_enabled {
            if enabled {
                settings.agent_proxy_enabled.insert(id.clone(), true);
            } else {
                settings.agent_proxy_enabled.remove(&id);
            }
        }
        tauri::async_runtime::spawn_blocking(move || {
            crate::app_settings::save_app_settings(settings)
        })
        .await
        .map_err(|e| e.to_string())??;
        super::audit::log("agent-config-saved", json!({ "id": id }));
        return Ok(json!({ "ok": true }));
    }

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
    if profile.models.is_empty() {
        return Err("At least one model is required".to_string());
    }
    if let Some(value) = enable_1m_context {
        profile.enable_1m_context = value;
    }
    if let Some(value) = enable_chat_completions_proxy {
        profile.enable_chat_completions_proxy = value;
    }

    let mut saved_settings = crate::app_settings::save_custom_agent_profile(profile).await?;
    if let Some(enabled) = proxy_enabled {
        if enabled {
            saved_settings.agent_proxy_enabled.insert(id.clone(), true);
        } else {
            saved_settings.agent_proxy_enabled.remove(&id);
        }
        tauri::async_runtime::spawn_blocking(move || {
            crate::app_settings::save_app_settings(saved_settings)
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    super::audit::log("agent-config-saved", json!({ "id": id }));
    Ok(json!({ "ok": true }))
}

pub(crate) async fn agent_config_detect_models(params: Value) -> Result<Value, String> {
    let kind = kind_param(&params)?;
    let base_url = text_param(&params, "baseUrl", MAX_BASE_URL_LEN)?.unwrap_or_default();
    let api_key = text_param(&params, "apiKey", MAX_API_KEY_LEN)?.unwrap_or_default();
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Err("Base URL and API key are required".to_string());
    }
    let models =
        crate::app_settings::detect_agent_models_for_remote(kind, base_url, api_key).await?;
    Ok(json!({ "models": models.models, "balance": models.balance }))
}

pub(crate) async fn agent_config_create(params: Value) -> Result<Value, String> {
    let label = str_param(&params, "label")?;
    if label.len() > MAX_ID_LEN {
        return Err("Invalid label".to_string());
    }
    let kind = kind_param(&params)?;
    let base_url = text_param(&params, "baseUrl", MAX_BASE_URL_LEN)?.unwrap_or_default();
    let api_key = text_param(&params, "apiKey", MAX_API_KEY_LEN)?.unwrap_or_default();
    let models = models_param(&params)?.unwrap_or_default();
    if models.is_empty() {
        return Err("At least one model is required".to_string());
    }
    let draft = AgentSetupDraft {
        id: setup_id(&label, text_param(&params, "id", MAX_ID_LEN)?, &kind),
        label,
        kind: kind.clone(),
        base_url,
        api_key,
        model: models[0].clone(),
        models,
        enable_1m_context: bool_param(&params, "enable1mContext")?.unwrap_or(false),
        enable_chat_completions_proxy: bool_param(&params, "enableChatCompletionsProxy")?
            .unwrap_or(false),
        proxy_enabled: bool_param(&params, "proxyEnabled")?.unwrap_or(false),
    };
    let settings = crate::app_settings::setup_agent_profile(draft).await?;
    let id = settings
        .custom_agents
        .last()
        .map(|profile| profile.id.clone())
        .unwrap_or_default();
    super::audit::log("agent-config-created", json!({ "id": id }));
    Ok(json!({ "ok": true, "id": id }))
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
