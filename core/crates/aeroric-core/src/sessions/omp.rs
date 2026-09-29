//! omp 族的路径层（P3-04 第二片的 omp 半边）。
//!
//! 逐字迁移自宿主 `src-tauri/src/session_omp.rs` 的 `omp_project_dir_name`、
//! `omp_session_id_from_file_name`，以及允许根的两段收集。编码规则镜像 omp 上游
//! `session-paths.ts`（home 相对 `-<rel>`、temp `-tmp<rel>`、否则 `--<absolute>--`，
//! `/ \ :` → `-`）——手写约束 5 要求「同一 cwd 两侧算出同一目录名」的用例，
//! 金样在 `tests/session_discovery.rs`。
//!
//! **权限边界（手写约束 2 ／ AGENTS.md『Native OMP session history boundary』）**：
//! core 把托管根与外部历史根分成两个函数，外部那一支（`~/.omp/agent/sessions` 与
//! `~/.omp/profiles/*/agent/sessions`）是**旧客户端现存行为**，由宿主自行组合调用；
//! 新 core 消费方只许 `~/.aeroric/agent-homes/**`（capability 默认 false，开启需
//! 满足 ADR-001 §5 的评审）。既不借迁移静默扩权限，也不删旧客户端行为。

use std::fs;
use std::path::{Path, PathBuf};

/// omp 的 cwd→会话目录名编码。`home` 由调用方注入；`None` 时跳过 home 相对分支。
pub fn omp_project_dir_name(home: Option<&Path>, project_path: &Path) -> String {
    let canonical_cwd = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    let home = home.map(|home| home.canonicalize().unwrap_or_else(|_| home.to_path_buf()));
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

/// 从文件名提取会话 id:`<ISO时间戳>_<uuidv7>.jsonl` 的 `_` 之后一段。
pub fn omp_session_id_from_file_name(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let stem = file_name.strip_suffix(".jsonl")?;
    let (_, id) = stem.split_once('_')?;
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

/// 托管根的会话目录：`<home>/.aeroric/agent-homes/*\/sessions`。
///
/// 宿主原文**不做 `is_dir()` 过滤**（自定义档案 home 在注册时未必已建 sessions），
/// 逐字保留——加过滤会让「先算根、后建目录」的时序变脆。
pub fn omp_managed_session_roots(home: &Path) -> Vec<PathBuf> {
    let root = home.join(".aeroric").join("agent-homes");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path().join("sessions"))
        .collect()
}

/// 外部历史根：用户直接跑系统 omp 的 `~/.omp/agent/sessions`，以及 profile 作用域的
/// `<home>/.omp/profiles/<name>/agent/sessions`（上游 `dirs.ts::resolveActiveAgentDirOverride`
/// 让 profile 优先于 `PI_CODING_AGENT_DIR`）。只放行 `<profile>/agent/sessions`
/// 这一层，不是整个 `~/.omp`。
///
/// **这条支是旧客户端行为，新 core 消费方默认不得调用**（capability=false，
/// AGENTS.md『Native OMP session history boundary』；开启需 ADR-001 §5 评审）。
pub fn omp_external_history_session_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots = vec![home.join(".omp").join("agent").join("sessions")];
    if let Ok(entries) = fs::read_dir(home.join(".omp").join("profiles")) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                roots.push(entry.path().join("agent").join("sessions"));
            }
        }
    }
    roots
}
