use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};
use tar::Archive;
use uuid::Uuid;

use crate::storage::{
    aeroric_dir, atomic_write, ensure_aeroric_dirs, load_projects, update_projects, Project,
};

/// 未配置技能库时使用的默认目录名（位于 `~/.aeroric/` 下）。
const DEFAULT_HUB_DIR_NAME: &str = "skills_hub";

const SKILLS_SH_ORIGIN: &str = "https://skills.sh";
const GITHUB_API_ORIGIN: &str = "https://api.github.com";
const MARKETPLACE_PAGE_SIZE: usize = 12;
const MARKETPLACE_CACHE_MAX_AGE_MS: i64 = 30 * 60 * 1000;
const SKILLS_SH_PAGE_SIZE: usize = 200;
const MARKETPLACE_MAX_ARCHIVE_FILES: usize = 5_000;
const MARKETPLACE_MAX_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
const MARKETPLACE_MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MARKETPLACE_MAX_SKILL_FILES: usize = 1_000;
const MARKETPLACE_MAX_SKILL_BYTES: u64 = 20 * 1024 * 1024;

static MARKETPLACE_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(24))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

/// Read a GitHub personal access token from `GITHUB_TOKEN` env var.
/// Returns `None` when the variable is absent or empty.
fn github_auth_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

// ── Data types ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillHubConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// SKILL 目录名（权威标识）
    pub name: String,
    /// frontmatter 的 name 字段，可与目录名不同
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// 解析后的 description（保留换行）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// skill 目录绝对路径
    pub path: String,
    /// frontmatter 解析失败时的错误描述
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallation {
    pub skill_name: String,
    pub project_id: String,
    pub agent: String,
    pub installed_at: i64,
    pub link_path: String,
    pub target_path: String,
    #[serde(default = "default_install_kind")]
    pub install_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>, // "ok" | "broken" | "diverged"
}

