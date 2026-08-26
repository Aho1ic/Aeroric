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
    #[serde(
        rename = "dshAgentPreset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dsh_agent_preset: Option<String>,
    // 前端统一展示 low/medium/high/xhigh/max/ultra；Claude 启动时将 ultra
    // 转成原生命令需要的 `ultracode`。
    #[serde(
        rename = "reasoningEffort",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<String>,
    // Codex 与 Claude 都支持 standard/fast，启动层负责映射到各自 CLI 参数。
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
    #[serde(
        rename = "dshSessionId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dsh_session_id: Option<String>,
    #[serde(
        rename = "dshSessionPath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dsh_session_path: Option<String>,
    #[serde(rename = "sessionAgent", skip_serializing_if = "Option::is_none")]
    pub session_agent: Option<String>,
    #[serde(
        rename = "sessionCodexLike",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub session_codex_like: Option<bool>,
    /// 会话协议族("claude"/"codex"/"dsh");读取优先于 sessionCodexLike。
    #[serde(
        rename = "sessionFamily",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub session_family: Option<String>,
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

fn projects_dir() -> Result<PathBuf, String> {
    Ok(aeroric_dir()?.join("projects"))
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
    ensure_private_dir(&aeroric_dir()?)
}

fn ensure_project_dir(project_id: &str) -> Result<(), String> {
    ensure_aeroric_dirs()?;
    ensure_private_dir(&projects_dir()?)?;
    ensure_private_dir(&project_dir(project_id)?)
}

fn ensure_terminal_history_dir() -> Result<(), String> {
    ensure_aeroric_dirs()?;
    ensure_private_dir(&terminal_history_dir()?)
}

pub(crate) fn ensure_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        // 目录的 ACE 带继承标记:之后在里面新建的文件天生就只有本人能读,
        // 不依赖每个写入点自己记得收紧。
        warn_once_on_acl_failure(path, windows_acl::restrict_to_current_user(path, true));
    }
    Ok(())
}

/// 数据目录解析结果:拿不到 `~/.aeroric` 时退到临时目录,而不是让启动失败。
pub(crate) struct DataDir {
    pub path: PathBuf,
    /// `Some` 表示走的是退路,内容是原始失败原因,供启动诊断展示。
    pub degraded_reason: Option<String>,
}

/// 解析可写的数据目录,永不失败。
///
/// 为什么需要它:`~/.aeroric` 建不出来的场景是真实存在的(home 只读、磁盘满、
/// 企业策略挡住、路径被同名文件占住)。原先这条路径上的错误会一路冒到
/// `.expect()`,在窗口出现之前就把进程打死,用户只看到图标闪一下——没有日志、
/// 没有弹窗、无从判断。退到临时目录后应用能正常起来,数据不跨重启保留,
/// 由前端横幅明确告知,把「静默崩溃」换成「可见降级」。
pub(crate) fn resolve_data_dir() -> DataDir {
    resolve_data_dir_from(aeroric_dir().and_then(|path| {
        ensure_private_dir(&path)?;
        Ok(path)
    }))
}

