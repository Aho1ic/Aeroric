//! Read-only projection of stored settings for the ordinary native composer.
//! No CLI/config/home probes, credential service, repair, shell or network —
//! except the built-in Codex model list, which comes from the host's structured
//! executor (`sessions::live::codex::models`, N4) and only when one is installed.
use aeroric_protocol::submission::{
    AgentConfiguration, ConfigurationCatalog, SubmissionRequest, SCHEMA_VERSION,
};
use aeroric_protocol::task::ProtocolFamily;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::agent_connections::launch::{catalog_id, harness_agent, CATALOG_ID_PREFIX};
use crate::agent_connections::model::ConnectionsDocument;
use crate::agents::families::{is_builtin, launch_family, FamilyProfile};
use crate::settings::schema::AppSettings;

/// Agent configurations from `agent-connections.json` and which legacy custom ids
/// the migration replaced (legacy id → configuration id). Holds no credentials.
#[derive(Clone, Debug, Default)]
pub struct AgentSources {
    pub connections: ConnectionsDocument,
    pub migrated: std::collections::BTreeMap<String, String>,
    /// The built-in Codex's own model list (N4): `None` when this host runs no
    /// structured Codex, `Err` when Codex could not answer.
    pub builtin_codex:
        Option<Result<Vec<crate::sessions::live::codex::models::CodexModel>, String>>,
    /// The built-in Claude runs as a native session here (N5).
    pub builtin_claude: bool,
}

impl AgentSources {
    /// A malformed connections document is an error, never an empty catalog; a
    /// missing or unreadable migration report only means nothing was migrated.
    pub fn load(context: &crate::context::CoreContext) -> Result<Self, String> {
        let data_dir = context.paths.data_dir();
        let connections = crate::agent_connections::store::load(
            &crate::agent_connections::store::document_path(data_dir),
        )
        .map_err(|_| "Agent 连接配置读取失败；未回退默认配置。".to_string())?;
        let migrated = crate::agent_connections::launch::load_report(data_dir)
            .map(|report| {
                crate::agent_connections::launch::migrated_legacy_ids(&connections, &report)
            })
            .unwrap_or_default();
        Ok(Self {
            connections,
            migrated,
            builtin_codex: crate::sessions::live::codex::models::builtin_models("codex"),
            builtin_claude: crate::sessions::live::codex::models::native_sessions_enabled(),
        })
    }
}

/// Exact stored project identity; no registration, path repair or remote fallback.
pub fn for_project(
    context: &crate::context::CoreContext,
    project_id: &str,
) -> Result<(crate::storage::Record, ConfigurationCatalog), String> {
    let projects = crate::storage::load_projects(context)
        .map_err(|_| "项目目录读取失败；未猜测目标。".to_string())?;
    let mut matches = projects
        .into_iter()
        .filter(|record| record.id() == Some(project_id));
    let project = matches.next().ok_or("原项目不存在；未选择其他项目。")?;
    if matches.next().is_some() {
        return Err("项目身份重复；未猜测目标。".into());
    }
    if !crate::projects::mutations::is_addressable(&project) {
        return Err("原项目地址格式无效。".into());
    }
    let target = crate::projects::mutations::resolve_location(&project);
    // An SSH project (2026-10-28) is named by its identity, never a local
    // path; the remote agent's availability is the host's to mark.
    let identity = target.identity_key();
    let path = match &target {
        aeroric_protocol::project::ProjectTarget::Ssh { .. } => identity.as_str(),
        _ => target
            .local_path()
            .filter(|path| std::path::Path::new(path).is_absolute())
            .ok_or("此提交契约仅支持已登记的本地项目或 SSH 项目；不会退到本地影子路径。")?,
    };
    let fields = crate::storage::load_settings(context)
        .map_err(|_| "Agent 设置读取失败；未回退默认配置。".to_string())?
        .found()
        .unwrap_or_default();
    let sources = AgentSources::load(context)?;
    let directory = catalog_with(&fields, &sources, project_id, path, context.clock.now_ms())?;
    Ok((project, directory))
}

pub fn catalog(
    fields: &Map<String, Value>,
    project_id: &str,
    project_path: &str,
    now: u64,
) -> Result<ConfigurationCatalog, String> {
    catalog_with(
        fields,
        &AgentSources::default(),
        project_id,
        project_path,
        now,
    )
}

