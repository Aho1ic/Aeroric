use super::*;

pub(super) fn validate_agent_config_bundle_path(
    path: &str,
    must_exist: bool,
) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_absolute() {
        return Err("Agent configuration bundle path must be absolute".to_string());
    }
    let file_name = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !file_name.ends_with(".aeroric-agent.json") {
        return Err("Agent configuration bundle must end with .aeroric-agent.json".to_string());
    }
    if must_exist {
        if !candidate.is_file() {
            return Err("Agent configuration bundle does not exist".to_string());
        }
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "Agent configuration bundle has no parent directory".to_string())?;
        if !parent.is_dir() {
            return Err("Agent configuration bundle parent directory does not exist".to_string());
        }
    }
    Ok(candidate)
}

pub(super) fn validate_all_agent_config_bundle_path(
    path: &str,
    must_exist: bool,
) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_absolute() {
        return Err("All-Agent configuration bundle path must be absolute".to_string());
    }
    let file_name = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !file_name.ends_with(".aeroric-agents.json") {
        return Err(
            "All-Agent configuration bundle must end with .aeroric-agents.json".to_string(),
        );
    }
    if must_exist {
        if !candidate.is_file() {
            return Err("All-Agent configuration bundle does not exist".to_string());
        }
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "All-Agent configuration bundle has no parent directory".to_string())?;
        if !parent.is_dir() {
            return Err(
                "All-Agent configuration bundle parent directory does not exist".to_string(),
            );
        }
    }
    Ok(candidate)
}

pub(super) fn validate_cc_switch_config_path(path: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_absolute() {
        return Err("CC Switch configuration path must be absolute".to_string());
    }
    if !candidate.is_file() {
        return Err("CC Switch configuration file does not exist".to_string());
    }
    if !candidate
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
    {
        return Err("CC Switch configuration file must use the .sql extension".to_string());
    }
    let metadata = fs::metadata(&candidate).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_ALL_AGENT_CONFIG_BUNDLE_BYTES {
        return Err("CC Switch configuration file is too large".to_string());
    }
    Ok(candidate)
}

pub(super) fn builtin_agent_details(agent: &str) -> Option<(&'static str, &'static str, bool)> {
    match agent {
        "claude" => Some(("Claude Code", "json", false)),
        "claude_gpt55" => Some(("Claude GPT-5.5", "shellscript", false)),
        "codex" => Some(("Codex", "toml", true)),
        "dsh" => Some(("DeepSeek Harness", "yaml", false)),
        _ => None,
    }
}

pub(super) fn validate_agent_config_bundle_agent(
    agent: &AgentConfigBundleAgent,
) -> Result<(), String> {
    if agent.id.trim().is_empty() || agent.label.trim().is_empty() {
        return Err("Agent configuration is missing an ID or name".to_string());
    }
    if !matches!(
        agent.config_lang.as_str(),
        "json" | "toml" | "yaml" | "shellscript"
    ) {
        return Err("Unsupported agent configuration language".to_string());
    }
    match agent.kind {
        AgentConfigBundleKind::BuiltIn => {
            let Some((_, expected_lang, expected_codex_like)) = builtin_agent_details(&agent.id)
            else {
                return Err("Unknown built-in agent in configuration bundle".to_string());
            };
            if agent.config_lang != expected_lang || agent.codex_like != expected_codex_like {
                return Err("Built-in agent configuration metadata does not match".to_string());
            }
        }
        AgentConfigBundleKind::Custom => {
            if sanitize_custom_agent_id(&agent.id).is_empty() {
                return Err("Invalid custom agent ID".to_string());
            }
        }
    }
    Ok(())
}

pub(super) fn parse_agent_config_bundle(raw: &str) -> Result<AgentConfigBundle, String> {
    let bundle: AgentConfigBundle = serde_json::from_str(raw)
        .map_err(|error| format!("Invalid agent configuration: {error}"))?;
    if bundle.format != AGENT_CONFIG_BUNDLE_FORMAT {
        return Err("Unsupported agent configuration format".to_string());
    }
    if bundle.version != AGENT_CONFIG_BUNDLE_VERSION {
        return Err(format!(
            "Unsupported agent configuration version: {}",
            bundle.version
        ));
    }
    validate_agent_config_bundle_agent(&bundle.agent)?;
    Ok(bundle)
}

