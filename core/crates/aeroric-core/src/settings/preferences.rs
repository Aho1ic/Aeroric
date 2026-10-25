//! Pure preferences plus a raw-map transaction. This path never invokes the
//! legacy loader's repair, script refresh, executable probing or home discovery.

use serde_json::{Map, Value};

use super::agent_env::migrate_agent_proxy_enabled_fields;
use super::normalize::{normalize_notebook_embedding_settings, normalize_proxy_settings};
use super::schema::{
    normalize_auto_cleanup_settings, normalize_send_shortcut, normalize_weekly_report_settings,
    AppSettings, AutoCleanupSettings, NotebookEmbeddingSettings, ProxySettings,
    WeeklyReportSettings,
};
use crate::context::CoreContext;
use crate::storage::{atomic::StorageError, settings};

/// The built-in DSH settings wire; not the Claude/Codex or OMP vocabulary.
pub const DSH_REASONING_EFFORTS: [&str; 3] = ["off", "high", "max"];

/// Constructed only after validating the exact built-in agent and effort.
/// Keeping the field private prevents an unchecked value reaching a transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DshReasoningEffort(&'static str);

impl DshReasoningEffort {
    pub fn for_builtin_agent(agent: &str, effort: &str) -> Result<Self, String> {
        if agent != "dsh" {
            return Err(
                "Reasoning effort is only configurable for the built-in DeepSeek Harness agent"
                    .to_string(),
            );
        }
        let normalized = effort.trim().to_ascii_lowercase();
        DSH_REASONING_EFFORTS
            .iter()
            .copied()
            .find(|candidate| *candidate == normalized.as_str())
            .map(Self)
            .ok_or_else(|| "Invalid DeepSeek Harness reasoning effort".to_string())
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Debug)]
pub enum PreferenceUpdate {
    SendShortcut(String),
    ShiftEnterNewline(bool),
    /// A14: keep the Mac awake while tasks run (absent means on).
    KeepAwakeWhileRunning(bool),
    AutoCleanup(AutoCleanupSettings),
    WeeklyReport(WeeklyReportSettings),
    NotebookEmbedding(NotebookEmbeddingSettings),
    DshReasoningEffort(DshReasoningEffort),
    Proxy(ProxySettings),
}

impl PreferenceUpdate {
    fn apply_to(self, fields: &mut Map<String, Value>) -> Result<(), String> {
        let (key, value) = match self {
            Self::SendShortcut(value) => (
                "send_shortcut",
                Value::String(normalize_send_shortcut(value)),
            ),
            Self::ShiftEnterNewline(value) => ("terminal_shift_enter_newline", Value::Bool(value)),
            Self::KeepAwakeWhileRunning(value) => ("keep_awake_while_running", Value::Bool(value)),
            Self::AutoCleanup(value) => (
                "auto_cleanup_settings",
                serde_json::to_value(normalize_auto_cleanup_settings(value))
                    .expect("cleanup settings contain only JSON-safe fields"),
            ),
            Self::WeeklyReport(value) => (
                "weekly_report_settings",
                serde_json::to_value(normalize_weekly_report_settings(value))
                    .expect("weekly report settings contain only JSON-safe fields"),
            ),
            Self::NotebookEmbedding(value) => (
                "notebook_embedding_settings",
                serde_json::to_value(normalize_notebook_embedding_settings(value))
                    .expect("embedding settings contain only JSON-safe fields"),
            ),
            Self::DshReasoningEffort(value) => {
                // This is a nested patch, not the legacy whole-map sanitizer:
                // preserve other entries rather than cleaning unknown agents.
                let efforts = fields
                    .entry("dsh_reasoning_efforts")
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .ok_or("dsh_reasoning_efforts must be an object")?;
                efforts.insert("dsh".into(), Value::String(value.as_str().into()));
                return Ok(());
            }
            Self::Proxy(value) => return apply_proxy_update(fields, value),
        };
        fields.insert(key.into(), value);
        Ok(())
    }
}

