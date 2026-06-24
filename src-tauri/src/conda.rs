use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::ssh::SshConnection;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CondaEnvironment {
    name: String,
    path: String,
    python_path: String,
}

fn conda_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("CONDA_EXE") {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(PathBuf::from("/opt/miniconda3/bin/conda"));
    candidates.push(PathBuf::from("/opt/homebrew/bin/conda"));
    candidates.push(PathBuf::from("/usr/local/bin/conda"));
    candidates.push(PathBuf::from("conda"));

    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(&home).join("miniconda3/bin/conda"));
        candidates.push(PathBuf::from(&home).join("anaconda3/bin/conda"));
        candidates.push(PathBuf::from(&home).join("mambaforge/bin/conda"));
    }
    candidates
}

fn env_name_from_path(path: &Path) -> String {
    if path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|value| value.to_str())
        == Some("envs")
    {
        return path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("base")
            .to_string();
    }
    "base".to_string()
}

fn python_path_for_env(path: &Path) -> PathBuf {
    path.join("bin").join("python")
}

fn parse_conda_envs_with_python_exists<F>(raw: &[u8], python_exists: F) -> Vec<CondaEnvironment>
where
    F: Fn(&Path) -> bool,
{
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(envs) = value.get("envs").and_then(|item| item.as_array()) else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    envs.iter()
        .filter_map(|item| item.as_str())
        .filter_map(|env_path| {
            let path = PathBuf::from(env_path);
            let python_path = python_path_for_env(&path);
            if !seen.insert(path.clone()) || !python_exists(&python_path) {
                return None;
            }
            Some(CondaEnvironment {
                name: env_name_from_path(&path),
                path: path.to_string_lossy().to_string(),
                python_path: python_path.to_string_lossy().to_string(),
            })
        })
        .collect()
}

fn parse_conda_envs(raw: &[u8]) -> Vec<CondaEnvironment> {
    parse_conda_envs_with_python_exists(raw, Path::exists)
}

fn detect_conda_environments_sync() -> Vec<CondaEnvironment> {
    for conda in conda_candidates() {
        let mut command = Command::new(&conda);
        crate::subprocess::configure_background_command(&mut command);
        let Ok(output) = command.args(["env", "list", "--json"]).output() else {
            continue;
        };
        if output.status.success() {
            let envs = parse_conda_envs(&output.stdout);
            if !envs.is_empty() {
                return envs;
            }
        }
    }
    Vec::new()
}

fn build_remote_conda_env_list_command() -> String {
    let candidates = [
        "${CONDA_EXE:-}",
        "/opt/miniconda3/bin/conda",
        "/opt/homebrew/bin/conda",
        "/usr/local/bin/conda",
        "$HOME/miniconda3/bin/conda",
        "$HOME/anaconda3/bin/conda",
        "$HOME/mambaforge/bin/conda",
        "conda",
    ];
    let script = format!(
        "for c in {}; do [ -n \"$c\" ] || continue; if command -v \"$c\" >/dev/null 2>&1 || [ -x \"$c\" ]; then raw=$(\"$c\" env list --json) || continue; if command -v python3 >/dev/null 2>&1; then python3 -c 'import json,os,sys; data=json.load(sys.stdin); data[\"envs\"]=[p for p in data.get(\"envs\", []) if isinstance(p, str) and os.path.exists(p.rstrip(\"/\") + \"/bin/python\")]; print(json.dumps(data))' <<EOF\n$raw\nEOF\nelse printf '%s\\n' \"$raw\"; fi; exit 0; fi; done; exit 1",
        candidates.join(" ")
    );
    format!("sh -lc {}", crate::ssh::shell_quote_posix(&script))
}

fn run_remote_conda_env_list(connection: &SshConnection) -> Result<Vec<u8>, String> {
    let mut command = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_conda_env_list_command(),
    );
    crate::subprocess::configure_background_command(&mut command);
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[tauri::command]
pub async fn detect_conda_environments() -> Result<Vec<CondaEnvironment>, String> {
    tauri::async_runtime::spawn_blocking(detect_conda_environments_sync)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn detect_remote_conda_environments(
    connection: SshConnection,
) -> Result<Vec<CondaEnvironment>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let raw = run_remote_conda_env_list(&connection)?;
        Ok(parse_conda_envs_with_python_exists(&raw, |_| true))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{
        build_remote_conda_env_list_command, conda_candidates, env_name_from_path,
        parse_conda_envs_with_python_exists,
    };
    use std::path::Path;

    #[test]
    fn env_name_uses_last_path_segment() {
        assert_eq!(env_name_from_path(Path::new("/opt/miniconda3")), "base");
        assert_eq!(
            env_name_from_path(Path::new("/opt/miniconda3/envs/cv")),
            "cv"
        );
    }

    #[test]
    fn conda_candidates_include_default_miniconda_on_macos() {
        assert!(conda_candidates()
            .iter()
            .any(|candidate| candidate == Path::new("/opt/miniconda3/bin/conda")));
    }

    #[test]
    fn parses_expected_miniconda_environment_names() {
        let raw = br#"{
          "envs": [
            "/opt/miniconda3",
            "/opt/miniconda3/envs/codex",
            "/opt/miniconda3/envs/detect",
            "/opt/miniconda3/envs/kiro",
            "/opt/miniconda3/envs/labelimg",
            "/opt/miniconda3/envs/labelme",
            "/opt/miniconda3/envs/mahjong"
          ]
        }"#;

        let names = parse_conda_envs_with_python_exists(raw, |_| true)
            .into_iter()
            .map(|env| env.name)
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec!["base", "codex", "detect", "kiro", "labelimg", "labelme", "mahjong"]
        );
    }

    #[test]
    fn remote_conda_command_checks_common_install_locations() {
        let command = build_remote_conda_env_list_command();

        assert!(command.starts_with("sh -lc "));
        assert!(command.contains("conda"));
        assert!(command.contains("env list --json"));
        assert!(command.contains("$HOME/miniconda3/bin/conda"));
    }
}
