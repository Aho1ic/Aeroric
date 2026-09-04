//! 内建 DeepSeek Harness(dsh)的托管 home(`DSH_HOME`)管理。
//!
//! Aeroric 不触碰用户自己的 `~/.dsh`,而是托管 `~/.aeroric/agent-homes/dsh`:
//! - `settings.yaml` 按需初始化,初始化后不再覆写;
//! - Aeroric 受管的 patch 层单独放在 `aeroric.patch.yml`,启动 dsh 时以 `--patch` 注入
//!   (dsh 的 `--patch` 覆盖层应用在 bundle/profile/home 层之后),固定两件事:
//!   会话持久化为明文 JSONL 且不打包 chunk 行(供 Aeroric 直接解析),遥测行禁用;
//! - home 层 `cordis.patch.yml` 归用户,但里面留一个标记界定的受管区块:`dsh web`
//!   拒收父进程的 `--patch`,Web 会话只认 home 层,持久化设置必须从这里下去。
//!   区块外的用户条目原样保留,且排在区块之后(后写覆盖前面的),用户改得回来;
//! - 受管文件首行带 `# AERORIC_DSH_PATCH_VERSION=N` 标记,内容版本升级时整体重写,
//!   与 `agent_scripts.rs` 的 wrapper 版本标记机制同型,不触碰用户自有文件。

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 受管 patch 内容版本;修改 `managed_patch_content` 后必须递增。
const MANAGED_PATCH_VERSION: u32 = 1;
const MANAGED_PATCH_MARKER_PREFIX: &str = "# AERORIC_DSH_PATCH_VERSION=";

/// home 层受管区块的内容版本;修改 `home_patch_block` 后必须递增。
const HOME_PATCH_BLOCK_VERSION: u32 = 1;
const HOME_PATCH_BEGIN: &str = "# >>> AERORIC MANAGED BLOCK";
const HOME_PATCH_END: &str = "# <<< AERORIC MANAGED BLOCK";
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
    crate::storage::atomic_write_private(&path, &content)?;
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
    crate::storage::atomic_write_private(path, &content)
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
    let path = home.join("settings.yaml");
    reject_symlink_target(&path, "dsh settings.yaml")?;
    crate::storage::atomic_write_private(&path, &out)
}

/// Refresh Aeroric's custom provider after access/model edits while preserving
/// the DSH API protocol chosen when the profile was created.
pub fn refresh_custom_provider_settings(
    home: &Path,
    base_url: &str,
    models: &[String],
) -> Result<(), String> {
    if base_url.trim().is_empty() || models.is_empty() {
        return Ok(());
    }
    let protocol = fs::read_to_string(home.join("settings.yaml"))
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                let value = line.trim().strip_prefix("api:")?.trim();
                matches!(
                    value,
                    "openai-completions" | "openai-responses" | "anthropic-messages"
                )
                .then(|| value.to_string())
            })
        })
        .unwrap_or_else(|| "openai-completions".to_string());
    write_custom_provider_settings(home, base_url, models, &protocol)
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
    reject_symlink_target(&path, "dsh credentials")?;
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
    crate::storage::atomic_write_private(&path, &content)
}

fn reject_symlink_target(path: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "Refusing to follow symlink for {label}: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Cannot inspect {label} {}: {error}",
            path.display()
        )),
    }
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

