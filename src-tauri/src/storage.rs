use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

static PROJECTS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const MAX_TERMINAL_HISTORY_BYTES: usize = 8 * 1024 * 1024;

// ── Data types (mirror TypeScript interfaces) ────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<ProjectLocation>,
    pub branch: Option<String>,
    #[serde(rename = "lastOpenedAt")]
    pub last_opened_at: i64,
    #[serde(rename = "orderIndex", skip_serializing_if = "Option::is_none")]
    pub order_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    // 缺省=常驻；旧数据无此字段时默认 false，序列化时省略 false 以保持文件简洁。
    #[serde(
        rename = "hiddenFromRail",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub hidden_from_rail: bool,
    // 置顶：在各自分组内排最前，分组折叠时仍露出。桌面与手机共享同一份状态。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum ProjectLocation {
    #[serde(rename = "local")]
    Local { path: String },
    #[serde(rename = "ssh")]
    Ssh {
        #[serde(rename = "connectionId")]
        connection_id: String,
        #[serde(rename = "remotePath")]
        remote_path: String,
    },
    #[serde(rename = "wsl")]
    Wsl {
        distribution: String,
        #[serde(rename = "linuxPath")]
        linux_path: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Task {
    pub id: String,
    #[serde(rename = "projectId")]
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub prompt: String,
    pub agent: String,
    #[serde(rename = "permissionMode")]
    pub permission_mode: String,
    // Per-task agent 行为旋钮。历史上仅存在于前端 Task 里，写盘时被 serde 忽略；
    // 这里补齐以支持重启/resume 后的持久化，也是 reasoning/speed 的搭档字段。
    #[serde(
        rename = "selectedModel",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub selected_model: Option<String>,
    // Codex：minimal/low/medium/high/xhigh；Claude：low/medium/high/xhigh/max/ultracode。
    // 值集合不同，前端各用各的原生值，不做统一映射。
    #[serde(
        rename = "reasoningEffort",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<String>,
    // 目前仅 Claude 支持 fast/normal；Codex 无独立 fast 概念。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(
        rename = "attentionRequestedAt",
        skip_serializing_if = "Option::is_none"
    )]
    pub attention_requested_at: Option<i64>,
    #[serde(rename = "claudeSessionId", skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    #[serde(rename = "claudeSessionPath", skip_serializing_if = "Option::is_none")]
    pub claude_session_path: Option<String>,
    #[serde(rename = "codexSessionId", skip_serializing_if = "Option::is_none")]
    pub codex_session_id: Option<String>,
    #[serde(rename = "codexSessionPath", skip_serializing_if = "Option::is_none")]
    pub codex_session_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    #[serde(rename = "failureReason", skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(rename = "worktreePath", skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(rename = "worktreeBranch", skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    #[serde(rename = "baseBranch", skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(rename = "worktreeDiscarded", skip_serializing_if = "Option::is_none")]
    pub worktree_discarded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i32>,
}

// ── Path helpers ─────────────────────────────────────────────────────────────

pub(crate) fn aeroric_dir() -> Result<PathBuf, String> {
    let home =
        crate::platform::home_dir().ok_or_else(|| "Cannot find home directory".to_string())?;
    Ok(home.join(".aeroric"))
}

fn projects_path() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("projects.json"))
}

fn projects_lock() -> &'static Mutex<()> {
    PROJECTS_LOCK.get_or_init(|| Mutex::new(()))
}

fn tasks_path(project_id: &str) -> Result<PathBuf, String> {
    Ok(project_dir(project_id)?.join("tasks.json"))
}

pub(crate) fn validate_storage_id(id: &str, label: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(format!("Invalid {label} id"));
    }
    Ok(())
}

fn project_dir(project_id: &str) -> Result<PathBuf, String> {
    validate_storage_id(project_id, "project")?;
    Ok(aeroric_dir()?.join("projects").join(project_id))
}

fn terminal_history_dir() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("terminal-history"))
}

fn safe_task_history_name(task_id: &str) -> Result<String, String> {
    validate_storage_id(task_id, "task")?;
    let trimmed = task_id.trim();
    Ok(format!("{trimmed}.log"))
}

pub(crate) fn terminal_history_path(task_id: &str) -> Result<PathBuf, String> {
    Ok(terminal_history_dir()?.join(safe_task_history_name(task_id)?))
}

pub(crate) fn ensure_aeroric_dirs() -> Result<(), String> {
    fs::create_dir_all(aeroric_dir()?).map_err(|e| e.to_string())
}

