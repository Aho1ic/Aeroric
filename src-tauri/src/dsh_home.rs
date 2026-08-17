//! 内建 DeepSeek Harness(dsh)的托管 home(`DSH_HOME`)管理。
//!
//! Aeroric 不触碰用户自己的 `~/.dsh`,而是托管 `~/.aeroric/agent-homes/dsh`:
//! - `settings.yaml` 与 home 层 `cordis.patch.yml`(用户可编辑)按需初始化,初始化后不再覆写;
//! - Aeroric 受管的 patch 层单独放在 `aeroric.patch.yml`,启动 dsh 时以 `--patch` 注入
//!   (dsh 的 `--patch` 覆盖层应用在 bundle/profile/home 层之后),固定两件事:
//!   会话持久化为明文 JSONL 且不打包 chunk 行(供 Aeroric 直接解析),遥测行禁用;
//! - 受管文件首行带 `# AERORIC_DSH_PATCH_VERSION=N` 标记,内容版本升级时整体重写,
//!   与 `agent_scripts.rs` 的 wrapper 版本标记机制同型,不触碰用户自有文件。

use std::fs;
use std::path::{Path, PathBuf};

/// 受管 patch 内容版本;修改 `managed_patch_content` 后必须递增。
const MANAGED_PATCH_VERSION: u32 = 1;
const MANAGED_PATCH_MARKER_PREFIX: &str = "# AERORIC_DSH_PATCH_VERSION=";
pub(crate) const MANAGED_PATCH_FILE_NAME: &str = "aeroric.patch.yml";
pub(crate) const PLUGINS_PATCH_FILE_NAME: &str = "aeroric.plugins.patch.yml";

const SETTINGS_TEMPLATE: &str = "# dsh user settings (hot-reloaded).\n\
# Provider/model sections such as `llm-deepseek:` / `llm-pi-ai:` go here.\n";

const USER_PATCH_TEMPLATE: &str = "# Your home-level dsh patch layer, applied after every bundle\n\
# and profile layer (Aeroric's managed overrides live in aeroric.patch.yml).\n\
[]\n";

/// 内建 dsh 的托管 home:`~/.aeroric/agent-homes/dsh`。
pub fn dsh_home() -> Result<PathBuf, String> {
    crate::app_settings::custom_agent_home("dsh")
}

/// dsh 族 agent 的托管 home:内建为 `agent-homes/dsh`,dsh-like 自定义档案为
/// `agent-homes/{id}`(与 codex-like 自定义的隔离 home 机制对等)。
pub fn dsh_home_for(agent: &str) -> Result<PathBuf, String> {
    crate::app_settings::custom_agent_home(agent)
}

/// 幂等初始化任意 dsh 族 agent 的托管 home,返回 home 路径。
pub fn ensure_dsh_home_for(agent: &str) -> Result<PathBuf, String> {
    let home = dsh_home_for(agent)?;
    ensure_dsh_home_at(&home)?;
    Ok(home)
}

pub fn managed_patch_path_in(home: &Path) -> PathBuf {
    home.join(MANAGED_PATCH_FILE_NAME)
}

pub fn plugins_patch_path_in(home: &Path) -> PathBuf {
    home.join(PLUGINS_PATCH_FILE_NAME)
}

/// 任务级模型覆盖 patch 在指定 home 下的路径。
pub fn task_model_patch_path_in(home: &Path, task_id: &str) -> PathBuf {
    home.join("tmp").join(format!("task-{task_id}.patch.yml"))
}