fn apply_proxy_update(
    fields: &mut Map<String, Value>,
    replacement: ProxySettings,
) -> Result<(), String> {
    let value = serde_json::to_value(normalize_proxy_settings(replacement))
        .expect("proxy settings contain only JSON-safe fields");
    // Validate migration inputs in the same transaction. Substitute only the
    // requested target so a valid replacement can repair a corrupt proxy field,
    // but cannot hide malformed legacy/custom fields or unrelated settings.
    let mut projection = fields.clone();
    projection.insert("proxy_settings".into(), value.clone());
    let settings: AppSettings = serde_json::from_value(Value::Object(projection))
        .map_err(|error| format!("invalid settings fields: {error}"))?;
    let enabled = migrate_agent_proxy_enabled_fields(
        &settings.agent_proxy_enabled,
        &settings.agent_proxy_overrides,
    );
    if let Some(profiles) = fields.get_mut("custom_agents") {
        let profiles = profiles
            .as_array_mut()
            .ok_or("custom_agents must be an array for proxy migration")?;
        for profile in profiles {
            let profile = profile
                .as_object_mut()
                .ok_or("custom_agents entries must be objects for proxy migration")?;
            // Consume exactly these deprecated fields, not unknown fields or
            // nested values. Do not filter/reorder profiles via the old loader.
            profile.remove("username");
            profile.remove("password");
        }
    }
    fields.remove("agent_proxy_overrides");
    fields.insert(
        "agent_proxy_enabled".into(),
        serde_json::to_value(enabled).expect("proxy flags contain only JSON-safe fields"),
    );
    // Old load consumes legacy values before the command replaces all four
    // fields. Running the legacy resolver here would resurrect explicit clears.
    fields.insert("proxy_settings".into(), value);
    Ok(())
}