pub fn catalog_with(
    fields: &Map<String, Value>,
    sources: &AgentSources,
    project_id: &str,
    project_path: &str,
    now: u64,
) -> Result<ConfigurationCatalog, String> {
    let settings: AppSettings = serde_json::from_value(Value::Object(fields.clone()))
        .map_err(|_| "已存 Agent 设置格式无效；未回退默认配置。".to_string())?;
    // Includes private settings only inside a one-way digest; never return them.
    // A settings change invalidates the selection rather than silently upgrading it.
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": SCHEMA_VERSION, "settings": fields,
        "projectId": project_id, "projectPath": project_path,
        "connections": sources.connections, "migrated": sources.migrated,
        "builtinCodex": sources.builtin_codex.as_ref().map(|answer| answer.as_ref().ok()),
        "builtinClaude": sources.builtin_claude
    }))
    .map_err(|_| "无法确定配置版本。".to_string())?;
    let revision = format!("v1:{:x}", Sha256::digest(bytes));
    let mut configurations = Vec::new();
    for (agent, label) in [
        ("codex", "Codex"),
        ("claude", "Claude Code"),
        ("omp", "oh-my-pi"),
        ("dsh", "DeepSeek"),
        ("claude_gpt55", "Claude GPT55"),
    ] {
        if agent == "codex" {
            if let Some(native) = builtin_codex(&revision, sources) {
                configurations.push(native);
                continue;
            }
        }
        if agent == "claude" && sources.builtin_claude {
            configurations.push(builtin_claude(&revision));
            continue;
        }
        configurations.push(AgentConfiguration {
            id: agent.into(),
            version: revision.clone(),
            display_name: label.into(),
            agent: agent.into(),
            harness: agent.into(),
            models: vec![],
            efforts: vec![],
            permission_modes: vec![],
            blocked_reasons: vec![
                "内建配置的路径、模型与托管执行装配尚未由当前原生执行器完整采用。".into(),
            ],
        });
    }
    agent_config_entries(&sources.connections, &revision, &mut configurations);
    let mut ids = std::collections::HashSet::new();
    for profile in &settings.custom_agents {
        if profile.id.trim().is_empty()
            || is_builtin(&profile.id)
            || profile.id.starts_with(CATALOG_ID_PREFIX)
            || !ids.insert(&profile.id)
        {
            return Err("已存自定义 Agent 身份为空、重复或与内建身份冲突；未猜测配置。".into());
        }
        // Migrated: offered once, as its agent configuration (D2).
        if sources.migrated.contains_key(&profile.id) {
            continue;
        }
        let family = launch_family(FamilyProfile {
            family: &profile.family,
            codex_like: profile.codex_like,
        });
        let harness = match family {
            ProtocolFamily::Claude => "claude",
            ProtocolFamily::Codex => "codex",
            ProtocolFamily::Dsh => "dsh",
            ProtocolFamily::Omp => "omp",
        };
        let mut blocked = Vec::new();
        if profile.path.trim().is_empty() {
            blocked.push("此配置未指定执行程序；不会按 Agent 名称猜测路径。".into());
        }
        if [
            &profile.base_url,
            &profile.api_key,
            &profile.username,
            &profile.password,
        ]
        .iter()
        .any(|value| !value.is_empty())
        {
            blocked
                .push("此配置的 API 地址或凭据尚未由当前原生执行器装配；不会改用其他连接。".into());
        }
        if profile.enable_1m_context
            || profile.disable_artifact_tool
            || profile.enable_chat_completions_proxy
            || !profile.bridge_python_path.is_empty()
        {
            blocked.push(
                "此配置的上下文、工具或代理选项尚未由当前原生执行器采用；不会静默忽略。".into(),
            );
        }
        let raw = fields
            .get("custom_agents")
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["id"].as_str() == Some(&profile.id))
            });
        if raw
            .and_then(|item| item.get("args"))
            .is_some_and(|args| args.as_array().is_none_or(|args| !args.is_empty()))
        {
            blocked.push(
                "此配置的固定参数尚未确认与模型及权限选择的关系；不会忽略或覆盖选择。".into(),
            );
        }
        if !matches!(family, ProtocolFamily::Claude | ProtocolFamily::Codex) {
            blocked.push(
                "此 Harness 的配置模型、推理强度或托管装配尚未被当前原生执行路径完整消费。".into(),
            );
        }
        let models: Vec<_> = profile
            .models
            .iter()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if models.is_empty() {
            blocked.push("已存配置没有模型目录；不会查询 API 或假设默认模型。".into());
        }
        configurations.push(AgentConfiguration {
            id: profile.id.clone(),
            version: revision.clone(),
            display_name: profile.label.clone(),
            agent: profile.id.clone(),
            harness: harness.into(),
            models,
            efforts: vec![],
            permission_modes: vec!["ask".into(), "auto_edit".into(), "full_access".into()],
            blocked_reasons: blocked,
        });
    }
    Ok(ConfigurationCatalog {
        schema_version: SCHEMA_VERSION,
        project_id: project_id.into(),
        revision,
        as_of: now.to_string(),
        configurations,
    })
}