/// 在指定 home 下写入任务级模型覆盖 patch。
/// provider:官方目录为 `deepseek-official`;带自定义 base_url 的 dsh-like 档案
/// 为 Aeroric 生成的 `aeroric` provider(见 `write_custom_provider_settings`)。
pub fn write_task_model_patch_in(
    home: &Path,
    task_id: &str,
    provider: &str,
    model: &str,
) -> Result<PathBuf, String> {
    let path = task_model_patch_path_in(home, task_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = format!(
        "# Aeroric task-scoped model override (auto-deleted when the task ends)\n\
         - id: agent-default-model\n\
         \x20 config:\n\
         \x20   provider: {provider}\n\
         \x20   model: {model}\n",
        provider = yaml_quote(provider),
        model = yaml_quote(model),
    );
    fs::write(&path, content).map_err(|error| error.to_string())?;
    Ok(path)
}

pub fn dsh_settings_path() -> Result<PathBuf, String> {
    Ok(dsh_home()?.join("settings.yaml"))
}

/// 每任务模型覆盖 patch(`--patch` 覆盖层,应用在受管层之后);任务结束后删除。
/// task_id 已经过 `validate_task_id` 校验(字母数字与 `:-_.`),可安全用作文件名。
pub fn task_model_patch_path(task_id: &str) -> Result<PathBuf, String> {
    Ok(dsh_home()?
        .join("tmp")
        .join(format!("task-{task_id}.patch.yml")))
}

/// 任务结束/取消时清理模型覆盖 patch;文件不存在时为无害 no-op。
/// 自定义 dsh-like 档案的 home 也一并清理(遍历 agent-homes/*/tmp)。
pub fn cleanup_task_model_patch(task_id: &str) {
    if let Ok(path) = task_model_patch_path(task_id) {
        let _ = fs::remove_file(path);
    }
    cleanup_task_feature_patch(task_id);
    if let Some(root) =
        crate::platform::home_dir().map(|home| home.join(".aeroric").join("agent-homes"))
    {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let candidate = task_model_patch_path_in(&entry.path(), task_id);
                let _ = fs::remove_file(candidate);
            }
        }
    }
}

/// 任务级特性开关 patch 路径(`tool-web` 禁用等)。
pub fn task_feature_patch_path(task_id: &str) -> Result<PathBuf, String> {
    Ok(dsh_home()?
        .join("tmp")
        .join(format!("task-{task_id}-features.patch.yml")))
}

/// 任务级特性开关 patch 路径(指定 home)。
pub fn task_feature_patch_path_in(home: &Path, task_id: &str) -> PathBuf {
    home.join("tmp")
        .join(format!("task-{task_id}-features.patch.yml"))
}

/// 写入任务级特性开关 patch 到指定 home。
pub fn write_task_feature_patch_in(
    home: &Path,
    task_id: &str,
    features: Vec<(&str, bool)>,
) -> Result<PathBuf, String> {
    let path = task_feature_patch_path_in(home, task_id);
    write_task_feature_patch_to(&path, features)?;
    Ok(path)
}

fn write_task_feature_patch_to(path: &Path, features: Vec<(&str, bool)>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut content =
        String::from("# Aeroric task-scoped feature toggles (auto-deleted when the task ends)\n");
    for (row_id, enabled) in features {
        content.push_str(&format!(
            "- id: {}\n  disabled: {}\n",
            row_id,
            if enabled { "false" } else { "true" }
        ));
    }
    fs::write(path, content).map_err(|error| error.to_string())
}

/// 任务结束时清理特性 patch。
fn cleanup_task_feature_patch(task_id: &str) {
    if let Ok(path) = task_feature_patch_path(task_id) {
        let _ = fs::remove_file(path);
    }
    if let Some(root) =
        crate::platform::home_dir().map(|home| home.join(".aeroric").join("agent-homes"))
    {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let candidate = task_feature_patch_path_in(&entry.path(), task_id);
                let _ = fs::remove_file(candidate);
            }
        }
    }
}

/// 为 dsh-like 自定义档案生成 settings.yaml 的自定义 provider 段
/// (`llm-pi-ai`;key 经 `DEEPSEEK_API_KEY` env 引用解析)。
/// 仅在档案创建时写入;此后 settings.yaml 归用户所有(dsh 热更新)。
pub fn write_custom_provider_settings(
    home: &Path,
    base_url: &str,
    models: &[String],
    api_protocol: &str,
) -> Result<(), String> {
    let api_protocol = match api_protocol.trim() {
        "" | "openai-completions" => "openai-completions",
        "openai-responses" => "openai-responses",
        "anthropic-messages" => "anthropic-messages",
        value => {
            return Err(format!(
                "Unsupported DeepSeek Harness API protocol: {value}"
            ))
        }
    };
    let mut out = format!(
        "# dsh user settings (hot-reloaded). Provider section generated by Aeroric agent setup.\n\
         llm-pi-ai:\n\
         \x20 providers:\n\
         \x20   aeroric:\n\
         \x20     apiKeyEnv: DEEPSEEK_API_KEY\n\
         \x20     api: {api_protocol}\n",
    );
    out.push_str(&format!("      baseURL: {}\n", yaml_quote(base_url)));
    out.push_str("      models:\n");
    for model in models {
        out.push_str(&format!("        - id: {}\n", yaml_quote(model)));
    }
    fs::write(home.join("settings.yaml"), out).map_err(|error| error.to_string())
}

