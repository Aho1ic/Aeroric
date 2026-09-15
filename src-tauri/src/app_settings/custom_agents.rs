//! Custom agent profile create/update internals (file transaction aware).
use std::path::PathBuf;

use super::*;

pub(super) fn upsert_custom_agent_profile_unlocked(
    settings: &mut AppSettings,
    profile: CustomAgentProfile,
) -> Result<AgentFileTransaction, String> {
    let mut profile = normalize_custom_agent_profile(profile)
        .ok_or_else(|| "Invalid custom agent profile".to_string())?;
    let clear_sidecar = profile.api_key.trim().is_empty();
    let existing = settings
        .custom_agents
        .iter()
        .find(|existing| existing.id == profile.id)
        .cloned();
    // A generated wrapper must remain a regular file owned by Aeroric.  Do
    // not let a generated-looking symlink fall through the ownership probe:
    // saving the profile while leaving the link untouched would make the
    // persisted URL/model/key disagree with the launcher that is actually
    // executed.  Refuse the update explicitly and leave both the link and
    // settings unchanged; the user can replace the link with a regular file
    // (or choose a new launcher path) before retrying.
    if existing
        .as_ref()
        .is_some_and(profile_uses_aeroric_generated_wrapper_symlink)
        || profile_uses_aeroric_generated_wrapper_symlink(&profile)
    {
        return Err(
            "Generated Agent script is symlinked; replace it with a regular file before updating"
                .to_string(),
        );
    }
    // Both Claude and Codex generated launchers read the API key from the
    // sidecar and embed the other setup values in the script.  Keep the
    // launcher and sidecar synchronized when an existing generated profile is
    // updated; otherwise `update_custom_agent_access` can persist a new
    // profile while the old wrapper keeps sending requests to the old URL.
    let existing_generated_wrapper = existing
        .as_ref()
        .is_some_and(profile_uses_aeroric_generated_wrapper);
    // `save_custom_agent_profile` is also used by imports and repair tools.
    // If a newly supplied profile already points at an Aeroric-generated
    // launcher, synchronize its sidecar too; otherwise the wrapper can launch
    // without the key that the profile claims to contain.
    let profile_is_generated_wrapper = profile_uses_aeroric_generated_wrapper(&profile);
    let managed_generated_wrapper = existing_generated_wrapper || profile_is_generated_wrapper;
    let family = profile.agent_family();
    let generated_shell_wrapper = managed_generated_wrapper
        && profile.config_lang == "shellscript"
        && matches!(family, AgentFamily::Claude | AgentFamily::Codex);
    let generated_settings_changed = match existing.as_ref() {
        Some(existing) => {
            generated_shell_wrapper
                && (existing.agent_family() != family
                    || existing.label != profile.label
                    || existing.path != profile.path
                    || existing.base_url != profile.base_url
                    || existing.api_key != profile.api_key
                    || existing.models != profile.models
                    || existing.enable_1m_context != profile.enable_1m_context
                    || existing.disable_artifact_tool != profile.disable_artifact_tool
                    || existing.enable_chat_completions_proxy
                        != profile.enable_chat_completions_proxy
                    || existing.bridge_python_path != profile.bridge_python_path)
        }
        None => generated_shell_wrapper,
    };
    let generated_plan = if generated_shell_wrapper
        && !profile.api_key.trim().is_empty()
        && !profile.base_url.trim().is_empty()
        && !profile.models.is_empty()
        && generated_settings_changed
    {
        let draft = AgentSetupDraft {
            id: profile.id.clone(),
            label: profile.label.clone(),
            kind: family.setup_kind(),
            base_url: profile.base_url.clone(),
            api_key: profile.api_key.clone(),
            model: profile.models[0].clone(),
            models: profile.models.clone(),
            enable_1m_context: profile.enable_1m_context,
            disable_artifact_tool: profile.disable_artifact_tool,
            enable_chat_completions_proxy: profile.enable_chat_completions_proxy,
            bridge_python_path: profile.bridge_python_path.clone(),
            dsh_api_protocol: String::new(),
            proxy_enabled: false,
        };
        validate_agent_setup_draft(&draft)?;
        let current_path = normalize_config_path(profile.path.clone());
        let target = generated_agent_script_target_path(&profile.id, &current_path)?;
        Some(GeneratedAgentScriptPlan {
            current_path,
            content: build_agent_script(&draft),
            target,
        })
    } else {
        None
    };

    let invalid_managed_wrapper = managed_generated_wrapper
        && (!generated_shell_wrapper
            || profile.api_key.trim().is_empty()
            || profile.base_url.trim().is_empty()
            || profile.models.is_empty());

    let mut paths = vec![agent_api_key_path(&profile.id)?];
    let dsh_home = if family == AgentFamily::Dsh {
        let home = crate::dsh_home::dsh_home_for(&profile.id)?;
        paths.extend([
            home.join("settings.yaml"),
            home.join(".credentials.yaml"),
            home.join("cordis.patch.yml"),
            crate::dsh_home::managed_patch_path_in(&home),
        ]);
        Some(home)
    } else {
        None
    };
    // omp 档案的凭据/网关写进托管 home 的 models.yml,与 settings.json 同事务更新。
    let omp_home = if family == AgentFamily::Omp {
        let home = crate::omp_home::omp_home_for(&profile.id)?;
        paths.push(home.join("models.yml"));
        Some(home)
    } else {
        None
    };
    if let Some(plan) = &generated_plan {
        paths.push(plan.target.clone());
        if !plan.current_path.trim().is_empty() {
            let previous = PathBuf::from(&plan.current_path);
            if previous != plan.target {
                paths.push(previous);
            }
        }
    }
    let transaction = AgentFileTransaction::capture_and_apply(paths, || -> Result<(), String> {
        if let Some(home) = dsh_home.as_deref() {
            // DSH profiles do not use a generated shell wrapper, but their
            // credentials and provider settings are still part of the same
            // profile update.  Keep those files in the transaction so a later
            // settings write cannot leave a half-applied DSH configuration.
            crate::dsh_home::ensure_dsh_home_at(home)?;
            let api_key = (!profile.api_key.trim().is_empty()).then_some(profile.api_key.trim());
            crate::dsh_home::sync_dsh_credentials(home, api_key)?;
            if !profile.base_url.trim().is_empty() && !profile.models.is_empty() {
                crate::dsh_home::refresh_custom_provider_settings(
                    home,
                    &normalize_base_url(&profile.base_url),
                    &profile.models,
                )?;
            }
        }
        if let Some(home) = omp_home.as_deref() {
            crate::omp_home::ensure_omp_home_for(&profile.id)?;
            crate::omp_home::write_custom_provider_models_yml(
                home,
                &normalize_base_url(&profile.base_url),
                profile.api_key.trim(),
                &crate::omp_home::read_omp_api_protocol(home),
                &profile.models,
            )?;
        }
        if let Some(plan) = generated_plan {
            let path = write_generated_agent_script(
                &profile.id,
                &plan.current_path,
                &plan.content,
                &profile.api_key,
            )?;
            profile.path = path.to_string_lossy().into_owned();
        } else if managed_generated_wrapper && !invalid_managed_wrapper {
            // The script is already current, but the sidecar may have been
            // deleted or edited outside Aeroric. Repair it without rewriting
            // the user's launcher file.
            sync_agent_credentials(&profile.id, &profile.api_key)?;
        }

        // A generated wrapper that is no longer a valid shell Agent profile,
        // or a profile explicitly cleared by the user, must not retain a
        // usable old credential. Do this after all fallible validation above.
        if clear_sidecar || invalid_managed_wrapper {
            remove_agent_api_key(&profile.id)?;
        }

        settings
            .custom_agents
            .retain(|existing| existing.id != profile.id);
        settings.custom_agents.push(profile);
        Ok(())
    })?;
    Ok(transaction)
}