fn ensure_project_dir(project_id: &str) -> Result<(), String> {
    fs::create_dir_all(project_dir(project_id)?).map_err(|e| e.to_string())
}

fn ensure_terminal_history_dir() -> Result<(), String> {
    fs::create_dir_all(terminal_history_dir()?).map_err(|e| e.to_string())
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn load_projects() -> Result<Vec<Project>, String> {
    let _guard = projects_lock().lock();
    load_projects_unlocked()
}

fn load_projects_unlocked() -> Result<Vec<Project>, String> {
    let path = projects_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_projects(projects: Vec<Project>) -> Result<(), String> {
    let _guard = projects_lock().lock();
    save_projects_unlocked(&projects)
}

fn save_projects_unlocked(projects: &[Project]) -> Result<(), String> {
    ensure_aeroric_dirs()?;
    let raw = serde_json::to_string_pretty(&projects).map_err(|e| e.to_string())?;
    atomic_write(&projects_path()?, &raw)
}

pub(crate) fn update_projects<R>(
    update: impl FnOnce(&mut Vec<Project>) -> Result<R, String>,
) -> Result<(R, Vec<Project>), String> {
    let _guard = projects_lock().lock();
    let mut projects = load_projects_unlocked()?;
    let result = update(&mut projects)?;
    save_projects_unlocked(&projects)?;
    Ok((result, projects))
}

#[tauri::command]
pub fn load_project_tasks(project_id: String) -> Result<Vec<Task>, String> {
    let path = tasks_path(&project_id)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_project_tasks(project_id: String, tasks: Vec<Task>) -> Result<(), String> {
    ensure_project_dir(&project_id)?;
    let path = tasks_path(&project_id)?;
    if tasks.is_empty() {
        // Remove the file if no tasks left
        if path.exists() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let raw = serde_json::to_string_pretty(&tasks).map_err(|e| e.to_string())?;
    atomic_write(&path, &raw)
}

pub(crate) fn append_task_terminal_history(task_id: &str, data: &str) -> Result<(), String> {
    if data.is_empty() {
        return Ok(());
    }
    ensure_terminal_history_dir()?;
    let path = terminal_history_path(task_id)?;
    append_terminal_history_file(&path, data, MAX_TERMINAL_HISTORY_BYTES)
}

pub(crate) fn truncate_task_terminal_history(task_id: &str) -> Result<(), String> {
    ensure_terminal_history_dir()?;
    let path = terminal_history_path(task_id)?;
    atomic_write_private(&path, "")
}

/// 终端历史当前字节数(文件缺失=0)。terminal hub 的水位初始化用。
pub(crate) fn task_terminal_history_len(task_id: &str) -> u64 {
    terminal_history_path(task_id)
        .ok()
        .and_then(|path| fs::metadata(path).ok())
        .map(|meta| meta.len())
        .unwrap_or(0)
}

/// 读取终端历史尾部(≤ max_bytes,起点对齐 UTF-8 字符边界)。
/// 返回 (文件总长, 尾部内容);远程终端流以总长为快照水位。
pub(crate) fn read_task_terminal_history_tail(
    task_id: &str,
    max_bytes: u64,
) -> Result<(u64, String), String> {
    let path = terminal_history_path(task_id)?;
    read_terminal_history_tail_from_path(&path, max_bytes)
}

fn read_terminal_history_tail_from_path(
    path: &Path,
    max_bytes: u64,
) -> Result<(u64, String), String> {
    use std::io::{Read as _, Seek, SeekFrom};
    if !path.exists() {
        return Ok((0, String::new()));
    }
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let total = file.metadata().map_err(|e| e.to_string())?.len();
    let start = total.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity((total - start) as usize);
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    // 掐头去掉截断的 UTF-8 续字节(0b10xxxxxx)
    let skip = bytes
        .iter()
        .take(4)
        .take_while(|b| (**b & 0b1100_0000) == 0b1000_0000)
        .count();
    let text = String::from_utf8_lossy(&bytes[skip..]).into_owned();
    Ok((total, text))
}

#[tauri::command]
pub async fn read_task_terminal_history(task_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        read_task_terminal_history_tail(&task_id, MAX_TERMINAL_HISTORY_BYTES as u64)
            .map(|(_, history)| history)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn delete_task_terminal_histories(task_ids: Vec<String>) -> Result<(), String> {
    for task_id in task_ids {
        let path = terminal_history_path(&task_id)?;
        if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ── Atomic write (write to tmp then rename) ───────────────────────────────────

/// 原子写入：先写入唯一临时文件，再 rename 到目标路径。
/// 临时文件名包含 pid + 纳秒时间戳，避免并发写入时临时文件相互覆盖。
pub fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let uid = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{file_name}.{uid}.tmp"));
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn utf8_tail(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }
    let mut start = content.len() - max_bytes;
    while start < content.len() && !content.is_char_boundary(start) {
        start += 1;
    }
    &content[start..]
}

fn append_terminal_history_file(path: &Path, data: &str, max_bytes: usize) -> Result<(), String> {
    let existing_len = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if existing_len.saturating_add(data.len() as u64) <= max_bytes as u64 {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).map_err(|e| e.to_string())?;
        ensure_private_file_permissions(path)?;
        return file.write_all(data.as_bytes()).map_err(|e| e.to_string());
    }

    let incoming = utf8_tail(data, max_bytes);
    let retained_bytes = max_bytes.saturating_sub(incoming.len());
    let (_, existing_tail) = read_terminal_history_tail_from_path(path, retained_bytes as u64)?;
    let mut compacted = String::with_capacity(existing_tail.len() + incoming.len());
    compacted.push_str(&existing_tail);
    compacted.push_str(incoming);
    atomic_write_private(path, utf8_tail(&compacted, max_bytes))
}

/// 原子写入,但把结果文件限制为仅所有者可读写 (0o600)。
/// 用于承载明文凭据的文件(数据库/SSH 密码、API key),避免同机其它用户读取。
/// 临时文件一开始就以 0o600 创建,消除 rename 前的 644 窗口。
pub fn atomic_write_private(path: &Path, content: &str) -> Result<(), String> {
    let uid = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(".{file_name}.{uid}.tmp"));

    {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
        file.write_all(content.as_bytes())
            .map_err(|e| e.to_string())?;
    }

    // On existing targets rename inherits the tmp file's 0o600; set it again
    // defensively in case the file already existed with looser bits.
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }

    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn ensure_private_file_permissions(path: &Path) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn atomic_write_private_creates_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("aeroric-priv-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("secret.json");
        // Pre-create with loose perms to prove the writer tightens them.
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        atomic_write_private(&path, "{\"password\":\"x\"}").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "credential file should be owner-only");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"password\":\"x\"}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn ensure_private_file_permissions_tightens_existing_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "aeroric-priv-existing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        ensure_private_file_permissions(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn terminal_history_keeps_only_a_bounded_utf8_tail() {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-terminal-history-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("task.log");

        append_terminal_history_file(&path, "012345", 10).unwrap();
        append_terminal_history_file(&path, "你好世界", 10).unwrap();

        let history = fs::read_to_string(&path).unwrap();
        assert!(history.len() <= 10);
        assert_eq!(history, "5好世界");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn storage_ids_reject_path_traversal() {
        assert!(validate_storage_id("1783647251756", "project").is_ok());
        assert!(validate_storage_id("task_1-safe", "task").is_ok());
        for invalid in ["", " ", "../escape", "a/b", r"a\b", "."] {
            assert!(validate_storage_id(invalid, "test").is_err(), "{invalid}");
        }
    }

    #[test]
    fn legacy_project_without_location_deserializes() {
        let raw = r#"{
          "id":"p1",
          "name":"legacy",
          "path":"/Users/me/work/legacy",
          "lastOpenedAt":1700000000000
        }"#;

        let project: Project = serde_json::from_str(raw).unwrap();

        assert_eq!(project.location, None);
    }

    #[test]
    fn ssh_project_location_round_trips() {
        let raw = r#"{
          "id":"p2",
          "name":"remote",
          "path":"ssh://conn-1/srv/app",
          "location":{"kind":"ssh","connectionId":"conn-1","remotePath":"/srv/app"},
          "lastOpenedAt":1700000000000
        }"#;

        let project: Project = serde_json::from_str(raw).unwrap();

        assert_eq!(
            project.location,
            Some(ProjectLocation::Ssh {
                connection_id: "conn-1".to_string(),
                remote_path: "/srv/app".to_string(),
            })
        );
    }

    #[test]
    fn wsl_project_location_round_trips() {
        let raw = r#"{
          "id":"p3",
          "name":"linux",
          "path":"wsl://Ubuntu-24.04/home/me/app",
          "location":{"kind":"wsl","distribution":"Ubuntu-24.04","linuxPath":"/home/me/app"},
          "lastOpenedAt":1700000000000
        }"#;

        let project: Project = serde_json::from_str(raw).unwrap();

        assert_eq!(
            project.location,
            Some(ProjectLocation::Wsl {
                distribution: "Ubuntu-24.04".to_string(),
                linux_path: "/home/me/app".to_string(),
            })
        );
    }
}
