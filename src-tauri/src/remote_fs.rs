use std::io::Write;
use std::process::Stdio;

use base64::Engine;
use serde::Serialize;

use crate::ssh::SshConnection;

const MAX_REMOTE_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_REMOTE_IMAGE_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Serialize)]
pub(crate) struct RemoteFsEntry {
    name: String,
    path: String,
    is_dir: bool,
    extension: Option<String>,
    is_gitignored: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteImagePreviewData {
    data_url: String,
    mime_type: String,
    byte_length: u64,
}

fn build_remote_read_dir_command(remote_path: &str) -> String {
    format!(
        "cd -- {} && find . -maxdepth 1 -mindepth 1 -printf '%f\\t%y\\n'",
        crate::ssh::shell_quote_posix(remote_path)
    )
}

fn build_remote_read_file_command(remote_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(remote_path);
    format!("size=$(wc -c < {path}) && [ \"$size\" -le {MAX_REMOTE_FILE_BYTES} ] && cat -- {path}")
}

fn build_remote_write_file_command(remote_path: &str) -> String {
    format!("cat > {}", crate::ssh::shell_quote_posix(remote_path))
}

fn build_remote_create_file_command(remote_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(remote_path);
    format!("test ! -e {path} && : > {path}")
}

fn build_remote_create_directory_command(remote_path: &str) -> String {
    format!("mkdir -- {}", crate::ssh::shell_quote_posix(remote_path))
}

fn build_remote_delete_path_command(remote_path: &str) -> String {
    format!("rm -rf -- {}", crate::ssh::shell_quote_posix(remote_path))
}

fn build_remote_image_preview_command(remote_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(remote_path);
    format!(
        "size=$(wc -c < {path}) && [ \"$size\" -le {MAX_REMOTE_IMAGE_PREVIEW_BYTES} ] && base64 < {path}"
    )
}

fn remote_image_mime_type(remote_path: &str) -> Option<&'static str> {
    let ext = remote_path.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

fn normalize_remote_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    }
}

fn ensure_remote_path_allowed(
    remote_path: &str,
    remote_project_path: Option<&str>,
    allow_project_root: bool,
) -> Result<(), String> {
    if !remote_path.starts_with('/') {
        return Err("Remote path must be absolute".to_string());
    }
    let Some(remote_project_path) = remote_project_path else {
        return Ok(());
    };
    let path = normalize_remote_path(remote_path);
    let root = normalize_remote_path(remote_project_path);
    if path == root {
        if allow_project_root {
            return Ok(());
        }
        return Err("Cannot modify the remote project root".to_string());
    }
    let root_prefix = format!("{}/", root.trim_end_matches('/'));
    if !path.starts_with(&root_prefix) {
        return Err("Remote path is outside the project root".to_string());
    }
    if let Some(first) = path[root_prefix.len()..].split('/').next() {
        if first == ".git" || first == ".nezha" {
            return Err(format!(
                "Cannot modify protected remote directory: {}",
                first
            ));
        }
    }
    Ok(())
}

fn run_ssh_output(connection: &SshConnection, remote_command: String) -> Result<Vec<u8>, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, remote_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

fn parse_remote_dir_entries(remote_path: &str, raw: &str) -> Vec<RemoteFsEntry> {
    raw.lines()
        .filter_map(|line| {
            let (name, kind) = line.split_once('\t')?;
            let is_dir = kind == "d";
            let extension = if is_dir {
                None
            } else {
                name.rsplit_once('.')
                    .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
                    .map(|(_, ext)| ext.to_string())
            };
            Some(RemoteFsEntry {
                name: name.to_string(),
                path: format!("{}/{}", remote_path.trim_end_matches('/'), name),
                is_dir,
                extension,
                is_gitignored: false,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn remote_read_dir_entries(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<Vec<RemoteFsEntry>, String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&remote_path, remote_project_path.as_deref(), true)?;
        let stdout = run_ssh_output(&connection, build_remote_read_dir_command(&remote_path))?;
        let raw = String::from_utf8_lossy(&stdout);
        Ok(parse_remote_dir_entries(&remote_path, &raw))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_read_file_content(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&remote_path, remote_project_path.as_deref(), true)?;
        let stdout = run_ssh_output(&connection, build_remote_read_file_command(&remote_path))?;
        String::from_utf8(stdout).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_write_file_content(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
    content: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&remote_path, remote_project_path.as_deref(), false)?;
        let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
            &connection,
            build_remote_write_file_command(&remote_path),
        );
        crate::subprocess::configure_background_command(&mut cmd);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| "Failed to open ssh stdin".to_string())?;
            stdin
                .write_all(content.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_read_image_preview(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<RemoteImagePreviewData, String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&remote_path, remote_project_path.as_deref(), true)?;
        let mime_type = remote_image_mime_type(&remote_path)
            .ok_or_else(|| "Unsupported image format".to_string())?;
        let stdout = run_ssh_output(
            &connection,
            build_remote_image_preview_command(&remote_path),
        )?;
        let encoded = String::from_utf8_lossy(&stdout)
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(RemoteImagePreviewData {
            data_url: format!(
                "data:{};base64,{}",
                mime_type,
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            ),
            mime_type: mime_type.to_string(),
            byte_length: bytes.len() as u64,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_create_file(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&remote_path, remote_project_path.as_deref(), false)?;
        run_ssh_output(&connection, build_remote_create_file_command(&remote_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_create_directory(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&remote_path, remote_project_path.as_deref(), false)?;
        run_ssh_output(
            &connection,
            build_remote_create_directory_command(&remote_path),
        )
        .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_delete_path(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&remote_path, remote_project_path.as_deref(), false)?;
        run_ssh_output(&connection, build_remote_delete_path_command(&remote_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_command_enforces_size_limit_and_quotes_path() {
        assert_eq!(
            build_remote_read_file_command("/srv/app's repo/README.md"),
            "size=$(wc -c < '/srv/app'\\''s repo/README.md') && [ \"$size\" -le 2097152 ] && cat -- '/srv/app'\\''s repo/README.md'"
        );
    }

    #[test]
    fn write_file_command_quotes_target_path() {
        assert_eq!(
            build_remote_write_file_command("/srv/app/config value.txt"),
            "cat > '/srv/app/config value.txt'"
        );
    }

    #[test]
    fn create_and_delete_commands_quote_target_paths() {
        assert_eq!(
            build_remote_create_file_command("/srv/app/new file.txt"),
            "test ! -e '/srv/app/new file.txt' && : > '/srv/app/new file.txt'"
        );
        assert_eq!(
            build_remote_create_directory_command("/srv/app/new folder"),
            "mkdir -- '/srv/app/new folder'"
        );
        assert_eq!(
            build_remote_delete_path_command("/srv/app/old file.txt"),
            "rm -rf -- '/srv/app/old file.txt'"
        );
    }

    #[test]
    fn image_preview_command_encodes_with_size_limit() {
        assert_eq!(
            build_remote_image_preview_command("/srv/app/logo.png"),
            "size=$(wc -c < '/srv/app/logo.png') && [ \"$size\" -le 10485760 ] && base64 < '/srv/app/logo.png'"
        );
    }
}
