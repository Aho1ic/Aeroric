//! 引导页的“已安装 Agent”检测（UI 改版 M4）：只看可执行文件是否存在，**不执行它们**。
//!
//! GUI 启动的宿主 PATH 很短，所以除 PATH 外还查常见安装位置；PATH 与 home 都由调用方注入，
//! 测试用临时目录。

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedAgent {
    pub agent: &'static str,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

/// (agent id, 可执行文件名候选)。
const CANDIDATES: [(&str, &[&str]); 4] = [
    ("claude", &["claude"]),
    ("codex", &["codex"]),
    ("dsh", &["dsh", "deepseek-harness"]),
    ("omp", &["omp"]),
];

pub fn detect_agents(path_var: Option<&str>, home: &Path) -> Vec<DetectedAgent> {
    let mut dirs: Vec<PathBuf> = path_var
        .map(|value| std::env::split_paths(value).collect())
        .unwrap_or_default();
    for extra in [
        home.join(".local/bin"),
        home.join(".claude/local"),
        home.join(".npm-global/bin"),
        home.join(".cargo/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ] {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    CANDIDATES
        .iter()
        .map(|(agent, names)| {
            let path = dirs
                .iter()
                .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
                .find(|candidate| is_executable(candidate));
            DetectedAgent {
                agent,
                installed: path.is_some(),
                path,
            }
        })
        .collect()
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_executables_on_path_and_in_known_locations_without_running_them() {
        let root = std::env::temp_dir().join(format!("aeroric-detect-{}", std::process::id()));
        let bin = root.join("bin");
        let home = root.join("home");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join(".local/bin")).unwrap();
        let write = |path: PathBuf, mode: u32| {
            std::fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
        };
        write(bin.join("codex"), 0o755);
        write(bin.join("claude"), 0o644);
        write(home.join(".local/bin/deepseek-harness"), 0o755);

        let path_var = bin.to_string_lossy().to_string();
        let found = detect_agents(Some(&path_var), &home);
        let by_id = |id: &str| found.iter().find(|a| a.agent == id).unwrap().clone();
        assert!(by_id("codex").installed);
        assert_eq!(by_id("codex").path, Some(bin.join("codex")));
        #[cfg(unix)]
        assert!(
            !by_id("claude").installed || by_id("claude").path != Some(bin.join("claude")),
            "a non-executable file is not an installation"
        );
        assert_eq!(
            by_id("dsh").path,
            Some(home.join(".local/bin/deepseek-harness"))
        );
        assert_eq!(found.len(), 4);
        std::fs::remove_dir_all(&root).ok();
    }
}