fn default_install_kind() -> String {
    "symlink".to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct InstallationsFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    installations: Vec<SkillInstallation>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SetHubResult {
    pub config: SkillHubConfig,
    pub project: Project,
    pub created_new_project: bool,
    /// 后端写入后的完整 projects 列表；前端用它替换 React state，避免竞态覆盖。
    pub projects: Vec<Project>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    /// "directory" | "file" | "symlink"
    pub existing_kind: String,
    /// 当现有路径是 symlink 时，这里是它指向的目标
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_target: Option<String>,
    pub link_path: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<ConflictInfo>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub already_installed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cancelled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installation: Option<SkillInstallation>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub ok: bool,
    pub removed_links: usize,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MarketplaceSort {
    Downloads,
    Stars,
    #[default]
    Installs,
    Updated,
    Published,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MarketplaceCategory {
    #[default]
    All,
    Agents,
    Integrations,
    Automation,
    Operations,
    Security,
    Research,
    Development,
    Finance,
    Lifestyle,
    Productivity,
    Other,
    Communication,
    Creative,
    Knowledge,
}

impl MarketplaceCategory {
    fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Agents => "agents",
            Self::Integrations => "integrations",
            Self::Automation => "automation",
            Self::Operations => "operations",
            Self::Security => "security",
            Self::Research => "research",
            Self::Development => "development",
            Self::Finance => "finance",
            Self::Lifestyle => "lifestyle",
            Self::Productivity => "productivity",
            Self::Other => "other",
            Self::Communication => "communication",
            Self::Creative => "creative",
            Self::Knowledge => "knowledge",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSkill {
    pub id: String,
    pub source: String,
    pub skill_id: String,
    pub name: String,
    pub publisher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_avatar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub latest_version: String,
    pub latest_ref: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub downloads_7d: u64,
    #[serde(default)]
    pub total_installs: u64,
    #[serde(default)]
    pub stars: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub install_status: String,
    #[serde(default)]
    pub is_official: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePage {
    pub items: Vec<MarketplaceSkill>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    pub has_more: bool,
    #[serde(default)]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceInstallRecord {
    pub source: String,
    pub skill_id: String,
    pub skill_name: String,
    pub version: String,
    pub git_ref: String,
    pub installed_at: i64,
    pub target_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct MarketplaceInstallationsFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    installations: Vec<MarketplaceInstallRecord>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct SkillsShSkill {
    source: String,
    skill_id: String,
    name: String,
    #[serde(default)]
    installs: u64,
    #[serde(default)]
    weekly_installs: Vec<u64>,
    #[serde(default)]
    is_official: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SkillsShResponse {
    #[serde(default)]
    skills: Vec<SkillsShSkill>,
    #[serde(default)]
    total: usize,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    count: usize,
}

struct FetchedSkillsSh {
    skills: Vec<SkillsShSkill>,
    total: usize,
    has_more: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubOwner {
    #[serde(default)]
    login: String,
    avatar_url: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubRepository {
    #[serde(default)]
    html_url: String,
    description: Option<String>,
    #[serde(default)]
    stargazers_count: u64,
    #[serde(default)]
    default_branch: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    pushed_at: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    owner: GithubOwner,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubTreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubTree {
    #[serde(default)]
    tree: Vec<GithubTreeEntry>,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubRelease {
    #[serde(default)]
    tag_name: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubTag {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubCommitAuthor {
    #[serde(default)]
    date: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubCommitDetails {
    #[serde(default)]
    committer: GithubCommitAuthor,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct GithubCommit {
    #[serde(default)]
    sha: String,
    #[serde(default)]
    commit: GithubCommitDetails,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct MarketplaceCacheEntry {
    key: String,
    fetched_at: i64,
    page: MarketplacePage,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct MarketplaceCacheFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    entries: Vec<MarketplaceCacheEntry>,
}

// ── Path helpers ─────────────────────────────────────────────────────────────

fn skill_hub_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("skill_hub.json"))
}

fn installations_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("skill_installations.json"))
}

fn marketplace_installations_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("marketplace_skill_installations.json"))
}

fn marketplace_cache_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("marketplace_cache.json"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn agent_skills_dir(project_path: &Path, agent: &str) -> PathBuf {
    let sub = match agent {
        "codex" => ".codex/skills",
        _ => ".claude/skills",
    };
    project_path.join(sub)
}

/// skill_name 必须是单段合法目录名：非空、非 `.` / `..`、不含路径分隔符。
/// 该名字会作为 `agent_skills_dir(...).join(&skill_name)` 的最后一段，必须严格限定。
fn validate_skill_name(skill_name: &str) -> Result<(), String> {
    if skill_name.is_empty() {
        return Err("Skill name cannot be empty".to_string());
    }
    if skill_name == "." || skill_name == ".." {
        return Err(format!("Invalid skill name: {}", skill_name));
    }
    if skill_name.contains('/') || skill_name.contains('\\') || skill_name.contains('\0') {
        return Err(format!(
            "Skill name must not contain path separators: {}",
            skill_name
        ));
    }
    Ok(())
}

fn target_health(target: &Path) -> &'static str {
    if target.exists() {
        "ok"
    } else {
        "broken"
    }
}

// ── Hub config I/O ───────────────────────────────────────────────────────────

fn load_hub_config_internal() -> SkillHubConfig {
    let Ok(path) = skill_hub_path() else {
        return SkillHubConfig::default();
    };
    if !path.exists() {
        return SkillHubConfig::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<SkillHubConfig>(&raw).ok())
        .unwrap_or_default()
}

fn save_hub_config_internal(config: &SkillHubConfig) -> Result<(), String> {
    ensure_aeroric_dirs()?;
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    atomic_write(&skill_hub_path()?, &raw)
}

/// 默认技能库目录：`~/.aeroric/skills_hub`。
fn default_hub_dir() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join(DEFAULT_HUB_DIR_NAME))
}

/// 读取 hub 配置；未配置 `hub_path` 时落到默认目录（按需创建并持久化）。
///
/// 已有 `hub_path` 一律原样保留——包括指向已删除目录的情况，用户手动选择的路径
/// 不能被默认值悄悄顶掉。只有真正“空配置”才会补默认值。
fn load_hub_config_or_default() -> SkillHubConfig {
    let config = load_hub_config_internal();
    if config
        .hub_path
        .as_deref()
        .is_some_and(|path| !path.trim().is_empty())
    {
        return config;
    }

    let Ok(dir) = default_hub_dir() else {
        return config;
    };
    if fs::create_dir_all(&dir).is_err() {
        return config;
    }

    let next = SkillHubConfig {
        hub_project_id: config.hub_project_id.clone(),
        hub_path: Some(dir.to_string_lossy().into_owned()),
        created_at: config.created_at.or_else(|| Some(now_ms())),
    };
    // 持久化失败不影响本次返回值：前端仍能用默认目录，下次启动会再试。
    let _ = save_hub_config_internal(&next);
    next
}

fn load_installations_internal() -> InstallationsFile {
    let Ok(path) = installations_path() else {
        return InstallationsFile::default();
    };
    if !path.exists() {
        return InstallationsFile::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<InstallationsFile>(&raw).ok())
        .unwrap_or_default()
}

fn save_installations_internal(file: &InstallationsFile) -> Result<(), String> {
    ensure_aeroric_dirs()?;
    let raw = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    atomic_write(&installations_path()?, &raw)
}

fn load_marketplace_installations_internal() -> MarketplaceInstallationsFile {
    let Ok(path) = marketplace_installations_path() else {
        return MarketplaceInstallationsFile::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_marketplace_installations_internal(
    file: &MarketplaceInstallationsFile,
) -> Result<(), String> {
    ensure_aeroric_dirs()?;
    let raw = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    atomic_write(&marketplace_installations_path()?, &raw)
}

fn load_marketplace_cache_internal() -> MarketplaceCacheFile {
    let Ok(path) = marketplace_cache_path() else {
        return MarketplaceCacheFile::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_marketplace_cache_page(key: &str, page: &MarketplacePage) -> Result<(), String> {
    ensure_aeroric_dirs()?;
    let mut cache = load_marketplace_cache_internal();
    cache.version = 1;
    cache.entries.retain(|entry| entry.key != key);
    cache.entries.push(MarketplaceCacheEntry {
        key: key.to_string(),
        fetched_at: now_ms(),
        page: page.clone(),
    });
    cache
        .entries
        .sort_by(|a, b| b.fetched_at.cmp(&a.fetched_at));
    cache.entries.truncate(24);
    let raw = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())?;
    atomic_write(&marketplace_cache_path()?, &raw)
}

fn cached_marketplace_page(key: &str, allow_expired: bool) -> Option<MarketplacePage> {
    let cache = load_marketplace_cache_internal();
    let entry = cache.entries.into_iter().find(|entry| entry.key == key)?;
    if !allow_expired && now_ms().saturating_sub(entry.fetched_at) > MARKETPLACE_CACHE_MAX_AGE_MS {
        return None;
    }
    Some(entry.page)
}

// ── SKILL.md frontmatter parsing ─────────────────────────────────────────────
// 手写解析器，只关心 frontmatter 顶层 `name` 和 `description`。
// 支持：单行（含引号）、literal block (`|`、`|-`、`|+`)、folded (`>`、`>-`、`>+`)。

fn strip_yaml_quotes(s: &str) -> String {
    let trimmed = s.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if first == last && (first == b'"' || first == b'\'') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

/// 解析 YAML literal block scalar 的多行内容。
/// `lines` 是块下方的全部候选行；返回 (拼接后的内容, 消耗的行数)。
fn parse_block_scalar(lines: &[&str], folded: bool) -> (String, usize) {
    // 确定基准缩进（第一个非空行的前导空格数）
    let mut base_indent: Option<usize> = None;
    let mut consumed = 0usize;
    let mut collected: Vec<String> = Vec::new();

    for line in lines {
        // 空行：始终归属当前块
        if line.trim().is_empty() {
            collected.push(String::new());
            consumed += 1;
            continue;
        }
        let leading = line.chars().take_while(|c| *c == ' ').count();
        // 顶层 key 一定从第 0 列开始；只要后续行没缩进就视为块结束
        if leading == 0 {
            break;
        }
        let base = *base_indent.get_or_insert(leading);
        if leading < base {
            break;
        }
        collected.push(line[base..].to_string());
        consumed += 1;
    }

    // 去掉块末尾的空行（默认 clip 行为）
    while collected.last().map(|s| s.is_empty()).unwrap_or(false) {
        collected.pop();
    }

    let joined = if folded {
        fold_lines(&collected)
    } else {
        collected.join("\n")
    };
    (joined, consumed)
}

/// YAML folded scalar 规则：
/// - 相邻非空行用空格连接
/// - 单个空行变成一个换行
/// - 多个连续空行 → n-1 个换行
fn fold_lines(lines: &[String]) -> String {
    let mut out = String::new();
    let mut prev_blank = false;
    let mut first = true;
    for line in lines {
        if line.is_empty() {
            if first {
                first = false;
                prev_blank = true;
                continue;
            }
            out.push('\n');
            prev_blank = true;
            continue;
        }
        if !first && !prev_blank {
            out.push(' ');
        }
        out.push_str(line);
        first = false;
        prev_blank = false;
    }
    out
}

#[derive(Default)]
struct ParsedFrontmatter {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
}

fn parse_frontmatter(content: &str) -> ParsedFrontmatter {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return ParsedFrontmatter::default();
    }

    // 定位 frontmatter 结束 `---`
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            end = i;
            break;
        }
    }
    let fm = &lines[1..end];

    let mut parsed = ParsedFrontmatter::default();
    let mut i = 0;
    while i < fm.len() {
        let line = fm[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        // 顶层 key 必须从第 0 列开始
        if line.starts_with(|c: char| c.is_whitespace()) {
            i += 1;
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            i += 1;
            continue;
        };
        let key = key.trim();
        let value_part = rest.trim();

        // 检测 block scalar 引导符
        let block_marker = value_part.chars().next().filter(|c| *c == '|' || *c == '>');

        if let Some(marker) = block_marker {
            // 跳过 chomping 修饰符 `-` / `+`，本实现统一按 clip 行为
            let folded = marker == '>';
            let (value, consumed) = parse_block_scalar(&fm[i + 1..], folded);
            match key {
                "name" => parsed.name = Some(value),
                "description" => parsed.description = Some(value),
                "version" => parsed.version = Some(value),
                _ => {}
            }
            i += 1 + consumed;
        } else {
            let value = strip_yaml_quotes(value_part);
            match key {
                "name" => parsed.name = Some(value),
                "description" => parsed.description = Some(value),
                "version" => parsed.version = Some(value),
                _ => {}
            }
            i += 1;
        }
    }

    parsed
}

// ── Skill scanning ───────────────────────────────────────────────────────────

fn parse_skill_entry(dir_path: &Path, name: &str) -> Skill {
    let skill_md = dir_path.join("SKILL.md");
    let (display_name, description, has_error) = match fs::read_to_string(&skill_md) {
        Ok(content) => {
            let parsed = parse_frontmatter(&content);
            (parsed.name, parsed.description, None)
        }
        Err(e) => (None, None, Some(format!("Failed to read SKILL.md: {}", e))),
    };
    Skill {
        name: name.to_string(),
        display_name,
        description,
        path: dir_path.to_string_lossy().into_owned(),
        has_error,
    }
}

/// 递归扫描目录：含 SKILL.md 的目录视为 skill，否则继续向下遍历子目录。
/// 限制深度以及拒绝 symlink 子目录，避免被恶意/意外构造的循环 symlink 撑爆栈。
const MAX_SCAN_DEPTH: usize = 6;

fn collect_skills(dir: &Path, skills: &mut Vec<Skill>, depth: usize) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // 用 symlink_metadata 避免 follow symlink（防止循环 symlink 爆栈）
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) if !n.starts_with('.') => n.to_string(),
            _ => continue,
        };
        if path.join("SKILL.md").is_file() {
            skills.push(parse_skill_entry(&path, &name));
        } else {
            collect_skills(&path, skills, depth + 1);
        }
    }
}

fn scan_skills_in(hub_path: &Path) -> Vec<Skill> {
    let mut skills: Vec<Skill> = Vec::new();
    collect_skills(hub_path, &mut skills, 0);
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    skills
}

// ── Symlink helpers ──────────────────────────────────────────────────────────

#[cfg(unix)]
fn create_skill_installation(target: &Path, link: &Path) -> Result<String, String> {
    std::os::unix::fs::symlink(target, link)
        .map(|_| "symlink".to_string())
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn create_skill_installation(target: &Path, link: &Path) -> Result<String, String> {
    let mut command = std::process::Command::new("cmd.exe");
    crate::subprocess::configure_background_command(&mut command);
    let junction = command
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output();
    if junction.is_ok_and(|output| output.status.success()) {
        return Ok("junction".to_string());
    }
    copy_skill_directory(target, link)?;
    Ok("copy".to_string())
}

#[cfg(windows)]
fn copy_skill_directory(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Skill copy fallback does not follow symlinks: {}",
                source_path.display()
            ));
        }
        if metadata.is_dir() {
            copy_skill_directory(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn classify_existing(path: &Path) -> Option<(String, Option<String>)> {
    let meta = fs::symlink_metadata(path).ok()?;
    let kind = if meta.file_type().is_symlink() {
        "symlink"
    } else if meta.is_dir() {
        "directory"
    } else {
        "file"
    };
    let target = if meta.file_type().is_symlink() {
        fs::read_link(path)
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };
    Some((kind.to_string(), target))
}

/// 删除已存在的 link_path（symlink / 普通目录 / 文件均支持）
fn remove_existing(path: &Path) -> Result<(), String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path).map_err(|e| e.to_string())
    } else {
        fs::remove_dir_all(path).map_err(|e| e.to_string())
    }
}

fn symlink_points_to(link_path: &Path, expected_canonical: &Path) -> bool {
    let Ok(target) = fs::read_link(link_path) else {
        return false;
    };
    let resolved = if target.is_absolute() {
        target
    } else {
        link_path
            .parent()
            .map(|parent| parent.join(&target))
            .unwrap_or(target)
    };
    resolved
        .canonicalize()
        .map(|actual| actual == expected_canonical)
        .unwrap_or(false)
}

fn installation_targets_skill(ins: &SkillInstallation, expected_canonical: &Path) -> bool {
    let target = Path::new(&ins.target_path);
    target
        .canonicalize()
        .map(|actual| actual == expected_canonical)
        .unwrap_or_else(|_| target == expected_canonical)
}

fn remove_symlink_if_present(link_path: &Path) -> Result<bool, String> {
    let Ok(meta) = fs::symlink_metadata(link_path) else {
        return Ok(false);
    };
    if !meta.file_type().is_symlink() {
        return Ok(false);
    }
    fs::remove_file(link_path)
        .map_err(|e| format!("Failed to remove symlink {}: {}", link_path.display(), e))?;
    Ok(true)
}

fn remove_recorded_installation(installation: &SkillInstallation) -> Result<bool, String> {
    let path = Path::new(&installation.link_path);
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(false);
    };
    match installation.install_kind.as_str() {
        "copy" => {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                fs::remove_dir_all(path).map_err(|error| error.to_string())?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        "junction" => {
            fs::remove_dir(path).map_err(|error| error.to_string())?;
            Ok(true)
        }
        _ if metadata.file_type().is_symlink() => {
            fs::remove_file(path).map_err(|error| error.to_string())?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

// ── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_skill_hub_config() -> Result<SkillHubConfig, String> {
    tokio::task::spawn_blocking(load_hub_config_or_default)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_skill_hub_path(path: String) -> Result<SetHubResult, String> {
    tokio::task::spawn_blocking(move || {
        let raw = path.trim();
        if raw.is_empty() {
            return Err("Hub path cannot be empty".to_string());
        }
        let target = Path::new(raw);
        if !target.is_absolute() {
            return Err("Hub path must be absolute".to_string());
        }
        let canonical = target
            .canonicalize()
            .map_err(|e| format!("Cannot resolve hub path: {}", e))?;
        if !canonical.is_dir() {
            return Err("Hub path is not a directory".to_string());
        }
        let hub_path_str = canonical.to_string_lossy().into_owned();

        let ((project, created_new_project), projects) = update_projects(|projects| {
            let existing = projects
                .iter()
                .find(|p| {
                    Path::new(&p.path).canonicalize().ok().as_deref() == Some(canonical.as_path())
                })
                .cloned();

            Ok(match existing {
                Some(project) => (project, false),
                None => {
                    let name = canonical
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("skills_hub")
                        .to_string();
                    let project = Project {
                        id: now_ms().to_string(),
                        name,
                        path: hub_path_str.clone(),
                        location: None,
                        branch: None,
                        last_opened_at: now_ms(),
                        order_index: None,
                        group: None,
                        hidden_from_rail: false,
                    };
                    projects.push(project.clone());
                    (project, true)
                }
            })
        })?;

        let config = SkillHubConfig {
            hub_project_id: Some(project.id.clone()),
            hub_path: Some(hub_path_str),
            created_at: Some(now_ms()),
        };
        save_hub_config_internal(&config)?;

        Ok(SetHubResult {
            config,
            project,
            created_new_project,
            projects,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_skill_hub() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let cfg = SkillHubConfig::default();
        save_hub_config_internal(&cfg)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_skills() -> Result<Vec<Skill>, String> {
    tokio::task::spawn_blocking(|| {
        let cfg = load_hub_config_internal();
        let Some(hub_path) = cfg.hub_path.as_deref() else {
            return Ok(Vec::new());
        };
        Ok(scan_skills_in(Path::new(hub_path)))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_skill_installations(
    skill_name: Option<String>,
) -> Result<Vec<SkillInstallation>, String> {
    tokio::task::spawn_blocking(move || {
        let file = load_installations_internal();
        let mut out: Vec<SkillInstallation> = file
            .installations
            .into_iter()
            .filter(|ins| match &skill_name {
                Some(name) => ins.skill_name == *name,
                None => true,
            })
            .collect();

        // 健康度校验：用 canonicalize 比对，避免 trailing `/` / 大小写差异误报 diverged
        for ins in &mut out {
            let link = Path::new(&ins.link_path);
            let target_canonical = Path::new(&ins.target_path).canonicalize();
            ins.health = Some(match fs::symlink_metadata(link) {
                Err(_) => "broken".to_string(),
                Ok(meta) => match ins.install_kind.as_str() {
                    "copy" if meta.is_dir() && link.join("SKILL.md").is_file() => "ok".to_string(),
                    "junction" => match target_canonical {
                        Err(_) => "broken".to_string(),
                        Ok(expected)
                            if link.canonicalize().is_ok_and(|actual| actual == expected) =>
                        {
                            "ok".to_string()
                        }
                        Ok(_) => "diverged".to_string(),
                    },
                    _ if meta.file_type().is_symlink() => match target_canonical {
                        Err(_) => "broken".to_string(),
                        Ok(expected) if symlink_points_to(link, &expected) => "ok".to_string(),
                        Ok(_) => "diverged".to_string(),
                    },
                    _ => "diverged".to_string(),
                },
            });
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn install_skill(
    skill_name: String,
    skill_path: String,
    project_id: String,
    agent: String,
    strategy: String,
) -> Result<InstallResult, String> {
    tokio::task::spawn_blocking(move || {
        if !matches!(agent.as_str(), "claude" | "codex") {
            return Err(format!("Unsupported agent: {}", agent));
        }
        if !matches!(
            strategy.as_str(),
            "detect" | "skip" | "overwrite" | "cancel"
        ) {
            return Err(format!("Unsupported strategy: {}", strategy));
        }
        validate_skill_name(&skill_name)?;

        // cancel 是显式无操作
        if strategy == "cancel" {
            return Ok(InstallResult {
                ok: false,
                cancelled: true,
                ..Default::default()
            });
        }

        let skill_dir = Path::new(&skill_path);
        if !skill_dir.is_dir() {
            return Err(format!(
                "Skill '{}' not found at path: {}",
                skill_name, skill_path
            ));
        }
        if !skill_dir.join("SKILL.md").is_file() {
            return Err(format!("Skill '{}' has no SKILL.md", skill_name));
        }
        // skill_path 最后一段必须与 skill_name 一致，防止伪造目录名
        if skill_dir.file_name().and_then(|s| s.to_str()) != Some(skill_name.as_str()) {
            return Err(format!(
                "Skill path '{}' does not match skill name '{}'",
                skill_path, skill_name
            ));
        }

        // 校验 skill 路径必须位于已配置的 hub 目录内
        let cfg = load_hub_config_internal();
        let hub_path = cfg
            .hub_path
            .as_deref()
            .ok_or_else(|| "Skill Hub is not configured".to_string())?;
        let hub_canonical = Path::new(hub_path)
            .canonicalize()
            .map_err(|e| format!("Cannot resolve hub path '{}': {}", hub_path, e))?;
        let skill_canonical = skill_dir
            .canonicalize()
            .map_err(|e| format!("Cannot resolve skill path '{}': {}", skill_path, e))?;
        if !skill_canonical.starts_with(&hub_canonical) {
            return Err(format!(
                "Skill path '{}' is not inside hub '{}'",
                skill_path, hub_path
            ));
        }

        let projects = load_projects()?;
        let project = projects
            .iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("Project '{}' not found", project_id))?;
        let project_path = Path::new(&project.path);
        if !project_path.is_dir() {
            return Err(format!("Project path does not exist: {}", project.path));
        }

        let skills_root = agent_skills_dir(project_path, &agent);
        fs::create_dir_all(&skills_root)
            .map_err(|e| format!("Failed to create {}: {}", skills_root.display(), e))?;
        let link_path = skills_root.join(&skill_name);

        let target_path_str = skill_canonical.to_string_lossy().into_owned();
        let link_path_str = link_path.to_string_lossy().into_owned();

        if strategy == "skip" {
            return Ok(InstallResult {
                ok: true,
                skipped: true,
                ..Default::default()
            });
        }

        // detect / overwrite 共同入口：检查 link_path 现状
        let existing = classify_existing(&link_path);

        if let Some((kind, existing_target)) = existing.as_ref() {
            let existing_installation = load_installations_internal()
                .installations
                .into_iter()
                .find(|installation| {
                    installation.skill_name == skill_name
                        && installation.project_id == project_id
                        && installation.agent == agent
                        && installation.link_path == link_path_str
                        && installation.target_path == target_path_str
                });
            let already_same_symlink =
                kind == "symlink" && symlink_points_to(&link_path, &skill_canonical);
            let already_managed = existing_installation.as_ref().is_some_and(|installation| {
                match installation.install_kind.as_str() {
                    "junction" => link_path
                        .canonicalize()
                        .is_ok_and(|actual| actual == skill_canonical),
                    "copy" => link_path.is_dir() && link_path.join("SKILL.md").is_file(),
                    _ => already_same_symlink,
                }
            });

            if already_managed {
                // 幂等：补全 installations 记录
                let installation = upsert_installation(
                    &skill_name,
                    &project_id,
                    &agent,
                    &link_path_str,
                    &target_path_str,
                    existing_installation
                        .as_ref()
                        .map(|installation| installation.install_kind.as_str())
                        .unwrap_or("symlink"),
                )?;
                return Ok(InstallResult {
                    ok: true,
                    already_installed: true,
                    installation: Some(installation),
                    ..Default::default()
                });
            }

            if strategy == "detect" {
                return Ok(InstallResult {
                    ok: false,
                    conflict: Some(ConflictInfo {
                        existing_kind: kind.clone(),
                        existing_target: existing_target.clone(),
                        link_path: link_path_str,
                    }),
                    ..Default::default()
                });
            }

            // overwrite
            remove_existing(&link_path)?;
        }

        let install_kind =
            create_skill_installation(&skill_canonical, &link_path).map_err(|e| {
                format!(
                    "Failed to install skill {} -> {}: {}",
                    link_path.display(),
                    skill_canonical.display(),
                    e
                )
            })?;

        let installation = upsert_installation(
            &skill_name,
            &project_id,
            &agent,
            &link_path_str,
            &target_path_str,
            &install_kind,
        )?;

        Ok(InstallResult {
            ok: true,
            installation: Some(installation),
            ..Default::default()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn uninstall_skill(
    skill_name: String,
    project_id: String,
    agent: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        validate_skill_name(&skill_name)?;
        if !matches!(agent.as_str(), "claude" | "codex") {
            return Err(format!("Unsupported agent: {}", agent));
        }
        let mut file = load_installations_internal();
        let target = file
            .installations
            .iter()
            .find(|ins| {
                ins.skill_name == skill_name && ins.project_id == project_id && ins.agent == agent
            })
            .cloned();

        let link_path = match target {
            Some(ref ins) => PathBuf::from(&ins.link_path),
            None => {
                // 即使没有记录，也尝试按约定路径清理
                let projects = load_projects()?;
                let project = projects
                    .iter()
                    .find(|p| p.id == project_id)
                    .ok_or_else(|| format!("Project '{}' not found", project_id))?;
                agent_skills_dir(Path::new(&project.path), &agent).join(&skill_name)
            }
        };

        if let Some(installation) = target.as_ref() {
            remove_recorded_installation(installation)?;
        } else if let Ok(meta) = fs::symlink_metadata(&link_path) {
            if meta.file_type().is_symlink() {
                fs::remove_file(&link_path).map_err(|e| e.to_string())?;
            }
        }

        file.installations.retain(|ins| {
            !(ins.skill_name == skill_name && ins.project_id == project_id && ins.agent == agent)
        });
        save_installations_internal(&file)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 删除项目时调用：清掉该项目所有 skill 安装记录，并尽力删除残留 symlink。
/// best-effort：symlink 删不掉（项目目录已不在等）不视为错误。
#[tauri::command]
pub async fn cleanup_installations_for_project(project_id: String) -> Result<usize, String> {
    tokio::task::spawn_blocking(move || {
        let mut file = load_installations_internal();
        let original_len = file.installations.len();

        for ins in file
            .installations
            .iter()
            .filter(|i| i.project_id == project_id)
        {
            let _ = remove_recorded_installation(ins);
        }

        file.installations
            .retain(|ins| ins.project_id != project_id);
        let removed = original_len - file.installations.len();
        if removed > 0 {
            save_installations_internal(&file)?;
        }
        Ok(removed)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_skill(skill_name: String, skill_path: String) -> Result<DeleteResult, String> {
    tokio::task::spawn_blocking(move || {
        validate_skill_name(&skill_name)?;
        let skill_dir = Path::new(&skill_path);
        if !skill_dir.is_dir() {
            return Err(format!(
                "Skill '{}' not found at path: {}",
                skill_name, skill_path
            ));
        }
        if !skill_dir.join("SKILL.md").is_file() {
            return Err(format!("Skill '{}' has no SKILL.md", skill_name));
        }
        if skill_dir.file_name().and_then(|s| s.to_str()) != Some(skill_name.as_str()) {
            return Err(format!(
                "Skill path '{}' does not match skill name '{}'",
                skill_path, skill_name
            ));
        }

        let cfg = load_hub_config_internal();
        let hub_path = cfg
            .hub_path
            .as_deref()
            .ok_or_else(|| "Skill Hub is not configured".to_string())?;
        let hub_canonical = Path::new(hub_path)
            .canonicalize()
            .map_err(|e| format!("Cannot resolve hub path: {}", e))?;
        let skill_canonical = skill_dir
            .canonicalize()
            .map_err(|e| format!("Cannot resolve skill path: {}", e))?;
        if !skill_canonical.starts_with(&hub_canonical) {
            return Err(format!(
                "Skill path '{}' is not inside hub '{}'",
                skill_path, hub_path
            ));
        }

        let file = load_installations_internal();
        let matching_installations = file
            .installations
            .iter()
            .filter(|ins| {
                ins.skill_name == skill_name && installation_targets_skill(ins, &skill_canonical)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut candidate_links: HashSet<PathBuf> = HashSet::new();
        let mut removed_links = 0usize;
        for installation in &matching_installations {
            if remove_recorded_installation(installation)? {
                removed_links += 1;
            }
            candidate_links.insert(PathBuf::from(&installation.link_path));
        }

        for project in load_projects()? {
            let project_path = Path::new(&project.path);
            for agent in ["claude", "codex"] {
                let link = agent_skills_dir(project_path, agent).join(&skill_name);
                if symlink_points_to(&link, &skill_canonical) {
                    candidate_links.insert(link);
                }
            }
        }

        for link_path in candidate_links {
            if remove_symlink_if_present(&link_path)? {
                removed_links += 1;
            }
        }

        fs::remove_dir_all(&skill_canonical)
            .map_err(|e| format!("Failed to delete skill directory: {}", e))?;

        let mut file = file;
        file.installations.retain(|ins| {
            !(ins.skill_name == skill_name && installation_targets_skill(ins, &skill_canonical))
        });
        save_installations_internal(&file)?;

        Ok(DeleteResult {
            ok: true,
            removed_links,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn upsert_installation(
    skill_name: &str,
    project_id: &str,
    agent: &str,
    link_path: &str,
    target_path: &str,
    install_kind: &str,
) -> Result<SkillInstallation, String> {
    let mut file = load_installations_internal();
    if file.version == 0 {
        file.version = 1;
    }
    let now = now_ms();
    let mut existing_idx: Option<usize> = None;
    for (i, ins) in file.installations.iter().enumerate() {
        if ins.skill_name == skill_name && ins.project_id == project_id && ins.agent == agent {
            existing_idx = Some(i);
            break;
        }
    }
    let health = target_health(Path::new(target_path)).to_string();
    let installation = SkillInstallation {
        skill_name: skill_name.to_string(),
        project_id: project_id.to_string(),
        agent: agent.to_string(),
        installed_at: now,
        link_path: link_path.to_string(),
        target_path: target_path.to_string(),
        install_kind: install_kind.to_string(),
        health: Some(health),
    };
    match existing_idx {
        Some(idx) => file.installations[idx] = installation.clone(),
        None => file.installations.push(installation.clone()),
    }
    save_installations_internal(&file)?;
    Ok(installation)
}

#[tauri::command]
pub async fn import_local_skill(source_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let source = Path::new(&source_path);
        if !source.is_dir() {
            return Err(format!("Source path is not a directory: {}", source_path));
        }
        if !source.join("SKILL.md").is_file() {
            return Err("Selected folder does not contain a SKILL.md file".to_string());
        }
        let skill_name = source
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "Cannot determine skill name from folder".to_string())?
            .to_string();
        validate_skill_name(&skill_name)?;

        let cfg = load_hub_config_internal();
        let hub_path = cfg
            .hub_path
            .as_deref()
            .ok_or_else(|| "Skill Hub is not configured".to_string())?;
        let dest = Path::new(hub_path).join(&skill_name);
        if dest.exists() {
            return Err(format!("Skill '{}' already exists in the hub", skill_name));
        }
        copy_dir_recursive(source, &dest).map_err(|e| {
            let _ = fs::remove_dir_all(&dest);
            format!("Failed to copy skill: {}", e)
        })?;
        Ok(skill_name)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn marketplace_cache_key(
    query: &str,
    sort: MarketplaceSort,
    category: &MarketplaceCategory,
    page: usize,
    page_size: usize,
) -> String {
    format!(
        "v2|{}|{:?}|{}|{}|{}",
        query.trim().to_lowercase(),
        sort,
        category.as_str(),
        page,
        page_size
    )
}

fn github_repo_parts(source: &str) -> Option<(&str, &str)> {
    let mut parts = source.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some()
        || owner.is_empty()
        || repo.is_empty()
        || owner.contains('.')
        || !owner
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        || !repo
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return None;
    }
    Some((owner, repo))
}

fn normalize_marketplace_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn classify_marketplace_skill(text: &str) -> Vec<String> {
    const RULES: &[(&str, &[&str])] = &[
        (
            "agents",
            &["agent", "assistant", "multi-agent", "copilot", "prompt"],
        ),
        (
            "integrations",
            &[
                "integration",
                "api",
                "slack",
                "discord",
                "github",
                "notion",
                "jira",
                "lark",
                "feishu",
            ],
        ),
        (
            "automation",
            &[
                "automation",
                "workflow",
                "browser",
                "scrape",
                "schedule",
                "bot",
            ],
        ),
        (
            "operations",
            &[
                "devops",
                "deploy",
                "docker",
                "kubernetes",
                "azure",
                "aws",
                "cloud",
                "infra",
                "monitor",
            ],
        ),
        (
            "security",
            &[
                "security",
                "audit",
                "auth",
                "vulnerability",
                "compliance",
                "rbac",
            ],
        ),
        (
            "research",
            &[
                "research",
                "paper",
                "academic",
                "analysis",
                "explore",
                "experiment",
            ],
        ),
        (
            "development",
            &[
                "code",
                "react",
                "typescript",
                "python",
                "rust",
                "java",
                "database",
                "frontend",
                "backend",
                "test",
                "debug",
            ],
        ),
        (
            "finance",
            &[
                "finance",
                "stock",
                "trading",
                "accounting",
                "invoice",
                "budget",
                "cost",
            ],
        ),
        (
            "lifestyle",
            &[
                "health",
                "fitness",
                "travel",
                "recipe",
                "food",
                "home",
                "lifestyle",
            ],
        ),
        (
            "productivity",
            &[
                "productivity",
                "task",
                "todo",
                "calendar",
                "meeting",
                "notes",
                "planning",
            ],
        ),
        (
            "communication",
            &[
                "communication",
                "email",
                "message",
                "chat",
                "social",
                "twitter",
                "reddit",
            ],
        ),
        (
            "creative",
            &[
                "design",
                "image",
                "video",
                "music",
                "creative",
                "animation",
                "brand",
                "ui",
                "ux",
            ],
        ),
        (
            "knowledge",
            &[
                "knowledge",
                "docs",
                "documentation",
                "wiki",
                "learn",
                "teach",
                "education",
            ],
        ),
    ];
    let lower = text.to_lowercase();
    let mut categories: Vec<String> = RULES
        .iter()
        .filter(|(_, needles)| needles.iter().any(|needle| lower.contains(needle)))
        .map(|(category, _)| (*category).to_string())
        .collect();
    categories.sort();
    categories.dedup();
    if categories.is_empty() {
        categories.push("other".to_string());
    }
    categories
}

async fn marketplace_get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let mut request = MARKETPLACE_HTTP_CLIENT
        .get(url)
        .header(USER_AGENT, "Aeroric/1.3.8")
        .header(ACCEPT, "application/vnd.github+json, application/json");
    if url.starts_with(GITHUB_API_ORIGIN) {
        if let Some(token) = github_auth_token() {
            request = request.header(reqwest::header::AUTHORIZATION, format!("token {token}"));
        }
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("Network request failed: {error}"))?;
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        let hint = if github_auth_token().is_none() {
            " (unauthenticated: 60 requests/hour; set GITHUB_TOKEN for 5000/hour)"
        } else {
            ""
        };
        return Err(format!("GitHub API rate limit reached{hint}"));
    }
    response
        .error_for_status()
        .map_err(|error| format!("Marketplace request failed: {error}"))?
        .json::<T>()
        .await
        .map_err(|error| format!("Invalid marketplace response: {error}"))
}

async fn fetch_skills_sh_entries(
    query: &str,
    max_directory_page: usize,
) -> Result<FetchedSkillsSh, String> {
    let query = query.trim();
    if query.len() >= 2 {
        let mut url = url::Url::parse(&format!("{SKILLS_SH_ORIGIN}/api/search"))
            .map_err(|error| error.to_string())?;
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("limit", "100");
        let mut response: SkillsShResponse = marketplace_get_json(url.as_str()).await?;
        let all_time: SkillsShResponse =
            marketplace_get_json(&format!("{SKILLS_SH_ORIGIN}/api/skills/all-time/0"))
                .await
                .unwrap_or_default();
        let trends: HashMap<(String, String), SkillsShSkill> = all_time
            .skills
            .into_iter()
            .map(|skill| ((skill.source.clone(), skill.skill_id.clone()), skill))
            .collect();
        for skill in &mut response.skills {
            if let Some(trend) = trends.get(&(skill.source.clone(), skill.skill_id.clone())) {
                skill.weekly_installs = trend.weekly_installs.clone();
                skill.is_official = trend.is_official;
                skill.installs = skill.installs.max(trend.installs);
            }
        }
        let total = response.count.max(response.skills.len());
        Ok(FetchedSkillsSh {
            skills: response.skills,
            total,
            has_more: false,
        })
    } else {
        let mut skills = Vec::new();
        let mut total = 0;
        let mut has_more = false;
        for directory_page in 0..=max_directory_page {
            let response: SkillsShResponse = marketplace_get_json(&format!(
                "{SKILLS_SH_ORIGIN}/api/skills/all-time/{directory_page}"
            ))
            .await?;
            total = response.total.max(total);
            has_more = response.has_more;
            skills.extend(response.skills);
            if !has_more {
                break;
            }
        }
        Ok(FetchedSkillsSh {
            total: total.max(skills.len()),
            skills,
            has_more,
        })
    }
}

fn marketplace_from_skills_sh(skill: SkillsShSkill) -> MarketplaceSkill {
    let publisher = skill
        .source
        .split('/')
        .next()
        .unwrap_or(&skill.source)
        .to_string();
    let categories = classify_marketplace_skill(&format!(
        "{} {} {}",
        skill.name, skill.skill_id, skill.source
    ));
    MarketplaceSkill {
        id: format!("{}/{}", skill.source, skill.skill_id),
        repository_url: github_repo_parts(&skill.source)
            .map(|_| format!("https://github.com/{}", skill.source)),
        source: skill.source,
        skill_id: skill.skill_id,
        name: skill.name,
        publisher,
        latest_version: "latest".to_string(),
        latest_ref: "HEAD".to_string(),
        categories,
        downloads_7d: skill.weekly_installs.last().copied().unwrap_or(0),
        total_installs: skill.installs,
        is_official: skill.is_official,
        install_status: "available".to_string(),
        ..Default::default()
    }
}

async fn verify_marketplace_skill_source(
    requested: &MarketplaceSkill,
) -> Result<MarketplaceSkill, String> {
    if github_repo_parts(&requested.source).is_none() {
        return Err("Marketplace source is not a GitHub repository".to_string());
    }
    let lookup = if requested.skill_id.trim().len() >= 2 {
        requested.skill_id.trim()
    } else {
        requested.name.trim()
    };
    let verified = fetch_skills_sh_entries(lookup, 0)
        .await?
        .skills
        .into_iter()
        .find(|skill| skill.source == requested.source && skill.skill_id == requested.skill_id)
        .ok_or_else(|| {
            "Marketplace skill source could not be verified with Skills.sh".to_string()
        })?;
    Ok(marketplace_from_skills_sh(verified))
}

fn find_skill_markdown_path(tree: &GithubTree, skill: &MarketplaceSkill) -> Option<String> {
    let wanted = normalize_marketplace_token(&skill.skill_id);
    let name = normalize_marketplace_token(&skill.name);
    let candidates: Vec<&GithubTreeEntry> = tree
        .tree
        .iter()
        .filter(|entry| {
            entry.kind == "blob" && (entry.path == "SKILL.md" || entry.path.ends_with("/SKILL.md"))
        })
        .collect();
    candidates
        .iter()
        .find(|entry| {
            Path::new(&entry.path)
                .parent()
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .map(normalize_marketplace_token)
                .is_some_and(|candidate| candidate == wanted || candidate == name)
        })
        .or_else(|| {
            candidates.iter().find(|entry| {
                let normalized = normalize_marketplace_token(&entry.path);
                normalized.contains(&wanted) || normalized.contains(&name)
            })
        })
        .or_else(|| (candidates.len() == 1).then(|| &candidates[0]))
        .map(|entry| entry.path.clone())
}

async fn fetch_raw_github_file(
    owner: &str,
    repo: &str,
    git_ref: &str,
    path: &str,
) -> Result<String, String> {
    let mut url =
        url::Url::parse("https://raw.githubusercontent.com").map_err(|error| error.to_string())?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "Cannot build GitHub raw URL".to_string())?;
        segments.push(owner).push(repo).push(git_ref);
        for component in Path::new(path).components() {
            if let std::path::Component::Normal(value) = component {
                segments.push(&value.to_string_lossy());
            }
        }
    }
    let response = MARKETPLACE_HTTP_CLIENT
        .get(url)
        .header(USER_AGENT, "Aeroric/1.3.8")
        .send()
        .await
        .map_err(|error| format!("Failed to download SKILL.md: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Failed to download SKILL.md: {error}"))?;
    response
        .text()
        .await
        .map_err(|error| format!("Failed to read SKILL.md: {error}"))
}

async fn fetch_latest_skill_commit(
    owner: &str,
    repo: &str,
    branch: &str,
    skill_markdown_path: &str,
) -> Option<GithubCommit> {
    let mut url =
        url::Url::parse(&format!("{GITHUB_API_ORIGIN}/repos/{owner}/{repo}/commits")).ok()?;
    url.query_pairs_mut()
        .append_pair("sha", branch)
        .append_pair("path", skill_markdown_path)
        .append_pair("per_page", "1");
    marketplace_get_json::<Vec<GithubCommit>>(url.as_str())
        .await
        .ok()?
        .into_iter()
        .next()
}

async fn fetch_latest_repository_version(owner: &str, repo: &str) -> Option<String> {
    if let Ok(release) = marketplace_get_json::<GithubRelease>(&format!(
        "{GITHUB_API_ORIGIN}/repos/{owner}/{repo}/releases/latest"
    ))
    .await
    {
        if !release.tag_name.trim().is_empty() {
            return Some(release.tag_name);
        }
    }
    marketplace_get_json::<Vec<GithubTag>>(&format!(
        "{GITHUB_API_ORIGIN}/repos/{owner}/{repo}/tags?per_page=1"
    ))
    .await
    .ok()?
    .into_iter()
    .find_map(|tag| (!tag.name.trim().is_empty()).then_some(tag.name))
}

fn apply_repository_metadata(
    skill: &mut MarketplaceSkill,
    owner: &str,
    repo: &str,
    repository: &GithubRepository,
) {
    skill.publisher = if repository.owner.login.is_empty() {
        owner.to_string()
    } else {
        repository.owner.login.clone()
    };
    skill.publisher_avatar = repository.owner.avatar_url.clone();
    skill.repository_url = Some(if repository.html_url.is_empty() {
        format!("https://github.com/{owner}/{repo}")
    } else {
        repository.html_url.clone()
    });
    if skill.description.is_none() {
        skill.description = repository.description.clone();
    }
    skill.stars = repository.stargazers_count;
    skill.published_at =
        (!repository.created_at.is_empty()).then_some(repository.created_at.clone());
    skill.updated_at = (!repository.pushed_at.is_empty()).then_some(repository.pushed_at.clone());
    let topic_text = repository.topics.join(" ");
    skill.categories = classify_marketplace_skill(&format!(
        "{} {} {} {} {}",
        skill.name,
        skill.skill_id,
        skill.source,
        skill.description.as_deref().unwrap_or_default(),
        topic_text
    ));
}

async fn enrich_marketplace_skill_with_repository(
    mut skill: MarketplaceSkill,
    owner: &str,
    repo: &str,
    repository: GithubRepository,
) -> Result<MarketplaceSkill, String> {
    let branch = if repository.default_branch.is_empty() {
        "main".to_string()
    } else {
        repository.default_branch.clone()
    };
    let tree: GithubTree = marketplace_get_json(&format!(
        "{GITHUB_API_ORIGIN}/repos/{owner}/{repo}/git/trees/{branch}?recursive=1"
    ))
    .await?;
    let skill_md_path = find_skill_markdown_path(&tree, &skill);
    let latest_commit = match skill_md_path.as_deref() {
        Some(path) => fetch_latest_skill_commit(owner, repo, &branch, path).await,
        None => None,
    };
    let content_ref = latest_commit
        .as_ref()
        .map(|commit| commit.sha.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| branch.clone());
    let mut parsed = ParsedFrontmatter::default();
    if let Some(path) = skill_md_path.as_deref() {
        if let Ok(content) = fetch_raw_github_file(owner, repo, &content_ref, path).await {
            parsed = parse_frontmatter(&content);
        }
    }
    let repository_version = if parsed.version.is_none() {
        fetch_latest_repository_version(owner, repo).await
    } else {
        None
    };

    apply_repository_metadata(&mut skill, owner, repo, &repository);
    skill.skill_path = skill_md_path.and_then(|path| {
        Path::new(&path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
    });
    skill.description = parsed.description.or(skill.description);
    skill.latest_ref = content_ref.clone();
    skill.latest_version = parsed.version.or(repository_version).unwrap_or_else(|| {
        if content_ref == branch {
            branch.to_string()
        } else {
            content_ref.chars().take(8).collect()
        }
    });
    skill.updated_at = latest_commit
        .and_then(|commit| {
            (!commit.commit.committer.date.is_empty()).then_some(commit.commit.committer.date)
        })
        .or(skill.updated_at);
    let topic_text = repository.topics.join(" ");
    skill.categories = classify_marketplace_skill(&format!(
        "{} {} {} {} {}",
        skill.name,
        skill.skill_id,
        skill.source,
        skill.description.as_deref().unwrap_or_default(),
        topic_text
    ));
    Ok(skill)
}

async fn enrich_marketplace_skill(skill: MarketplaceSkill) -> Result<MarketplaceSkill, String> {
    let source = skill.source.clone();
    let (owner, repo) = github_repo_parts(&source)
        .ok_or_else(|| "Marketplace source is not a GitHub repository".to_string())?;
    let repository: GithubRepository =
        marketplace_get_json(&format!("{GITHUB_API_ORIGIN}/repos/{owner}/{repo}")).await?;
    enrich_marketplace_skill_with_repository(skill, owner, repo, repository).await
}

fn refresh_marketplace_install_status(skill: &mut MarketplaceSkill) {
    let file = load_marketplace_installations_internal();
    if let Some(record) = file
        .installations
        .iter()
        .find(|record| record.source == skill.source && record.skill_id == skill.skill_id)
    {
        skill.install_status = if record.git_ref == skill.latest_ref {
            "installed".to_string()
        } else {
            "update".to_string()
        };
        return;
    }

    let config = load_hub_config_internal();
    if let Some(hub_path) = config.hub_path {
        if Path::new(&hub_path).join(&skill.name).exists() {
            skill.install_status = "conflict".to_string();
        } else {
            skill.install_status = "available".to_string();
        }
    }
}

async fn enrich_marketplace_repository_metadata(
    items: &mut [MarketplaceSkill],
    repository_cache: &mut HashMap<String, GithubRepository>,
) -> Option<String> {
    let mut warning = None;
    for skill in items {
        let source = skill.source.clone();
        let Some((owner, repo)) = github_repo_parts(&source) else {
            continue;
        };
        let repository = if let Some(repository) = repository_cache.get(&source) {
            Ok(repository.clone())
        } else {
            marketplace_get_json::<GithubRepository>(&format!(
                "{GITHUB_API_ORIGIN}/repos/{owner}/{repo}"
            ))
            .await
            .inspect(|repository| {
                repository_cache.insert(source.clone(), repository.clone());
            })
        };
        match repository {
            Ok(repository) => {
                apply_repository_metadata(skill, owner, repo, &repository);
            }
            Err(error) => {
                warning.get_or_insert(error);
            }
        }
    }
    warning
}

fn sort_marketplace_skills(items: &mut [MarketplaceSkill], sort: MarketplaceSort) {
    items.sort_by(|left, right| match sort {
        MarketplaceSort::Downloads => right
            .downloads_7d
            .cmp(&left.downloads_7d)
            .then_with(|| right.total_installs.cmp(&left.total_installs)),
        MarketplaceSort::Stars => right
            .stars
            .cmp(&left.stars)
            .then_with(|| right.total_installs.cmp(&left.total_installs)),
        MarketplaceSort::Installs => right
            .total_installs
            .cmp(&left.total_installs)
            .then_with(|| right.downloads_7d.cmp(&left.downloads_7d)),
        MarketplaceSort::Updated => right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.total_installs.cmp(&left.total_installs)),
        MarketplaceSort::Published => right
            .published_at
            .cmp(&left.published_at)
            .then_with(|| right.total_installs.cmp(&left.total_installs)),
    });
}

#[tauri::command]
pub async fn search_marketplace_skills(
    query: String,
    sort: MarketplaceSort,
    category: MarketplaceCategory,
    page: Option<usize>,
    page_size: Option<usize>,
) -> Result<MarketplacePage, String> {
    let page = page.unwrap_or(0);
    let page_size = page_size
        .unwrap_or(MARKETPLACE_PAGE_SIZE)
        .clamp(1, MARKETPLACE_PAGE_SIZE);
    let cache_key = marketplace_cache_key(&query, sort, &category, page, page_size);
    if let Some(mut cached) = cached_marketplace_page(&cache_key, false) {
        cached
            .items
            .iter_mut()
            .for_each(refresh_marketplace_install_status);
        return Ok(cached);
    }

    let result = async {
        let requested_end = page.saturating_add(1).saturating_mul(page_size);
        let directory_page = requested_end
            .saturating_sub(1)
            .checked_div(SKILLS_SH_PAGE_SIZE)
            .unwrap_or(0);
        let fetched = fetch_skills_sh_entries(&query, directory_page).await?;
        let source_total = fetched.total;
        let source_has_more = fetched.has_more;
        let mut items: Vec<MarketplaceSkill> = fetched
            .skills
            .into_iter()
            .filter(|skill| github_repo_parts(&skill.source).is_some())
            .map(marketplace_from_skills_sh)
            .collect();
        let mut warning = None;
        let mut repository_cache: HashMap<String, GithubRepository> = HashMap::new();
        let enrich_listing = github_auth_token().is_some();
        if enrich_listing
            && (category != MarketplaceCategory::All
                || matches!(
                    sort,
                    MarketplaceSort::Stars | MarketplaceSort::Updated | MarketplaceSort::Published
                ))
        {
            warning =
                enrich_marketplace_repository_metadata(&mut items, &mut repository_cache).await;
        }
        items.retain(|skill| {
            category == MarketplaceCategory::All
                || skill
                    .categories
                    .iter()
                    .any(|value| value == category.as_str())
        });

        sort_marketplace_skills(&mut items, sort);
        let loaded_total = items.len();
        let total = if category == MarketplaceCategory::All {
            source_total.max(loaded_total)
        } else {
            loaded_total
        };
        let start = page.saturating_mul(page_size);
        let end = (start + page_size).min(loaded_total);
        let mut selected = if start < loaded_total {
            items[start..end].to_vec()
        } else {
            Vec::new()
        };

        if enrich_listing {
            if let Some(error) =
                enrich_marketplace_repository_metadata(&mut selected, &mut repository_cache).await
            {
                warning.get_or_insert(error);
            }
        }
        sort_marketplace_skills(&mut selected, sort);
        selected
            .iter_mut()
            .for_each(refresh_marketplace_install_status);

        Ok::<MarketplacePage, String>(MarketplacePage {
            items: selected,
            total,
            page,
            page_size,
            has_more: end < loaded_total || source_has_more,
            stale: false,
            warning,
        })
    }
    .await;

    match result {
        Ok(page) => {
            let _ = save_marketplace_cache_page(&cache_key, &page);
            Ok(page)
        }
        Err(error) => {
            if let Some(mut cached) = cached_marketplace_page(&cache_key, true) {
                cached.stale = true;
                cached.warning = Some(error);
                cached
                    .items
                    .iter_mut()
                    .for_each(refresh_marketplace_install_status);
                Ok(cached)
            } else {
                Err(error)
            }
        }
    }
}

#[tauri::command]
pub async fn get_marketplace_skill_details(
    skill: MarketplaceSkill,
) -> Result<MarketplaceSkill, String> {
    let mut detailed = enrich_marketplace_skill(skill).await?;
    refresh_marketplace_install_status(&mut detailed);
    Ok(detailed)
}

fn safe_archive_relative_path(path: &Path) -> Result<PathBuf, String> {
    let mut components = path.components();
    let archive_root = components
        .next()
        .ok_or_else(|| "Archive entry has no path".to_string())?;
    if !matches!(archive_root, std::path::Component::Normal(_)) {
        return Err("Archive contains an unsafe path".to_string());
    }
    let mut output = PathBuf::new();
    for component in components {
        match component {
            std::path::Component::Normal(value) => output.push(value),
            _ => return Err("Archive contains an unsafe path".to_string()),
        }
    }
    Ok(output)
}

fn extract_marketplace_archive(bytes: &[u8], target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;
    let entries = archive
        .entries()
        .map_err(|error| format!("Cannot read GitHub archive: {error}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("Cannot read archive entry: {error}"))?;
        file_count += 1;
        if file_count > MARKETPLACE_MAX_ARCHIVE_FILES {
            return Err("Marketplace skill archive contains too many entries".to_string());
        }
        let header = entry.header();
        let entry_type = header.entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err("Marketplace archive contains a symbolic link".to_string());
        }
        if !entry_type.is_dir() && !entry_type.is_file() {
            continue;
        }
        let relative = safe_archive_relative_path(
            &entry
                .path()
                .map_err(|error| format!("Invalid archive path: {error}"))?,
        )?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = target.join(&relative);
        if !destination.starts_with(target) {
            return Err("Archive path escapes the temporary directory".to_string());
        }
        if entry_type.is_dir() {
            fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
            continue;
        }
        let size = header.size().map_err(|error| error.to_string())?;
        if size > MARKETPLACE_MAX_FILE_BYTES {
            return Err("Marketplace skill archive contains an oversized file".to_string());
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MARKETPLACE_MAX_ARCHIVE_BYTES {
            return Err("Marketplace skill archive is too large".to_string());
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = fs::File::create(&destination).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn locate_extracted_skill(root: &Path, skill: &MarketplaceSkill) -> Result<PathBuf, String> {
    if let Some(path) = skill.skill_path.as_deref() {
        let candidate = root.join(path);
        if candidate.join("SKILL.md").is_file() {
            return Ok(candidate);
        }
    }
    let wanted = normalize_marketplace_token(&skill.skill_id);
    let name = normalize_marketplace_token(&skill.name);
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut fallbacks = Vec::new();
    if root.join("SKILL.md").is_file() {
        fallbacks.push(root.to_path_buf());
    }
    while let Some((directory, depth)) = stack.pop() {
        if depth > MAX_SCAN_DEPTH + 2 {
            continue;
        }
        let entries = fs::read_dir(&directory).map_err(|error| error.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("Extracted skill contains a symbolic link".to_string());
            }
            if !metadata.is_dir() {
                continue;
            }
            if path.join("SKILL.md").is_file() {
                let normalized = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(normalize_marketplace_token)
                    .unwrap_or_default();
                if normalized == wanted || normalized == name {
                    return Ok(path);
                }
                fallbacks.push(path);
            } else {
                stack.push((path, depth + 1));
            }
        }
    }
    match fallbacks.len() {
        1 => Ok(fallbacks.remove(0)),
        0 => Err("Downloaded repository does not contain a matching SKILL.md".to_string()),
        _ => Err(
            "Downloaded repository contains multiple skills but none match the requested skill"
                .to_string(),
        ),
    }
}

fn copy_marketplace_skill_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fn copy_checked(
        src: &Path,
        dst: &Path,
        file_count: &mut usize,
        total_bytes: &mut u64,
    ) -> Result<(), String> {
        fs::create_dir_all(dst).map_err(|error| error.to_string())?;
        for entry in fs::read_dir(src).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let metadata = fs::symlink_metadata(&src_path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("Marketplace skill contains a symbolic link".to_string());
            }
            if metadata.is_dir() {
                copy_checked(&src_path, &dst_path, file_count, total_bytes)?;
                continue;
            }
            if !metadata.is_file() {
                return Err("Marketplace skill contains an unsupported file type".to_string());
            }
            *file_count += 1;
            if *file_count > MARKETPLACE_MAX_SKILL_FILES {
                return Err("Marketplace skill contains too many files".to_string());
            }
            if metadata.len() > MARKETPLACE_MAX_FILE_BYTES {
                return Err("Marketplace skill contains an oversized file".to_string());
            }
            *total_bytes = total_bytes.saturating_add(metadata.len());
            if *total_bytes > MARKETPLACE_MAX_SKILL_BYTES {
                return Err("Marketplace skill is too large".to_string());
            }
            fs::copy(&src_path, &dst_path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    if !src.join("SKILL.md").is_file() {
        return Err("Marketplace skill has no SKILL.md".to_string());
    }
    let mut file_count = 0;
    let mut total_bytes = 0;
    copy_checked(src, dst, &mut file_count, &mut total_bytes)
}

fn install_extracted_marketplace_skill(
    extracted: &Path,
    skill: &MarketplaceSkill,
    overwrite_conflict: bool,
) -> Result<MarketplaceInstallRecord, String> {
    validate_skill_name(&skill.name)?;
    let config = load_hub_config_internal();
    let hub_path = config
        .hub_path
        .ok_or_else(|| "Skill Hub is not configured".to_string())?;
    let hub = Path::new(&hub_path)
        .canonicalize()
        .map_err(|error| format!("Cannot resolve Skill Hub: {error}"))?;
    let destination = hub.join(&skill.name);
    let mut records = load_marketplace_installations_internal();
    let previous = records
        .installations
        .iter()
        .find(|record| record.source == skill.source && record.skill_id == skill.skill_id)
        .cloned();
    if let Some(record) = previous.as_ref() {
        if record.git_ref == skill.latest_ref
            && fs::symlink_metadata(&destination)
                .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                .unwrap_or(false)
        {
            return Ok(record.clone());
        }
    }
    let destination_exists = fs::symlink_metadata(&destination).is_ok();
    if destination_exists && previous.is_none() && !overwrite_conflict {
        return Err(format!("MARKETPLACE_NAME_CONFLICT:{}", skill.name));
    }

    let staging = hub.join(format!(".{}.marketplace-{}", skill.name, Uuid::new_v4()));
    copy_marketplace_skill_dir(extracted, &staging).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        format!("Failed to stage marketplace skill: {error}")
    })?;
    let backup = hub.join(format!(".{}.backup-{}", skill.name, Uuid::new_v4()));
    if destination_exists {
        fs::rename(&destination, &backup)
            .map_err(|error| format!("Failed to prepare skill update: {error}"))?;
    }
    if let Err(error) = fs::rename(&staging, &destination) {
        let _ = fs::remove_dir_all(&staging);
        if fs::symlink_metadata(&backup).is_ok() {
            let _ = fs::rename(&backup, &destination);
        }
        return Err(format!("Failed to install marketplace skill: {error}"));
    }

    let record = MarketplaceInstallRecord {
        source: skill.source.clone(),
        skill_id: skill.skill_id.clone(),
        skill_name: skill.name.clone(),
        version: skill.latest_version.clone(),
        git_ref: skill.latest_ref.clone(),
        installed_at: now_ms(),
        target_path: destination.to_string_lossy().into_owned(),
    };
    records.version = 1;
    records.installations.retain(|existing| {
        !(existing.source == record.source && existing.skill_id == record.skill_id)
    });
    records.installations.push(record.clone());
    if let Err(error) = save_marketplace_installations_internal(&records) {
        let _ = remove_existing(&destination);
        if fs::symlink_metadata(&backup).is_ok() {
            let _ = fs::rename(&backup, &destination);
        }
        return Err(error);
    }
    if fs::symlink_metadata(&backup).is_ok() {
        let _ = remove_existing(&backup);
    }
    Ok(record)
}

#[tauri::command]
pub async fn install_marketplace_skill(
    skill: MarketplaceSkill,
    overwrite_conflict: Option<bool>,
) -> Result<MarketplaceInstallRecord, String> {
    let verified = verify_marketplace_skill_source(&skill).await?;
    let detailed = enrich_marketplace_skill(verified).await?;
    let (owner, repo) = github_repo_parts(&detailed.source)
        .ok_or_else(|| "Marketplace source is not a GitHub repository".to_string())?;
    if detailed.latest_ref.is_empty() || detailed.latest_ref == "HEAD" {
        return Err("Marketplace skill has no installable Git ref".to_string());
    }
    let archive_url = format!(
        "{GITHUB_API_ORIGIN}/repos/{owner}/{repo}/tarball/{}",
        detailed.latest_ref
    );
    let mut archive_request = MARKETPLACE_HTTP_CLIENT
        .get(archive_url)
        .header(USER_AGENT, "Aeroric/1.3.8")
        .header(ACCEPT, "application/vnd.github+json");
    if let Some(token) = github_auth_token() {
        archive_request =
            archive_request.header(reqwest::header::AUTHORIZATION, format!("token {token}"));
    }
    let response = archive_request
        .send()
        .await
        .map_err(|error| format!("Failed to download marketplace skill: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Failed to download marketplace skill: {error}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Failed to read marketplace archive: {error}"))?;
    if bytes.len() as u64 > MARKETPLACE_MAX_ARCHIVE_BYTES * 2 {
        return Err("Marketplace repository archive is too large".to_string());
    }

    tokio::task::spawn_blocking(move || {
        let temp_root =
            std::env::temp_dir().join(format!("aeroric-marketplace-{}", Uuid::new_v4()));
        let result = (|| {
            extract_marketplace_archive(&bytes, &temp_root)?;
            let extracted = locate_extracted_skill(&temp_root, &detailed)?;
            install_extracted_marketplace_skill(
                &extracted,
                &detailed,
                overwrite_conflict.unwrap_or(false),
            )
        })();
        let _ = fs::remove_dir_all(&temp_root);
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_line_description() {
        let md = "---\nname: foo\ndescription: hello world\n---\nbody";
        let p = parse_frontmatter(md);
        assert_eq!(p.name.as_deref(), Some("foo"));
        assert_eq!(p.description.as_deref(), Some("hello world"));
    }

    #[test]
    fn parse_literal_block_description() {
        let md = "---\nname: foo\ndescription: |\n  line 1\n  line 2\n  line 3\n---\n";
        let p = parse_frontmatter(md);
        assert_eq!(p.description.as_deref(), Some("line 1\nline 2\nline 3"));
    }

    #[test]
    fn parse_literal_block_with_blank_line() {
        let md = "---\ndescription: |\n  para 1\n\n  para 2\n---\n";
        let p = parse_frontmatter(md);
        assert_eq!(p.description.as_deref(), Some("para 1\n\npara 2"));
    }

    #[test]
    fn parse_folded_block() {
        let md = "---\ndescription: >\n  line 1\n  line 2\n\n  line 3\n---\n";
        let p = parse_frontmatter(md);
        assert_eq!(p.description.as_deref(), Some("line 1 line 2\nline 3"));
    }

    #[test]
    fn parse_quoted_value() {
        let md = "---\nname: \"my-skill\"\n---\n";
        let p = parse_frontmatter(md);
        assert_eq!(p.name.as_deref(), Some("my-skill"));
    }

    #[test]
    fn parse_ignores_other_fields() {
        let md = "---\nname: foo\ndisable-model-invocation: false\ndescription: bar\n---\n";
        let p = parse_frontmatter(md);
        assert_eq!(p.name.as_deref(), Some("foo"));
        assert_eq!(p.description.as_deref(), Some("bar"));
    }

    #[test]
    fn parse_marketplace_frontmatter_version() {
        let md = "---\nname: foo\nversion: \"1.4.2\"\ndescription: bar\n---\n";
        let parsed = parse_frontmatter(md);
        assert_eq!(parsed.version.as_deref(), Some("1.4.2"));
    }

    #[test]
    fn marketplace_categories_are_deterministic_and_fall_back_to_other() {
        assert_eq!(
            classify_marketplace_skill("React security audit automation"),
            vec![
                "automation".to_string(),
                "development".to_string(),
                "security".to_string()
            ]
        );
        assert_eq!(
            classify_marketplace_skill("unclassifiable-token"),
            vec!["other".to_string()]
        );
    }

    #[test]
    fn marketplace_sort_uses_requested_metric() {
        let mut skills = vec![
            MarketplaceSkill {
                id: "a".to_string(),
                stars: 2,
                total_installs: 100,
                downloads_7d: 50,
                ..Default::default()
            },
            MarketplaceSkill {
                id: "b".to_string(),
                stars: 20,
                total_installs: 10,
                downloads_7d: 5,
                ..Default::default()
            },
        ];
        sort_marketplace_skills(&mut skills, MarketplaceSort::Stars);
        assert_eq!(skills[0].id, "b");
        sort_marketplace_skills(&mut skills, MarketplaceSort::Installs);
        assert_eq!(skills[0].id, "a");
    }

    #[test]
    fn marketplace_rejects_non_github_sources_and_unsafe_archive_paths() {
        assert!(github_repo_parts("open.feishu.cn").is_none());
        assert_eq!(
            github_repo_parts("vercel-labs/agent-skills"),
            Some(("vercel-labs", "agent-skills"))
        );
        assert!(safe_archive_relative_path(Path::new("root/../escape")).is_err());
        assert!(safe_archive_relative_path(Path::new("../escape")).is_err());
        assert!(safe_archive_relative_path(Path::new("/root/escape")).is_err());
        assert_eq!(
            safe_archive_relative_path(Path::new("root/skills/demo")).unwrap(),
            PathBuf::from("skills/demo")
        );
    }

    #[test]
    fn marketplace_finds_matching_skill_directory_in_git_tree() {
        let tree = GithubTree {
            tree: vec![
                GithubTreeEntry {
                    path: "other/SKILL.md".to_string(),
                    kind: "blob".to_string(),
                },
                GithubTreeEntry {
                    path: "skills/review-code/SKILL.md".to_string(),
                    kind: "blob".to_string(),
                },
            ],
        };
        let skill = MarketplaceSkill {
            name: "review-code".to_string(),
            skill_id: "review-code".to_string(),
            ..Default::default()
        };
        assert_eq!(
            find_skill_markdown_path(&tree, &skill).as_deref(),
            Some("skills/review-code/SKILL.md")
        );
    }

    #[test]
    fn marketplace_supports_a_repository_root_skill() {
        let tree = GithubTree {
            tree: vec![GithubTreeEntry {
                path: "SKILL.md".to_string(),
                kind: "blob".to_string(),
            }],
        };
        let skill = MarketplaceSkill {
            name: "root-skill".to_string(),
            skill_id: "root-skill".to_string(),
            ..Default::default()
        };
        assert_eq!(
            find_skill_markdown_path(&tree, &skill).as_deref(),
            Some("SKILL.md")
        );

        let root = std::env::temp_dir().join(format!("aeroric-root-skill-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("SKILL.md"), "---\nname: root-skill\n---\n").unwrap();
        assert_eq!(locate_extracted_skill(&root, &skill).unwrap(), root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn marketplace_does_not_choose_an_ambiguous_skill_directory() {
        let root = std::env::temp_dir().join(format!("aeroric-skill-locate-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::create_dir_all(root.join("beta")).unwrap();
        fs::write(root.join("alpha/SKILL.md"), "---\nname: alpha\n---\n").unwrap();
        fs::write(root.join("beta/SKILL.md"), "---\nname: beta\n---\n").unwrap();
        let skill = MarketplaceSkill {
            name: "missing".to_string(),
            skill_id: "missing".to_string(),
            ..Default::default()
        };
        let result = locate_extracted_skill(&root, &skill);
        let _ = fs::remove_dir_all(&root);
        assert!(result.is_err());
    }
}