/// home 层 `cordis.patch.yml` 里的受管区块。
///
/// `dsh web` 会拒绝父进程传进来的 `--patch`,所以 `aeroric.patch.yml` 那一层
/// 只对 PTY 会话生效,Web 会话完全看不到——会话持久化因此仍按插件默认的 zstd
/// 落盘。要让 Web 会话也听话,唯一能到达它的就是 home 层这个文件。
///
/// `config` 是整体替换(`applyEntryPatches` 对每个 key 做浅赋值),所以 `root`
/// 必须一起重述,漏掉它插件会因为缺少必填项直接起不来。
fn home_patch_block(sessions_root: &Path) -> String {
    format!(
        "{HOME_PATCH_BEGIN} v{HOME_PATCH_BLOCK_VERSION} >>>\n\
         # Managed by Aeroric — regenerated automatically; do not edit inside this block.\n\
         # 由 Aeroric 维护,请勿修改本区块;自定义条目写在区块外面即可(后写的覆盖前面的)。\n\
         - id: session-persistence-jsonl\n\
         \x20 config:\n\
         \x20   root: {root}\n\
         \x20   compression: none\n\
         \x20   packChunks: false\n\
         {HOME_PATCH_END} v{HOME_PATCH_BLOCK_VERSION} <<<\n",
        root = yaml_quote(sessions_root.to_string_lossy().as_ref()),
    )
}