/// One entry per agent configuration. Models are the mapped names some enabled
/// connection can serve for the harness; credentials are not probed (a missing key
/// surfaces at the gateway). For Claude/Codex every reach `routing::candidates`
/// returns is a translation the gateway implements (4c/4d).
fn agent_config_entries(
    document: &ConnectionsDocument,
    revision: &str,
    out: &mut Vec<AgentConfiguration>,
) {
    let names: std::collections::BTreeSet<String> = document
        .connections
        .iter()
        .flat_map(|connection| connection.routable_models())
        .map(|model| model.routing_name().to_string())
        .filter(|name| !name.trim().is_empty())
        .collect();
    // Saved connections first (2026-10-27: the picker offers them directly),
    // then the user's own configurations (pools, routes, fallbacks).
    let derived = document.connection_configs();
    for config in derived.iter().chain(&document.configs) {
        let agent = harness_agent(config.harness);
        let mut blocked = Vec::new();
        if !config.harness.gateway_supported() {
            blocked.push("此 Harness 的 Agent 配置尚不能经本机网关启动。".to_string());
        }
        let models: Vec<String> = names
            .iter()
            .filter(|name| {
                !crate::agent_connections::routing::candidates(document, config, name)
                    .0
                    .is_empty()
            })
            .cloned()
            .collect();
        if models.is_empty() {
            blocked.push("此 Agent 配置没有可用渠道提供的模型；不会假设默认模型。".to_string());
        }
        out.push(AgentConfiguration {
            id: catalog_id(&config.id),
            version: revision.to_string(),
            display_name: config.label.clone(),
            agent: agent.into(),
            harness: agent.into(),
            models,
            // D4: the gateway does not map reasoning tiers upstream yet.
            efforts: vec![],
            permission_modes: vec!["ask".into(), "auto_edit".into(), "full_access".into()],
            blocked_reasons: blocked,
        });
    }
}

/// Admission is repeated immediately before a receipt and any task execution.
/// N4: the built-in Codex runs as a native session with its own login, so it
/// is offered once its model list is known; a failed read keeps it unavailable
/// with the reason.
fn builtin_codex(revision: &str, sources: &AgentSources) -> Option<AgentConfiguration> {
    let answer = sources.builtin_codex.as_ref()?;
    let (models, efforts, blocked) = match answer {
        Ok(models) => {
            let mut efforts: Vec<String> = Vec::new();
            for effort in models.iter().flat_map(|m| m.efforts.iter()) {
                if !efforts.contains(effort) {
                    efforts.push(effort.clone());
                }
            }
            let mut ordered: Vec<&crate::sessions::live::codex::models::CodexModel> =
                models.iter().collect();
            ordered.sort_by_key(|m| !m.is_default);
            (
                ordered.into_iter().map(|m| m.model.clone()).collect(),
                efforts,
                Vec::new(),
            )
        }
        Err(reason) => (Vec::new(), Vec::new(), vec![reason.clone()]),
    };
    Some(AgentConfiguration {
        id: "codex".into(),
        version: revision.to_string(),
        display_name: "Codex".into(),
        agent: "codex".into(),
        harness: "codex".into(),
        models,
        efforts,
        permission_modes: vec!["ask".into(), "auto_edit".into(), "full_access".into()],
        blocked_reasons: blocked,
    })
}