/// `resolve_data_dir` 的决策部分,与「主目录是哪个」解耦以便测试。
///
/// 拆出来是因为真实路径取自 `$HOME`:测试里改环境变量是进程级的,会和并行跑的
/// 上千个测试互相干扰。把判断收成纯函数后,退路选择可以直接断言。
fn resolve_data_dir_from(primary: Result<PathBuf, String>) -> DataDir {
    match primary {
        Ok(path) => DataDir {
            path,
            degraded_reason: None,
        },
        Err(reason) => {
            // 临时目录带 pid 后缀:同机多实例各用一份,不会互相踩 SQLite 锁。
            let fallback =
                std::env::temp_dir().join(format!("aeroric-fallback-{}", std::process::id()));
            let note = match ensure_private_dir(&fallback) {
                Ok(()) => reason,
                // 连临时目录都建不出来时仍然返回该路径:调用方会继续降级到
                // 内存库,这里只把两层原因都带上。
                Err(fallback_error) => {
                    format!("{reason}; fallback dir also failed: {fallback_error}")
                }
            };
            DataDir {
                path: fallback,
                degraded_reason: Some(note),
            }
        }
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn load_projects() -> Result<Vec<Project>, String> {
    let _guard = projects_lock().lock();
    load_projects_unlocked()
}

fn load_projects_unlocked() -> Result<Vec<Project>, String> {
    ensure_aeroric_dirs()?;
    let path = projects_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    ensure_private_file_permissions(&path)?;
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
    atomic_write_private(&projects_path()?, &raw)
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
    ensure_project_dir(&project_id)?;
    let path = tasks_path(&project_id)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    ensure_private_file_permissions(&path)?;
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
    atomic_write_private(&path, &raw)
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
    ensure_private_file_permissions(path)?;
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
///
/// Windows 没有 mode 位,对应动作是把 DACL 收紧成"只有当前用户"。同样在 rename
/// **之前**做,目标路径上不会出现一瞬间的宽松权限。
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
    #[cfg(windows)]
    {
        // 显式 ACE 会跟着文件一起被 rename 带到目标路径上,所以这里收紧就够了。
        // 父目录通常已经被 `ensure_private_dir` 收紧过、这一步只是兜底(文件可能
        // 是更早的版本、或者别的工具建出来的)。
        warn_once_on_acl_failure(&tmp, windows_acl::restrict_to_current_user(&tmp, false));
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
        warn_once_on_acl_failure(path, windows_acl::restrict_to_current_user(path, false));
    }
    Ok(())
}

/// Windows 侧 0o600 / 0o700 的等价物:把 DACL 换成"只有当前用户"的一条 ACE。
///
/// 两个细节决定它是否真的收紧了:
///   * `PROTECTED_DACL_SECURITY_INFORMATION` —— 不加这个,父目录继承下来的 ACE
///     (域环境里常见 `Users: 读取`)会留在新表里,等于白改。
///   * 目录用 `SUB_CONTAINERS_AND_OBJECTS_INHERIT` —— 让里面后续新建的文件自动继承,
///     不然每个写入点都得自己记得收紧。
///
/// 只授当前用户,不额外授 Administrators / SYSTEM:这与 Unix 的 0o600 语义对齐
/// (root 靠特权绕过,而不是靠 ACL 里的一条项)。
#[cfg(windows)]
mod windows_acl {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE,
        SET_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE,
        PROTECTED_DACL_SECURITY_INFORMATION, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY,
        TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct OwnedToken(HANDLE);

    impl Drop for OwnedToken {
        fn drop(&mut self) {
            // SAFETY: 句柄来自下面成功的 OpenProcessToken,只在这里关一次。
            unsafe { CloseHandle(self.0) };
        }
    }

    /// 取当前进程 token 里的用户 SID。返回整块缓冲区,因为 SID 指针指向它内部,
    /// 缓冲区一旦释放指针就悬空 —— 调用方必须让它活到用完 SID 为止。
    fn current_user_sid_buffer() -> Result<Vec<u64>, String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: 只查询当前进程自己的 token。
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(format!(
                "OpenProcessToken failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let token = OwnedToken(token);

        let mut needed: u32 = 0;
        // SAFETY: 传 null + 0 长度是文档规定的"先问需要多大"调用,失败也只回填 needed。
        unsafe {
            GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        }
        if needed == 0 {
            return Err("GetTokenInformation reported a zero-sized TOKEN_USER".to_string());
        }
        // 用 u64 而不是 u8 装:TOKEN_USER 里有指针,必须按指针对齐。
        let mut buffer = vec![0_u64; needed as usize / 8 + 1];
        // SAFETY: buffer 至少 needed 字节且按 8 字节对齐。
        let ok = unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr() as *mut c_void,
                needed,
                &mut needed,
            )
        };
        if ok == 0 {
            return Err(format!(
                "GetTokenInformation failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(buffer)
    }

    /// 把 `path` 的 DACL 换成"只有当前用户完全控制"。`inheritable` 给目录用。
    pub(super) fn restrict_to_current_user(path: &Path, inheritable: bool) -> Result<(), String> {
        let sid_buffer = current_user_sid_buffer()?;
        // SAFETY: 缓冲区刚由 GetTokenInformation(TokenUser) 填满,布局就是 TOKEN_USER;
        // sid 指向 sid_buffer 内部,下面用完之前 sid_buffer 一直活着。
        let sid = unsafe { (*(sid_buffer.as_ptr() as *const TOKEN_USER)).User.Sid };

        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: if inheritable {
                SUB_CONTAINERS_AND_OBJECTS_INHERIT
            } else {
                NO_INHERITANCE
            },
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: sid as *mut u16,
            },
        };

        let mut acl: *mut ACL = std::ptr::null_mut();
        // SAFETY: 一条 entry;旧 ACL 传 null 表示"从零建",而不是在原表上增量加项 ——
        // 增量加会把继承下来的宽松 ACE 留着,那就没收紧。
        let status = unsafe { SetEntriesInAclW(1, &entry, std::ptr::null(), &mut acl) };
        if status != ERROR_SUCCESS {
            return Err(format!("SetEntriesInAclW failed with Win32 error {status}"));
        }

        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        // SAFETY: acl 来自上面成功的 SetEntriesInAclW;wide 以 NUL 结尾。
        let status = unsafe {
            SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl,
                std::ptr::null_mut(),
            )
        };
        // SAFETY: SetEntriesInAclW 分配的 ACL 必须用 LocalFree 归还。
        unsafe { LocalFree(acl as *mut c_void) };
        if status != ERROR_SUCCESS {
            return Err(format!(
                "SetNamedSecurityInfoW failed with Win32 error {status}"
            ));
        }
        Ok(())
    }
}

/// DACL 收紧失败只警告,不让调用方失败,而且整个进程只说一次。
///
/// 不能硬失败:`ensure_private_dir` 经由 `ensure_aeroric_dirs` 挂在几乎每个存储
/// 操作前面,而 `SetNamedSecurityInfoW` 在不支持 ACL 的卷上(FAT32 的 U 盘、某些
/// 网络盘)必然返回错误 —— 把它变成致命错误等于让这些用户连项目列表都打不开。
/// 所以退回"尽力而为 + 讲清楚",这与 Unix 侧 `atomic_write_private` 里那次
/// 防御性 `set_permissions` 用 `let _ =` 吞掉错误是同一个取舍。
///
/// 只打一次是因为上面那个调用频率:失败通常是卷本身不支持,重复几百遍没有新信息。
#[cfg(windows)]
fn warn_once_on_acl_failure(path: &Path, result: Result<(), String>) {
    static WARNED: OnceLock<()> = OnceLock::new();
    if let Err(error) = result {
        if WARNED.set(()).is_ok() {
            eprintln!(
                "[storage] could not restrict {} to the current user; \
                 files on this volume may be readable by other local accounts: {error}",
                path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正常路径不许被标成降级——否则每次启动都会弹一条假告警。
    #[test]
    fn healthy_data_dir_is_not_reported_as_degraded() {
        let resolved = resolve_data_dir_from(Ok(PathBuf::from("/some/real/dir")));
        assert_eq!(resolved.path, PathBuf::from("/some/real/dir"));
        assert!(resolved.degraded_reason.is_none());
    }

    /// 主目录不可用时必须退到别处并带上原因,而不是把错误一路抛到 `.expect()`。
    /// 这条测试守的就是「启动前静默崩溃」这个回归。
    #[test]
    fn unusable_data_dir_falls_back_and_keeps_the_reason() {
        let resolved = resolve_data_dir_from(Err("home is read-only".to_string()));
        assert_ne!(resolved.path, PathBuf::from(""));
        let reason = resolved
            .degraded_reason
            .expect("a fallback must always explain itself");
        // 原始原因要原样带出:用户和我们都靠它判断到底是磁盘满还是权限问题。
        assert!(reason.contains("home is read-only"), "{reason}");
    }

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

    #[cfg(not(windows))]
    #[test]
    fn ensure_private_dir_tightens_existing_directory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "aeroric-private-dir-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

        ensure_private_dir(&dir).unwrap();

        let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 唯一的临时目录名。Windows 测试与上面的 Unix 测试并行跑,共用名字会互删。
    #[cfg(windows)]
    fn unique_temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aeroric-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    // Windows 侧没有 mode 位可断言,而 CI 的 Windows job 只编译不执行测试
    // (见 checks.yml 的 "Compile Windows Rust tests"),所以这里不去把 DACL 读回来
    // 逐条比对 —— 那些代码永远不会在 CI 里跑,只会变成没人验证的第二套 FFI。
    // 真正值得钉住的是两件在 Windows 开发机上 `cargo test` 能立刻暴露的事:
    // 收紧本身没报错,以及收紧之后应用还读写得动自己的凭据文件(ACE 写错就会
    // 把自己一起关在外面,那比原来的宽松权限更糟)。

    #[cfg(windows)]
    #[test]
    fn tightening_a_directory_keeps_it_usable_by_this_process() {
        let dir = unique_temp_dir("private-dir-win");

        ensure_private_dir(&dir).expect("ensure_private_dir succeeds");
        windows_acl::restrict_to_current_user(&dir, true).expect("directory DACL is settable");

        let path = dir.join("inherited.json");
        fs::write(&path, "{}").expect("we can still create files inside our own directory");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_tightened_credential_file_is_still_readable_and_rewritable() {
        let dir = unique_temp_dir("private-file-win");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.json");
        // 先用宽松权限建出来,证明写入路径会把已存在的文件也收紧。
        fs::write(&path, "{}").unwrap();

        atomic_write_private(&path, "{\"password\":\"x\"}").expect("private write succeeds");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"password\":\"x\"}");

        ensure_private_file_permissions(&path).expect("tightening an existing file succeeds");
        atomic_write_private(&path, "{\"password\":\"y\"}").expect("rewrite still succeeds");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"password\":\"y\"}");

        // 临时文件不能留下:rename 之后目录里只该有目标文件。
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "secret.json")
            .collect();
        assert!(leftovers.is_empty(), "unexpected leftovers: {leftovers:?}");
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
