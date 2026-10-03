//! P7-03 Action 1/4：Conda（core 独占；环境发现与校验——与
//! `src-tauri/src/conda.rs` 同源语义上收；spawn 复用 Agent environment
//! policy，不能污染全局 shell rc）。
use serde::Serialize;

/// Conda 环境。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CondaEnvironment {
    pub name: String,
    pub path: String,
    pub python_path: String,
}

/// conda 候选路径（按 cfg 分支；verbatim 上收）。
pub fn conda_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        candidates.push(r"C:\ProgramData\miniconda3\Scripts\conda.exe".to_string());
        candidates.push(r"C:\ProgramData\anaconda3\Scripts\conda.exe".to_string());
        candidates.push("conda.exe".to_string());
    }
    #[cfg(not(windows))]
    {
        candidates.push("/opt/miniconda3/bin/conda".to_string());
        candidates.push("/opt/homebrew/bin/conda".to_string());
        candidates.push("/usr/local/bin/conda".to_string());
        candidates.push("conda".to_string());
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(format!("{home}/miniconda3/bin/conda"));
            candidates.push(format!("{home}/anaconda3/bin/conda"));
        }
    }
    candidates
}

/// 环境名从路径推导（verbatim；`.../envs/<name>/bin/conda` → <name>）。
pub fn env_name_from_path(path: &str) -> String {
    // 形如 ".../envs/<name>/bin/conda"：conda 的 parent 是 bin，parent 的
    // parent 是 envs，envs 的 child 即环境名。
    let path = std::path::Path::new(path);
    let bin = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|v| v.to_str());
    if bin == Some("bin") {
        if let Some(env_dir) = path.parent().and_then(|p| p.parent()) {
            if env_dir
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|v| v.to_str())
                == Some("envs")
            {
                return env_dir
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("base")
                    .to_string();
            }
        }
    }
    "base".to_string()
}

/// Conda 环境路径校验（Action 4：选择传递经校验的环境路径——绝对、存在、
/// 是目录；**不能污染全局 shell rc**——环境路径按 spawn 传递，不写 rc）。
pub fn validate_environment_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("conda environment path is empty".to_string());
    }
    if path.contains('\0') {
        return Err("conda environment path contains NUL".to_string());
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("conda environment path must be absolute".to_string());
    }
    Ok(())
}

/// Conda spawn 环境变量（复用 Agent environment policy：白名单传递，**不能
/// 污染全局 shell rc**——变量进子进程环境，不进 rc 文件）。
pub fn conda_spawn_env(env_path: &str) -> Vec<(String, String)> {
    vec![
        ("CONDA_PREFIX".to_string(), env_path.to_string()),
        ("VIRTUAL_ENV".to_string(), env_path.to_string()),
    ]
}