/// N5 D4: the built-in Claude, native with the user's own login. Claude Code
/// has no model catalog to ask, so its stable aliases are offered; the
/// reasoning effort is not offered (it is fixed for a session).
fn builtin_claude(revision: &str) -> AgentConfiguration {
    AgentConfiguration {
        id: "claude".into(),
        version: revision.to_string(),
        display_name: "Claude Code".into(),
        agent: "claude".into(),
        harness: "claude".into(),
        models: vec!["opus".into(), "sonnet".into(), "haiku".into()],
        efforts: vec![],
        permission_modes: vec!["ask".into(), "auto_edit".into(), "full_access".into()],
        blocked_reasons: vec![],
    }
}

pub fn validate(catalog: &ConfigurationCatalog, request: &SubmissionRequest) -> Vec<String> {
    let mut reasons = Vec::new();
    if request.project_id != catalog.project_id {
        reasons.push("原项目身份不匹配。".into());
    }
    if let Some(configuration) = catalog
        .configurations
        .iter()
        .find(|item| item.id == request.configuration_id && item.agent == request.agent)
    {
        reasons.extend(configuration.blocked_reasons.iter().cloned());
        if configuration.version != request.configuration_version {
            reasons.push("Agent 配置版本已变化，请明确重新选择。".into());
        }
        if !configuration
            .permission_modes
            .contains(&request.permission_mode)
        {
            reasons.push("此配置不支持已选权限。".into());
        }
        if !request
            .selected_model
            .as_ref()
            .is_some_and(|model| configuration.models.contains(model))
        {
            reasons.push("请明确选择此配置支持的模型。".into());
        }
        if request
            .reasoning_effort
            .as_ref()
            .is_some_and(|effort| !configuration.efforts.contains(effort))
        {
            reasons.push("此配置没有确认已选推理强度；不会静默忽略。".into());
        }
    } else {
        reasons.push("原 Agent 配置不存在；未换绑其他配置。".into());
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields() -> Map<String, Value> {
        json!({"custom_agents":[{
            "id":"custom-fixture", "label":"Fixture", "path":"/synthetic/never-executed",
            "codex_like":true, "family":"codex", "models":["fixture/model"]
        }]})
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn projection_redacts_secrets_paths_and_preserves_custom_identity() {
        let mut fields = fields();
        for (key, value) in [
            ("api_key", "FAKE-SECRET-MUST-NOT-LEAK"),
            ("username", "FAKE-USER"),
            ("password", "FAKE-PASSWORD"),
            ("base_url", "https://fixture.invalid/secret-endpoint"),
        ] {
            fields["custom_agents"][0][key] = json!(value);
        }
        let directory = catalog(&fields, "fixture-project", "/synthetic/project", 42).unwrap();
        let encoded = serde_json::to_string(&directory).unwrap();
        for secret in [
            "FAKE-SECRET",
            "FAKE-USER",
            "FAKE-PASSWORD",
            "/synthetic",
            "fixture.invalid",
            "api_key",
            "base_url",
            "args",
        ] {
            assert!(!encoded.contains(secret), "{secret}");
        }
        let custom = directory.configurations.last().unwrap();
        assert_eq!(custom.agent, "custom-fixture");
        assert_eq!(custom.harness, "codex");
        assert_eq!(custom.models, ["fixture/model"]);
        assert!(custom.efforts.is_empty());
        assert!(custom
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("凭据")));
        assert!(directory.configurations[..5]
            .iter()
            .all(|item| !item.blocked_reasons.is_empty()));
        assert_ne!(
            directory.revision,
            catalog(&fields, "other", "/synthetic/project", 42)
                .unwrap()
                .revision
        );
        assert_eq!(
            directory.revision,
            catalog(&fields, "fixture-project", "/synthetic/project", 100)
                .unwrap()
                .revision
        );
    }

    #[test]
    fn stale_configuration_and_missing_or_unsupported_model_are_never_defaulted() {
        let mut fields = fields();
        let directory = catalog(&fields, "fixture-project", "/synthetic/project", 42).unwrap();
        let mut request = aeroric_protocol::submission::fixture_request();
        request.configuration_version = directory.revision.clone();
        assert!(validate(&directory, &request).is_empty());
        request.selected_model = None;
        assert!(validate(&directory, &request)
            .iter()
            .any(|reason| reason.contains("模型")));
        request.selected_model = Some("not-in-catalog".into());
        assert!(!validate(&directory, &request).is_empty());
        request.selected_model = Some("fixture/model".into());
        request.reasoning_effort = Some("high".into());
        assert!(validate(&directory, &request)
            .iter()
            .any(|reason| reason.contains("推理强度")));
        fields["custom_agents"][0]["api_key"] = json!("ROTATED-FAKE-KEY");
        let newer = catalog(&fields, "fixture-project", "/synthetic/project", 43).unwrap();
        assert!(validate(&newer, &request)
            .iter()
            .any(|reason| reason.contains("版本")));
    }

    #[test]
    fn malformed_duplicate_and_unconsumed_configuration_are_explicitly_refused() {
        let mut fields = fields();
        fields["custom_agents"][0]["path"] = json!(123);
        assert!(catalog(&fields, "p", "/synthetic", 0).is_err());
        fields = self::fields();
        let duplicate = fields["custom_agents"][0].clone();
        fields
            .get_mut("custom_agents")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(catalog(&fields, "p", "/synthetic", 0).is_err());
        for (family, codex_like) in [("dsh", false), ("omp", true)] {
            fields = self::fields();
            fields["custom_agents"][0]["family"] = json!(family);
            fields["custom_agents"][0]["codex_like"] = json!(codex_like);
            assert!(!catalog(&fields, "p", "/synthetic", 0)
                .unwrap()
                .configurations
                .last()
                .unwrap()
                .blocked_reasons
                .is_empty());
        }
        fields = self::fields();
        fields["custom_agents"][0]["args"] = json!(["--dangerously-skip-permissions"]);
        assert!(!catalog(&fields, "p", "/synthetic", 0)
            .unwrap()
            .configurations
            .last()
            .unwrap()
            .blocked_reasons
            .is_empty());
    }

    #[test]
    fn unconsumed_credentials_and_execution_options_block_the_whole_configuration() {
        for (key, value) in [
            ("base_url", json!("https://fixture.invalid")),
            ("api_key", json!("FAKE-KEY")),
            ("username", json!("FAKE-USER")),
            ("password", json!("FAKE-PASSWORD")),
            ("enable_1m_context", json!(true)),
            ("disable_artifact_tool", json!(true)),
            ("enable_chat_completions_proxy", json!(true)),
            ("bridge_python_path", json!("/synthetic/never-executed")),
        ] {
            let mut fields = json!({"custom_agents":[{
                "id":"custom-fixture", "label":"Fixture",
                "path":"/synthetic/never-executed", "family":"codex",
                "models":["fixture/model"]
            }]})
            .as_object()
            .unwrap()
            .clone();
            assert!(catalog(&fields, "p", "/synthetic", 0)
                .unwrap()
                .configurations
                .last()
                .unwrap()
                .blocked_reasons
                .is_empty());
            fields["custom_agents"][0][key] = value;
            assert!(
                !catalog(&fields, "p", "/synthetic", 0)
                    .unwrap()
                    .configurations
                    .last()
                    .unwrap()
                    .blocked_reasons
                    .is_empty(),
                "{key} must not be silently ignored"
            );
        }
    }

    fn sources(migrated: &[(&str, &str)]) -> AgentSources {
        let connections: ConnectionsDocument = serde_json::from_value(json!({
            "version": 1,
            "connections": [{
                "id": "conn-messages", "label": "Messages", "kind": "api_key", "vendor": "other",
                "base_url": "https://fixture.invalid/v1", "protocols": ["messages"],
                "models": [
                    {"upstream_id": "up-a", "mapped_name": "model-a"},
                    {"upstream_id": "up-off", "mapped_name": "model-off", "enabled": false}
                ]
            }, {
                "id": "conn-off", "label": "Disabled", "kind": "api_key", "vendor": "other",
                "base_url": "https://off.invalid", "protocols": ["responses"], "enabled": false,
                "models": [{"upstream_id": "up-b", "mapped_name": "model-b"}]
            }],
            "configs": [
                {"id": "cfg-claude", "label": "Claude via gateway", "harness": "claude",
                 "target": {"kind": "route"}},
                {"id": "cfg-codex", "label": "Codex pinned", "harness": "codex",
                 "target": {"kind": "connection", "connection_id": "conn-messages"}},
                {"id": "cfg-empty", "label": "Nothing to serve", "harness": "claude",
                 "target": {"kind": "connection", "connection_id": "conn-off"}},
                {"id": "cfg-omp", "label": "OMP", "harness": "omp", "target": {"kind": "route"}}
            ]
        }))
        .unwrap();
        AgentSources {
            connections,
            migrated: migrated
                .iter()
                .map(|(legacy, config)| ((*legacy).to_string(), (*config).to_string()))
                .collect(),
            builtin_codex: None,
            builtin_claude: false,
        }
    }

    fn entry<'a>(directory: &'a ConfigurationCatalog, id: &str) -> &'a AgentConfiguration {
        directory
            .configurations
            .iter()
            .find(|item| item.id == id)
            .unwrap_or_else(|| panic!("{id} missing"))
    }

    #[test]
    fn agent_configurations_are_offered_under_their_own_namespace() {
        let directory = catalog_with(
            &fields(),
            &sources(&[]),
            "fixture-project",
            "/synthetic/project",
            0,
        )
        .unwrap();
        let claude = entry(&directory, "agent-config:cfg-claude");
        assert_eq!(claude.agent, "claude");
        assert_eq!(claude.harness, "claude");
        assert_eq!(claude.display_name, "Claude via gateway");
        assert_eq!(
            claude.models,
            ["model-a"],
            "disabled models/connections are not offered"
        );
        assert!(claude.efforts.is_empty());
        assert!(
            claude.blocked_reasons.is_empty(),
            "{:?}",
            claude.blocked_reasons
        );
        // Codex reaches a Messages-only connection through the gateway (4d).
        let codex = entry(&directory, "agent-config:cfg-codex");
        assert_eq!(codex.agent, "codex");
        assert_eq!(codex.models, ["model-a"]);
        assert!(codex.blocked_reasons.is_empty());
        assert!(!entry(&directory, "agent-config:cfg-empty")
            .blocked_reasons
            .is_empty());
        // omp reaches the gateway too (2026-10-27).
        assert!(entry(&directory, "agent-config:cfg-omp")
            .blocked_reasons
            .is_empty());
        // The legacy entry is still there and keeps its own identity.
        assert_eq!(entry(&directory, "custom-fixture").agent, "custom-fixture");

        let encoded = serde_json::to_string(&directory).unwrap();
        for leaked in [
            "fixture.invalid",
            "off.invalid",
            "conn-messages",
            "up-a",
            "base_url",
        ] {
            assert!(!encoded.contains(leaked), "{leaked}");
        }

        let mut request = aeroric_protocol::submission::fixture_request();
        request.configuration_id = "agent-config:cfg-claude".into();
        request.agent = "claude".into();
        request.configuration_version = directory.revision.clone();
        request.selected_model = Some("model-a".into());
        assert!(validate(&directory, &request).is_empty());
        request.agent = "codex".into();
        assert!(
            !validate(&directory, &request).is_empty(),
            "agent must match"
        );
    }

    #[test]
    fn migrated_legacy_entries_are_replaced_by_their_configuration() {
        let migrated = sources(&[("custom-fixture", "cfg-codex")]);
        let directory = catalog_with(&fields(), &migrated, "p", "/synthetic", 0).unwrap();
        assert!(directory
            .configurations
            .iter()
            .all(|item| item.id != "custom-fixture"));
        assert!(directory
            .configurations
            .iter()
            .any(|item| item.id == "agent-config:cfg-codex"));
    }

    #[test]
    fn revision_covers_connections_and_migration() {
        let base = catalog_with(&fields(), &sources(&[]), "p", "/synthetic", 0)
            .unwrap()
            .revision;
        let mut changed = sources(&[]);
        changed.connections.configs[0].label = "Renamed".into();
        assert_ne!(
            base,
            catalog_with(&fields(), &changed, "p", "/synthetic", 0)
                .unwrap()
                .revision
        );
        assert_ne!(
            base,
            catalog_with(
                &fields(),
                &sources(&[("x", "cfg-claude")]),
                "p",
                "/synthetic",
                0
            )
            .unwrap()
            .revision
        );
        assert_eq!(
            base,
            catalog_with(&fields(), &sources(&[]), "p", "/synthetic", 99)
                .unwrap()
                .revision
        );
    }

    #[test]
    fn legacy_ids_cannot_take_the_configuration_namespace() {
        let mut fields = fields();
        fields["custom_agents"][0]["id"] = json!("agent-config:cfg-claude");
        assert!(catalog_with(&fields, &sources(&[]), "p", "/synthetic", 0).is_err());
    }

    #[test]
    fn the_built_in_codex_is_offered_with_its_own_models_once_known() {
        use crate::sessions::live::codex::models::CodexModel;
        let fields = Map::new();
        let sources = AgentSources {
            builtin_codex: Some(Ok(vec![
                CodexModel {
                    model: "mini".into(),
                    efforts: vec!["low".into()],
                    is_default: false,
                },
                CodexModel {
                    model: "main".into(),
                    efforts: vec!["low".into(), "high".into()],
                    is_default: true,
                },
            ])),
            ..AgentSources::default()
        };
        let directory = catalog_with(&fields, &sources, "p", "/synthetic/project", 1).unwrap();
        let codex = directory
            .configurations
            .iter()
            .find(|c| c.id == "codex")
            .unwrap();
        assert!(codex.blocked_reasons.is_empty());
        assert_eq!(codex.models, ["main", "mini"], "the default first");
        assert_eq!(codex.efforts, ["low", "high"]);
        assert_eq!(codex.permission_modes, ["ask", "auto_edit", "full_access"]);
        let request = SubmissionRequest {
            project_id: "p".into(),
            configuration_id: "codex".into(),
            configuration_version: directory.revision.clone(),
            agent: "codex".into(),
            permission_mode: "auto_edit".into(),
            selected_model: Some("main".into()),
            reasoning_effort: Some("high".into()),
            ..aeroric_protocol::submission::fixture_request()
        };
        assert_eq!(validate(&directory, &request), Vec::<String>::new());
        let claude = directory
            .configurations
            .iter()
            .find(|c| c.id == "claude")
            .unwrap();
        assert!(
            !claude.blocked_reasons.is_empty(),
            "only Codex is native in N4"
        );
    }

    #[test]
    fn a_codex_that_cannot_answer_stays_unavailable_with_its_reason() {
        let sources = AgentSources {
            builtin_codex: Some(Err("请升级 Codex".into())),
            ..AgentSources::default()
        };
        let directory = catalog_with(&Map::new(), &sources, "p", "/synthetic/project", 1).unwrap();
        let codex = directory
            .configurations
            .iter()
            .find(|c| c.id == "codex")
            .unwrap();
        assert_eq!(codex.blocked_reasons, ["请升级 Codex"]);
        let none = catalog_with(
            &Map::new(),
            &AgentSources::default(),
            "p",
            "/synthetic/project",
            1,
        )
        .unwrap();
        assert!(none
            .configurations
            .iter()
            .find(|c| c.id == "codex")
            .unwrap()
            .blocked_reasons[0]
            .contains("内建配置"));
    }

    #[test]
    fn the_built_in_claude_is_offered_with_its_aliases_only_on_a_native_host() {
        let native = AgentSources {
            builtin_claude: true,
            ..AgentSources::default()
        };
        let directory = catalog_with(&Map::new(), &native, "p", "/synthetic/project", 1).unwrap();
        let claude = directory
            .configurations
            .iter()
            .find(|c| c.id == "claude")
            .unwrap();
        assert!(claude.blocked_reasons.is_empty());
        assert_eq!(claude.models, ["opus", "sonnet", "haiku"]);
        assert!(claude.efforts.is_empty());
        let request = SubmissionRequest {
            project_id: "p".into(),
            configuration_id: "claude".into(),
            configuration_version: directory.revision.clone(),
            agent: "claude".into(),
            permission_mode: "ask".into(),
            permission_agent: "claude".into(),
            selected_model: Some("sonnet".into()),
            reasoning_effort: None,
            ..aeroric_protocol::submission::fixture_request()
        };
        assert_eq!(validate(&directory, &request), Vec::<String>::new());
        let pty = catalog_with(
            &Map::new(),
            &AgentSources::default(),
            "p",
            "/synthetic/project",
            1,
        )
        .unwrap();
        let blocked = pty
            .configurations
            .iter()
            .find(|c| c.id == "claude")
            .unwrap();
        assert!(!blocked.blocked_reasons.is_empty());
        assert_ne!(pty.revision, directory.revision);
    }
}
