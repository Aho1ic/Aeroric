//! Local Conda discovery, shared by the desktop and headless host (slice 111).
//! Candidate order, empty-result fallback and Python filtering are the legacy
//! behavior. The remote adapter also uses this parser, without local file IO.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::conda::CondaEnvironment;
use crate::runtime::subprocess::configure_background_command;

/// Trusted composition only, never decoded from an RPC request. Overrides apply
/// to children, not the process environment or shell rc. `None` uses the system
/// candidates; `Some([])` deliberately probes nothing (including no PATH search).
#[derive(Clone, Debug, Default)]
pub struct CondaProbe {
    pub candidates: Option<Vec<PathBuf>>,
    pub env: Vec<(OsString, OsString)>,
    pub clear_env: bool,
}

impl CondaProbe {
    pub fn detect(&self) -> Vec<CondaEnvironment> {
        let candidates = self.candidates.clone().unwrap_or_else(conda_candidates);
        for conda in candidates {
            let mut command = Command::new(&conda);
            configure_background_command(&mut command);
            if self.clear_env {
                command.env_clear();
            }
            command.envs(self.env.iter().map(|(key, value)| (key, value)));
            let Ok(output) = command.args(["env", "list", "--json"]).output() else {
                continue;
            };
            if output.status.success() {
                let envs = parse_conda_envs_with_python_exists(&output.stdout, Path::exists);
                if !envs.is_empty() {
                    return envs;
                }
            }
        }
        Vec::new()
    }
}

pub fn conda_candidates() -> Vec<PathBuf> {
    candidates_with_env(|key| std::env::var_os(key))
}

fn candidates_with_env(env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    // Legacy CONDA_EXE/HOME used env::var, whereas Windows roots used var_os.
    // In particular, a present empty CONDA_EXE remains the first candidate.
    if let Some(path) = env("CONDA_EXE").and_then(|value| value.into_string().ok()) {
        candidates.push(PathBuf::from(path));
    }
    #[cfg(windows)]
    {
        for key in ["USERPROFILE", "LOCALAPPDATA", "ProgramData"] {
            if let Some(root) = env(key).map(PathBuf::from) {
                candidates.push(root.join("miniconda3").join("Scripts").join("conda.exe"));
                candidates.push(root.join("anaconda3").join("Scripts").join("conda.exe"));
                candidates.push(root.join("Miniconda3").join("Scripts").join("conda.exe"));
                candidates.push(root.join("Anaconda3").join("Scripts").join("conda.exe"));
            }
        }
        candidates.push(PathBuf::from(
            r"C:\ProgramData\miniconda3\Scripts\conda.exe",
        ));
        candidates.push(PathBuf::from(r"C:\ProgramData\anaconda3\Scripts\conda.exe"));
        candidates.push(PathBuf::from("conda.exe"));
    }
    #[cfg(not(windows))]
    {
        candidates.push(PathBuf::from("/opt/miniconda3/bin/conda"));
        candidates.push(PathBuf::from("/opt/homebrew/bin/conda"));
        candidates.push(PathBuf::from("/usr/local/bin/conda"));
        candidates.push(PathBuf::from("conda"));
        if let Some(home) = env("HOME").and_then(|value| value.into_string().ok()) {
            candidates.push(PathBuf::from(&home).join("miniconda3/bin/conda"));
            candidates.push(PathBuf::from(&home).join("anaconda3/bin/conda"));
            candidates.push(PathBuf::from(&home).join("mambaforge/bin/conda"));
        }
    }
    candidates
}

/// This takes an *environment directory*, unlike conda::env_name_from_path,
/// whose existing native contract takes an executable (.../envs/dev/bin/conda).
pub fn env_name_from_path(path: &Path) -> String {
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

pub fn python_path_for_env(path: &Path) -> PathBuf {
    if cfg!(windows) {
        path.join("python.exe")
    } else {
        path.join("bin").join("python")
    }
}

pub fn parse_conda_envs_with_python_exists<F>(raw: &[u8], python_exists: F) -> Vec<CondaEnvironment>
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

#[cfg(test)]
#[path = "conda_face_tests.rs"]
mod tests;