pub(super) fn parse_all_agent_config_bundle(raw: &str) -> Result<AllAgentConfigBundle, String> {
    let bundle: AllAgentConfigBundle = serde_json::from_str(raw)
        .map_err(|error| format!("Invalid All-Agent configuration: {error}"))?;
    if bundle.format != ALL_AGENT_CONFIG_BUNDLE_FORMAT {
        return Err("Unsupported All-Agent configuration format".to_string());
    }
    if bundle.version != ALL_AGENT_CONFIG_BUNDLE_VERSION {
        return Err(format!(
            "Unsupported All-Agent configuration version: {}",
            bundle.version
        ));
    }
    if bundle.agents.is_empty() {
        return Err("All-Agent configuration bundle is empty".to_string());
    }
    let mut ids = HashSet::new();
    for agent in &bundle.agents {
        validate_agent_config_bundle_agent(agent)?;
        let normalized_id = match agent.kind {
            AgentConfigBundleKind::BuiltIn => agent.id.clone(),
            AgentConfigBundleKind::Custom => sanitize_custom_agent_id(&agent.id),
        };
        if !ids.insert(normalized_id.clone()) {
            return Err(format!(
                "All-Agent configuration contains a duplicate Agent ID: {normalized_id}"
            ));
        }
    }
    Ok(bundle)
}

pub(super) fn collect_agent_config_bundle_agent(
    settings: &AppSettings,
    agent: &str,
    config_content: Option<String>,
) -> Result<AgentConfigBundleAgent, String> {
    if let Some((default_label, config_lang, codex_like)) = builtin_agent_details(agent) {
        let configured_path = match agent {
            "claude" => settings.claude_config_path.clone(),
            "claude_gpt55" => settings.claude_gpt55_config_path.clone(),
            "codex" => settings.codex_config_path.clone(),
            "dsh" => settings.dsh_config_path.clone(),
            _ => String::new(),
        };
        let path = if configured_path.trim().is_empty() {
            default_builtin_agent_config_path(agent)?
        } else {
            PathBuf::from(normalize_config_path(configured_path))
        };
        let config_present = config_content.is_some() || path.is_file();
        let config_content = match config_content {
            Some(content) => content,
            None if path.is_file() => {
                fs::read_to_string(&path).map_err(|error| error.to_string())?
            }
            None => String::new(),
        };
        let credentials = detect_builtin_agent_credentials(settings, agent, &path, &config_content);
        return Ok(AgentConfigBundleAgent {
            id: agent.to_string(),
            label: settings
                .agent_label_overrides
                .get(agent)
                .cloned()
                .unwrap_or_else(|| default_label.to_string()),
            kind: AgentConfigBundleKind::BuiltIn,
            codex_like,
            family: String::new(),
            config_lang: config_lang.to_string(),
            config_content,
            config_present,
            base_url: credentials.base_url,
            api_key: credentials.api_key,
            models: credentials.models,
            enable_1m_context: credentials.enable_1m_context,
            enable_chat_completions_proxy: false,
        });
    }

    let profile = settings
        .custom_agents
        .iter()
        .find(|profile| profile.id == agent)
        .ok_or_else(|| format!("Unknown agent: {agent}"))?;
    let path = PathBuf::from(normalize_config_path(profile.path.clone()));
    let config_present = config_content.is_some() || path.is_file();
    let config_content = match config_content {
        Some(content) => content,
        None if path.is_file() => fs::read_to_string(&path).map_err(|error| error.to_string())?,
        None => String::new(),
    };
    Ok(AgentConfigBundleAgent {
        id: profile.id.clone(),
        label: profile.label.clone(),
        kind: AgentConfigBundleKind::Custom,
        codex_like: profile.codex_like,
        family: profile.family.clone(),
        config_lang: profile.config_lang.clone(),
        config_content,
        config_present,
        base_url: profile.base_url.clone(),
        api_key: profile.api_key.clone(),
        models: profile.models.clone(),
        enable_1m_context: profile.enable_1m_context,
        enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
    })
}

