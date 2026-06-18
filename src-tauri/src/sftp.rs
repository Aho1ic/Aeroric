use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::ssh::SshConnection;

const MAX_SFTP_TEXT_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SFTP_IMAGE_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum SftpEndpoint {
    Local {
        path: String,
    },
    Ssh {
        connection: SshConnection,
        path: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SftpConflictStrategy {
    Fail,
    Merge,
    Replace,
}

impl Default for SftpConflictStrategy {
    fn default() -> Self {
        Self::Fail
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SftpEntry {
    name: String,
    path: String,
    is_dir: bool,
    extension: Option<String>,
    size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SftpImagePreviewData {
    data_url: String,
    mime_type: String,
    byte_length: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SftpDirectorySummary {
    file_count: u64,
    directory_count: u64,
    total_size: u64,
    modified_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

fn validate_sftp_local_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("Path must be absolute".to_string());
    }
    Ok(path)
}

fn validate_sftp_remote_path(path: &str) -> Result<String, String> {
    if !path.starts_with('/') {
        return Err("Remote path must be absolute".to_string());
    }
    if path.contains('\0') {
        return Err("Remote path contains forbidden characters".to_string());
    }
    Ok(if path == "/" {
        "/".to_string()
    } else {
        path.trim_end_matches('/').to_string()
    })
}

fn validate_entry_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err("Invalid file name".to_string());
    }
    if name.len() > 255 {
        return Err("File name is too long".to_string());
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err("File name contains forbidden characters".to_string());
    }
    Ok(())
}

fn basename(path: &str) -> Result<&str, String> {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit_once('/')
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Invalid file name".to_string())
}

fn remote_parent(path: &str) -> Result<&str, String> {
    let trimmed = path.trim_end_matches('/');
    let Some((parent, _)) = trimmed.rsplit_once('/') else {
        return Err("Cannot resolve parent directory".to_string());
    };
    Ok(if parent.is_empty() { "/" } else { parent })
}

fn join_remote_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

fn extension_for_name(name: &str, is_dir: bool) -> Option<String> {
    if is_dir {
        return None;
    }
    name.rsplit_once('.')
        .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

fn build_remote_read_dir_command(remote_path: &str) -> String {
    let script = "cd \"$1\" && for p in ./* ./.[!.]* ./..?*; do [ -e \"$p\" ] || continue; name=${p#./}; if [ \"$name\" = \".\" ] || [ \"$name\" = \"..\" ]; then continue; fi; if [ -d \"$p\" ]; then type=d; else type=f; fi; size=\"\"; if [ \"$type\" = f ]; then size=$(wc -c < \"$p\" 2>/dev/null || true); fi; printf '%s\\t%s\\t%s\\n' \"$name\" \"$type\" \"$size\"; done";
    format!(
        "sh -c {} sh {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(remote_path)
    )
}

fn build_remote_read_text_command(remote_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(remote_path);
    format!(
        "size=$(wc -c < {path}) && [ \"$size\" -le {MAX_SFTP_TEXT_FILE_BYTES} ] && cat -- {path}"
    )
}

fn build_remote_image_preview_command(remote_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(remote_path);
    format!(
        "size=$(wc -c < {path}) && [ \"$size\" -le {MAX_SFTP_IMAGE_PREVIEW_BYTES} ] && base64 < {path}"
    )
}

fn build_remote_directory_summary_command(remote_path: &str) -> String {
    let script = r#"root=$1
files=$(find "$root" -type f 2>/dev/null | wc -l | tr -d ' ')
dirs=$(find "$root" -type d 2>/dev/null | sed 1d | wc -l | tr -d ' ')
bytes=$(find "$root" -type f -exec wc -c {} \; 2>/dev/null | awk '{sum += $1} END {print sum + 0}')
mtime=$(stat -c %Y "$root" 2>/dev/null || echo 0)
printf '%s\t%s\t%s\t%s\n' "$files" "$dirs" "$bytes" "$mtime""#;
    format!(
        "sh -c {} sh {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(remote_path)
    )
}

fn build_remote_create_dir_command(remote_path: &str) -> String {
    format!("mkdir -- {}", crate::ssh::shell_quote_posix(remote_path))
}

fn build_remote_delete_command(paths: &[String]) -> String {
    let quoted = paths
        .iter()
        .map(|path| crate::ssh::shell_quote_posix(path))
        .collect::<Vec<_>>()
        .join(" ");
    format!("rm -rf -- {quoted}")
}

fn build_remote_conflict_check_command(
    names: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if names.is_empty() {
        return Ok(":".to_string());
    }
    let target = crate::ssh::shell_quote_posix(target_directory);
    let names = names
        .iter()
        .map(|name| {
            validate_entry_name(name)?;
            Ok(crate::ssh::shell_quote_posix(name))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && for name in {names}; do [ ! -e \"$target/$name\" ] || {{ echo \"A file or folder with that name already exists\" >&2; exit 1; }}; done",
        names = names.join(" ")
    ))
}

fn build_remote_delete_target_names_command(
    names: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if names.is_empty() {
        return Ok(":".to_string());
    }
    let target = crate::ssh::shell_quote_posix(target_directory);
    let names = names
        .iter()
        .map(|name| {
            validate_entry_name(name)?;
            Ok(crate::ssh::shell_quote_posix(name))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && for name in {names}; do rm -rf -- \"$target/$name\"; done",
        names = names.join(" ")
    ))
}

fn build_remote_merge_conflict_check_command(
    names: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if names.is_empty() {
        return Ok(":".to_string());
    }
    let target = crate::ssh::shell_quote_posix(target_directory);
    let names = names
        .iter()
        .map(|name| {
            validate_entry_name(name)?;
            Ok(crate::ssh::shell_quote_posix(name))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && for name in {names}; do [ ! -e \"$target/$name\" ] || [ -d \"$target/$name\" ] || {{ echo \"Cannot merge a file into an existing file\" >&2; exit 1; }}; done",
        names = names.join(" ")
    ))
}

fn build_remote_rename_command(remote_path: &str, new_name: &str) -> Result<String, String> {
    validate_entry_name(new_name)?;
    let destination = join_remote_path(remote_parent(remote_path)?, new_name);
    Ok(format!(
        "[ ! -e {dest} ] && mv -- {src} {dest}",
        src = crate::ssh::shell_quote_posix(remote_path),
        dest = crate::ssh::shell_quote_posix(&destination)
    ))
}

fn build_remote_copy_or_move_command(
    source_paths: &[String],
    target_directory: &str,
    move_paths: bool,
    conflict_strategy: SftpConflictStrategy,
) -> Result<String, String> {
    if source_paths.is_empty() {
        return Ok(":".to_string());
    }
    let target = crate::ssh::shell_quote_posix(target_directory);
    let sources = source_paths
        .iter()
        .map(|source| {
            validate_entry_name(basename(source)?)?;
            if remote_parent(source)? == target_directory {
                return Err("Cannot replace a file or folder with itself".to_string());
            }
            Ok(crate::ssh::shell_quote_posix(source))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let tool = if move_paths { "mv" } else { "cp -R" };
    let sources = sources.join(" ");
    let preflight = match conflict_strategy {
        SftpConflictStrategy::Fail => format!(
            "for src in {sources}; do name=${{src##*/}}; [ ! -e \"$target/$name\" ] || {{ echo \"A file or folder with that name already exists\" >&2; exit 1; }}; done && "
        ),
        SftpConflictStrategy::Merge => format!(
            "for src in {sources}; do name=${{src##*/}}; [ ! -e \"$target/$name\" ] || [ -d \"$target/$name\" ] || {{ echo \"Cannot merge a file into an existing file\" >&2; exit 1; }}; done && "
        ),
        SftpConflictStrategy::Replace => format!(
            "for src in {sources}; do name=${{src##*/}}; rm -rf -- \"$target/$name\"; done && "
        ),
    };
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && {preflight}{tool} -- {sources} \"$target/\"",
    ))
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

fn local_image_mime_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
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

fn run_ssh_output(connection: &SshConnection, remote_command: String) -> Result<Vec<u8>, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, remote_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

fn scp_base_spec(connection: &SshConnection) -> CommandSpec {
    let password = connection
        .password
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    let (program, mut args, env) = if let Some(password) = password {
        (
            {
                let detected = crate::platform::detect_path("sshpass");
                if detected.is_empty() {
                    "sshpass".to_string()
                } else {
                    detected
                }
            },
            vec![
                "-e".to_string(),
                "scp".to_string(),
                "-o".to_string(),
                "PreferredAuthentications=password,keyboard-interactive".to_string(),
                "-o".to_string(),
                "PubkeyAuthentication=no".to_string(),
            ],
            vec![("SSHPASS".to_string(), password.to_string())],
        )
    } else {
        ("scp".to_string(), Vec::new(), Vec::new())
    };
    args.push("-P".to_string());
    args.push(connection.port.to_string());
    args.push("-r".to_string());
    if let Some(identity_file) = connection
        .identity_file
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        args.push("-i".to_string());
        args.push(identity_file.to_string());
    }
    CommandSpec { program, args, env }
}

fn remote_scp_target(connection: &SshConnection, remote_path: &str) -> String {
    format!(
        "{}@{}:{}",
        connection.username, connection.host, remote_path
    )
}

fn scp_upload_spec(
    connection: &SshConnection,
    local_paths: &[String],
    remote_directory: &str,
) -> Result<CommandSpec, String> {
    let mut spec = scp_base_spec(connection);
    for path in local_paths {
        validate_sftp_local_path(path)?;
        spec.args.push(path.to_string());
    }
    spec.args
        .push(remote_scp_target(connection, remote_directory));
    Ok(spec)
}

fn scp_download_spec(
    connection: &SshConnection,
    remote_paths: &[String],
    local_directory: &str,
) -> Result<CommandSpec, String> {
    validate_sftp_local_path(local_directory)?;
    let mut spec = scp_base_spec(connection);
    for path in remote_paths {
        validate_sftp_remote_path(path)?;
        spec.args.push(remote_scp_target(connection, path));
    }
    spec.args.push(local_directory.to_string());
    Ok(spec)
}

fn run_command_spec(spec: CommandSpec) -> Result<(), String> {
    let mut cmd = Command::new(spec.program);
    cmd.args(spec.args);
    for (key, value) in spec.env {
        cmd.env(key, value);
    }
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn parse_remote_entries(remote_path: &str, raw: &str) -> Vec<SftpEntry> {
    let mut entries = raw
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?;
            let kind = parts.next()?;
            let size = parts.next().and_then(|value| value.parse::<u64>().ok());
            let is_dir = kind == "d";
            Some(SftpEntry {
                name: name.to_string(),
                path: join_remote_path(remote_path, name),
                is_dir,
                extension: extension_for_name(name, is_dir),
                size,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
    entries
}

fn read_local_dir(path: String) -> Result<Vec<SftpEntry>, String> {
    let path = validate_sftp_local_path(&path)?;
    let entries = std::fs::read_dir(&path).map_err(|e| e.to_string())?;
    let mut result = entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(|meta| meta.is_dir());
            SftpEntry {
                name: name.clone(),
                path: path.to_string_lossy().into_owned(),
                is_dir,
                extension: extension_for_name(&name, is_dir),
                size: meta.filter(|meta| meta.is_file()).map(|meta| meta.len()),
            }
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
    Ok(result)
}

fn read_local_directory_summary(path: &Path) -> Result<SftpDirectorySummary, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir() {
        return Err("Path must be a directory".to_string());
    }

    fn walk(path: &Path, summary: &mut SftpDirectorySummary) -> Result<(), String> {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let metadata = entry.path().symlink_metadata().map_err(|e| e.to_string())?;
            if metadata.is_dir() {
                summary.directory_count += 1;
                walk(&entry.path(), summary)?;
            } else if metadata.is_file() {
                summary.file_count += 1;
                summary.total_size += metadata.len();
            }
        }
        Ok(())
    }

    let mut summary = SftpDirectorySummary {
        file_count: 0,
        directory_count: 0,
        total_size: 0,
        modified_at_ms: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64),
    };
    walk(path, &mut summary)?;
    Ok(summary)
}

fn parse_remote_directory_summary(raw: &[u8]) -> Result<SftpDirectorySummary, String> {
    let text = String::from_utf8_lossy(raw);
    let mut parts = text.trim().split('\t');
    let file_count = parts
        .next()
        .ok_or_else(|| "Missing file count".to_string())?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    let directory_count = parts
        .next()
        .ok_or_else(|| "Missing directory count".to_string())?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    let total_size = parts
        .next()
        .ok_or_else(|| "Missing total size".to_string())?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    let modified_at_ms = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .and_then(|seconds| seconds.checked_mul(1000))
        .filter(|value| *value > 0);
    Ok(SftpDirectorySummary {
        file_count,
        directory_count,
        total_size,
        modified_at_ms,
    })
}

fn copy_path_recursive(
    source: &Path,
    destination: &Path,
    conflict_strategy: SftpConflictStrategy,
) -> Result<(), String> {
    let metadata = source.symlink_metadata().map_err(|e| e.to_string())?;
    if destination.exists() && conflict_strategy == SftpConflictStrategy::Replace {
        delete_local_path(destination)?;
    }
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(source).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            if source.is_dir() {
                std::os::windows::fs::symlink_dir(target, destination)
                    .map_err(|e| e.to_string())?;
            } else {
                std::os::windows::fs::symlink_file(target, destination)
                    .map_err(|e| e.to_string())?;
            }
        }
        return Ok(());
    }
    if metadata.is_dir() {
        if !destination.exists() {
            std::fs::create_dir(destination).map_err(|e| e.to_string())?;
        } else if !destination.is_dir() {
            return Err("Cannot merge a directory into a file".to_string());
        }
        for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_path_recursive(
                &entry.path(),
                &destination.join(entry.file_name()),
                conflict_strategy,
            )?;
        }
        return Ok(());
    }
    if destination.exists() && conflict_strategy == SftpConflictStrategy::Merge {
        return Err("Cannot merge a file into an existing file".to_string());
    }
    std::fs::copy(source, destination)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn copy_local_paths_to_directory(
    source_paths: Vec<String>,
    target_directory: String,
    conflict_strategy: SftpConflictStrategy,
) -> Result<(), String> {
    let target = validate_sftp_local_path(&target_directory)?;
    if !target.is_dir() {
        return Err("Target must be a directory".to_string());
    }
    for source_path in source_paths {
        let source = validate_sftp_local_path(&source_path)?;
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "Invalid file name".to_string())?;
        validate_entry_name(name)?;
        let destination = target.join(name);
        if source == destination {
            return Err("Cannot replace a file or folder with itself".to_string());
        }
        if destination.exists() && conflict_strategy == SftpConflictStrategy::Fail {
            return Err("A file or folder with that name already exists".to_string());
        }
        if destination.exists() && conflict_strategy == SftpConflictStrategy::Replace {
            delete_local_path(&destination)?;
        }
        copy_path_recursive(&source, &destination, conflict_strategy)?;
    }
    Ok(())
}

fn local_basenames(paths: &[String]) -> Result<Vec<String>, String> {
    paths
        .iter()
        .map(|source| {
            let path = validate_sftp_local_path(source)?;
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| "Invalid file name".to_string())?
                .to_string();
            validate_entry_name(&name)?;
            Ok(name)
        })
        .collect()
}

fn remote_basenames(paths: &[String]) -> Result<Vec<String>, String> {
    paths
        .iter()
        .map(|path| {
            validate_entry_name(basename(path)?)?;
            Ok(basename(path)?.to_string())
        })
        .collect()
}

fn ensure_local_target_names_available(
    names: &[String],
    target_directory: &str,
    conflict_strategy: SftpConflictStrategy,
) -> Result<(), String> {
    let target = validate_sftp_local_path(target_directory)?;
    if !target.is_dir() {
        return Err("Target must be a directory".to_string());
    }
    for name in names {
        validate_entry_name(name)?;
        if target.join(name).exists() && conflict_strategy == SftpConflictStrategy::Fail {
            return Err("A file or folder with that name already exists".to_string());
        }
        if target.join(name).exists() && conflict_strategy == SftpConflictStrategy::Replace {
            delete_local_path(&target.join(name))?;
        }
    }
    Ok(())
}

fn delete_local_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}

fn delete_local_sources(paths: &[String]) -> Result<(), String> {
    for path in paths {
        let path = validate_sftp_local_path(path)?;
        delete_local_path(&path)?;
    }
    Ok(())
}

fn move_local_paths_to_directory(
    source_paths: Vec<String>,
    target_directory: String,
    conflict_strategy: SftpConflictStrategy,
) -> Result<(), String> {
    let target = validate_sftp_local_path(&target_directory)?;
    if !target.is_dir() {
        return Err("Target must be a directory".to_string());
    }
    for source_path in source_paths {
        let source = validate_sftp_local_path(&source_path)?;
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "Invalid file name".to_string())?;
        validate_entry_name(name)?;
        let destination = target.join(name);
        if source == destination {
            return Err("Cannot replace a file or folder with itself".to_string());
        }
        if destination.exists() && conflict_strategy == SftpConflictStrategy::Fail {
            return Err("A file or folder with that name already exists".to_string());
        }
        if destination.exists() && conflict_strategy == SftpConflictStrategy::Replace {
            delete_local_path(&destination)?;
        }
        std::fs::rename(&source, &destination).or_else(|_| {
            copy_path_recursive(&source, &destination, conflict_strategy)?;
            if source.is_dir() {
                std::fs::remove_dir_all(&source).map_err(|e| e.to_string())
            } else {
                std::fs::remove_file(&source).map_err(|e| e.to_string())
            }
        })?;
    }
    Ok(())
}

