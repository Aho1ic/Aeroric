//! Tauri command shells for app settings.
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use super::*;

#[tauri::command]
pub async fn update_builtin_agent_access(
    agent: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_builtin_agent_config_internal(
            agent,
            base_url,
            api_key,
            clear_api_key,
            models,
            None,
            proxy_enabled,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn update_dsh_reasoning_effort(
    agent: String,
    effort: String,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            apply_dsh_reasoning_effort_update(settings, &agent, &effort)
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn update_proxy_settings(proxy_settings: ProxySettings) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            settings.proxy_settings = proxy_settings;
            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 随手记 embedding provider 的配置。**不含 key** —— key 走
/// `notebook_embedding_key_set`(OS 钥匙串)。
#[tauri::command]
pub async fn update_notebook_embedding_settings(
    notebook_embedding_settings: NotebookEmbeddingSettings,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            settings.notebook_embedding_settings = notebook_embedding_settings;
            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// 夹紧不在这里做:`normalize_settings` 已经过一遍 `normalize_auto_cleanup_settings`,
/// 于是手改文件与走 UI 两条路得到同一份约束。
#[tauri::command]
pub async fn update_auto_cleanup_settings(
    auto_cleanup_settings: AutoCleanupSettings,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            settings.auto_cleanup_settings = auto_cleanup_settings;
            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn update_weekly_report_settings(
    weekly_report_settings: WeeklyReportSettings,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            settings.weekly_report_settings = weekly_report_settings;
            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn update_agent_path_settings(
    agent: String,
    executable_path: Option<String>,
    config_path: Option<String>,
    proxy_enabled: Option<bool>,
    builtin_credentials: Option<BuiltInAgentCredentials>,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked(move |settings| {
            match agent.as_str() {
                "claude" => {
                    if let Some(executable_path) = executable_path {
                        settings.claude_path = executable_path;
                    }
                    if let Some(config_path) = config_path {
                        settings.claude_config_path = config_path;
                    }
                }
                "claude_gpt55" => {
                    if let Some(executable_path) = executable_path {
                        settings.claude_gpt55_path = executable_path;
                    }
                    if let Some(config_path) = config_path {
                        settings.claude_gpt55_config_path = config_path;
                    }
                }
                "codex" => {
                    if let Some(executable_path) = executable_path {
                        settings.codex_path = executable_path;
                    }
                    if let Some(config_path) = config_path {
                        settings.codex_config_path = config_path;
                    }
                }
                "dsh" => {
                    if let Some(executable_path) = executable_path {
                        settings.dsh_path = executable_path;
                    }
                    if let Some(config_path) = config_path {
                        settings.dsh_config_path = config_path;
                    }
                }
                "omp" => {
                    if let Some(executable_path) = executable_path {
                        settings.omp_path = executable_path;
                    }
                    // omp 配置固定在托管 home(~/.aeroric/agent-homes/omp/config.yml),
                    // 不支持 config_path 覆盖。
                }
                _ => {
                    if let Some(executable_path) = executable_path {
                        let normalized_id = sanitize_custom_agent_id(&agent);
                        let profile = settings
                            .custom_agents
                            .iter_mut()
                            .find(|profile| profile.id == normalized_id)
                            .ok_or_else(|| "Custom Agent not found".to_string())?;
                        profile.path = executable_path;
                    }
                }
            }
            if let Some(credentials) = builtin_credentials {
                if matches!(agent.as_str(), "claude" | "claude_gpt55" | "codex" | "dsh") {
                    settings
                        .builtin_agent_credentials
                        .insert(agent.clone(), credentials);
                }
            }
            if let Some(enabled) = proxy_enabled {
                set_agent_proxy_enabled(settings, &agent, enabled);
            }
            Ok(())
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

pub fn get_agent_launch_spec(agent: &str) -> AgentLaunchSpec {
    get_agent_launch_spec_from_settings(&load_settings_internal(), agent)
}

/// codex 是否真正可用：实际执行 `codex --version` 成功才算（走全局带缓存的探测，
/// 与 `hooks::usable_for` 同源）。不能用 launch spec 的 `program` 是否非空来判断——
/// 路径解析在二进制缺失时会回退成裸名 `"codex"`，导致永远非空、永远误判为已安装。
/// 注意：只验证二进制能否运行，不验证登录状态，未登录的 codex 调用仍会在运行时失败。
pub fn codex_available() -> bool {
    detect_codex_version().is_some()
}

#[tauri::command]
pub async fn load_app_settings() -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(load_settings_internal)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_app_settings(settings: AppSettings) -> Result<(), String> {
    {
        let _guard = settings_lock().lock();
        persist_settings_unlocked(settings)?;
    }
    clear_cached_versions();
    Ok(())
}

#[tauri::command]
pub async fn export_agent_config_bundle(
    agent: String,
    output_path: String,
    config_content: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let output_path = validate_agent_config_bundle_path(&output_path, false)?;
        let settings = load_settings_internal();
        let bundle_agent = collect_agent_config_bundle_agent(&settings, &agent, config_content)?;
        let bundle = AgentConfigBundle {
            format: AGENT_CONFIG_BUNDLE_FORMAT.to_string(),
            version: AGENT_CONFIG_BUNDLE_VERSION,
            exported_at: chrono::Utc::now().to_rfc3339(),
            agent: bundle_agent,
        };
        let raw = serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())?;
        if raw.len() as u64 > MAX_AGENT_CONFIG_BUNDLE_BYTES {
            return Err("Agent configuration bundle is too large".to_string());
        }
        atomic_write_private(&output_path, &raw)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn export_all_agent_config_bundle(
    output_path: String,
) -> Result<AllAgentConfigExportResult, String> {
    tokio::task::spawn_blocking(move || {
        let output_path = validate_all_agent_config_bundle_path(&output_path, false)?;
        let settings = load_settings_internal();
        let mut agent_ids = vec![
            "claude".to_string(),
            "claude_gpt55".to_string(),
            "codex".to_string(),
            "dsh".to_string(),
            "omp".to_string(),
        ];
        agent_ids.extend(
            settings
                .custom_agents
                .iter()
                .map(|profile| profile.id.clone()),
        );
        let agents = agent_ids
            .iter()
            .map(|agent| collect_portable_agent_config_bundle_agent(&settings, agent))
            .collect::<Result<Vec<_>, _>>()?;
        let exported_agent_ids = agents.iter().map(|agent| agent.id.clone()).collect();
        let bundle = AllAgentConfigBundle {
            format: ALL_AGENT_CONFIG_BUNDLE_FORMAT.to_string(),
            version: ALL_AGENT_CONFIG_BUNDLE_VERSION,
            exported_at: chrono::Utc::now().to_rfc3339(),
            agents,
        };
        let raw = serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())?;
        if raw.len() as u64 > MAX_ALL_AGENT_CONFIG_BUNDLE_BYTES {
            return Err("All-Agent configuration bundle is too large".to_string());
        }
        atomic_write_private(&output_path, &raw)?;
        Ok(AllAgentConfigExportResult { exported_agent_ids })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_agent_config_bundle(
    input_path: String,
) -> Result<AgentConfigImportResult, String> {
    tokio::task::spawn_blocking(move || {
        let input_path = validate_agent_config_bundle_path(&input_path, true)?;
        let metadata = fs::metadata(&input_path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_AGENT_CONFIG_BUNDLE_BYTES {
            return Err("Agent configuration bundle is too large".to_string());
        }
        let raw = fs::read_to_string(&input_path).map_err(|error| error.to_string())?;
        let bundle = parse_agent_config_bundle(&raw)?;
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let settings_file = settings_path()?;
        let mut imported = import_agent_config_entries_transaction(
            &mut settings,
            vec![bundle.agent],
            &settings_file,
        )?;
        clear_cached_versions();
        cache_settings(&settings_file, &settings);
        imported
            .pop()
            .ok_or_else(|| "Agent configuration bundle is empty".to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_all_agent_config_bundle(
    input_path: String,
) -> Result<AllAgentConfigImportResult, String> {
    tokio::task::spawn_blocking(move || {
        let input_path = validate_all_agent_config_bundle_path(&input_path, true)?;
        let metadata = fs::metadata(&input_path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_ALL_AGENT_CONFIG_BUNDLE_BYTES {
            return Err("All-Agent configuration bundle is too large".to_string());
        }
        let raw = fs::read_to_string(&input_path).map_err(|error| error.to_string())?;
        let bundle = parse_all_agent_config_bundle(&raw)?;
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let settings_file = settings_path()?;
        let imported =
            import_agent_config_entries_transaction(&mut settings, bundle.agents, &settings_file)?;
        clear_cached_versions();
        cache_settings(&settings_file, &settings);
        Ok(AllAgentConfigImportResult {
            imported_agent_ids: imported.into_iter().map(|result| result.agent_id).collect(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_cc_switch_config(
    input_path: String,
) -> Result<AllAgentConfigImportResult, String> {
    tokio::task::spawn_blocking(move || {
        let input_path = validate_cc_switch_config_path(&input_path)?;
        let raw = fs::read_to_string(&input_path).map_err(|e| e.to_string())?;
        if !raw.contains("-- CC Switch") && !raw.contains("providers") {
            return Err("Not a valid CC Switch export file".to_string());
        }
        let agents = parse_cc_switch_providers(&raw)?;
        if agents.is_empty() {
            return Err("No provider configurations found in CC Switch export".to_string());
        }
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let settings_file = settings_path()?;
        let imported =
            import_agent_config_entries_transaction(&mut settings, agents, &settings_file)?;
        clear_cached_versions();
        cache_settings(&settings_file, &settings);
        Ok(AllAgentConfigImportResult {
            imported_agent_ids: imported.into_iter().map(|result| result.agent_id).collect(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn save_agent_paths(
    claude_path: String,
    claude_gpt55_path: String,
    codex_path: String,
) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        settings.claude_path = claude_path;
        settings.claude_gpt55_path = claude_gpt55_path;
        settings.codex_path = codex_path;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn save_custom_agent_profile(profile: CustomAgentProfile) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_settings_locked_with_agent_files(move |settings| {
            upsert_custom_agent_profile_unlocked(settings, profile)
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn update_custom_agent_access(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    enable_chat_completions_proxy: Option<bool>,
) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        update_custom_agent_config_internal(
            id,
            base_url,
            api_key,
            clear_api_key,
            None,
            None,
            None,
            enable_chat_completions_proxy,
            None,
            None,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn setup_agent_profile(draft: AgentSetupDraft) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        update_settings_locked_with_agent_files(move |settings| {
            validate_agent_setup_draft(&draft)?;
            let mut draft = draft;
            let id = allocate_setup_agent_id(&draft.id, &draft.kind, settings)?;
            draft.id = id.clone();
            let is_dsh = matches!(draft.kind, AgentSetupKind::Dsh);
            let is_omp = matches!(draft.kind, AgentSetupKind::Omp);
            let models = normalize_setup_models(&draft);
            let (profile_path, config_lang, family, dsh_home, file_paths) = if is_dsh {
                // dsh-like 档案不生成 wrapper 脚本:直接运行 dsh 二进制,隔离 home 与
                // API key 由启动层按档案注入(DSH_HOME / DEEPSEEK_API_KEY env)。
                let program = {
                    let detected = crate::platform::detect_path("dsh");
                    if detected.is_empty() {
                        "dsh".to_string()
                    } else {
                        detected
                    }
                };
                let home = crate::dsh_home::dsh_home_for(&id)?;
                let file_paths = vec![
                    home.join("settings.yaml"),
                    home.join(".credentials.yaml"),
                    home.join("cordis.patch.yml"),
                    crate::dsh_home::managed_patch_path_in(&home),
                ];
                (
                    program,
                    "yaml".to_string(),
                    "dsh".to_string(),
                    Some(home),
                    file_paths,
                )
            } else if is_omp {
                // omp-like 档案同理:直接运行 omp 二进制,托管 home
                // (PI_CODING_AGENT_DIR = agent-homes/{id})在下方事务里初始化,
                // 自定义 provider 写进 home 的 models.yml。
                let program = {
                    let detected = crate::platform::detect_path("omp");
                    if detected.is_empty() {
                        "omp".to_string()
                    } else {
                        detected
                    }
                };
                let home = crate::omp_home::omp_home_for(&id)?;
                (
                    program,
                    "yaml".to_string(),
                    "omp".to_string(),
                    None,
                    vec![home.join("config.yml"), home.join("models.yml")],
                )
            } else {
                let script_path = default_agent_script_path(&id)?;
                let sidecar = agent_api_key_path(&id)?;
                (
                    script_path.to_string_lossy().into_owned(),
                    "shellscript".to_string(),
                    String::new(),
                    None,
                    vec![script_path, sidecar],
                )
            };
            let profile = normalize_custom_agent_profile(CustomAgentProfile {
                id: id.clone(),
                label: draft.label.trim().to_string(),
                path: profile_path,
                codex_like: matches!(draft.kind, AgentSetupKind::Codex),
                family,
                config_lang,
                base_url: normalize_base_url(&draft.base_url),
                api_key: draft.api_key.trim().to_string(),
                models,
                enable_1m_context: draft.enable_1m_context,
                disable_artifact_tool: draft.disable_artifact_tool,
                enable_chat_completions_proxy: draft.enable_chat_completions_proxy,
                bridge_python_path: draft.bridge_python_path.trim().to_string(),
                username: String::new(),
                password: String::new(),
            })
            .ok_or_else(|| "Invalid custom agent profile".to_string())?;

            AgentFileTransaction::capture_and_apply(file_paths, || {
                if let Some(home) = dsh_home.as_deref() {
                    crate::dsh_home::ensure_dsh_home_at(home)?;
                    crate::dsh_home::sync_dsh_credentials(home, Some(draft.api_key.trim()))?;
                    let base_url = normalize_base_url(&draft.base_url);
                    if !base_url.is_empty() {
                        crate::dsh_home::write_custom_provider_settings(
                            home,
                            &base_url,
                            &profile.models,
                            &draft.dsh_api_protocol,
                        )?;
                    }
                } else if is_omp {
                    let home = crate::omp_home::omp_home_for(&id)?;
                    crate::omp_home::ensure_omp_home_for(&id)?;
                    crate::omp_home::write_custom_provider_models_yml(
                        &home,
                        &normalize_base_url(&draft.base_url),
                        draft.api_key.trim(),
                        &if draft.dsh_api_protocol.is_empty() {
                            "openai-completions".to_string()
                        } else {
                            draft.dsh_api_protocol
                        },
                        &profile.models,
                    )?;
                } else {
                    let script = build_agent_script(&draft);
                    write_agent_script(&id, &script, &draft.api_key)?;
                }
                settings
                    .agent_proxy_enabled
                    .insert(id.clone(), draft.proxy_enabled);
                settings.custom_agents.push(profile.clone());
                Ok(())
            })
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn detect_agent_models(
    kind: AgentSetupKind,
    base_url: String,
    api_key: String,
) -> Result<AgentModels, String> {
    detect_agent_models_with_policy(kind, base_url, api_key, ModelDetectionPolicy::LocalUser).await
}

pub(crate) async fn detect_agent_models_for_remote(
    kind: AgentSetupKind,
    base_url: String,
    api_key: String,
) -> Result<AgentModels, String> {
    detect_agent_models_with_policy(kind, base_url, api_key, ModelDetectionPolicy::PairedDevice)
        .await
}

#[tauri::command]
pub async fn list_agent_models(agent: String) -> Result<AgentModels, String> {
    tokio::task::spawn_blocking(move || {
        let settings = load_settings_internal();
        let (reasoning_effort, reasoning_speed) =
            crate::config::read_agent_reasoning_settings_from_settings(&agent, &settings);
        if let Some(profile) = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == agent)
        {
            let models = normalize_model_list(profile.models.clone());
            if !models.is_empty() {
                return Ok(AgentModels {
                    models,
                    balance: None,
                    reasoning_effort,
                    reasoning_speed,
                });
            }
        }

        if let Some(credentials) = settings.builtin_agent_credentials.get(&agent) {
            let models = normalize_model_list(credentials.models.clone());
            if !models.is_empty() {
                return Ok(AgentModels {
                    models,
                    balance: None,
                    reasoning_effort,
                    reasoning_speed,
                });
            }
        }

        if agent == "claude" {
            return Ok(AgentModels {
                models: list_builtin_claude_models(),
                balance: None,
                reasoning_effort,
                reasoning_speed,
            });
        }

        if agent == "dsh" {
            // 内建 DeepSeek 官方目录(dsh-llm-deepseek);自定义 provider 的
            // 模型在配置面板通过 /models 探测后存入 builtin_agent_credentials。
            return Ok(AgentModels {
                models: list_builtin_dsh_models(),
                balance: None,
                reasoning_effort,
                reasoning_speed,
            });
        }

        if is_dsh_agent(&agent) {
            // dsh-like 自定义档案未探测/保存模型时回落内建 DeepSeek 目录。
            return Ok(AgentModels {
                models: list_builtin_dsh_models(),
                balance: None,
                reasoning_effort,
                reasoning_speed,
            });
        }

        if agent == "omp" {
            // omp 无 CLI 模型列表;起一个 --no-session 的 rpc-ui 进程经
            // `get_available_models` 拉取,再归并为 `provider/model-id`。
            return Ok(AgentModels {
                models: list_builtin_omp_models()?,
                balance: None,
                reasoning_effort,
                reasoning_speed,
            });
        }

        if !is_codex_like_agent(&agent) {
            return Ok(AgentModels {
                models: Vec::new(),
                balance: None,
                reasoning_effort,
                reasoning_speed,
            });
        }

        let launch = get_agent_launch_spec(&agent);
        let mut cmd = Command::new(&launch.program);
        crate::subprocess::configure_background_command(&mut cmd);
        cmd.args(&launch.args)
            .arg("debug")
            .arg("models")
            .env("PATH", get_login_shell_path())
            .stdin(Stdio::null())
            .stderr(Stdio::piped());
        for (key, value) in &launch.extra_env {
            cmd.env(key, value);
        }

        let output = cmd.output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("Model list failed with status {}", output.status)
            } else {
                stderr
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(AgentModels {
            models: parse_codex_model_catalog(&stdout)?,
            balance: None,
            reasoning_effort,
            reasoning_speed,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn update_custom_agent_models(
    id: String,
    models: Vec<String>,
) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        update_custom_agent_config_internal(
            id,
            None,
            None,
            false,
            Some(models),
            None,
            None,
            None,
            None,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn probe_chat_bridge_python(
    bridge_python_path: Option<String>,
) -> Result<ChatBridgePythonStatus, String> {
    let requested = bridge_python_path.unwrap_or_default().trim().to_string();
    tokio::task::spawn_blocking(move || {
        if requested.is_empty() {
            return match resolve_chat_bridge_python() {
                Ok(probe) => ChatBridgePythonStatus {
                    usable: true,
                    program: probe.program,
                    version: probe.version.unwrap_or_default(),
                    configured: false,
                    failure: String::new(),
                    checked: Vec::new(),
                },
                Err(failures) => ChatBridgePythonStatus {
                    usable: false,
                    program: String::new(),
                    version: String::new(),
                    configured: false,
                    failure: String::new(),
                    checked: failures
                        .into_iter()
                        .map(|probe| {
                            format!(
                                "{} -> {}",
                                probe.program,
                                probe.failure.unwrap_or_else(|| "unknown".to_string())
                            )
                        })
                        .collect(),
                },
            };
        }
        let probe = probe_chat_bridge_python_program(&requested);
        ChatBridgePythonStatus {
            usable: probe.is_usable(),
            program: if probe.is_usable() {
                probe.program.clone()
            } else {
                String::new()
            },
            version: probe.version.clone().unwrap_or_default(),
            configured: true,
            failure: probe.failure.clone().unwrap_or_default(),
            checked: Vec::new(),
        }
    })
    .await
    .map_err(|error| error.to_string())
}

/// 开关 bridge,并可同时改解释器路径。
///
/// `bridge_python_path` 为 `None` 表示"这次不动解释器设置",`Some("")` 表示显式清空
/// 回自动探测——两者语义不同,不能合并成一个空串。
#[tauri::command]
pub async fn update_custom_agent_chat_completions_proxy(
    id: String,
    enabled: bool,
    bridge_python_path: Option<String>,
) -> Result<AppSettings, String> {
    let bridge_python_path = bridge_python_path.map(|path| path.trim().to_string());
    let normalized = tokio::task::spawn_blocking(move || {
        update_custom_agent_config_internal(
            id,
            None,
            None,
            false,
            None,
            None,
            None,
            Some(enabled),
            bridge_python_path,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn update_custom_agent_context(
    id: String,
    enable_1m_context: bool,
) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        update_custom_agent_config_internal(
            id,
            None,
            None,
            false,
            None,
            Some(enable_1m_context),
            None,
            None,
            None,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

/// 开关「禁用 Artifact 工具」。仅 Claude 族可用,写进 profile 后 wrapper 会被重刷,
/// 从而带上/去掉 `CLAUDE_CODE_DISABLE_ARTIFACT`。
#[tauri::command]
pub async fn update_custom_agent_artifact_tool(
    id: String,
    disable_artifact_tool: bool,
) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        update_custom_agent_config_internal(
            id,
            None,
            None,
            false,
            None,
            None,
            Some(disable_artifact_tool),
            None,
            None,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn delete_custom_agent_profile(id: String) -> Result<AppSettings, String> {
    let normalized = tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let normalized_id = sanitize_custom_agent_id(&id);
        let removed_profile = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == normalized_id)
            .cloned();
        if let Some(profile) = removed_profile.as_ref() {
            let generated = profile_uses_aeroric_generated_wrapper(profile);
            // 只删 Aeroric 自己生成的 wrapper 脚本。dsh / omp 档案不生成 wrapper,
            // 它们的 `path` 是 detect_path() 探到的用户自己装的二进制
            // (`/opt/homebrew/bin/omp` 之类),而 profile_uses_aeroric_generated_wrapper
            // 要求 config_lang == "shellscript",这两族都是 "yaml" ——
            // 无条件调用会把用户的 omp/dsh 可执行文件本体删掉,连带拆掉内置档案
            // 和其余同族档案。
            if generated {
                remove_agent_profile_file(&profile.path)?;
            }
            remove_agent_api_key(&normalized_id)?;
            // 凭据不只在 agent-credentials sidecar 里:dsh 的 .credentials.yaml 与
            // omp 的 models.yml(内含明文 apiKey)都躺在隔离 home 中,删档案必须一起清,
            // 否则"删除"之后密钥仍留在磁盘上。
            if generated || matches!(profile.agent_family(), AgentFamily::Dsh | AgentFamily::Omp) {
                remove_exact_generated_agent_home(&normalized_id)?;
            }
        }
        settings
            .custom_agents
            .retain(|profile| profile.id != normalized_id);
        settings.agent_label_overrides.remove(&normalized_id);
        settings.builtin_agent_credentials.remove(&normalized_id);
        settings.dsh_reasoning_efforts.remove(&normalized_id);
        settings.agent_proxy_enabled.remove(&normalized_id);
        settings.agent_proxy_overrides.remove(&normalized_id);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())??;
    clear_cached_versions();
    Ok(normalized)
}

#[tauri::command]
pub async fn rename_custom_agent_profile(id: String, label: String) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        let normalized_id = sanitize_custom_agent_id(&id);
        let next_label = label.trim().to_string();
        if normalized_id.is_empty() || next_label.is_empty() {
            return Err("Invalid custom agent name".to_string());
        }

        let Some(profile) = settings
            .custom_agents
            .iter_mut()
            .find(|profile| profile.id == normalized_id)
        else {
            return Err("Custom agent not found".to_string());
        };
        profile.label = next_label;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_send_shortcut(send_shortcut: String) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        settings.send_shortcut = normalize_send_shortcut(send_shortcut);

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_shift_enter_newline(enabled: bool) -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = settings_lock().lock();
        let mut settings = load_settings_unlocked();
        settings.terminal_shift_enter_newline = enabled;

        let dir = aeroric_dir()?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = settings_path()?;
        let normalized = normalize_settings(settings);
        let raw = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
        atomic_write_private(&path, &raw)?;
        Ok::<AppSettings, String>(normalized)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn detect_agent_paths() -> Result<AppSettings, String> {
    tokio::task::spawn_blocking(|| {
        let mut settings = load_settings_internal();
        settings.claude_path = detect_path("claude");
        settings.claude_gpt55_path = default_claude_gpt55_path();
        settings.codex_path = detect_path("codex");

        for agent in ["claude", "codex"] {
            let config_path = match agent {
                "claude" => {
                    if settings.claude_config_path.trim().is_empty() {
                        default_builtin_agent_config_path(agent)?
                    } else {
                        PathBuf::from(normalize_config_path(settings.claude_config_path.clone()))
                    }
                }
                "codex" => {
                    if settings.codex_config_path.trim().is_empty() {
                        default_builtin_agent_config_path(agent)?
                    } else {
                        PathBuf::from(normalize_config_path(settings.codex_config_path.clone()))
                    }
                }
                _ => return Err(format!("unexpected built-in agent: {agent}")),
            };
            let config_content = fs::read_to_string(&config_path).unwrap_or_default();
            let credentials =
                detect_builtin_agent_credentials(&settings, agent, &config_path, &config_content);
            let config_path_string = config_path.to_string_lossy().into_owned();
            match agent {
                "claude" => settings.claude_config_path = config_path_string,
                "codex" => settings.codex_config_path = config_path_string,
                _ => return Err(format!("unexpected built-in agent: {agent}")),
            }
            if !credentials.base_url.is_empty()
                || !credentials.api_key.is_empty()
                || !credentials.models.is_empty()
            {
                settings
                    .builtin_agent_credentials
                    .insert(agent.to_string(), credentials);
            }
        }
        Ok(normalize_settings(settings))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn detect_agent_versions_for_settings(
    settings: AppSettings,
) -> Result<AgentVersions, String> {
    tokio::task::spawn_blocking(move || detect_versions_for_settings(&settings))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn detect_agent_version(agent: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        detect_version(&get_agent_launch_spec(&agent)).unwrap_or_default()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upgrade_agent_versions(
    app: tauri::AppHandle,
    webui: tauri::State<'_, crate::dsh_webui::DshWebUiManager>,
    agents: Vec<String>,
    expected_versions: Option<HashMap<String, String>>,
) -> Result<Vec<AgentUpgradeResult>, String> {
    // 前端允许多个 Agent 同时显示升级中；包管理器本身仍需串行，避免
    // 两个 Homebrew/npm 进程互相抢锁或覆盖同一份全局安装状态。
    let _upgrade_guard = agent_upgrade_lock().lock().await;
    let settings = load_settings_internal();
    let mut requested = Vec::new();
    for agent in agents {
        if requested.contains(&agent) {
            continue;
        }
        if upgrade_kind_for_agent(&settings, &agent).is_none() {
            return Err(format!("Unknown agent: {}", agent));
        }
        requested.push(agent);
    }
    if requested.is_empty() {
        return Err("Select at least one agent to upgrade".to_string());
    }

    let expected_versions = expected_versions.unwrap_or_default();
    let mut expected_by_kind = HashMap::<AgentUpgradeKind, String>::new();
    for agent in &requested {
        let kind = upgrade_kind_for_agent(&settings, agent)
            .ok_or_else(|| format!("Unknown agent: {agent}"))?;
        let binary_agent = upgrade_binary_agent(kind);
        let expected = expected_versions
            .get(agent)
            .or_else(|| expected_versions.get(binary_agent))
            .map(|version| version.trim())
            .filter(|version| !version.is_empty());
        let Some(expected) = expected else {
            continue;
        };
        if let Some(existing) = expected_by_kind.get(&kind) {
            if existing != expected {
                return Err(format!(
                    "Conflicting expected versions for {binary_agent}: {existing} and {expected}"
                ));
            }
        } else {
            expected_by_kind.insert(kind, expected.to_string());
        }
    }

    type UpgradeOutcome = (
        String,
        String,
        Vec<AgentUpgradeChannel>,
        Option<crate::dsh_webui::DshRuntimeRecovery>,
    );
    let mut outcomes: HashMap<AgentUpgradeKind, UpgradeOutcome> = HashMap::new();
    for agent in &requested {
        let kind = upgrade_kind_for_agent(&settings, agent)
            .ok_or_else(|| format!("Unknown agent: {}", agent))?;
        if outcomes.contains_key(&kind) {
            continue;
        }
        let binary_agent = upgrade_binary_agent(kind);
        let launch = get_agent_launch_spec_from_settings(&settings, binary_agent);
        let configured_program = get_agent_configured_path(&settings, binary_agent);
        let active_program = agent_upgrade_detection_program(&configured_program, &launch);
        let suspended = if kind == AgentUpgradeKind::Dsh {
            match webui.suspend_for_upgrade(binary_agent).await {
                Ok(suspended) => Some(suspended),
                Err(error) => {
                    let launch = launch.clone();
                    let version = tokio::task::spawn_blocking(move || {
                        detect_version(&launch).unwrap_or_default()
                    })
                    .await
                    .map_err(|join_error| join_error.to_string())?;
                    outcomes.insert(
                        kind,
                        (
                            version.clone(),
                            version,
                            vec![AgentUpgradeChannel {
                                channel: "runtime-recovery".to_string(),
                                success: false,
                                message: error.clone(),
                            }],
                            Some(crate::dsh_webui::DshRuntimeRecovery {
                                errors: vec![error],
                                ..crate::dsh_webui::DshRuntimeRecovery::default()
                            }),
                        ),
                    );
                    continue;
                }
            }
        } else {
            None
        };
        let launch_for_upgrade = launch.clone();
        let upgrade_program = active_program.clone();
        let target_version = expected_by_kind.get(&kind).cloned();
        let upgrade_task = tokio::task::spawn_blocking(move || {
            let previous_version = detect_version(&launch_for_upgrade).unwrap_or_default();
            let channels = match build_agent_upgrade_commands(
                kind,
                &upgrade_program,
                target_version.as_deref(),
            ) {
                Ok(commands) => run_agent_upgrades(&commands),
                Err(error) => vec![AgentUpgradeChannel {
                    channel: "detection".to_string(),
                    success: false,
                    message: error,
                }],
            };
            clear_cached_versions();
            let current_version = detect_version(&launch_for_upgrade).unwrap_or_default();
            (previous_version, current_version, channels)
        })
        .await;
        let (previous_version, current_version, mut channels) = match upgrade_task {
            Ok(outcome) => outcome,
            Err(error) => (
                String::new(),
                String::new(),
                vec![AgentUpgradeChannel {
                    channel: "internal".to_string(),
                    success: false,
                    message: format!("The Agent upgrade worker failed: {error}"),
                }],
            ),
        };
        append_upgrade_verification(
            &mut channels,
            &active_program,
            &previous_version,
            &current_version,
            expected_by_kind.get(&kind).map(String::as_str),
        );
        let runtime_recovery = if let Some(suspended) = suspended {
            let was_running = suspended.was_running();
            let recovery = webui.resume_after_upgrade(&app, suspended).await;
            if was_running {
                let success = recovery.errors.is_empty() && recovery.restarted;
                channels.push(AgentUpgradeChannel {
                    channel: "runtime-recovery".to_string(),
                    success,
                    message: if success {
                        format!(
                            "restarted; reconnected {} session(s); cancelled {} running turn(s)",
                            recovery.reconnected_sessions, recovery.cancelled_turns
                        )
                    } else {
                        recovery.errors.join("\n")
                    },
                });
            }
            Some(recovery)
        } else {
            None
        };
        outcomes.insert(
            kind,
            (
                previous_version,
                current_version,
                channels,
                runtime_recovery,
            ),
        );
    }

    clear_cached_versions();
    Ok(requested
        .into_iter()
        .filter_map(|agent| {
            let kind = upgrade_kind_for_agent(&settings, &agent)?;
            let (previous_version, current_version, channels, runtime_recovery) =
                outcomes.get(&kind)?;
            let success = channels.iter().all(|ch| ch.success);
            let message = channels
                .iter()
                .map(|ch| format!("{}: {}", ch.channel, ch.message))
                .collect::<Vec<_>>()
                .join("\n");
            Some(AgentUpgradeResult {
                agent,
                success,
                previous_version: previous_version.clone(),
                current_version: current_version.clone(),
                message,
                channels: channels.clone(),
                channel: channels
                    .iter()
                    .map(|channel| channel.channel.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                managed: false,
                runtime_recovery: runtime_recovery.clone(),
            })
        })
        .collect())
}

static SYSTEM_FONTS: OnceLock<Vec<String>> = OnceLock::new();

#[tauri::command]
pub async fn get_system_fonts() -> Vec<String> {
    tokio::task::spawn_blocking(|| {
        SYSTEM_FONTS
            .get_or_init(|| {
                let source = font_kit::source::SystemSource::new();
                match source.all_families() {
                    Ok(mut families) => {
                        families.sort();
                        families
                    }
                    Err(_) => Vec::new(),
                }
            })
            .clone()
    })
    .await
    .unwrap_or_default()
}