pub(super) fn collect_portable_agent_config_bundle_agent(
    settings: &AppSettings,
    agent: &str,
) -> Result<AgentConfigBundleAgent, String> {
    let mut bundle_agent = collect_agent_config_bundle_agent(settings, agent, None)?;
    if matches!(bundle_agent.kind, AgentConfigBundleKind::Custom) {
        bundle_agent.config_content.clear();
        bundle_agent.config_present = false;
        if bundle_agent.base_url.trim().is_empty()
            || bundle_agent.api_key.trim().is_empty()
            || normalize_model_list(bundle_agent.models.clone()).is_empty()
        {
            return Err(format!(
                "Custom Agent {} is missing Base URL, API Key, or model settings",
                bundle_agent.id
            ));
        }
        bundle_agent.models = normalize_model_list(bundle_agent.models);
        bundle_agent.config_present = true;
    }
    Ok(bundle_agent)
}

pub(super) fn custom_agent_setup_draft(agent: &AgentConfigBundleAgent) -> Option<AgentSetupDraft> {
    if agent.base_url.trim().is_empty()
        || agent.api_key.trim().is_empty()
        || agent.models.is_empty()
    {
        return None;
    }
    Some(AgentSetupDraft {
        id: agent.id.clone(),
        label: agent.label.clone(),
        kind: if agent.codex_like {
            AgentSetupKind::Codex
        } else {
            AgentSetupKind::ClaudeCode
        },
        base_url: agent.base_url.clone(),
        api_key: agent.api_key.clone(),
        model: agent.models[0].clone(),
        models: agent.models.clone(),
        enable_1m_context: agent.enable_1m_context,
        enable_chat_completions_proxy: agent.enable_chat_completions_proxy,
        dsh_api_protocol: String::new(),
        proxy_enabled: false,
    })
}

pub(super) fn import_agent_config_entry(
    settings: &mut AppSettings,
    agent: AgentConfigBundleAgent,
) -> Result<AgentConfigImportResult, String> {
    validate_agent_config_bundle_agent(&agent)?;
    let mut imported_agent_id = agent.id.clone();
    let config_path = match agent.kind {
        AgentConfigBundleKind::BuiltIn => {
            let configured_path = match agent.id.as_str() {
                "claude" => &mut settings.claude_config_path,
                "claude_gpt55" => &mut settings.claude_gpt55_config_path,
                "codex" => &mut settings.codex_config_path,
                _ => unreachable!(),
            };
            let path = if configured_path.trim().is_empty() {
                let default_path = default_builtin_agent_config_path(&agent.id)?;
                if agent.config_present {
                    *configured_path = default_path.to_string_lossy().into_owned();
                }
                default_path
            } else {
                PathBuf::from(normalize_config_path(configured_path.clone()))
            };
            if agent.config_present {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                if agent.config_lang == "shellscript" {
                    write_agent_script_at_path(&path, &agent.config_content)?;
                } else {
                    atomic_write_private(&path, &agent.config_content)?;
                }
            }
            settings
                .agent_label_overrides
                .insert(agent.id.clone(), agent.label.trim().to_string());
            let mut imported_credentials =
                parse_builtin_agent_credentials(&agent.id, &agent.config_content);
            if !agent.base_url.trim().is_empty() {
                imported_credentials.base_url = normalize_base_url(&agent.base_url);
            }
            if !agent.api_key.trim().is_empty() {
                imported_credentials.api_key = agent.api_key.trim().to_string();
            }
            if !agent.models.is_empty() {
                imported_credentials.models = normalize_model_list(agent.models.clone());
            }
            imported_credentials.enable_1m_context |= agent.enable_1m_context;
            if agent.id == "codex" && !imported_credentials.api_key.is_empty() {
                write_codex_auth_api_key(&path, &imported_credentials.api_key)?;
            }
            if !imported_credentials.base_url.is_empty()
                || !imported_credentials.api_key.is_empty()
                || !imported_credentials.models.is_empty()
            {
                settings
                    .builtin_agent_credentials
                    .insert(agent.id.clone(), imported_credentials);
            }
            path
        }
        AgentConfigBundleKind::Custom => {
            let id = sanitize_custom_agent_id(&agent.id);
            imported_agent_id = id.clone();
            let (path, config_lang) = if let Some(draft) = custom_agent_setup_draft(&agent) {
                let id = validate_agent_setup_draft(&draft)?;
                let script = build_agent_script(&draft);
                (
                    write_agent_script(&id, &script, &draft.api_key)?,
                    "shellscript".to_string(),
                )
            } else {
                let extension = match agent.config_lang.as_str() {
                    "json" => "json",
                    "toml" => "toml",
                    _ => "sh",
                };
                let path = agent_scripts_dir()?.join(format!("{id}.{extension}"));
                fs::create_dir_all(agent_scripts_dir()?).map_err(|error| error.to_string())?;
                if agent.config_present {
                    if agent.config_lang == "shellscript" {
                        write_agent_script_at_path(&path, &agent.config_content)?;
                    } else {
                        atomic_write_private(&path, &agent.config_content)?;
                    }
                }
                (path, agent.config_lang.clone())
            };
            let profile = normalize_custom_agent_profile(CustomAgentProfile {
                id: id.clone(),
                label: agent.label,
                path: path.to_string_lossy().into_owned(),
                codex_like: agent.codex_like,
                family: agent.family,
                config_lang,
                base_url: agent.base_url,
                api_key: agent.api_key,
                models: agent.models,
                enable_1m_context: agent.enable_1m_context,
                enable_chat_completions_proxy: agent.enable_chat_completions_proxy,
                username: String::new(),
                password: String::new(),
            })
            .ok_or_else(|| "Invalid custom agent configuration".to_string())?;
            settings
                .custom_agents
                .retain(|existing| existing.id != profile.id);
            settings.custom_agents.push(profile);
            path
        }
    };
    Ok(AgentConfigImportResult {
        agent_id: imported_agent_id,
        config_path: config_path.to_string_lossy().into_owned(),
    })
}