/// 将设置面板保存的 DEEPSEEK_API_KEY 同步进托管 home 的 `.credentials.yaml`。
///
/// 行级 upsert:仅改写本键、保留用户手工添加的其他键;`api_key` 为 None 时移除。
/// dsh 凭据分层里"继承环境"优先于该文件,headless 任务实际走 env 注入;文件同步
/// 服务于 Phase 7 常驻 `dsh web` 进程与用户直接使用 CLI 的场景。权限收紧为 0600。
///
/// 降级路径:凭据保存的主路径现在是 webui 的 `credentials.set` RPC(原子写、跨进程
/// 加锁、保留注释;见 `dsh_plugins::persist_dsh_api_key`)。当 `dsh web` 未运行或 RPC
/// 失败时,才回落到本函数的文件级直写——文件是耐久来源,后续 webui 启动会热加载它,
/// 所以降级写入与 RPC 写入最终落到同一文件、不并发(此处无锁,仅在 RPC 不可用时使用)。
pub fn sync_dsh_credentials(home: &Path, api_key: Option<&str>) -> Result<(), String> {
    let path = home.join(".credentials.yaml");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| !line.trim_start().starts_with("DEEPSEEK_API_KEY:"))
        .map(str::to_string)
        .collect();
    if let Some(key) = api_key.map(str::trim).filter(|key| !key.is_empty()) {
        lines.push(format!("DEEPSEEK_API_KEY: {}", yaml_quote(key)));
    }
    let content = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    if content == existing && path.exists() {
        return Ok(());
    }
    fs::write(&path, content).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// YAML 双引号字符串(路径可能含反斜杠、引号或非 ASCII 字符)。
fn yaml_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn managed_patch_content(sessions_root: &Path) -> String {
    format!(
        "{MANAGED_PATCH_MARKER_PREFIX}{MANAGED_PATCH_VERSION}\n\
         # Managed by Aeroric — regenerated automatically; do not edit.\n\
         # 由 Aeroric 生成并维护,请勿手工修改;自定义配置请写入同目录 cordis.patch.yml。\n\
         - id: session-persistence-jsonl\n\
         \x20 config:\n\
         \x20   root: {root}\n\
         \x20   compression: none\n\
         \x20   packChunks: false\n\
         - id: session-telemetry-otel\n\
         \x20 disabled: true\n",
        root = yaml_quote(sessions_root.to_string_lossy().as_ref()),
    )
}

fn managed_patch_marker_version(content: &str) -> Option<u32> {
    content
        .lines()
        .next()?
        .strip_prefix(MANAGED_PATCH_MARKER_PREFIX)?
        .trim()
        .parse()
        .ok()
}

