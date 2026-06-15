use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
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
    let script = "cd \"$1\" && for p in ./* ./.[!.]* ./..?*; do [ -e \"$p\" ] || continue; name=${p#./}; if [ \"$name\" = \".\" ] || [ \"$name\" = \"..\" ]; then continue; fi; if [ -d \"$p\" ]; then type=d; else type=f; fi; printf '%s\\t%s\\n' \"$name\" \"$type\"; done";
    format!(
        "sh -c {} sh {}",
        crate::ssh::shell_quote_posix(script),
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

fn validate_remote_entry_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("File name cannot be empty".to_string());
    }
    if name.len() > 255 {
        return Err("File name is too long (max 255 bytes)".to_string());
    }
    if name == "." || name == ".." {
        return Err("Invalid file name".to_string());
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err("File name contains forbidden characters".to_string());
    }
    Ok(())
}

fn remote_parent_path(remote_path: &str) -> Result<&str, String> {
    let trimmed = remote_path.trim_end_matches('/');
    let Some((parent, _)) = trimmed.rsplit_once('/') else {
        return Err("Cannot resolve parent directory".to_string());
    };
    if parent.is_empty() {
        Ok("/")
    } else {
        Ok(parent)
    }
}

fn remote_basename(remote_path: &str) -> Result<&str, String> {
    let trimmed = remote_path.trim_end_matches('/');
    trimmed
        .rsplit_once('/')
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Invalid file name".to_string())
}

fn build_remote_rename_path_command(remote_path: &str, new_name: &str) -> Result<String, String> {
    validate_remote_entry_name(new_name)?;
    let parent = remote_parent_path(remote_path)?;
    let destination = if parent == "/" {
        format!("/{}", new_name)
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), new_name)
    };
    let source = crate::ssh::shell_quote_posix(remote_path);
    let dest = crate::ssh::shell_quote_posix(&destination);
    Ok(format!(
        "[ ! -e {dest} ] && mv -- {source} {dest}",
        source = source,
        dest = dest
    ))
}