pub fn update_preference(
    context: &CoreContext,
    update: PreferenceUpdate,
) -> Result<Map<String, Value>, StorageError> {
    settings::update_settings(context, |fields| {
        update
            .apply_to(fields)
            .map_err(|reason| StorageError::Corrupt {
                path: context.paths.settings_file(),
                reason,
            })?;
        // Tauri returns AppSettings. Refuse an incompatible existing field before
        // committing, not after a successful write followed by a decoding error.
        // This projection is validation only: never persist it and lose unknown keys.
        serde_json::from_value::<AppSettings>(Value::Object(fields.clone())).map_err(|error| {
            StorageError::Corrupt {
                path: context.paths.settings_file(),
                reason: format!("invalid settings fields: {error}"),
            }
        })?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::writer;
    use serde_json::json;

    #[test]
    fn preference_transaction_keeps_unknown_and_unrelated_fields_without_running_agents() {
        let (root, context) = writer("preference-unknown");
        let original = json!({
            "unknown": {"nested": [1, 2, 3]}, "claude_path": "/never/run",
            "custom_agents": [{"id": "future", "label": "Synthetic", "path": "/not/executed",
                "future_option": {"keep": true}}],
        });
        settings::save_settings(&context, original.as_object().unwrap()).unwrap();
        let after =
            update_preference(&context, PreferenceUpdate::SendShortcut("invalid".into())).unwrap();
        assert_eq!(after["send_shortcut"], "mod_enter");
        for (key, value) in original.as_object().unwrap() {
            assert_eq!(&after[key], value);
        }
        assert!(!root.join("agent-homes").exists());
        assert_eq!(
            settings::load_settings(&context).unwrap().found().unwrap(),
            after
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn preference_schedules_reuse_clamps_without_changing_last_run_or_creating_paths() {
        let (root, context) = writer("preference-clamp");
        let cleanup = AutoCleanupSettings {
            enabled: true,
            mode: "invalid".into(),
            weekday: 9,
            hour: 99,
            interval_days: 0,
            retain_days: u16::MAX,
            last_run_at: Some(123),
        };
        let fields = update_preference(&context, PreferenceUpdate::AutoCleanup(cleanup)).unwrap();
        assert_eq!(
            fields["auto_cleanup_settings"],
            json!({
                "enabled": true, "mode": "weekly", "weekday": 6, "hour": 23,
                "interval_days": 1, "retain_days": 3650, "last_run_at": 123,
            })
        );
        let out = root.join("not-created");
        let fields = update_preference(
            &context,
            PreferenceUpdate::WeeklyReport(WeeklyReportSettings {
                week_start_day: 99,
                week_end_day: 99,
                output_dir: out.to_string_lossy().into_owned(),
            }),
        )
        .unwrap();
        assert_eq!(fields["weekly_report_settings"]["week_start_day"], 6);
        assert_eq!(fields["weekly_report_settings"]["week_end_day"], 6);
        assert_eq!(fields["weekly_report_settings"]["output_dir"], json!(out));
        assert!(!out.exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn preference_rejects_unreturnable_settings_before_writing_but_can_repair_its_own_field() {
        let (root, context) = writer("preference-corrupt");
        let path = context.paths.settings_file();
        std::fs::write(&path, br#"{"custom_agents":false,"send_shortcut":"enter"}"#).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            update_preference(&context, PreferenceUpdate::SendShortcut("mod_enter".into())),
            Err(StorageError::Corrupt { .. })
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::write(
            &path,
            br#"{"terminal_shift_enter_newline":"bad","unknown":true}"#,
        )
        .unwrap();
        let fields =
            update_preference(&context, PreferenceUpdate::ShiftEnterNewline(false)).unwrap();
        assert_eq!(fields["terminal_shift_enter_newline"], false);
        assert_eq!(fields["unknown"], true);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn preference_defaults_and_read_only_refusal_use_only_injected_paths() {
        let (root, context) = writer("preference-readonly");
        let fields =
            update_preference(&context, PreferenceUpdate::AutoCleanup(Default::default())).unwrap();
        assert_eq!(
            fields["auto_cleanup_settings"],
            serde_json::to_value(AutoCleanupSettings::default()).unwrap()
        );
        let before = std::fs::read(context.paths.settings_file()).unwrap();
        let reader = CoreContext::read_only(context.paths.clone());
        assert!(matches!(
            update_preference(&reader, PreferenceUpdate::ShiftEnterNewline(false)),
            Err(StorageError::WritePermission(_))
        ));
        assert_eq!(
            std::fs::read(context.paths.settings_file()).unwrap(),
            before
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn embedding_preference_preserves_provider_and_normalizes_only_public_config() {
        let (root, context) = writer("preference-embedding");
        let input = serde_json::from_value::<NotebookEmbeddingSettings>(json!({
            "provider": "openAi", "base_url": "  ", "model": "\t",
            "api_key": "synthetic-never-persist",
        }))
        .unwrap();
        let fields =
            update_preference(&context, PreferenceUpdate::NotebookEmbedding(input)).unwrap();
        assert_eq!(
            fields["notebook_embedding_settings"],
            json!({
                "provider": "openAi", "base_url": "http://127.0.0.1:11434", "model": "nomic-embed-text",
            })
        );
        assert!(!std::fs::read_to_string(context.paths.settings_file())
            .unwrap()
            .contains("synthetic-never-persist"));
        let input = NotebookEmbeddingSettings {
            base_url: " https://never-contact.invalid/ ".into(),
            model: " synthetic ".into(),
            ..Default::default()
        };
        let fields =
            update_preference(&context, PreferenceUpdate::NotebookEmbedding(input)).unwrap();
        assert_eq!(
            fields["notebook_embedding_settings"]["base_url"],
            "https://never-contact.invalid/"
        );
        assert_eq!(fields["notebook_embedding_settings"]["model"], "synthetic");
        assert!(!root.join("agent-homes").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn dsh_effort_validation_uses_exact_builtin_and_current_levels() {
        assert_eq!(DSH_REASONING_EFFORTS, ["off", "high", "max"]);
        for (input, expected) in [(" OFF ", "off"), ("high", "high"), ("\tMAX\n", "max")] {
            assert_eq!(
                DshReasoningEffort::for_builtin_agent("dsh", input)
                    .unwrap()
                    .as_str(),
                expected
            );
        }
        for agent in ["", "DSH", "codex", "custom-dsh"] {
            assert!(DshReasoningEffort::for_builtin_agent(agent, "high").is_err());
        }
        for value in ["", "low", "medium", "ultra", "xhigh"] {
            assert!(DshReasoningEffort::for_builtin_agent("dsh", value).is_err());
        }
    }

    #[test]
    fn dsh_effort_changes_only_target_key_and_refuses_read_only_context() {
        let (root, context) = writer("preference-dsh");
        let original = json!({
            "unknown": {"keep": [1, 2]},
            "custom_agents": [{"id": "future", "label": "Synthetic",
                "path": "/never/run", "future_option": {"keep": true}}],
            "dsh_reasoning_efforts": {"other": "low", "future": "future-value", "dsh": "off"},
        });
        settings::save_settings(&context, original.as_object().unwrap()).unwrap();
        let update = DshReasoningEffort::for_builtin_agent("dsh", " MAX ").unwrap();
        let fields =
            update_preference(&context, PreferenceUpdate::DshReasoningEffort(update)).unwrap();
        assert_eq!(
            fields["dsh_reasoning_efforts"],
            json!({
                "other": "low", "future": "future-value", "dsh": "max",
            })
        );
        assert_eq!(fields["unknown"], original["unknown"]);
        assert_eq!(fields["custom_agents"], original["custom_agents"]);
        let before = std::fs::read(context.paths.settings_file()).unwrap();
        assert!(matches!(
            update_preference(
                &CoreContext::read_only(context.paths.clone()),
                PreferenceUpdate::DshReasoningEffort(update),
            ),
            Err(StorageError::WritePermission(_)),
        ));
        assert_eq!(
            std::fs::read(context.paths.settings_file()).unwrap(),
            before
        );
        assert!(!root.join("agent-homes").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn dsh_effort_corruption_refuses_before_replacing_original_bytes() {
        let (root, context) = writer("preference-dsh-corrupt");
        let update = DshReasoningEffort::for_builtin_agent("dsh", "high").unwrap();
        for original in [
            json!({"dsh_reasoning_efforts": false}),
            json!({"dsh_reasoning_efforts": null}),
            json!({"dsh_reasoning_efforts": ["high"]}),
            json!({"dsh_reasoning_efforts": {"other": false}}),
            json!({"custom_agents": false, "dsh_reasoning_efforts": {"dsh": "off"}}),
        ] {
            settings::save_settings(&context, original.as_object().unwrap()).unwrap();
            let before = std::fs::read(context.paths.settings_file()).unwrap();
            assert!(matches!(
                update_preference(&context, PreferenceUpdate::DshReasoningEffort(update)),
                Err(StorageError::Corrupt { .. }),
            ));
            assert_eq!(
                std::fs::read(context.paths.settings_file()).unwrap(),
                before
            );
        }
        // The exact target can be repaired without hiding corruption elsewhere.
        let original = json!({"dsh_reasoning_efforts": {"dsh": false, "other": "low"}});
        settings::save_settings(&context, original.as_object().unwrap()).unwrap();
        let fields =
            update_preference(&context, PreferenceUpdate::DshReasoningEffort(update)).unwrap();
        assert_eq!(
            fields["dsh_reasoning_efforts"],
            json!({"dsh": "high", "other": "low"})
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn proxy_preference_normalizes_public_fields_without_probing_a_proxy() {
        let (root, context) = writer("preference-proxy-normalize");
        settings::save_settings(
            &context,
            json!({"unknown": {"keep": [1, 2]}, "send_shortcut": "enter"})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        let fields = update_preference(
            &context,
            PreferenceUpdate::Proxy(ProxySettings {
                url: " 127.0.0.1:7890 ".into(),
                no_proxy: " localhost, , example.invalid,localhost ".into(),
                username: " synthetic-user ".into(),
                password: " synthetic-pass ".into(),
            }),
        )
        .unwrap();
        assert_eq!(
            fields["proxy_settings"],
            json!({
                "url": "http://127.0.0.1:7890", "no_proxy": "localhost,example.invalid,localhost",
                "username": "synthetic-user", "password": "synthetic-pass",
            })
        );
        assert_eq!(fields["unknown"], json!({"keep": [1, 2]}));
        assert_eq!(fields["send_shortcut"], "enter");
        // Scheme/endpoint validation remains the consumer's job, as before.
        let fields = update_preference(
            &context,
            PreferenceUpdate::Proxy(ProxySettings {
                url: " ftp://never-contact.invalid ".into(),
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(
            fields["proxy_settings"]["url"],
            "ftp://never-contact.invalid"
        );
        assert!(!root.join("agent-homes").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn proxy_preference_consumes_only_legacy_fields_and_empty_replacement_cannot_revive_them() {
        let (root, context) = writer("preference-proxy-legacy");
        let profiles = json!([
            {"id": " first ", "label": " First ", "path": " /never/run-first ",
                "username": "synthetic-old-user", "password": "synthetic-old-pass",
                "unknown": {"username": "not-a-legacy-field"}},
            {"id": "second", "label": "Second", "path": "/never/run-second",
                "username": "synthetic-second-user", "password": "synthetic-second-pass",
                "unknown": [1, 2, 3]},
        ]);
        let original = json!({
            "unknown": {"keep": true}, "custom_agents": profiles,
            "proxy_settings": {"url": "http://old.invalid", "password": "synthetic-global-pass"},
            "agent_proxy_enabled": {"claude": false, " OMP ": true},
            "agent_proxy_overrides": {" CLAUDE ": {"enabled": true, "url": "legacy:7890"}},
        });
        settings::save_settings(&context, original.as_object().unwrap()).unwrap();
        let fields = update_preference(
            &context,
            PreferenceUpdate::Proxy(ProxySettings {
                url: " new.invalid:7890 ".into(),
                ..Default::default()
            }),
        )
        .unwrap();
        let mut expected_profiles = profiles;
        for profile in expected_profiles.as_array_mut().unwrap() {
            let profile = profile.as_object_mut().unwrap();
            profile.remove("username");
            profile.remove("password");
        }
        assert_eq!(fields["custom_agents"], expected_profiles);
        assert_eq!(
            fields["agent_proxy_enabled"],
            json!({"claude": true, "omp": true})
        );
        assert!(!fields.contains_key("agent_proxy_overrides"));
        assert_eq!(
            fields["proxy_settings"],
            json!({
                "url": "http://new.invalid:7890", "no_proxy": "", "username": "", "password": "",
            })
        );
        let fields =
            update_preference(&context, PreferenceUpdate::Proxy(Default::default())).unwrap();
        assert_eq!(
            fields["proxy_settings"],
            serde_json::to_value(ProxySettings::default()).unwrap()
        );
        assert_eq!(fields["custom_agents"], expected_profiles);
        assert_eq!(fields["unknown"], json!({"keep": true}));
        let before = std::fs::read(context.paths.settings_file()).unwrap();
        assert!(matches!(
            update_preference(
                &CoreContext::read_only(context.paths.clone()),
                PreferenceUpdate::Proxy(Default::default()),
            ),
            Err(StorageError::WritePermission(_)),
        ));
        assert_eq!(
            std::fs::read(context.paths.settings_file()).unwrap(),
            before
        );
        // Also clear on the very first request, while all legacy inputs still
        // exist. A previous URL-only replacement must not be a prerequisite.
        settings::save_settings(&context, original.as_object().unwrap()).unwrap();
        let cleared =
            update_preference(&context, PreferenceUpdate::Proxy(Default::default())).unwrap();
        assert_eq!(
            cleared["proxy_settings"],
            serde_json::to_value(ProxySettings::default()).unwrap()
        );
        assert_eq!(cleared["custom_agents"], expected_profiles);
        assert_eq!(
            cleared["agent_proxy_enabled"],
            json!({"claude": true, "omp": true})
        );
        assert!(!cleared.contains_key("agent_proxy_overrides"));
        assert_eq!(cleared["unknown"], json!({"keep": true}));
        assert_eq!(
            settings::load_settings(&context).unwrap().found().unwrap(),
            cleared
        );
        assert!(!root.join("agent-homes").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn proxy_preference_can_repair_its_target_but_never_hides_corrupt_migration_inputs() {
        let (root, context) = writer("preference-proxy-corrupt");
        for original in [
            json!({"custom_agents": false}),
            json!({"agent_proxy_overrides": false}),
            json!({"agent_proxy_enabled": {"dsh": "true"}}),
            json!({"custom_agents": [["positional", "Synthetic", "/never/run"]]}),
            json!({"terminal_shift_enter_newline": "bad"}),
        ] {
            settings::save_settings(&context, original.as_object().unwrap()).unwrap();
            let before = std::fs::read(context.paths.settings_file()).unwrap();
            assert!(matches!(
                update_preference(&context, PreferenceUpdate::Proxy(Default::default())),
                Err(StorageError::Corrupt { .. }),
            ));
            assert_eq!(
                std::fs::read(context.paths.settings_file()).unwrap(),
                before
            );
        }
        settings::save_settings(
            &context,
            json!({"proxy_settings": null, "unknown": true})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        let fields =
            update_preference(&context, PreferenceUpdate::Proxy(Default::default())).unwrap();
        assert_eq!(
            fields["proxy_settings"],
            serde_json::to_value(ProxySettings::default()).unwrap()
        );
        assert_eq!(fields["unknown"], true);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn legacy_proxy_field_helpers_preserve_presence_order_pair_selection_and_enabled_precedence() {
        use crate::settings::agent_env::migrate_legacy_proxy_fields;
        use crate::settings::schema::LegacyAgentProxyConfig;
        use std::collections::HashMap;

        let overrides = HashMap::from([(
            "legacy".into(),
            LegacyAgentProxyConfig {
                enabled: false,
                url: " legacy:7890 ".into(),
                no_proxy: String::new(),
            },
        )]);
        let comma_only = ProxySettings {
            no_proxy: " , , ".into(),
            ..Default::default()
        };
        assert_eq!(
            migrate_legacy_proxy_fields(&comma_only, &overrides, [("", "")]),
            ProxySettings::default(),
            "raw presence must be checked before normalization",
        );
        let migrated = migrate_legacy_proxy_fields(
            &ProxySettings::default(),
            &overrides,
            [("", ""), (" first ", " "), ("second", "second-pass")],
        );
        assert_eq!(
            migrated.url, "http://legacy:7890",
            "legacy address need not be enabled"
        );
        assert_eq!(migrated.username, "first");
        assert_eq!(migrated.password, "", "credential pairs are never mixed");
        let current = ProxySettings {
            username: " current ".into(),
            ..Default::default()
        };
        let migrated = migrate_legacy_proxy_fields(&current, &overrides, [("old", "old-pass")]);
        assert_eq!(migrated.username, "current");
        assert_eq!(migrated.password, "");
        assert_eq!(migrated.url, "");

        let enabled = HashMap::from([
            ("claude".into(), false),
            (" OMP ".into(), true),
            ("".into(), true),
            ("dsh".into(), true),
        ]);
        let overrides = HashMap::from([
            (
                " CLAUDE ".into(),
                LegacyAgentProxyConfig {
                    enabled: true,
                    ..Default::default()
                },
            ),
            (
                "dsh".into(),
                LegacyAgentProxyConfig {
                    enabled: false,
                    ..Default::default()
                },
            ),
        ]);
        assert_eq!(
            migrate_agent_proxy_enabled_fields(&enabled, &overrides),
            HashMap::from([
                ("claude".into(), true),
                ("omp".into(), true),
                ("dsh".into(), true),
            ])
        );
    }
}