pub(crate) fn ensure_dsh_home_at(home: &Path) -> Result<(), String> {
    if let Some(parent) = home.parent() {
        crate::storage::ensure_private_dir(parent)?;
    }
    crate::storage::ensure_private_dir(home)?;
    let sessions_root = home.join("sessions");
    crate::storage::ensure_private_dir(&sessions_root)
        .map_err(|error| format!("Failed to create dsh home: {error}"))?;

    let settings = home.join("settings.yaml");
    if !settings.exists() {
        crate::storage::atomic_write_private(&settings, SETTINGS_TEMPLATE)
            .map_err(|error| format!("Failed to write dsh settings.yaml: {error}"))?;
    } else {
        crate::storage::ensure_private_file_permissions(&settings)
            .map_err(|error| format!("Failed to secure dsh settings.yaml: {error}"))?;
    }

    let user_patch = home.join("cordis.patch.yml");
    if !user_patch.exists() {
        crate::storage::atomic_write_private(&user_patch, USER_PATCH_TEMPLATE)
            .map_err(|error| format!("Failed to write dsh cordis.patch.yml: {error}"))?;
    } else {
        crate::storage::ensure_private_file_permissions(&user_patch)
            .map_err(|error| format!("Failed to secure dsh cordis.patch.yml: {error}"))?;
    }

    let managed = home.join(MANAGED_PATCH_FILE_NAME);
    let current_version = fs::read_to_string(&managed)
        .ok()
        .as_deref()
        .and_then(managed_patch_marker_version);
    if current_version != Some(MANAGED_PATCH_VERSION) {
        crate::storage::atomic_write_private(&managed, &managed_patch_content(&sessions_root))
            .map_err(|error| format!("Failed to write {MANAGED_PATCH_FILE_NAME}: {error}"))?;
    } else {
        crate::storage::ensure_private_file_permissions(&managed)
            .map_err(|error| format!("Failed to secure {MANAGED_PATCH_FILE_NAME}: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("aeroric-dsh-home-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root.join("home")
    }

    fn cleanup_temp_home(home: &Path) {
        if let Some(root) = home.parent() {
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn initializes_home_idempotently_and_preserves_user_files() {
        let home = temp_home("init");
        ensure_dsh_home_at(&home).unwrap();
        assert!(home.join("sessions").is_dir());
        assert!(home.join("settings.yaml").is_file());
        assert!(home.join("cordis.patch.yml").is_file());
        let managed = fs::read_to_string(home.join(MANAGED_PATCH_FILE_NAME)).unwrap();
        assert!(managed.starts_with(MANAGED_PATCH_MARKER_PREFIX));
        assert!(managed.contains("compression: none"));
        assert!(managed.contains("packChunks: false"));
        assert!(managed.contains("session-telemetry-otel"));

        // 用户文件改动在重复初始化后保留。
        fs::write(home.join("settings.yaml"), "llm-deepseek: {}\n").unwrap();
        fs::write(
            home.join("cordis.patch.yml"),
            "- id: tool-web\n  disabled: true\n",
        )
        .unwrap();
        ensure_dsh_home_at(&home).unwrap();
        assert_eq!(
            fs::read_to_string(home.join("settings.yaml")).unwrap(),
            "llm-deepseek: {}\n"
        );
        assert!(fs::read_to_string(home.join("cordis.patch.yml"))
            .unwrap()
            .contains("tool-web"));
        cleanup_temp_home(&home);
    }

    #[test]
    fn rewrites_managed_patch_when_marker_version_differs() {
        let home = temp_home("upgrade");
        ensure_dsh_home_at(&home).unwrap();
        let managed = home.join(MANAGED_PATCH_FILE_NAME);
        fs::write(&managed, "# AERORIC_DSH_PATCH_VERSION=0\n- id: stale\n").unwrap();
        ensure_dsh_home_at(&home).unwrap();
        let content = fs::read_to_string(&managed).unwrap();
        assert!(content.starts_with(&format!(
            "{MANAGED_PATCH_MARKER_PREFIX}{MANAGED_PATCH_VERSION}"
        )));
        assert!(!content.contains("stale"));
        cleanup_temp_home(&home);
    }

    #[test]
    fn quotes_yaml_paths_with_special_characters() {
        assert_eq!(yaml_quote(r"C:\dsh home"), "\"C:\\\\dsh home\"");
        assert_eq!(yaml_quote("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn writes_supported_custom_provider_protocols() {
        let home = temp_home("provider-protocols");
        fs::create_dir_all(&home).unwrap();
        let models = vec!["model-a".to_string()];

        for protocol in [
            "openai-completions",
            "openai-responses",
            "anthropic-messages",
        ] {
            write_custom_provider_settings(&home, "https://example.com/v1", &models, protocol)
                .unwrap();
            let content = fs::read_to_string(home.join("settings.yaml")).unwrap();
            assert!(content.contains(&format!("api: {protocol}")));
            assert!(content.contains("baseURL: \"https://example.com/v1\""));
            assert!(content.contains("- id: \"model-a\""));
        }

        let error = write_custom_provider_settings(
            &home,
            "https://example.com/v1",
            &models,
            "unsupported-protocol",
        )
        .unwrap_err();
        assert!(error.contains("Unsupported DeepSeek Harness API protocol"));
        cleanup_temp_home(&home);
    }

    #[test]
    fn syncs_credentials_preserving_foreign_keys() {
        let home = temp_home("creds");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join(".credentials.yaml"),
            "OTHER_KEY: \"keep-me\"\nDEEPSEEK_API_KEY: \"old\"\n",
        )
        .unwrap();
        sync_dsh_credentials(&home, Some("sk-new")).unwrap();
        let content = fs::read_to_string(home.join(".credentials.yaml")).unwrap();
        assert!(content.contains("OTHER_KEY: \"keep-me\""));
        assert!(content.contains("DEEPSEEK_API_KEY: \"sk-new\""));
        assert!(!content.contains("old"));

        // 清空 key 时移除本键但保留其他键。
        sync_dsh_credentials(&home, None).unwrap();
        let content = fs::read_to_string(home.join(".credentials.yaml")).unwrap();
        assert!(content.contains("OTHER_KEY"));
        assert!(!content.contains("DEEPSEEK_API_KEY"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(home.join(".credentials.yaml"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        cleanup_temp_home(&home);
    }
}