fn build_remote_copy_paths_command(
    source_paths: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if source_paths.is_empty() {
        return Ok(":".to_string());
    }
    let target = crate::ssh::shell_quote_posix(target_directory);
    let sources = source_paths
        .iter()
        .map(|source| {
            validate_remote_entry_name(remote_basename(source)?)?;
            Ok(crate::ssh::shell_quote_posix(source))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && for src in {sources}; do name=${{src##*/}}; [ ! -e \"$target/$name\" ] || {{ echo \"A file or folder with that name already exists\" >&2; exit 1; }}; done && cp -R -- {sources} \"$target/\"",
        target = target,
        sources = sources.join(" ")
    ))
}

fn build_remote_upload_conflict_check_command(
    local_source_paths: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if local_source_paths.is_empty() {
        return Ok(":".to_string());
    }
    let target = crate::ssh::shell_quote_posix(target_directory);
    let names = local_source_paths
        .iter()
        .map(|source| {
            let name = Path::new(source)
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| "Invalid source file name".to_string())?;
            validate_remote_entry_name(name)?;
            Ok(crate::ssh::shell_quote_posix(name))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && for name in {names}; do [ ! -e \"$target/$name\" ] || {{ echo \"A file or folder with that name already exists\" >&2; exit 1; }}; done",
        target = target,
        names = names.join(" ")
    ))
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

fn remote_path_has_relative_components(path: &str) -> bool {
    path.split('/')
        .any(|component| component == "." || component == "..")
}

fn ensure_remote_path_allowed(
    remote_path: &str,
    remote_project_path: Option<&str>,
    allow_project_root: bool,
) -> Result<(), String> {
    if !remote_path.starts_with('/') {
        return Err("Remote path must be absolute".to_string());
    }
    if remote_path_has_relative_components(remote_path) {
        return Err("Remote path cannot contain . or .. components".to_string());
    }
    let Some(remote_project_path) = remote_project_path else {
        return Ok(());
    };
    if !remote_project_path.starts_with('/') {
        return Err("Remote project path must be absolute".to_string());
    }
    if remote_path_has_relative_components(remote_project_path) {
        return Err("Remote project path cannot contain . or .. components".to_string());
    }
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
        if first == ".git" || first == ".aeroric" {
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

fn std_scp_upload_command(
    connection: &SshConnection,
    source_paths: &[String],
    target_directory: &str,
) -> Command {
    let password = connection
        .password
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    let mut cmd = if password.is_some() {
        let detected = crate::platform::detect_path("sshpass");
        let program = if detected.is_empty() {
            "sshpass".to_string()
        } else {
            detected
        };
        let mut command = Command::new(program);
        command.arg("-e").arg("scp");
        command
    } else {
        Command::new("scp")
    };

    cmd.arg("-P").arg(connection.port.to_string()).arg("-r");
    if let Some(identity_file) = connection
        .identity_file
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        cmd.arg("-i").arg(identity_file);
    }
    for source in source_paths {
        cmd.arg(source);
    }
    cmd.arg(format!(
        "{}@{}:{}",
        connection.username,
        connection.host,
        crate::ssh::shell_quote_posix(target_directory)
    ));
    if let Some(password) = password {
        cmd.env("SSHPASS", password);
    }
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    crate::subprocess::configure_background_command(&mut cmd);
    cmd
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

#[tauri::command]
pub async fn remote_rename_path(
    connection: SshConnection,
    remote_path: String,
    new_name: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&remote_path, remote_project_path.as_deref(), false)?;
        let command = build_remote_rename_path_command(&remote_path, new_name.trim())?;
        run_ssh_output(&connection, command).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_copy_paths_to_directory(
    connection: SshConnection,
    source_paths: Vec<String>,
    target_directory: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&target_directory, remote_project_path.as_deref(), true)?;
        for source in &source_paths {
            ensure_remote_path_allowed(source, remote_project_path.as_deref(), false)?;
        }
        let command = build_remote_copy_paths_command(&source_paths, &target_directory)?;
        run_ssh_output(&connection, command).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_upload_local_paths_to_directory(
    connection: SshConnection,
    local_source_paths: Vec<String>,
    target_directory: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        ensure_remote_path_allowed(&target_directory, remote_project_path.as_deref(), true)?;
        if local_source_paths.is_empty() {
            return Ok(());
        }
        let mut validated_sources = Vec::with_capacity(local_source_paths.len());
        for source in local_source_paths {
            let source_path = PathBuf::from(&source);
            if !source_path.is_absolute() {
                return Err("Source path must be absolute".to_string());
            }
            if !source_path.exists() {
                return Err(format!("Source path does not exist: {}", source));
            }
            let name = source_path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| "Invalid source file name".to_string())?;
            validate_remote_entry_name(name)?;
            validated_sources.push(source);
        }
        run_ssh_output(
            &connection,
            build_remote_upload_conflict_check_command(&validated_sources, &target_directory)?,
        )?;
        let output = std_scp_upload_command(&connection, &validated_sources, &target_directory)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
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
    fn allows_reading_remote_project_root() {
        assert!(ensure_remote_path_allowed("/srv/app", Some("/srv/app"), true).is_ok());
        assert!(ensure_remote_path_allowed("/srv/app", Some("/srv/app"), false).is_err());
    }

    #[test]
    fn rejects_remote_paths_with_parent_components() {
        assert!(
            ensure_remote_path_allowed("/srv/app/../outside.txt", Some("/srv/app"), false).is_err()
        );
        assert!(
            ensure_remote_path_allowed("/srv/app/./file.txt", Some("/srv/app"), false).is_err()
        );
    }

    #[test]
    fn read_dir_command_is_posix_compatible_without_gnu_printf() {
        let command = build_remote_read_dir_command("/srv/app");
        assert!(!command.contains("-printf"));
        assert!(!command.contains("cd --"));
        assert!(command.contains("printf"));
        assert!(command.contains("%s\\t%s\\n"));
    }

    #[test]
    fn read_dir_command_runs_globs_inside_posix_sh() {
        let command = build_remote_read_dir_command("/Users/lyx/Documents");

        assert!(command.starts_with("sh -c "));
        assert!(command.contains("'cd \"$1\" && for p in ./* ./.[!.]* ./..?*;"));
        assert!(command.ends_with(" sh '/Users/lyx/Documents'"));
    }

    #[test]
    fn rename_command_validates_basename_and_quotes_destination() {
        assert_eq!(
            build_remote_rename_path_command("/srv/app/old file.txt", "new file.txt").unwrap(),
            "[ ! -e '/srv/app/new file.txt' ] && mv -- '/srv/app/old file.txt' '/srv/app/new file.txt'"
        );
        assert!(build_remote_rename_path_command("/srv/app/old", "../new").is_err());
    }

    #[test]
    fn copy_paths_command_quotes_sources_and_target() {
        assert_eq!(
            build_remote_copy_paths_command(
                &[
                    "/srv/app/a file.txt".to_string(),
                    "/srv/app/folder".to_string(),
                ],
                "/srv/app/target dir",
            )
            .unwrap(),
            "target='/srv/app/target dir'; [ -d \"$target\" ] && for src in '/srv/app/a file.txt' '/srv/app/folder'; do name=${src##*/}; [ ! -e \"$target/$name\" ] || { echo \"A file or folder with that name already exists\" >&2; exit 1; }; done && cp -R -- '/srv/app/a file.txt' '/srv/app/folder' \"$target/\""
        );
    }

    #[test]
    fn upload_conflict_command_checks_local_basenames_on_remote_target() {
        assert_eq!(
            build_remote_upload_conflict_check_command(
                &[
                    "/Users/me/Desktop/a file.txt".to_string(),
                    "/Users/me/Desktop/folder".to_string(),
                ],
                "/srv/app/target dir",
            )
            .unwrap(),
            "target='/srv/app/target dir'; [ -d \"$target\" ] && for name in 'a file.txt' 'folder'; do [ ! -e \"$target/$name\" ] || { echo \"A file or folder with that name already exists\" >&2; exit 1; }; done"
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