pub(super) fn update_custom_agent_config_internal_with_policy(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    disable_artifact_tool: Option<bool>,
    enable_chat_completions_proxy: Option<bool>,
    bridge_python_path: Option<String>,
    proxy_enabled: Option<bool>,
    enforce_remote_key_boundary: bool,
) -> Result<AppSettings, String> {
    update_settings_locked_with_agent_files(move |settings| {
        let normalized_id = sanitize_custom_agent_id(&id);
        let mut profile = settings
            .custom_agents
            .iter()
            .find(|profile| profile.id == normalized_id)
            .cloned()
            .ok_or_else(|| format!("Agent not found: {id}"))?;
        let normalized_models = models.map(normalize_model_list);
        if let Some(models) = normalized_models.as_ref() {
            if models.is_empty() {
                return Err("At least one model is required".to_string());
            }
            if models.iter().any(|model| !validate_model_name(model)) {
                return Err(
                    "Model names cannot contain quotes, backslashes, or newlines".to_string(),
                );
            }
            let family = profile.agent_family();
            if profile.api_key.trim().is_empty()
                || (family != AgentFamily::Dsh && profile.base_url.trim().is_empty())
            {
                return Err("This agent does not have saved model detection settings".to_string());
            }
        }
        if enforce_remote_key_boundary {
            validate_remote_api_key_reuse(
                &profile.base_url,
                &profile.api_key,
                base_url.as_deref(),
                api_key.as_deref(),
                clear_api_key,
            )?;
        }
        if let Some(base_url) = base_url {
            profile.base_url = base_url;
        }
        if clear_api_key {
            profile.api_key.clear();
        } else if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
            profile.api_key = api_key;
        }
        if let Some(models) = normalized_models {
            profile.models = models;
        }
        let family = profile.agent_family();
        if profile.models.is_empty() && family != AgentFamily::Dsh {
            return Err("At least one model is required".to_string());
        }
        if let Some(enabled) = enable_1m_context {
            if family != AgentFamily::Claude {
                return Err("1M context is only available for Claude Code agents".to_string());
            }
            profile.enable_1m_context = enabled;
        }
        if let Some(disabled) = disable_artifact_tool {
            if family != AgentFamily::Claude {
                return Err(
                    "Disabling the Artifact tool is only available for Claude Code agents"
                        .to_string(),
                );
            }
            profile.disable_artifact_tool = disabled;
        }
        if let Some(enabled) = enable_chat_completions_proxy {
            if family != AgentFamily::Codex {
                return Err(
                    "Chat Completions bridge is only available for Codex agents".to_string()
                );
            }
            if profile.config_lang != "shellscript" {
                return Err(
                    "Chat Completions bridge requires a shell-script Codex agent".to_string(),
                );
            }
            profile.enable_chat_completions_proxy = enabled;
        }
        if let Some(bridge_python_path) = bridge_python_path {
            if family != AgentFamily::Codex {
                return Err(
                    "Chat Completions bridge is only available for Codex agents".to_string()
                );
            }
            if profile.config_lang != "shellscript" {
                return Err(
                    "Chat Completions bridge requires a shell-script Codex agent".to_string(),
                );
            }
            profile.bridge_python_path = bridge_python_path;
        }
        if family == AgentFamily::Codex
            && profile.enable_chat_completions_proxy
            && !profile.bridge_python_path.is_empty()
        {
            let probe = probe_chat_bridge_python_program(&profile.bridge_python_path);
            if let Some(failure) = probe.failure {
                return Err(format!(
                    "This Python cannot run the Chat Completions bridge: {failure}"
                ));
            }
        }
        let transaction = upsert_custom_agent_profile_unlocked(settings, profile)?;
        if let Some(enabled) = proxy_enabled {
            set_agent_proxy_enabled(settings, &normalized_id, enabled);
        }
        Ok(transaction)
    })
}

pub(crate) fn update_custom_agent_config_internal(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    disable_artifact_tool: Option<bool>,
    enable_chat_completions_proxy: Option<bool>,
    bridge_python_path: Option<String>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_custom_agent_config_internal_with_policy(
        id,
        base_url,
        api_key,
        clear_api_key,
        models,
        enable_1m_context,
        disable_artifact_tool,
        enable_chat_completions_proxy,
        bridge_python_path,
        proxy_enabled,
        false,
    )
}

/// Remote/mobile variant of [`update_custom_agent_config_internal`].
pub(crate) fn update_custom_agent_config_remote_internal(
    id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    clear_api_key: bool,
    models: Option<Vec<String>>,
    enable_1m_context: Option<bool>,
    disable_artifact_tool: Option<bool>,
    enable_chat_completions_proxy: Option<bool>,
    bridge_python_path: Option<String>,
    proxy_enabled: Option<bool>,
) -> Result<AppSettings, String> {
    update_custom_agent_config_internal_with_policy(
        id,
        base_url,
        api_key,
        clear_api_key,
        models,
        enable_1m_context,
        disable_artifact_tool,
        enable_chat_completions_proxy,
        bridge_python_path,
        proxy_enabled,
        true,
    )
}