/// 摘掉任意版本的受管区块,返回用户自己那部分。
///
/// 起止标记都按前缀匹配,旧版本区块一样会被摘干净;缺了结束标记就一路吃到
/// 文件尾——宁可少留几行用户内容,也不能把半个区块留在文件里。
fn strip_home_patch_block(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut inside = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !inside && trimmed.starts_with(HOME_PATCH_BEGIN) {
            inside = true;
            continue;
        }
        if inside {
            if trimmed.starts_with(HOME_PATCH_END) {
                inside = false;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// 用户那部分是不是"什么条目都没有"。
///
/// 初始模板写的是流式空序列 `[]`,它和后面的块式条目拼不到一起,得整行丢掉;
/// 注释和空行则原样留着。
fn home_patch_user_entries(rest: &str) -> Option<String> {
    let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(rest).ok()?;
    let empty = match &parsed {
        serde_yaml_ng::Value::Null => true,
        serde_yaml_ng::Value::Sequence(items) => items.is_empty(),
        // 不是序列的内容不认识,交回原文由调用方去校验合成结果。
        _ => false,
    };
    if !empty {
        return Some(rest.to_string());
    }
    let comments: String = rest
        .lines()
        .filter(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
        .fold(String::new(), |mut acc, line| {
            acc.push_str(line);
            acc.push('\n');
            acc
        });
    Some(comments)
}

/// 合成 home 层 patch 全文:受管区块在前,用户条目在后。
///
/// 顺序是有意的——同一个文件里后写的条目覆盖前面的,所以用户想改回压缩格式
/// 随时能改(读取侧两种编码都认)。返回 `None` 表示合成结果不是合法的 patch
/// 序列,调用方应当原样保留用户文件。
fn compose_home_patch(existing: &str, sessions_root: &Path) -> Option<String> {
    let rest = strip_home_patch_block(existing);
    let user = home_patch_user_entries(&rest)?;
    let mut composed = home_patch_block(sessions_root);
    let user = user.trim_end();
    if !user.is_empty() {
        composed.push_str(user);
        composed.push('\n');
    }
    // 写盘前校验:用户可能留了流式序列或缩进异常,拼出来未必还是合法序列。
    let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&composed).ok()?;
    matches!(parsed, serde_yaml_ng::Value::Sequence(_)).then_some(composed)
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

/// 迁移后给旧压缩产物留的备份后缀。
///
/// 插件只按精确文件名探测编码(`session.jsonl` / `session.jsonl.zstd`),project
/// 层的扁平布局检查也只看这两个后缀,所以带 `.bak` 的文件对它完全隐形;Aeroric
/// 侧 `dsh_transcript_in` 同样只匹配精确名。改名而不删除,迁移永不丢数据。
const MIGRATED_ZSTD_SUFFIX: &str = ".bak";

/// 把 root 里遗留的 zstd 会话产物对齐到受管的 `compression: none`。
///
/// 插件的 `session-persistence-jsonl` 在 load/list/save/delete 之前都会跑一遍
/// **整个 root** 的编码校验,只要发现一个与当前 `compression` 相反后缀的产物就
/// 直接抛错("use a separate root or select the matching compression mode")。
/// Aeroric 的受管 patch 固定要求明文,但 patch 生效之前(或旧版本 web 会话)落
/// 下的产物仍是 zstd——它们会把这个 home 的 dsh 永久堵死。读取端两种编码都认,
/// 写入端不认,所以必须在启动前把磁盘对齐。
///
/// best-effort:单个会话失败只告警,不让 home 初始化连带失败(否则设置页与任务
/// 启动会一起挂掉)。迁移后 root 里不再有 `.zstd`,重复调用是纯 readdir 空转。
fn migrate_sessions_encoding(sessions_root: &Path) {
    let Ok(projects) = fs::read_dir(sessions_root) else {
        return;
    };
    for project in projects.flatten() {
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(sessions) = fs::read_dir(project.path()) else {
            continue;
        };
        for session in sessions.flatten() {
            if !session.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            if let Err(error) = migrate_session_dir(&session.path()) {
                eprintln!(
                    "dsh session encoding migration failed for {}: {error}",
                    session.path().display()
                );
            }
        }
    }
}

/// 单个会话目录的编码迁移。
///
/// 明文已存在时不覆盖(与 `dsh_transcript_in` 的"明文优先"一致),只把压缩产物
/// 移开。解压失败(产物损坏/截断)时同样移开:留着它会让整个 home 起不来,而单
/// 个坏会话本来也读不出内容,备份文件仍在磁盘上可供事后取证。
fn migrate_session_dir(session_dir: &Path) -> Result<(), String> {
    let compressed = session_dir.join(crate::session_dsh::DSH_TRANSCRIPT_ZSTD);
    if !compressed.is_file() {
        return Ok(());
    }
    let raw = session_dir.join(crate::session_dsh::DSH_TRANSCRIPT_RAW);
    let decoded = if raw.exists() {
        None
    } else {
        match decode_zstd_transcript(&compressed) {
            Ok(text) => Some(text),
            Err(error) => {
                eprintln!(
                    "dsh transcript {} could not be decompressed ({error}); moving it aside",
                    compressed.display()
                );
                None
            }
        }
    };
    if let Some(text) = decoded {
        crate::storage::atomic_write_private(&raw, &text)
            .map_err(|error| format!("failed to write {}: {error}", raw.display()))?;
    }
    let backup = backup_path_for(&compressed);
    fs::rename(&compressed, &backup)
        .map_err(|error| format!("failed to archive {}: {error}", compressed.display()))
}

/// 解出压缩 transcript 的全文。
///
/// 物理格式是"多个独立 zstd frame 串接",`Decoder` 默认一路串到 EOF,正好对上
/// (加 `single_frame()` 只能拿到 header 帧),与 `session_dsh` 的读法同源。
fn decode_zstd_transcript(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut text = String::new();
    zstd::stream::read::Decoder::new(file)
        .map_err(|error| error.to_string())?
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    Ok(text)
}

/// 备份路径;`.bak` 已被占用时追加序号,不覆盖上一次迁移留下的存档。
fn backup_path_for(compressed: &Path) -> PathBuf {
    let base = compressed.with_file_name(format!(
        "{}{MIGRATED_ZSTD_SUFFIX}",
        crate::session_dsh::DSH_TRANSCRIPT_ZSTD
    ));
    if !base.exists() {
        return base;
    }
    for index in 2u32.. {
        let candidate = compressed.with_file_name(format!(
            "{}{MIGRATED_ZSTD_SUFFIX}.{index}",
            crate::session_dsh::DSH_TRANSCRIPT_ZSTD
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
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
    reject_symlink_target(&settings, "dsh settings.yaml")?;
    if !settings.exists() {
        crate::storage::atomic_write_private(&settings, SETTINGS_TEMPLATE)
            .map_err(|error| format!("Failed to write dsh settings.yaml: {error}"))?;
    } else {
        crate::storage::ensure_private_file_permissions(&settings)
            .map_err(|error| format!("Failed to secure dsh settings.yaml: {error}"))?;
    }

    let user_patch = home.join("cordis.patch.yml");
    reject_symlink_target(&user_patch, "dsh cordis.patch.yml")?;
    let existing = fs::read_to_string(&user_patch).unwrap_or_else(|_| USER_PATCH_TEMPLATE.into());
    match compose_home_patch(&existing, &sessions_root) {
        Some(composed) if composed != existing => {
            crate::storage::atomic_write_private(&user_patch, &composed)
                .map_err(|error| format!("Failed to write dsh cordis.patch.yml: {error}"))?;
        }
        // 合成结果没变,或者用户文件解析不出来:都不动它,只把权限收紧。
        _ => {
            if user_patch.exists() {
                crate::storage::ensure_private_file_permissions(&user_patch)
                    .map_err(|error| format!("Failed to secure dsh cordis.patch.yml: {error}"))?;
            } else {
                crate::storage::atomic_write_private(&user_patch, USER_PATCH_TEMPLATE)
                    .map_err(|error| format!("Failed to write dsh cordis.patch.yml: {error}"))?;
            }
        }
    }

    let managed = home.join(MANAGED_PATCH_FILE_NAME);
    reject_symlink_target(&managed, MANAGED_PATCH_FILE_NAME)?;
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

    // patch 落盘之后再对齐磁盘产物:受管层要求明文,root 里不能留下 zstd 产物,
    // 否则插件自己的编码校验会把这个 home 的会话读写全部拒掉。
    migrate_sessions_encoding(&sessions_root);
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
    fn writes_the_home_patch_block_that_dsh_web_can_actually_see() {
        let home = temp_home("home-block");
        ensure_dsh_home_at(&home).unwrap();
        let patch = fs::read_to_string(home.join("cordis.patch.yml")).unwrap();
        // `dsh web` 拒收 --patch,持久化设置只能从 home 层下去。
        assert!(patch.contains(HOME_PATCH_BEGIN));
        assert!(patch.contains(HOME_PATCH_END));
        assert!(patch.contains("compression: none"));
        assert!(patch.contains("packChunks: false"));
        // config 是整体替换,root 必须一起重述。
        assert!(patch.contains("root: "));
        assert!(!patch.contains("[]"));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&patch).unwrap();
        assert_eq!(parsed.as_sequence().map(Vec::len), Some(1));
        cleanup_temp_home(&home);
    }

    #[test]
    fn keeps_home_patch_block_single_and_preserves_user_entries() {
        let home = temp_home("home-merge");
        ensure_dsh_home_at(&home).unwrap();
        let patch_path = home.join("cordis.patch.yml");
        let first = fs::read_to_string(&patch_path).unwrap();
        // 重复初始化不追加第二个区块。
        ensure_dsh_home_at(&home).unwrap();
        assert_eq!(fs::read_to_string(&patch_path).unwrap(), first);

        fs::write(
            &patch_path,
            format!("{first}- id: tool-web\n  disabled: true\n"),
        )
        .unwrap();
        ensure_dsh_home_at(&home).unwrap();
        let merged = fs::read_to_string(&patch_path).unwrap();
        assert_eq!(merged.matches(HOME_PATCH_BEGIN).count(), 1);
        assert!(merged.contains("tool-web"));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&merged).unwrap();
        let items = parsed.as_sequence().expect("patch is a sequence");
        assert_eq!(items.len(), 2);
        // 用户条目排在受管区块之后,后写的覆盖前面的。
        assert_eq!(items[1]["id"].as_str(), Some("tool-web"));
        cleanup_temp_home(&home);
    }

    #[test]
    fn replaces_a_stale_home_patch_block_version() {
        let home = temp_home("home-old");
        ensure_dsh_home_at(&home).unwrap();
        let patch_path = home.join("cordis.patch.yml");
        fs::write(
            &patch_path,
            "# >>> AERORIC MANAGED BLOCK v0 >>>\n- id: outdated\n\
             # <<< AERORIC MANAGED BLOCK v0 <<<\n- id: tool-web\n  disabled: true\n",
        )
        .unwrap();
        ensure_dsh_home_at(&home).unwrap();
        let merged = fs::read_to_string(&patch_path).unwrap();
        assert_eq!(merged.matches(HOME_PATCH_BEGIN).count(), 1);
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&merged).unwrap();
        let items = parsed.as_sequence().expect("patch is a sequence");
        let ids: Vec<_> = items
            .iter()
            .filter_map(|item| item["id"].as_str())
            .collect();
        assert_eq!(ids, vec!["session-persistence-jsonl", "tool-web"]);
        cleanup_temp_home(&home);
    }

    #[test]
    fn leaves_an_unparsable_home_patch_alone() {
        let home = temp_home("home-broken");
        ensure_dsh_home_at(&home).unwrap();
        let patch_path = home.join("cordis.patch.yml");
        let broken = "- id: tool-web\n   bad: [indent\n";
        fs::write(&patch_path, broken).unwrap();
        // 用户文件解析不出来时宁可不生效,也不能把它覆盖掉。
        ensure_dsh_home_at(&home).unwrap();
        assert_eq!(fs::read_to_string(&patch_path).unwrap(), broken);
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
    fn refreshes_custom_provider_without_changing_its_protocol() {
        let home = temp_home("refresh-provider");
        ensure_dsh_home_at(&home).unwrap();
        write_custom_provider_settings(
            &home,
            "https://old.example/v1",
            &["old-model".to_string()],
            "anthropic-messages",
        )
        .unwrap();

        refresh_custom_provider_settings(
            &home,
            "https://new.example/v1",
            &["deepseek-v4-pro".to_string()],
        )
        .unwrap();
        let settings = fs::read_to_string(home.join("settings.yaml")).unwrap();
        assert!(settings.contains("api: anthropic-messages"));
        assert!(settings.contains("https://new.example/v1"));
        assert!(settings.contains("deepseek-v4-pro"));
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

    #[cfg(unix)]
    #[test]
    fn credential_sync_rejects_symlinks_without_touching_the_target() {
        use std::os::unix::fs::symlink;

        let home = temp_home("credential-symlink");
        fs::create_dir_all(&home).unwrap();
        let outside = home.join("outside.yaml");
        let credentials = home.join(".credentials.yaml");
        fs::write(&outside, "DEEPSEEK_API_KEY: \"outside\"\n").unwrap();
        symlink(&outside, &credentials).unwrap();

        let error = sync_dsh_credentials(&home, Some("inside"))
            .expect_err("credential symlinks must fail closed");
        assert!(error.contains("symlink"), "unexpected error: {error}");
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "DEEPSEEK_API_KEY: \"outside\"\n"
        );

        cleanup_temp_home(&home);
    }

    /// 造一个和插件物理格式一致的压缩 transcript:header 一帧,正文一帧,串接。
    fn write_compressed_session(session_dir: &Path, header: &str, body: &str) -> PathBuf {
        fs::create_dir_all(session_dir).unwrap();
        let mut bytes = zstd::encode_all(format!("{header}\n").as_bytes(), 0).unwrap();
        bytes.extend(zstd::encode_all(format!("{body}\n").as_bytes(), 0).unwrap());
        let path = session_dir.join(crate::session_dsh::DSH_TRANSCRIPT_ZSTD);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn session_dir_in(home: &Path, name: &str) -> PathBuf {
        home.join("sessions").join("--project--").join(name)
    }

    #[test]
    fn migrates_legacy_compressed_transcripts_to_plaintext() {
        let home = temp_home("encoding-migrate");
        ensure_dsh_home_at(&home).unwrap();
        let session = session_dir_in(&home, "session-1");
        let compressed = write_compressed_session(
            &session,
            r#"{"type":"session","version":0}"#,
            r#"{"type":"message","seq":1}"#,
        );

        ensure_dsh_home_at(&home).unwrap();

        // 受管层要求明文,root 里不能再留下插件会拒绝的相反后缀。
        assert!(!compressed.exists());
        let raw = session.join(crate::session_dsh::DSH_TRANSCRIPT_RAW);
        let text = fs::read_to_string(&raw).unwrap();
        assert_eq!(
            text,
            "{\"type\":\"session\",\"version\":0}\n{\"type\":\"message\",\"seq\":1}\n"
        );
        // 压缩产物只是改名归档,不删除。
        assert!(session
            .join(format!(
                "{}{MIGRATED_ZSTD_SUFFIX}",
                crate::session_dsh::DSH_TRANSCRIPT_ZSTD
            ))
            .is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&raw).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        cleanup_temp_home(&home);
    }

    #[test]
    fn keeps_existing_plaintext_when_both_encodings_are_present() {
        let home = temp_home("encoding-both");
        ensure_dsh_home_at(&home).unwrap();
        let session = session_dir_in(&home, "session-2");
        let compressed = write_compressed_session(&session, "compressed-header", "compressed-body");
        let raw = session.join(crate::session_dsh::DSH_TRANSCRIPT_RAW);
        fs::write(&raw, "plaintext-wins\n").unwrap();

        ensure_dsh_home_at(&home).unwrap();

        // 明文优先(与 dsh_transcript_in 一致),压缩产物只是移开。
        assert_eq!(fs::read_to_string(&raw).unwrap(), "plaintext-wins\n");
        assert!(!compressed.exists());
        assert!(session
            .join(format!(
                "{}{MIGRATED_ZSTD_SUFFIX}",
                crate::session_dsh::DSH_TRANSCRIPT_ZSTD
            ))
            .is_file());
        cleanup_temp_home(&home);
    }

    #[test]
    fn archives_corrupt_compressed_transcripts_instead_of_blocking_the_home() {
        let home = temp_home("encoding-corrupt");
        ensure_dsh_home_at(&home).unwrap();
        let session = session_dir_in(&home, "session-3");
        fs::create_dir_all(&session).unwrap();
        let compressed = session.join(crate::session_dsh::DSH_TRANSCRIPT_ZSTD);
        fs::write(&compressed, b"not really zstd").unwrap();

        ensure_dsh_home_at(&home).unwrap();

        // 坏产物留在原地会让整个 home 的 dsh 永久起不来,所以一律移开归档。
        assert!(!compressed.exists());
        assert!(!session
            .join(crate::session_dsh::DSH_TRANSCRIPT_RAW)
            .exists());
        assert!(session
            .join(format!(
                "{}{MIGRATED_ZSTD_SUFFIX}",
                crate::session_dsh::DSH_TRANSCRIPT_ZSTD
            ))
            .is_file());
        cleanup_temp_home(&home);
    }

    #[test]
    fn session_encoding_migration_is_idempotent_and_keeps_earlier_archives() {
        let home = temp_home("encoding-idempotent");
        ensure_dsh_home_at(&home).unwrap();
        let session = session_dir_in(&home, "session-4");
        write_compressed_session(&session, "header-a", "body-a");
        ensure_dsh_home_at(&home).unwrap();
        let raw = session.join(crate::session_dsh::DSH_TRANSCRIPT_RAW);
        let first = fs::read_to_string(&raw).unwrap();

        // 空转一次不改动任何东西。
        ensure_dsh_home_at(&home).unwrap();
        assert_eq!(fs::read_to_string(&raw).unwrap(), first);

        // 又出现一个压缩产物时,上一次的存档不能被覆盖。
        write_compressed_session(&session, "header-b", "body-b");
        ensure_dsh_home_at(&home).unwrap();
        assert_eq!(fs::read_to_string(&raw).unwrap(), first);
        let archive = format!(
            "{}{MIGRATED_ZSTD_SUFFIX}",
            crate::session_dsh::DSH_TRANSCRIPT_ZSTD
        );
        assert!(session.join(&archive).is_file());
        assert!(session.join(format!("{archive}.2")).is_file());
        cleanup_temp_home(&home);
    }
}