#[tauri::command]
pub async fn sftp_read_dir(endpoint: SftpEndpoint) -> Result<Vec<SftpEntry>, String> {
    tokio::task::spawn_blocking(move || match endpoint {
        SftpEndpoint::Local { path } => read_local_dir(path),
        SftpEndpoint::Ssh { connection, path } => {
            let path = validate_sftp_remote_path(&path)?;
            let stdout = run_ssh_output(&connection, build_remote_read_dir_command(&path))?;
            Ok(parse_remote_entries(
                &path,
                &String::from_utf8_lossy(&stdout),
            ))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_read_text_file(endpoint: SftpEndpoint) -> Result<String, String> {
    tokio::task::spawn_blocking(move || match endpoint {
        SftpEndpoint::Local { path } => {
            let path = validate_sftp_local_path(&path)?;
            let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
            if meta.len() > MAX_SFTP_TEXT_FILE_BYTES {
                return Err(format!(
                    "File too large ({:.1} MB)",
                    meta.len() as f64 / 1024.0 / 1024.0
                ));
            }
            std::fs::read_to_string(&path).map_err(|e| e.to_string())
        }
        SftpEndpoint::Ssh { connection, path } => {
            let path = validate_sftp_remote_path(&path)?;
            let stdout = run_ssh_output(&connection, build_remote_read_text_command(&path))?;
            String::from_utf8(stdout).map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_read_image_preview(
    endpoint: SftpEndpoint,
) -> Result<SftpImagePreviewData, String> {
    tokio::task::spawn_blocking(move || match endpoint {
        SftpEndpoint::Local { path } => {
            let path = validate_sftp_local_path(&path)?;
            let mime_type = local_image_mime_type(&path)
                .ok_or_else(|| "Unsupported image format".to_string())?;
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_SFTP_IMAGE_PREVIEW_BYTES {
                return Err(format!(
                    "Image too large ({:.1} MB)",
                    bytes.len() as f64 / 1024.0 / 1024.0
                ));
            }
            Ok(SftpImagePreviewData {
                data_url: format!(
                    "data:{};base64,{}",
                    mime_type,
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                ),
                mime_type: mime_type.to_string(),
                byte_length: bytes.len() as u64,
            })
        }
        SftpEndpoint::Ssh { connection, path } => {
            let path = validate_sftp_remote_path(&path)?;
            let mime_type = remote_image_mime_type(&path)
                .ok_or_else(|| "Unsupported image format".to_string())?;
            let stdout = run_ssh_output(&connection, build_remote_image_preview_command(&path))?;
            let encoded = String::from_utf8_lossy(&stdout)
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.as_bytes())
                .map_err(|e| e.to_string())?;
            Ok(SftpImagePreviewData {
                data_url: format!(
                    "data:{};base64,{}",
                    mime_type,
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                ),
                mime_type: mime_type.to_string(),
                byte_length: bytes.len() as u64,
            })
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_read_directory_summary(
    endpoint: SftpEndpoint,
) -> Result<SftpDirectorySummary, String> {
    tokio::task::spawn_blocking(move || match endpoint {
        SftpEndpoint::Local { path } => {
            let path = validate_sftp_local_path(&path)?;
            read_local_directory_summary(&path)
        }
        SftpEndpoint::Ssh { connection, path } => {
            let path = validate_sftp_remote_path(&path)?;
            let stdout =
                run_ssh_output(&connection, build_remote_directory_summary_command(&path))?;
            parse_remote_directory_summary(&stdout)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_create_directory(endpoint: SftpEndpoint, name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let name = name.trim();
        validate_entry_name(name)?;
        match endpoint {
            SftpEndpoint::Local { path } => {
                let target = validate_sftp_local_path(&path)?.join(name);
                std::fs::create_dir(&target).map_err(|e| e.to_string())
            }
            SftpEndpoint::Ssh { connection, path } => {
                let target = join_remote_path(&validate_sftp_remote_path(&path)?, name);
                run_ssh_output(&connection, build_remote_create_dir_command(&target)).map(|_| ())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_delete_paths(endpoint: SftpEndpoint, paths: Vec<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || match endpoint {
        SftpEndpoint::Local { .. } => {
            for path in paths {
                let path = validate_sftp_local_path(&path)?;
                trash::delete(&path).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        SftpEndpoint::Ssh { connection, .. } => {
            let paths = paths
                .into_iter()
                .map(|path| validate_sftp_remote_path(&path))
                .collect::<Result<Vec<_>, String>>()?;
            run_ssh_output(&connection, build_remote_delete_command(&paths)).map(|_| ())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_rename_path(
    endpoint: SftpEndpoint,
    path: String,
    new_name: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let new_name = new_name.trim();
        validate_entry_name(new_name)?;
        match endpoint {
            SftpEndpoint::Local { .. } => {
                let source = validate_sftp_local_path(&path)?;
                let destination = source
                    .parent()
                    .ok_or_else(|| "Cannot resolve parent directory".to_string())?
                    .join(new_name);
                if destination.exists() {
                    return Err("A file or folder with that name already exists".to_string());
                }
                std::fs::rename(&source, destination).map_err(|e| e.to_string())
            }
            SftpEndpoint::Ssh { connection, .. } => {
                let path = validate_sftp_remote_path(&path)?;
                run_ssh_output(&connection, build_remote_rename_command(&path, new_name)?)
                    .map(|_| ())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_copy_paths(
    source: SftpEndpoint,
    paths: Vec<String>,
    target: SftpEndpoint,
    conflict_strategy: Option<SftpConflictStrategy>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        copy_or_move_paths(
            source,
            paths,
            target,
            false,
            conflict_strategy.unwrap_or_default(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sftp_move_paths(
    source: SftpEndpoint,
    paths: Vec<String>,
    target: SftpEndpoint,
    conflict_strategy: Option<SftpConflictStrategy>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        copy_or_move_paths(
            source,
            paths,
            target,
            true,
            conflict_strategy.unwrap_or_default(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn copy_or_move_paths(
    source: SftpEndpoint,
    paths: Vec<String>,
    target: SftpEndpoint,
    move_paths: bool,
    conflict_strategy: SftpConflictStrategy,
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    match (&source, &target) {
        (SftpEndpoint::Local { .. }, SftpEndpoint::Local { path: target_path }) => {
            if move_paths {
                move_local_paths_to_directory(paths, target_path.clone(), conflict_strategy)
            } else {
                copy_local_paths_to_directory(paths, target_path.clone(), conflict_strategy)
            }
        }
        (
            SftpEndpoint::Ssh { connection, .. },
            SftpEndpoint::Ssh {
                connection: target_connection,
                path: target_path,
            },
        ) if connection.id == target_connection.id => {
            let source_paths = paths
                .into_iter()
                .map(|path| validate_sftp_remote_path(&path))
                .collect::<Result<Vec<_>, String>>()?;
            let target_path = validate_sftp_remote_path(target_path)?;
            run_ssh_output(
                connection,
                build_remote_copy_or_move_command(
                    &source_paths,
                    &target_path,
                    move_paths,
                    conflict_strategy,
                )?,
            )
            .map(|_| ())
        }
        (
            SftpEndpoint::Local { .. },
            SftpEndpoint::Ssh {
                connection,
                path: target_path,
            },
        ) => {
            let target_path = validate_sftp_remote_path(target_path)?;
            let names = local_basenames(&paths)?;
            if conflict_strategy == SftpConflictStrategy::Fail {
                run_ssh_output(
                    connection,
                    build_remote_conflict_check_command(&names, &target_path)?,
                )?;
            } else if conflict_strategy == SftpConflictStrategy::Merge {
                run_ssh_output(
                    connection,
                    build_remote_merge_conflict_check_command(&names, &target_path)?,
                )?;
            } else if conflict_strategy == SftpConflictStrategy::Replace {
                run_ssh_output(
                    connection,
                    build_remote_delete_target_names_command(&names, &target_path)?,
                )?;
            }
            run_command_spec(scp_upload_spec(connection, &paths, &target_path)?)?;
            if move_paths {
                delete_local_sources(&paths)?;
            }
            Ok(())
        }
        (SftpEndpoint::Ssh { connection, .. }, SftpEndpoint::Local { path: target_path }) => {
            let source_paths = paths
                .into_iter()
                .map(|path| validate_sftp_remote_path(&path))
                .collect::<Result<Vec<_>, String>>()?;
            ensure_local_target_names_available(
                &remote_basenames(&source_paths)?,
                target_path,
                conflict_strategy,
            )?;
            run_command_spec(scp_download_spec(connection, &source_paths, target_path)?)?;
            if move_paths {
                run_ssh_output(connection, build_remote_delete_command(&source_paths))?;
            }
            Ok(())
        }
        (
            SftpEndpoint::Ssh { connection, .. },
            SftpEndpoint::Ssh {
                connection: target_connection,
                path: target_path,
            },
        ) => {
            let temp_root = std::env::temp_dir().join(format!(
                "aeroric-sftp-transfer-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_nanos()
            ));
            std::fs::create_dir(&temp_root).map_err(|e| e.to_string())?;
            let temp_string = temp_root.to_string_lossy().into_owned();
            let source_paths = paths
                .iter()
                .map(|path| validate_sftp_remote_path(path))
                .collect::<Result<Vec<_>, String>>()?;
            let transfer_result = (|| {
                run_command_spec(scp_download_spec(connection, &source_paths, &temp_string)?)?;
                let local_paths = std::fs::read_dir(&temp_root)
                    .map_err(|e| e.to_string())?
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path().to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                let target_path = validate_sftp_remote_path(target_path)?;
                let names = remote_basenames(&source_paths)?;
                if conflict_strategy == SftpConflictStrategy::Fail {
                    run_ssh_output(
                        target_connection,
                        build_remote_conflict_check_command(&names, &target_path)?,
                    )?;
                } else if conflict_strategy == SftpConflictStrategy::Merge {
                    run_ssh_output(
                        target_connection,
                        build_remote_merge_conflict_check_command(&names, &target_path)?,
                    )?;
                } else if conflict_strategy == SftpConflictStrategy::Replace {
                    run_ssh_output(
                        target_connection,
                        build_remote_delete_target_names_command(&names, &target_path)?,
                    )?;
                }
                run_command_spec(scp_upload_spec(
                    target_connection,
                    &local_paths,
                    &target_path,
                )?)?;
                if move_paths {
                    run_ssh_output(connection, build_remote_delete_command(&source_paths))?;
                }
                Ok(())
            })();
            let _ = std::fs::remove_dir_all(&temp_root);
            transfer_result
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ssh::SshConnection;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("aeroric-sftp-test-{}-{}", name, suffix))
    }

    #[test]
    fn local_endpoint_rejects_relative_paths() {
        let result = super::validate_sftp_local_path("relative/path");

        assert!(result.is_err());
    }

    #[test]
    fn remote_endpoint_requires_absolute_paths() {
        let result = super::validate_sftp_remote_path("tmp/app");

        assert!(result.is_err());
    }

    #[test]
    fn local_directory_summary_counts_files_dirs_and_bytes() {
        let root = unique_test_dir("summary");
        std::fs::create_dir_all(root.join("src/nested")).expect("create dirs");
        std::fs::write(root.join("README.md"), b"hello").expect("write readme");
        std::fs::write(root.join("src/main.rs"), b"fn main() {}\n").expect("write main");
        std::fs::write(root.join("src/nested/mod.rs"), b"mod inner;\n").expect("write nested");

        let summary = super::read_local_directory_summary(&root).expect("summary");

        assert_eq!(summary.file_count, 3);
        assert_eq!(summary.directory_count, 2);
        assert_eq!(summary.total_size, 5 + 13 + 11);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scp_download_command_uses_password_auth_options() {
        let connection = SshConnection {
            id: "prod".to_string(),
            name: "Prod".to_string(),
            group: None,
            host: "example.com".to_string(),
            port: 2222,
            username: "deploy".to_string(),
            identity_file: None,
            password: Some("secret".to_string()),
            remote_path: None,
            created_at: 1,
            last_connected_at: None,
        };

        let spec = super::scp_download_spec(
            &connection,
            &["/srv/app/a file.txt".to_string()],
            "/tmp/out",
        )
        .expect("build scp spec");

        assert!(spec.args.contains(&"-P".to_string()));
        assert!(spec.args.contains(&"2222".to_string()));
        assert!(spec.args.contains(&"-o".to_string()));
        assert!(spec
            .args
            .contains(&"PreferredAuthentications=password,keyboard-interactive".to_string()));
        assert!(spec.args.contains(&"PubkeyAuthentication=no".to_string()));
        assert!(spec
            .args
            .contains(&"deploy@example.com:/srv/app/a file.txt".to_string()));
        assert_eq!(
            spec.env,
            vec![("SSHPASS".to_string(), "secret".to_string())]
        );
    }

    #[test]
    fn scp_upload_target_uses_unquoted_remote_path_for_sftp_mode() {
        let connection = SshConnection {
            id: "prod".to_string(),
            name: "Prod".to_string(),
            group: None,
            host: "example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            password: None,
            remote_path: None,
            created_at: 1,
            last_connected_at: None,
        };

        let spec = super::scp_upload_spec(
            &connection,
            &["/tmp/source.txt".to_string()],
            "/home/home/algorithm",
        )
        .expect("build scp spec");

        assert!(spec
            .args
            .contains(&"deploy@example.com:/home/home/algorithm".to_string()));
        assert!(!spec
            .args
            .iter()
            .any(|arg| arg.contains(":'/home/home/algorithm'")));
    }

    #[test]
    fn remote_copy_merge_checks_file_conflicts_before_copy() {
        let command = super::build_remote_copy_or_move_command(
            &["/srv/source/same.txt".to_string()],
            "/srv/target",
            false,
            super::SftpConflictStrategy::Merge,
        )
        .expect("build command");

        assert!(command.contains("[ ! -e \"$target/$name\" ] || [ -d \"$target/$name\" ]"));
        assert!(command.contains("Cannot merge a file into an existing file"));
        assert!(command.contains("cp -R --"));
        assert!(command.contains("/srv/source/same.txt"));
        assert!(command.contains("\"$target/\""));
    }

    #[test]
    fn remote_merge_conflict_check_allows_only_missing_or_directory_targets() {
        let command = super::build_remote_merge_conflict_check_command(
            &["same.txt".to_string()],
            "/srv/target",
        )
        .expect("build command");

        assert!(command.contains("for name in"));
        assert!(command.contains("same.txt"));
        assert!(command.contains("[ ! -e \"$target/$name\" ] || [ -d \"$target/$name\" ]"));
        assert!(command.contains("Cannot merge a file into an existing file"));
    }

    #[test]
    fn local_copy_rejects_existing_destination() {
        let root = unique_test_dir("copy-conflict");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        std::fs::create_dir_all(&source_dir).expect("create source");
        std::fs::create_dir_all(&target_dir).expect("create target");
        std::fs::write(source_dir.join("same.txt"), "source").expect("write source");
        std::fs::write(target_dir.join("same.txt"), "target").expect("write target");

        let result = super::copy_local_paths_to_directory(
            vec![source_dir.join("same.txt").to_string_lossy().into_owned()],
            target_dir.to_string_lossy().into_owned(),
            super::SftpConflictStrategy::Fail,
        );

        let _ = std::fs::remove_dir_all(&root);
        assert!(result.is_err());
    }

    #[test]
    fn local_copy_replaces_existing_destination_when_requested() {
        let root = unique_test_dir("copy-replace");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        std::fs::create_dir_all(&source_dir).expect("create source");
        std::fs::create_dir_all(&target_dir).expect("create target");
        std::fs::write(source_dir.join("same.txt"), "source").expect("write source");
        std::fs::write(target_dir.join("same.txt"), "target").expect("write target");

        let result = super::copy_local_paths_to_directory(
            vec![source_dir.join("same.txt").to_string_lossy().into_owned()],
            target_dir.to_string_lossy().into_owned(),
            super::SftpConflictStrategy::Replace,
        );

        assert!(result.is_ok());
        assert_eq!(
            std::fs::read_to_string(target_dir.join("same.txt")).expect("read target"),
            "source"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn local_copy_merge_rejects_file_overwrite() {
        let root = unique_test_dir("copy-merge-file-conflict");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        std::fs::create_dir_all(&source_dir).expect("create source");
        std::fs::create_dir_all(&target_dir).expect("create target");
        std::fs::write(source_dir.join("same.txt"), "source").expect("write source");
        std::fs::write(target_dir.join("same.txt"), "target").expect("write target");

        let result = super::copy_local_paths_to_directory(
            vec![source_dir.join("same.txt").to_string_lossy().into_owned()],
            target_dir.to_string_lossy().into_owned(),
            super::SftpConflictStrategy::Merge,
        );

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(target_dir.join("same.txt")).expect("read target"),
            "target"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn local_copy_rejects_replacing_source_with_itself() {
        let root = unique_test_dir("copy-replace-self");
        std::fs::create_dir_all(&root).expect("create root");
        let source = root.join("same.txt");
        std::fs::write(&source, "source").expect("write source");

        let result = super::copy_local_paths_to_directory(
            vec![source.to_string_lossy().into_owned()],
            root.to_string_lossy().into_owned(),
            super::SftpConflictStrategy::Replace,
        );

        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&source).expect("source should remain"),
            "source"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn local_move_removes_source_file() {
        let root = unique_test_dir("move-file");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        std::fs::create_dir_all(&source_dir).expect("create source");
        std::fs::create_dir_all(&target_dir).expect("create target");
        let source = source_dir.join("move.txt");
        std::fs::write(&source, "data").expect("write source");

        let result = super::move_local_paths_to_directory(
            vec![source.to_string_lossy().into_owned()],
            target_dir.to_string_lossy().into_owned(),
            super::SftpConflictStrategy::Fail,
        );

        assert!(result.is_ok());
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(target_dir.join("move.txt")).expect("read target"),
            "data"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
