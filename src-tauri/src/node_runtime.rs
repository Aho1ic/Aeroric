use std::path::PathBuf;

use serde::Serialize;

#[cfg(any(windows, test))]
const NODE_WINGET_PACKAGE_ID: &str = "OpenJS.NodeJS.LTS";

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NodeRuntimeInstallResult {
    pub node_path: String,
    pub version: String,
    pub already_installed: bool,
}

/// Locate Node without relying exclusively on the process PATH. On Windows the
/// PATH inherited by Aeroric predates a just-completed installer, so standard
/// Node install locations must also be checked directly.
pub(crate) fn detect_node() -> Option<PathBuf> {
    let from_path = crate::platform::detect_path("node");
    if !from_path.is_empty() {
        let candidate = PathBuf::from(from_path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    #[cfg(any(windows, test))]
    {
        for directory in windows_node_directories() {
            let candidate = directory.join("node.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(any(windows, test))]
fn windows_node_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for key in ["NODE_HOME", "NVM_SYMLINK"] {
        if let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            directories.push(PathBuf::from(value));
        }
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            directories.push(PathBuf::from(value).join("nodejs"));
        }
    }
    if let Some(value) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        directories.push(PathBuf::from(value).join("Programs").join("nodejs"));
    }
    directories
}

#[cfg(any(windows, test))]
fn winget_node_install_args() -> &'static [&'static str] {
    &[
        "install",
        "--id",
        NODE_WINGET_PACKAGE_ID,
        "--exact",
        "--source",
        "winget",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
        "--silent",
    ]
}

#[cfg(any(windows, test))]
#[cfg_attr(test, allow(dead_code))]
fn winget_program() -> String {
    let detected = crate::platform::detect_path("winget");
    if !detected.is_empty() {
        return detected;
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty())
    {
        let alias = PathBuf::from(local_app_data)
            .join("Microsoft")
            .join("WindowsApps")
            .join("winget.exe");
        if alias.is_file() {
            return alias.to_string_lossy().into_owned();
        }
    }
    "winget.exe".to_string()
}

#[cfg(any(windows, test))]
#[cfg_attr(test, allow(dead_code))]
async fn node_version(node_path: &std::path::Path) -> Result<String, String> {
    use std::process::Stdio;
    use std::time::Duration;

    use tokio::process::Command;

    let mut command = Command::new(node_path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::subprocess::configure_background_tokio_command(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| "verification_failed: Node.js version check timed out".to_string())?
        .map_err(|error| format!("verification_failed: Cannot start Node.js: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "verification_failed: Node.js version check exited with {}",
            output.status
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    if version.is_empty() {
        return Err("verification_failed: Node.js did not report a version".to_string());
    }
    Ok(version)
}

#[cfg(any(windows, test))]
#[cfg_attr(test, allow(dead_code))]
async fn install_node_with_winget() -> Result<(), String> {
    use std::process::Stdio;
    use std::time::Duration;

    use tokio::process::Command;

    let mut command = Command::new(winget_program());
    command
        .args(winget_node_install_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::subprocess::configure_background_tokio_command(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(15 * 60), command.output())
        .await
        .map_err(|_| "node_install_failed: Node.js installation timed out".to_string())?
        .map_err(|error| {
            format!(
                "winget_unavailable: Cannot start Windows Package Manager: {error}. Install App Installer and try again."
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "node_install_failed: Windows Package Manager exited with {}",
            output.status
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn install_nodejs_on_windows() -> Result<NodeRuntimeInstallResult, String> {
    #[cfg(windows)]
    {
        if let Some(path) = detect_node() {
            let version = node_version(&path).await?;
            // The desktop process may have cached a previous `no_node` result;
            // refresh hooks even when Node was installed before the button was
            // clicked so the project home reflects the current runtime.
            crate::hooks::cache_status(crate::hooks::ensure_installed());
            return Ok(NodeRuntimeInstallResult {
                node_path: path.to_string_lossy().into_owned(),
                version,
                already_installed: true,
            });
        }

        install_node_with_winget().await?;
        let path = detect_node().ok_or_else(|| {
            "verification_failed: Node.js was installed but node.exe was not found. Restart Aeroric and try again."
                .to_string()
        })?;
        let version = node_version(&path).await?;
        crate::hooks::cache_status(crate::hooks::ensure_installed());
        Ok(NodeRuntimeInstallResult {
            node_path: path.to_string_lossy().into_owned(),
            version,
            already_installed: false,
        })
    }

    #[cfg(not(windows))]
    {
        Err(
            "unsupported_platform: One-click Node.js installation is only available on Windows"
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winget_install_uses_the_exact_official_lts_package() {
        let args = winget_node_install_args();
        assert_eq!(args[0], "install");
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--id", NODE_WINGET_PACKAGE_ID]));
        assert!(args.contains(&"--exact"));
        assert!(args.windows(2).any(|pair| pair == ["--source", "winget"]));
        assert!(args.contains(&"--accept-package-agreements"));
        assert!(args.contains(&"--accept-source-agreements"));
        assert!(args.contains(&"--disable-interactivity"));
        assert!(args.contains(&"--silent"));
    }
}