#[derive(Clone, Debug)]
enum FileSnapshot {
    Missing,
    File {
        content: Vec<u8>,
        permissions: fs::Permissions,
    },
    Symlink(PathBuf),
}

fn capture_file_snapshot(path: &Path) -> Result<FileSnapshot, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileSnapshot::Missing)
        }
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(FileSnapshot::Symlink(
            fs::read_link(path).map_err(|error| error.to_string())?,
        ));
    }
    if !metadata.is_file() {
        return Err(format!(
            "Refusing to import over a non-file path: {}",
            path.display()
        ));
    }
    Ok(FileSnapshot::File {
        content: fs::read(path).map_err(|error| error.to_string())?,
        permissions: metadata.permissions(),
    })
}

fn remove_snapshot_target(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => {
            fs::remove_file(path).map_err(|error| error.to_string())
        }
        Ok(_) => Err(format!(
            "Refusing to remove a non-file rollback target: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn restore_file_snapshot(path: &Path, snapshot: &FileSnapshot) -> Result<(), String> {
    match snapshot {
        FileSnapshot::Missing => remove_snapshot_target(path),
        FileSnapshot::File {
            content,
            permissions,
        } => {
            remove_snapshot_target(path)?;
            fs::write(path, content).map_err(|error| error.to_string())?;
            fs::set_permissions(path, permissions.clone()).map_err(|error| error.to_string())
        }
        FileSnapshot::Symlink(target) => {
            remove_snapshot_target(path)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, path).map_err(|error| error.to_string())?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(target, path).map_err(|error| error.to_string())?;
            Ok(())
        }
    }
}

fn import_agent_paths(
    settings: &AppSettings,
    agent: &AgentConfigBundleAgent,
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    match agent.kind {
        AgentConfigBundleKind::BuiltIn => {
            let configured_path = match agent.id.as_str() {
                "claude" => &settings.claude_config_path,
                "claude_gpt55" => &settings.claude_gpt55_config_path,
                "codex" => &settings.codex_config_path,
                _ => unreachable!(),
            };
            let path = if configured_path.trim().is_empty() {
                default_builtin_agent_config_path(&agent.id)?
            } else {
                PathBuf::from(normalize_config_path(configured_path.clone()))
            };
            paths.push(path.clone());
            if agent.id == "codex" {
                if let Some(auth_path) = codex_auth_path(&path) {
                    paths.push(auth_path);
                }
            }
        }
        AgentConfigBundleKind::Custom => {
            let id = sanitize_custom_agent_id(&agent.id);
            if let Some(draft) = custom_agent_setup_draft(agent) {
                validate_agent_setup_draft(&draft)?;
                paths.push(default_agent_script_path(&id)?);
                paths.push(agent_api_key_path(&id)?);
            } else {
                let extension = match agent.config_lang.as_str() {
                    "json" => "json",
                    "toml" => "toml",
                    _ => "sh",
                };
                paths.push(agent_scripts_dir()?.join(format!("{id}.{extension}")));
            }
        }
    }
    Ok(paths)
}

pub(super) fn import_agent_config_entries_transaction(
    settings: &mut AppSettings,
    agents: Vec<AgentConfigBundleAgent>,
    settings_file: &Path,
) -> Result<Vec<AgentConfigImportResult>, String> {
    let original_settings = settings.clone();
    for agent in &agents {
        validate_agent_config_bundle_agent(agent)?;
        let _ = import_agent_paths(settings, agent)?;
    }

    let mut paths = vec![settings_file.to_path_buf()];
    for agent in &agents {
        paths.extend(import_agent_paths(settings, agent)?);
    }
    paths.sort();
    paths.dedup();
    if let Some(parent) = settings_file.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let snapshots = paths
        .iter()
        .map(|path| capture_file_snapshot(path).map(|snapshot| (path.clone(), snapshot)))
        .collect::<Result<Vec<_>, _>>()?;

    let result = (|| {
        let mut imported = Vec::with_capacity(agents.len());
        for agent in agents {
            imported.push(import_agent_config_entry(settings, agent)?);
        }
        let normalized = normalize_settings(settings.clone());
        let raw = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
        atomic_write_private(settings_file, &raw)?;
        *settings = normalized;
        Ok(imported)
    })();

    match result {
        Ok(imported) => Ok(imported),
        Err(error) => {
            *settings = original_settings;
            let rollback_errors = snapshots
                .iter()
                .rev()
                .filter_map(|(path, snapshot)| {
                    restore_file_snapshot(path, snapshot)
                        .err()
                        .map(|rollback| format!("{}: {rollback}", path.display()))
                })
                .collect::<Vec<_>>();
            if rollback_errors.is_empty() {
                Err(error)
            } else {
                Err(format!(
                    "{error}; rollback failed: {}",
                    rollback_errors.join("; ")
                ))
            }
        }
    }
}
pub(super) fn parse_cc_switch_providers(sql: &str) -> Result<Vec<AgentConfigBundleAgent>, String> {
    let mut agents = Vec::new();
    for line in sql.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("INSERT INTO \"providers\"")
            && !trimmed.starts_with("INSERT INTO providers")
        {
            continue;
        }
        // Extract column names from INSERT INTO "providers" ("col1", "col2", ...) VALUES (...)
        let cols_start = match trimmed.find('(') {
            Some(pos) => pos + 1,
            None => continue,
        };
        let cols_end = match trimmed[cols_start..].find(')') {
            Some(pos) => cols_start + pos,
            None => continue,
        };
        let cols_str = &trimmed[cols_start..cols_end];
        let columns: Vec<&str> = cols_str
            .split(',')
            .map(|c| c.trim().trim_matches('"').trim_matches('\''))
            .collect();

        // Find VALUES (...) part
        let values_marker = match trimmed[cols_end..].find("VALUES") {
            Some(pos) => cols_end + pos + 6,
            None => match trimmed[cols_end..].find("values") {
                Some(pos) => cols_end + pos + 6,
                None => continue,
            },
        };
        let values_str = trimmed[values_marker..].trim();
        let values_str = values_str
            .trim_start_matches('(')
            .trim_end_matches(';')
            .trim_end_matches(')');
        let values = split_sql_values(values_str);

        if values.len() != columns.len() {
            continue;
        }

        let get_col = |name: &str| -> String {
            columns
                .iter()
                .position(|c| *c == name)
                .and_then(|idx| values.get(idx))
                .map(|v| unescape_sql_string(v))
                .unwrap_or_default()
        };

        let app_type = get_col("app_type");
        let name = get_col("name");
        let settings_config = get_col("settings_config");
        let meta_str = get_col("meta");

        if name.is_empty() || settings_config.is_empty() {
            continue;
        }

        let is_codex = app_type == "codex";
        let is_claude = app_type == "claude";
        if !is_codex && !is_claude {
            continue;
        }

        let config: serde_json::Value = match serde_json::from_str(&settings_config) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let meta: serde_json::Value = serde_json::from_str(&meta_str).unwrap_or_default();

        let (base_url, api_key, models) = if is_claude {
            let env = config.get("env").and_then(|v| v.as_object());
            let base_url = env
                .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let api_key_field = meta
                .get("apiKeyField")
                .and_then(|v| v.as_str())
                .unwrap_or("ANTHROPIC_AUTH_TOKEN");
            let api_key = env
                .and_then(|e| {
                    e.get(api_key_field)
                        .or_else(|| e.get("ANTHROPIC_AUTH_TOKEN"))
                        .or_else(|| e.get("ANTHROPIC_API_KEY"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut model_list = Vec::new();
            for key in &[
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
            ] {
                if let Some(m) = env.and_then(|e| e.get(*key)).and_then(|v| v.as_str()) {
                    let m = m.to_string();
                    if !m.is_empty() && !model_list.contains(&m) {
                        model_list.push(m);
                    }
                }
            }
            (base_url, api_key, model_list)
        } else {
            // codex
            let api_key = config
                .get("auth")
                .and_then(|a| a.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let base_url = config
                .get("env")
                .and_then(|e| e.as_object())
                .and_then(|e| {
                    e.get("OPENAI_BASE_URL")
                        .or_else(|| e.get("OPENAI_API_BASE"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let config_toml = config.get("config").and_then(|v| v.as_str()).unwrap_or("");
            let mut model_list = Vec::new();
            for toml_line in config_toml.lines() {
                let toml_line = toml_line.trim();
                if !toml_line.starts_with("model") {
                    continue;
                }
                if let Some((_, raw_value)) = toml_line.split_once('=') {
                    let val = raw_value.trim().trim_matches('"');
                    if !val.is_empty() && !model_list.contains(&val.to_string()) {
                        model_list.push(val.to_string());
                    }
                }
            }
            (base_url, api_key, model_list)
        };

        if api_key.is_empty() && base_url.is_empty() {
            continue;
        }

        let agent_id = sanitize_custom_agent_id(&format!(
            "ccswitch_{}_{}",
            name.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .take(30)
                .collect::<String>(),
            if is_codex { "codex" } else { "claude" }
        ));

        agents.push(AgentConfigBundleAgent {
            id: agent_id,
            label: name,
            kind: AgentConfigBundleKind::Custom,
            codex_like: is_codex,
            family: String::new(),
            config_lang: "shellscript".to_string(),
            config_content: String::new(),
            config_present: true,
            base_url,
            api_key,
            models,
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
        });
    }
    Ok(agents)
}

pub(super) fn split_sql_values(input: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            if ch == '\'' {
                if i + 1 < chars.len() && chars[i + 1] == '\'' {
                    current.push('\'');
                    i += 2;
                    continue;
                }
                in_string = false;
            } else {
                current.push(ch);
            }
        } else {
            match ch {
                '\'' => {
                    in_string = true;
                }
                ',' => {
                    values.push(current.trim().to_string());
                    current = String::new();
                    i += 1;
                    continue;
                }
                _ => {
                    current.push(ch);
                }
            }
        }
        i += 1;
    }
    values.push(current.trim().to_string());
    values
}

pub(super) fn unescape_sql_string(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed == "NULL" || trimmed.is_empty() {
        return String::new();
    }
    trimmed.replace("''", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built_in_import(
        id: &str,
        config_lang: &str,
        codex_like: bool,
        config_content: &str,
    ) -> AgentConfigBundleAgent {
        AgentConfigBundleAgent {
            id: id.to_string(),
            label: id.to_string(),
            kind: AgentConfigBundleKind::BuiltIn,
            codex_like,
            family: String::new(),
            config_lang: config_lang.to_string(),
            config_content: config_content.to_string(),
            config_present: true,
            base_url: String::new(),
            api_key: String::new(),
            models: Vec::new(),
            enable_1m_context: false,
            enable_chat_completions_proxy: false,
        }
    }

    #[test]
    fn parses_versioned_agent_configuration_bundle() {
        let raw = serde_json::json!({
            "format": AGENT_CONFIG_BUNDLE_FORMAT,
            "version": AGENT_CONFIG_BUNDLE_VERSION,
            "exported_at": "2026-07-17T00:00:00Z",
            "agent": {
                "id": "codex",
                "label": "Codex",
                "kind": "built_in",
                "codex_like": true,
                "config_lang": "toml",
                "config_content": "model = \"gpt-5\""
            }
        })
        .to_string();

        let bundle = parse_agent_config_bundle(&raw).unwrap();
        assert_eq!(bundle.agent.id, "codex");
        assert_eq!(bundle.agent.config_lang, "toml");
    }

    #[test]
    fn rejects_unknown_agent_configuration_bundle_versions() {
        let raw = serde_json::json!({
            "format": AGENT_CONFIG_BUNDLE_FORMAT,
            "version": 99,
            "exported_at": "2026-07-17T00:00:00Z",
            "agent": {
                "id": "codex",
                "label": "Codex",
                "kind": "built_in",
                "codex_like": true,
                "config_lang": "toml",
                "config_content": ""
            }
        })
        .to_string();

        assert!(parse_agent_config_bundle(&raw).is_err());
    }

    #[test]
    fn parses_all_agent_configuration_bundle_without_history_payloads() {
        let raw = serde_json::json!({
            "format": ALL_AGENT_CONFIG_BUNDLE_FORMAT,
            "version": ALL_AGENT_CONFIG_BUNDLE_VERSION,
            "exported_at": "2026-07-17T00:00:00Z",
            "agents": [
                {
                    "id": "claude",
                    "label": "Claude Code",
                    "kind": "built_in",
                    "codex_like": false,
                    "config_lang": "json",
                    "config_content": "{}"
                },
                {
                    "id": "codex",
                    "label": "Codex",
                    "kind": "built_in",
                    "codex_like": true,
                    "config_lang": "toml",
                    "config_content": ""
                }
            ]
        })
        .to_string();

        let bundle = parse_all_agent_config_bundle(&raw).unwrap();
        assert_eq!(bundle.agents.len(), 2);
        assert!(!raw.contains("conversation"));
        assert!(!raw.contains("terminal_history"));
    }

    #[test]
    fn portable_agent_bundle_keeps_credentials_and_drops_source_paths() {
        let mut settings = AppSettings::default();
        settings.custom_agents.push(CustomAgentProfile {
            id: "portable".to_string(),
            label: "Portable Agent".to_string(),
            path: "/Users/source/.aeroric/agents/portable.sh".to_string(),
            codex_like: false,
            family: String::new(),
            config_lang: "shellscript".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            models: vec!["claude-sonnet".to_string()],
            enable_1m_context: true,
            enable_chat_completions_proxy: false,
            username: String::new(),
            password: String::new(),
        });

        let bundle = collect_portable_agent_config_bundle_agent(&settings, "portable").unwrap();
        assert_eq!(bundle.label, "Portable Agent");
        assert_eq!(bundle.base_url, "https://example.com/v1");
        assert_eq!(bundle.api_key, "sk-test");
        assert_eq!(bundle.models, vec!["claude-sonnet"]);
        assert!(bundle.config_content.is_empty());
        assert!(bundle.config_present);

        let draft = custom_agent_setup_draft(&bundle).unwrap();
        let script = build_agent_script(&draft);
        assert!(script.contains("$HOME/.aeroric/agent-homes/portable"));
        assert!(!script.contains("/Users/source/.aeroric"));
    }

    #[test]
    fn portable_builtin_bundle_keeps_config_and_credentials_without_source_path() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-builtin-agent-export-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("settings.json");
        let config_content = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.example.com/v1/",
    "ANTHROPIC_AUTH_TOKEN": "sk-builtin",
    "ANTHROPIC_MODEL": "claude-sonnet[1m]"
  }
}"#;
        std::fs::write(&config_path, config_content).unwrap();

        let mut settings = AppSettings {
            claude_config_path: config_path.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        settings
            .agent_label_overrides
            .insert("claude".to_string(), "Work Claude".to_string());

        let bundle = collect_portable_agent_config_bundle_agent(&settings, "claude").unwrap();
        assert_eq!(bundle.label, "Work Claude");
        assert_eq!(bundle.config_content, config_content);
        assert!(bundle.config_present);
        assert_eq!(bundle.base_url, "https://api.example.com/v1");
        assert_eq!(bundle.api_key, "sk-builtin");
        assert_eq!(bundle.models, vec!["claude-sonnet"]);
        assert!(bundle.enable_1m_context);
        assert!(!serde_json::to_string(&bundle)
            .unwrap()
            .contains(&config_path.to_string_lossy().to_string()));

        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn rejects_duplicate_ids_in_all_agent_configuration_bundle() {
        let raw = serde_json::json!({
            "format": ALL_AGENT_CONFIG_BUNDLE_FORMAT,
            "version": ALL_AGENT_CONFIG_BUNDLE_VERSION,
            "exported_at": "2026-07-17T00:00:00Z",
            "agents": [
                {
                    "id": "custom agent",
                    "label": "Custom Agent",
                    "kind": "custom",
                    "codex_like": true,
                    "config_lang": "shellscript",
                    "config_content": ""
                },
                {
                    "id": "custom_agent",
                    "label": "Duplicate",
                    "kind": "custom",
                    "codex_like": true,
                    "config_lang": "shellscript",
                    "config_content": ""
                }
            ]
        })
        .to_string();

        assert!(parse_all_agent_config_bundle(&raw).is_err());
    }

    #[test]
    fn validates_cc_switch_file_path_extension_and_size() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-cc-switch-validation-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();

        assert!(validate_cc_switch_config_path("relative.sql").is_err());

        let wrong_extension = root.join("providers.txt");
        std::fs::write(&wrong_extension, "-- CC Switch").unwrap();
        assert!(validate_cc_switch_config_path(&wrong_extension.to_string_lossy()).is_err());

        let oversized = root.join("providers.sql");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(MAX_ALL_AGENT_CONFIG_BUNDLE_BYTES + 1).unwrap();
        assert!(validate_cc_switch_config_path(&oversized.to_string_lossy())
            .is_err_and(|error| error.contains("too large")));

        file.set_len(16).unwrap();
        assert_eq!(
            validate_cc_switch_config_path(&oversized.to_string_lossy()).unwrap(),
            oversized
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_multi_agent_import_restores_files_and_settings() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-import-rollback-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let first_config = root.join("claude.json");
        let blocked_parent = root.join("blocked-parent");
        let second_config = blocked_parent.join("gpt55.sh");
        let settings_file = root.join("settings.json");
        std::fs::write(&first_config, "original-config").unwrap();
        std::fs::write(&blocked_parent, "not-a-directory").unwrap();
        std::fs::write(&settings_file, "original-settings").unwrap();

        let mut settings = AppSettings {
            claude_config_path: first_config.to_string_lossy().into_owned(),
            claude_gpt55_config_path: second_config.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let original_settings = settings.clone();
        let result = import_agent_config_entries_transaction(
            &mut settings,
            vec![
                built_in_import("claude", "json", false, "new-config"),
                built_in_import("claude_gpt55", "shellscript", false, "new-script"),
            ],
            &settings_file,
        );

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&first_config).unwrap(),
            "original-config"
        );
        assert_eq!(
            std::fs::read_to_string(&settings_file).unwrap(),
            "original-settings"
        );
        assert_eq!(settings, original_settings);
        assert!(!second_config.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rollback_restores_replaced_symbolic_links() {
        let root = std::env::temp_dir().join(format!(
            "aeroric-agent-import-symlink-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("target.json");
        let link = root.join("config.json");
        std::fs::write(&target, "target-content").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let snapshot = capture_file_snapshot(&link).unwrap();

        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, "replacement").unwrap();
        restore_file_snapshot(&link, &snapshot).unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "target-content");
        let _ = std::fs::remove_dir_all(root);
    }
}
