//! omp(rpc-ui)会话发现与注册。
//!
//! omp 的会话文件是明文 JSONL:`~/.omp/agent/sessions/<encoded-cwd>/<ISO>_<uuidv7>.jsonl`
//! (编码规则见 `omp_project_dir_name`:home 内 `-<relative>`、temp 内 `-tmp<relative>`、
//! 其它 `--<absolute>--`,`/ \ :` 统一替换为 `-`)。文件懒创建:首个 assistant 消息
//! 产出时才落盘,但文件名(含 uuid)在会话创建时就已确定——因此优先信任
//! `get_state` 返回的 sessionFile,发现线程只作兜底。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager};

use crate::session::OmpSessionInfo;
use crate::TaskManager;

pub(crate) fn register_omp_session(
    app: &AppHandle,
    task_id: &str,
    session_id: &str,
    session_path: &Path,
) {
    let path_string = session_path.to_string_lossy().into_owned();
    {
        let tm = app.state::<TaskManager>();
        let mut claimed = tm.claimed_session_paths.lock();
        if !claimed.insert(path_string.clone()) {
            return;
        }
        tm.omp_sessions.lock().insert(
            task_id.to_string(),
            OmpSessionInfo {
                session_id: session_id.to_string(),
                session_path: path_string.clone(),
            },
        );
    }
    let _ = app.emit(
        "task-session",
        serde_json::json!({
            "task_id": task_id,
            "session_id": session_id,
            "session_path": path_string,
            "codex_like": false,
            "family": "omp",
        }),
    );
}

/// omp 会话目录名编码(对应 omp 官方 `session-paths.ts` 的 `getDefaultSessionDirName`):
/// home 内 `-<relative>`、系统 temp 内 `-tmp<relative>`、其余 `--<absolute>--`;
/// `/` `\` `:` 替换为 `-`(18.1.10 无 UTF-16 转义)。
pub(crate) fn omp_project_dir_name(project_path: &Path) -> String {
    let canonical_cwd = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    let home = crate::platform::home_dir().map(|home| home.canonicalize().unwrap_or(home));
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .unwrap_or(std::env::temp_dir());

    let encode_relative = |prefix: &str, relative: &Path| -> String {
        let encoded = relative.to_string_lossy().replace(['/', '\\', ':'], "-");
        format!("{prefix}{encoded}")
    };
    let encode_absolute = |absolute: &Path| -> String {
        let stripped = absolute
            .to_string_lossy()
            .trim_start_matches(['/', '\\'])
            .replace(['/', '\\', ':'], "-");
        format!("--{stripped}--")
    };

    if let Some(home) = home.as_ref() {
        if let Ok(relative) = canonical_cwd.strip_prefix(home) {
            if relative.as_os_str().is_empty() {
                return "-".to_string();
            }
            return encode_relative("-", relative);
        }
    }
    if let Ok(relative) = canonical_cwd.strip_prefix(&temp_root) {
        if relative.as_os_str().is_empty() {
            return "-tmp".to_string();
        }
        return encode_relative("-tmp", relative);
    }
    encode_absolute(&canonical_cwd)
}

/// omp 族 agent 的会话根目录(托管 home 优先)。
pub(crate) fn omp_sessions_dir_for(agent: &str) -> Result<PathBuf, String> {
    Ok(crate::omp_home::omp_home_for(agent)?.join("sessions"))
}

/// 会话读取的路径白名单:内建 omp 的托管 home sessions + 用户自己的
/// `~/.omp/agent/sessions`(用户可能手动用系统 omp 在同一项目里跑过会话)。
/// 自定义 omp 档案(home 为 `agent-homes/{id}`)的路径读取走 task 里持久化的
/// session 路径,Phase 7 接入时按 agent 校验。
pub(crate) fn omp_session_allowed_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(managed) = crate::omp_home::omp_home_for("omp") {
        roots.push(managed.join("sessions"));
    }
    if let Some(default_dir) = crate::platform::home_dir() {
        roots.push(default_dir.join(".omp").join("agent").join("sessions"));
    }
    roots
}

/// 从文件名提取会话 id:`<ISO时间戳>_<uuidv7>.jsonl` 的 `_` 之后一段。
pub(crate) fn omp_session_id_from_file_name(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let stem = file_name.strip_suffix(".jsonl")?;
    let (_, id) = stem.split_once('_')?;
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

/// 扫描 sessions 目录,返回 mtime 晚于 `since_ms` 的最新 `.jsonl` 会话文件。
pub(crate) fn discover_omp_session_since(
    agent: &str,
    project_path: &Path,
    since_ms: u128,
) -> Option<(String, PathBuf)> {
    let dir = omp_sessions_dir_for(agent)
        .ok()?
        .join(omp_project_dir_name(project_path));
    let entries = fs::read_dir(dir).ok()?;
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        // 单个条目元数据异常只跳过,不能让整个发现流程返回 None。
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(modified_ms) = modified.duration_since(UNIX_EPOCH).map(|d| d.as_millis()) else {
            continue;
        };
        if modified_ms <= since_ms {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_time, _)| modified > *best_time)
        {
            best = Some((modified, path));
        }
    }
    let (_, path) = best?;
    let session_id = omp_session_id_from_file_name(&path)?;
    Some((session_id, path))
}

/// 兜底 watcher:`get_state` 没给 sessionFile(懒创建)时轮询会话目录,直到文件
/// 落盘或 `stop` 置位。发现后注册会话;从未落盘则静默结束(短会话可能根本没有
/// assistant 输出,同样不产生文件)。
pub(crate) fn spawn_omp_session_watcher(
    app: AppHandle,
    task_id: String,
    agent: String,
    project_path: PathBuf,
    stop: std::sync::Arc<AtomicBool>,
    since_ms: u128,
) {
    thread::spawn(move || loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        if let Some((session_id, path)) =
            discover_omp_session_since(&agent, &project_path, since_ms)
        {
            register_omp_session(&app, &task_id, &session_id, &path);
            return;
        }
        if stop.load(Ordering::Acquire) {
            return;
        }
        thread::sleep(Duration::from_millis(500));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_extracts_from_omp_file_name() {
        let path = Path::new(
            "/home/u/.omp/agent/sessions/-tmp-x/2026-09-06T10-30-00_0198c7a2-7f4a-7d3e-9f2a-3b6c5d4e4f50.jsonl",
        );
        assert_eq!(
            omp_session_id_from_file_name(path).as_deref(),
            Some("0198c7a2-7f4a-7d3e-9f2a-3b6c5d4e4f50")
        );
        assert_eq!(
            omp_session_id_from_file_name(Path::new("garbage.jsonl")),
            None
        );
    }

    #[test]
    fn project_dir_encoding_matches_omp_rules() {
        let root = std::env::temp_dir().join(format!("omp-enc-{}", std::process::id()));
        let project = root.join("同步 demo");
        fs::create_dir_all(&project).unwrap();
        // temp 内 → "-tmp<relative>":仅 / \ : 替换为 "-",空格保留,前缀无附加分隔符
        // (对应 omp encodeRelativeSessionDirName("-tmp", relative))。
        let expected = format!("-tmpomp-enc-{}-同步 demo", std::process::id());
        assert_eq!(omp_project_dir_name(&project), expected);
        // temp 根目录本身(tempRelative === "")→ 裸 "-tmp"。
        assert_eq!(omp_project_dir_name(&std::env::temp_dir()), "-tmp");
        fs::remove_dir_all(&root).ok();
    }
}
