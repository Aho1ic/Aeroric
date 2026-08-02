use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::ShellCommand;

static LOGIN_SHELL_ENV: OnceLock<Vec<(String, String)>> = OnceLock::new();
static LOGIN_SHELL_PATH: OnceLock<String> = OnceLock::new();

pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(
            || match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
                (Some(drive), Some(path)) => {
                    let mut full = PathBuf::from(drive);
                    full.push(PathBuf::from(path));
                    Some(full)
                }
                _ => None,
            },
        )
}

pub(crate) fn login_shell_env() -> &'static [(String, String)] {
    LOGIN_SHELL_ENV
        .get_or_init(|| {
            let mut env: Vec<(String, String)> = std::env::vars().collect();
            if !env.iter().any(|(key, _)| key.eq_ignore_ascii_case("HOME")) {
                if let Some(home) = home_dir() {
                    env.push(("HOME".to_string(), home.to_string_lossy().into_owned()));
                }
            }
            let inherited_path = env
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
                .map(|(_, value)| value.as_str())
                .unwrap_or_default();
            let augmented_path = augment_windows_path(inherited_path);
            if let Some((_, value)) = env
                .iter_mut()
                .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            {
                *value = augmented_path;
            } else if !augmented_path.is_empty() {
                env.push(("PATH".to_string(), augmented_path));
            }
            env
        })
        .as_slice()
}

pub(crate) fn login_shell_path() -> &'static str {
    LOGIN_SHELL_PATH.get_or_init(|| {
        login_shell_env()
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    })
}

pub(crate) fn default_shell_command() -> ShellCommand {
    if !detect_path("pwsh").is_empty() {
        return ShellCommand {
            program: "pwsh".to_string(),
            args: vec!["-NoLogo".to_string()],
        };
    }

    if !detect_path("powershell").is_empty() {
        return ShellCommand {
            program: "powershell".to_string(),
            args: vec!["-NoLogo".to_string()],
        };
    }

    ShellCommand {
        program: std::env::var("ComSpec")
            .unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string()),
        args: Vec::new(),
    }
}

fn powershell_program() -> String {
    for candidate in ["pwsh", "powershell"] {
        let detected = detect_path(candidate);
        if !detected.is_empty() {
            return detected;
        }
    }
    r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe".to_string()
}

pub(crate) fn agent_script_command(script: &Path) -> Option<ShellCommand> {
    let extension = script
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    let script_path = script.to_string_lossy().into_owned();
    match extension.as_str() {
        "ps1" => Some(ShellCommand {
            program: powershell_program(),
            args: vec![
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                script_path,
            ],
        }),
        "sh" => {
            let detected = detect_path("bash");
            if detected.is_empty() {
                return Some(ShellCommand {
                    program: powershell_program(),
                    args: vec![
                        "-NoLogo".to_string(),
                        "-NoProfile".to_string(),
                        "-NonInteractive".to_string(),
                        "-Command".to_string(),
                        format!(
                            "Write-Error 'Legacy Aeroric Agent launcher requires Git Bash. Recreate this Agent in Settings to migrate it to PowerShell: {}'; exit 1",
                            script.to_string_lossy().replace('\'', "''")
                        ),
                    ],
                });
            }
            Some(ShellCommand {
                program: detected,
                args: vec![script_path],
            })
        }
        "cmd" | "bat" => Some(ShellCommand {
            program: std::env::var("ComSpec")
                .unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_string()),
            args: vec![
                "/D".to_string(),
                "/S".to_string(),
                "/C".to_string(),
                script_path,
            ],
        }),
        _ => None,
    }
}

pub(crate) fn detect_path(binary: &str) -> String {
    if binary.contains('\\') || binary.contains('/') {
        let candidate = PathBuf::from(binary);
        return if candidate.exists() {
            candidate.to_string_lossy().into_owned()
        } else {
            String::new()
        };
    }

    let path_value = login_shell_path();
    if path_value.is_empty() {
        return String::new();
    }

    let has_extension = Path::new(binary).extension().is_some();
    find_on_path(binary, path_value, has_extension).unwrap_or_default()
}

fn find_on_path(binary: &str, path_value: &str, has_extension: bool) -> Option<String> {
    let path_exts = if has_extension {
        vec![String::new()]
    } else {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| ext.to_string())
            .collect::<Vec<_>>()
    };

    for dir in path_value.split(';').filter(|segment| !segment.is_empty()) {
        if has_extension {
            let candidate = Path::new(dir).join(binary);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
            continue;
        }

        for ext in &path_exts {
            let candidate = Path::new(dir).join(format!("{binary}{ext}"));
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }

    None
}

fn augment_windows_path(inherited_path: &str) -> String {
    let mut entries = inherited_path
        .split(';')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>();

    let mut candidates = Vec::new();
    for key in [
        "NODE_HOME",
        "NVM_SYMLINK",
        "FNM_MULTISHELL_PATH",
        "PNPM_HOME",
    ] {
        if let Some(value) = std::env::var_os(key) {
            candidates.push(PathBuf::from(value));
        }
    }
    if let Some(value) = std::env::var_os("VOLTA_HOME") {
        candidates.push(PathBuf::from(value).join("bin"));
    }
    if let Some(value) = std::env::var_os("MISE_DATA_DIR") {
        candidates.push(PathBuf::from(value).join("shims"));
    }
    if let Some(value) = std::env::var_os("GIT_INSTALL_ROOT") {
        let root = PathBuf::from(value);
        candidates.push(root.join("bin"));
        candidates.push(root.join("usr").join("bin"));
    }
    if let Some(value) = std::env::var_os("ProgramFiles") {
        let root = PathBuf::from(value);
        candidates.push(root.join("nodejs"));
        candidates.push(root.join("Git").join("bin"));
        candidates.push(root.join("Git").join("usr").join("bin"));
    }
    if let Some(value) = std::env::var_os("ProgramFiles(x86)") {
        let root = PathBuf::from(value);
        candidates.push(root.join("nodejs"));
        candidates.push(root.join("Git").join("bin"));
    }
    if let Some(value) = std::env::var_os("LOCALAPPDATA") {
        let root = PathBuf::from(value);
        candidates.push(root.join("Programs").join("nodejs"));
        candidates.push(root.join("Programs").join("Git").join("bin"));
    }
    if let Some(value) = std::env::var_os("APPDATA") {
        candidates.push(PathBuf::from(value).join("npm"));
    }
    if let Some(value) = std::env::var_os("USERPROFILE") {
        let home = PathBuf::from(value);
        candidates.push(home.join(".volta").join("bin"));
        candidates.push(home.join("scoop").join("shims"));
        candidates.push(home.join(".local").join("bin"));
        candidates.push(home.join(".local").join("share").join("mise").join("shims"));
    }

    for entry in &entries {
        let dir = Path::new(entry);
        if dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("cmd"))
            && dir.join("git.exe").is_file()
        {
            if let Some(root) = dir.parent() {
                candidates.push(root.join("bin"));
                candidates.push(root.join("usr").join("bin"));
            }
        }
    }

    for candidate in candidates {
        append_existing_path(&mut entries, candidate);
    }
    entries.join(";")
}

fn append_existing_path(entries: &mut Vec<String>, candidate: PathBuf) {
    if !candidate.is_dir() {
        return;
    }
    let value = candidate.to_string_lossy().into_owned();
    if entries
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(&value))
    {
        return;
    }
    entries.push(value);
}
