use serde::Serialize;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(not(windows))]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
use self::unix as imp;
#[cfg(windows)]
use self::windows as imp;

pub(crate) struct ShellCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunnableFileCommand {
    command: Option<String>,
    unavailable_reason: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlatformRuntimeInfo {
    os: String,
    arch: String,
    shell_kind: String,
    shell_label: String,
    path_separator: String,
    can_run_shell_scripts: bool,
    shell_script_unavailable_reason: String,
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    imp::home_dir()
}

pub(crate) fn login_shell_env() -> &'static [(String, String)] {
    imp::login_shell_env()
}

pub(crate) fn login_shell_path() -> &'static str {
    imp::login_shell_path()
}

pub(crate) fn default_shell_command() -> ShellCommand {
    imp::default_shell_command()
}

pub(crate) fn shell_command(command: &str) -> ShellCommand {
    let mut shell = default_shell_command();
    let (kind, _) = shell_kind_and_label(&shell);
    match kind.as_str() {
        "powershell" => shell.args.extend([
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            command.to_string(),
        ]),
        "cmd" => shell.args.extend([
            "/D".to_string(),
            "/S".to_string(),
            "/C".to_string(),
            command.to_string(),
        ]),
        _ => shell.args.extend(["-lc".to_string(), command.to_string()]),
    }
    shell
}

#[cfg(windows)]
pub(crate) fn agent_script_command(script: &Path) -> Option<ShellCommand> {
    imp::agent_script_command(script)
}

pub(crate) fn detect_path(binary: &str) -> String {
    imp::detect_path(binary)
}

pub(crate) fn local_python_program() -> String {
    if let Some(configured) = std::env::var_os("PYTHON").filter(|value| !value.is_empty()) {
        return configured.to_string_lossy().into_owned();
    }
    #[cfg(windows)]
    if let Some(prefix) = std::env::var_os("CONDA_PREFIX") {
        let python = PathBuf::from(prefix).join("python.exe");
        if python.is_file() {
            return python.to_string_lossy().into_owned();
        }
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["py", "python"]
    } else {
        &["python3", "python"]
    };
    for candidate in candidates {
        let detected = detect_path(candidate);
        if !detected.is_empty() {
            return detected;
        }
    }
    candidates[0].to_string()
}

fn shell_kind_and_label(command: &ShellCommand) -> (String, String) {
    let executable = command
        .program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command.program.as_str());
    let file_name = executable
        .strip_suffix(".exe")
        .unwrap_or(executable)
        .to_ascii_lowercase();
    match file_name.as_str() {
        "pwsh" => ("powershell".to_string(), "PowerShell".to_string()),
        "powershell" => ("powershell".to_string(), "Windows PowerShell".to_string()),
        "cmd" => ("cmd".to_string(), "Command Prompt".to_string()),
        "bash" => ("bash".to_string(), "bash".to_string()),
        "zsh" => ("zsh".to_string(), "zsh".to_string()),
        "fish" => ("fish".to_string(), "fish".to_string()),
        "sh" => ("sh".to_string(), "sh".to_string()),
        _ => (file_name.clone(), file_name),
    }
}

fn validate_command_value(value: &str, label: &str) -> Result<(), String> {
    if value.contains('\0') || value.contains('\n') || value.contains('\r') {
        return Err(format!("{label} contains unsupported control characters"));
    }
    Ok(())
}

fn quote_posix(value: &str) -> String {
    crate::ssh::shell_quote_posix(value)
}

fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// cmd.exe 没有通用转义:`"` 能从引号里逃出去接 `&& ...`，而 `%VAR%` 在解析阶段就被
/// 展开（加引号也拦不住）。两者都无法靠引用消除，只能拒绝。
fn validate_cmd_value(value: &str, label: &str) -> Result<(), String> {
    if value.contains('"') {
        return Err(format!("{label} cannot contain double quotes"));
    }
    if value.contains('%') {
        return Err(format!("{label} cannot contain '%' on Windows"));
    }
    Ok(())
}

fn quote_cmd(value: &str) -> Result<String, String> {
    validate_cmd_value(value, "Windows command paths")?;
    Ok(format!("\"{value}\""))
}

fn conda_bin_path(conda_path: &str, shell_kind: &str) -> String {
    if shell_kind == "powershell" || shell_kind == "cmd" {
        format!(
            "{};{}",
            PathBuf::from(conda_path).join("Scripts").to_string_lossy(),
            PathBuf::from(conda_path)
                .join("Library")
                .join("bin")
                .to_string_lossy()
        )
    } else {
        format!("{}/bin", conda_path.trim_end_matches('/'))
    }
}

fn build_remote_runnable_file_command(
    file_path: &str,
    conda_path: Option<&str>,
    conda_python_path: Option<&str>,
) -> RunnableFileCommand {
    let lower = file_path.to_ascii_lowercase();
    let command = if lower.ends_with(".py") {
        let python = conda_python_path
            .filter(|path| !path.trim().is_empty())
            .unwrap_or("python3");
        format!("{} {}\r", quote_posix(python), quote_posix(file_path))
    } else if lower.ends_with(".sh") {
        match conda_path.filter(|path| !path.trim().is_empty()) {
            Some(path) => format!(
                "CONDA_PREFIX={} PATH={}:\"$PATH\" bash {}\r",
                quote_posix(path),
                quote_posix(&conda_bin_path(path, "sh")),
                quote_posix(file_path)
            ),
            None => format!("bash {}\r", quote_posix(file_path)),
        }
    } else {
        return RunnableFileCommand {
            command: None,
            unavailable_reason: Some("Only Python or shell scripts can be run.".to_string()),
        };
    };
    RunnableFileCommand {
        command: Some(command),
        unavailable_reason: None,
    }
}

fn build_local_runnable_file_command(
    file_path: &str,
    conda_path: Option<&str>,
    conda_python_path: Option<&str>,
    shell: &ShellCommand,
) -> Result<RunnableFileCommand, String> {
    let lower = file_path.to_ascii_lowercase();
    let (shell_kind, _) = shell_kind_and_label(shell);
    let command = if lower.ends_with(".py") {
        let python = conda_python_path
            .filter(|path| !path.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(local_python_program);
        match shell_kind.as_str() {
            "powershell" => format!(
                "& {} {}\r",
                quote_powershell(&python),
                quote_powershell(file_path)
            ),
            "cmd" => format!("{} {}\r", quote_cmd(&python)?, quote_cmd(file_path)?),
            _ => format!("{} {}\r", quote_posix(&python), quote_posix(file_path)),
        }
    } else if lower.ends_with(".sh") {
        #[cfg(windows)]
        let bash = detect_path("bash");
        #[cfg(not(windows))]
        let bash = {
            let detected = detect_path("bash");
            if detected.is_empty() {
                "bash".to_string()
            } else {
                detected
            }
        };
        if bash.is_empty() {
            return Ok(RunnableFileCommand {
                command: None,
                unavailable_reason: Some(
                    "Git Bash is required to run .sh files on Windows.".to_string(),
                ),
            });
        }
        match (
            shell_kind.as_str(),
            conda_path.filter(|path| !path.trim().is_empty()),
        ) {
            ("powershell", Some(path)) => format!(
                "$env:CONDA_PREFIX = {}; $env:PATH = {} + ';' + $env:PATH; & {} {}\r",
                quote_powershell(path),
                quote_powershell(&conda_bin_path(path, "powershell")),
                quote_powershell(&bash),
                quote_powershell(file_path)
            ),
            ("powershell", None) => format!(
                "& {} {}\r",
                quote_powershell(&bash),
                quote_powershell(file_path)
            ),
            ("cmd", Some(path)) => {
                // `set "VAR=value"` 不能再套一层引号，路径只能原样插入，
                // 因此这里必须先校验，否则一个 `"` 就能接出 `&& <任意命令>`。
                validate_cmd_value(path, "Conda path")?;
                format!(
                    "set \"CONDA_PREFIX={path}\" && set \"PATH={};%PATH%\" && {} {}\r",
                    conda_bin_path(path, "cmd"),
                    quote_cmd(&bash)?,
                    quote_cmd(file_path)?
                )
            }
            ("cmd", None) => format!("{} {}\r", quote_cmd(&bash)?, quote_cmd(file_path)?),
            (_, Some(path)) => format!(
                "CONDA_PREFIX={} PATH={}:\"$PATH\" {} {}\r",
                quote_posix(path),
                quote_posix(&conda_bin_path(path, &shell_kind)),
                quote_posix(&bash),
                quote_posix(file_path)
            ),
            (_, None) => format!("{} {}\r", quote_posix(&bash), quote_posix(file_path)),
        }
    } else {
        return Ok(RunnableFileCommand {
            command: None,
            unavailable_reason: Some("Only Python or shell scripts can be run.".to_string()),
        });
    };
    Ok(RunnableFileCommand {
        command: Some(command),
        unavailable_reason: None,
    })
}

#[tauri::command]
pub(crate) fn build_runnable_file_command(
    file_path: String,
    conda_path: Option<String>,
    conda_python_path: Option<String>,
    remote: bool,
) -> Result<RunnableFileCommand, String> {
    validate_command_value(&file_path, "File path")?;
    if let Some(value) = conda_path.as_deref() {
        validate_command_value(value, "Conda path")?;
    }
    if let Some(value) = conda_python_path.as_deref() {
        validate_command_value(value, "Conda Python path")?;
    }
    if remote {
        return Ok(build_remote_runnable_file_command(
            &file_path,
            conda_path.as_deref(),
            conda_python_path.as_deref(),
        ));
    }
    build_local_runnable_file_command(
        &file_path,
        conda_path.as_deref(),
        conda_python_path.as_deref(),
        &default_shell_command(),
    )
}

#[tauri::command]
pub(crate) fn get_platform_runtime_info() -> PlatformRuntimeInfo {
    let shell = default_shell_command();
    let (shell_kind, shell_label) = shell_kind_and_label(&shell);
    #[cfg(windows)]
    let can_run_shell_scripts = !detect_path("bash").is_empty();
    #[cfg(not(windows))]
    let can_run_shell_scripts = true;
    PlatformRuntimeInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        shell_kind,
        shell_label,
        path_separator: std::path::MAIN_SEPARATOR.to_string(),
        can_run_shell_scripts,
        shell_script_unavailable_reason: if can_run_shell_scripts {
            String::new()
        } else {
            "Git Bash is required to run legacy .sh Agent launchers on Windows.".to_string()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_labels_follow_the_selected_program() {
        assert_eq!(
            shell_kind_and_label(&ShellCommand {
                program: "/bin/bash".to_string(),
                args: Vec::new(),
            }),
            ("bash".to_string(), "bash".to_string())
        );
        assert_eq!(
            shell_kind_and_label(&ShellCommand {
                program: "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
                    .to_string(),
                args: Vec::new(),
            }),
            ("powershell".to_string(), "Windows PowerShell".to_string())
        );
    }

    #[test]
    fn remote_file_commands_remain_posix() {
        assert_eq!(
            build_remote_runnable_file_command(
                "/srv/项目/my script.py",
                Some("/opt/miniconda env"),
                Some("/opt/miniconda env/bin/python")
            )
            .command
            .as_deref(),
            Some("'/opt/miniconda env/bin/python' '/srv/项目/my script.py'\r")
        );
        assert_eq!(
            build_remote_runnable_file_command(
                "/srv/项目/setup env.sh",
                Some("/opt/miniconda env"),
                None
            )
            .command
            .as_deref(),
            Some(
                "CONDA_PREFIX='/opt/miniconda env' PATH='/opt/miniconda env/bin':\"$PATH\" bash '/srv/项目/setup env.sh'\r"
            )
        );
    }

    #[test]
    fn powershell_file_commands_escape_apostrophes() {
        let shell = ShellCommand {
            program: "pwsh".to_string(),
            args: Vec::new(),
        };
        let command = build_local_runnable_file_command(
            r"C:\Users\O'Brien\项目\train.py",
            Some(r"C:\Users\O'Brien\Miniconda3"),
            Some(r"C:\Users\O'Brien\Miniconda3\python.exe"),
            &shell,
        )
        .unwrap();
        assert_eq!(
            command.command.as_deref(),
            Some(
                "& 'C:\\Users\\O''Brien\\Miniconda3\\python.exe' 'C:\\Users\\O''Brien\\项目\\train.py'\r"
            )
        );
    }

    #[test]
    fn cmd_file_commands_quote_spaces_and_unicode() {
        let shell = ShellCommand {
            program: "cmd.exe".to_string(),
            args: Vec::new(),
        };
        let command = build_local_runnable_file_command(
            r"C:\Users\Test User\项目\train.py",
            None,
            Some(r"C:\Python 3\python.exe"),
            &shell,
        )
        .unwrap();
        assert_eq!(
            command.command.as_deref(),
            Some("\"C:\\Python 3\\python.exe\" \"C:\\Users\\Test User\\项目\\train.py\"\r")
        );
    }

    #[test]
    fn cmd_file_commands_reject_unquotable_characters() {
        let shell = ShellCommand {
            program: "cmd.exe".to_string(),
            args: Vec::new(),
        };
        // 加引号也拦不住 cmd.exe 的变量展开，所以 `%` 必须直接拒绝。
        let expanded = build_local_runnable_file_command(
            r"C:\scripts\%USERPROFILE%\train.py",
            None,
            None,
            &shell,
        );
        assert!(expanded.is_err_and(|err| err.contains('%')));

        // `"` 能从引号里逃出去接 `&& <任意命令>`。
        let escaped = build_local_runnable_file_command("C:\\scripts\\a\".py", None, None, &shell);
        assert!(escaped.is_err_and(|err| err.contains("double quotes")));

        // conda 路径走 `set "VAR=value"`，不经过 quote_cmd，同样必须校验。
        let conda = build_local_runnable_file_command(
            r"C:\scripts\setup.sh",
            Some("C:\\Conda\" && calc &&\""),
            None,
            &shell,
        );
        assert!(conda.is_err_and(|err| err.contains("Conda path")));
    }
}
