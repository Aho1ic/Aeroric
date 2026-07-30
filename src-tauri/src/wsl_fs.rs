use std::io::Write;
use std::process::Stdio;

use base64::Engine;
use serde::Serialize;

const MAX_REMOTE_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_REMOTE_IMAGE_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Serialize)]
pub(crate) struct WslFsEntry {
    name: String,
    path: String,
    is_dir: bool,
    extension: Option<String>,
    modified_at_ms: Option<u64>,
    is_gitignored: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WslImagePreviewData {
    data_url: String,
    mime_type: String,
    byte_length: u64,
}

fn build_wsl_read_dir_command(linux_path: &str) -> String {
    let script = "cd \"$1\" && for p in ./* ./.[!.]* ./..?*; do [ -e \"$p\" ] || continue; name=${p#./}; if [ \"$name\" = \".\" ] || [ \"$name\" = \"..\" ]; then continue; fi; if [ -d \"$p\" ]; then type=d; else type=f; fi; mtime=$(stat -c %Y \"$p\" 2>/dev/null || stat -f %m \"$p\" 2>/dev/null || echo 0); printf '%s\\t%s\\t%s\\n' \"$name\" \"$type\" \"$mtime\"; done";
    format!(
        "sh -c {} sh {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(linux_path)
    )
}

fn build_wsl_read_file_command(linux_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(linux_path);
    format!("size=$(wc -c < {path}) && [ \"$size\" -le {MAX_REMOTE_FILE_BYTES} ] && cat -- {path}")
}

fn build_wsl_write_file_command(linux_path: &str) -> String {
    format!("cat > {}", crate::ssh::shell_quote_posix(linux_path))
}

fn build_wsl_create_file_command(linux_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(linux_path);
    format!("test ! -e {path} && : > {path}")
}

fn build_wsl_create_directory_command(linux_path: &str) -> String {
    format!("mkdir -- {}", crate::ssh::shell_quote_posix(linux_path))
}

fn build_wsl_delete_path_command(linux_path: &str) -> String {
    format!("rm -rf -- {}", crate::ssh::shell_quote_posix(linux_path))
}

fn validate_wsl_entry_name(name: &str) -> Result<(), String> {
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

fn wsl_parent_path(linux_path: &str) -> Result<&str, String> {
    let trimmed = linux_path.trim_end_matches('/');
    let Some((parent, _)) = trimmed.rsplit_once('/') else {
        return Err("Cannot resolve parent directory".to_string());
    };
    if parent.is_empty() {
        Ok("/")
    } else {
        Ok(parent)
    }
}

fn wsl_basename(linux_path: &str) -> Result<&str, String> {
    let trimmed = linux_path.trim_end_matches('/');
    trimmed
        .rsplit_once('/')
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Invalid file name".to_string())
}

fn build_wsl_rename_path_command(linux_path: &str, new_name: &str) -> Result<String, String> {
    validate_wsl_entry_name(new_name)?;
    let parent = wsl_parent_path(linux_path)?;
    let destination = if parent == "/" {
        format!("/{}", new_name)
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), new_name)
    };
    let source = crate::ssh::shell_quote_posix(linux_path);
    let dest = crate::ssh::shell_quote_posix(&destination);
    Ok(format!(
        "[ ! -e {dest} ] && mv -- {source} {dest}",
        source = source,
        dest = dest
    ))
}

fn build_wsl_copy_paths_command(
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
            validate_wsl_entry_name(wsl_basename(source)?)?;
            Ok(crate::ssh::shell_quote_posix(source))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && for src in {sources}; do name=${{src##*/}}; [ ! -e \"$target/$name\" ] || {{ echo \"A file or folder with that name already exists\" >&2; exit 1; }}; done && cp -R -- {sources} \"$target/\"",
        target = target,
        sources = sources.join(" ")
    ))
}

fn build_wsl_image_preview_command(linux_path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(linux_path);
    format!(
        "size=$(wc -c < {path}) && [ \"$size\" -le {MAX_REMOTE_IMAGE_PREVIEW_BYTES} ] && base64 < {path}"
    )
}

fn wsl_image_mime_type(linux_path: &str) -> Option<&'static str> {
    let ext = linux_path.rsplit_once('.')?.1.to_ascii_lowercase();
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

fn normalize_wsl_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    }
}

fn linux_path_has_relative_components(path: &str) -> bool {
    path.split('/')
        .any(|component| component == "." || component == "..")
}

fn ensure_wsl_path_allowed(
    linux_path: &str,
    linux_project_path: Option<&str>,
    allow_project_root: bool,
) -> Result<(), String> {
    if !linux_path.starts_with('/') {
        return Err("WSL path must be absolute".to_string());
    }
    if linux_path_has_relative_components(linux_path) {
        return Err("WSL path cannot contain . or .. components".to_string());
    }
    let Some(linux_project_path) = linux_project_path else {
        return Ok(());
    };
    if !linux_project_path.starts_with('/') {
        return Err("WSL project path must be absolute".to_string());
    }
    if linux_path_has_relative_components(linux_project_path) {
        return Err("WSL project path cannot contain . or .. components".to_string());
    }
    let path = normalize_wsl_path(linux_path);
    let root = normalize_wsl_path(linux_project_path);
    if path == root {
        if allow_project_root {
            return Ok(());
        }
        return Err("Cannot modify the WSL project root".to_string());
    }
    let root_prefix = if root == "/" {
        "/".to_string()
    } else {
        format!("{root}/")
    };
    if !path.starts_with(&root_prefix) {
        return Err("WSL path is outside the project root".to_string());
    }
    if let Some(first) = path[root_prefix.len()..].split('/').next() {
        if first == ".git" || first == ".aeroric" {
            return Err(format!("Cannot modify protected WSL directory: {}", first));
        }
    }
    Ok(())
}

fn build_wsl_resolve_path_command(linux_path: &str, linux_project_path: &str) -> String {
    let script = r#"resolve_path() {
  path=$1
  hops=0
  while [ -L "$path" ]; do
    hops=$((hops + 1))
    [ "$hops" -le 40 ] || exit 72
    link=$(readlink "$path") || exit 72
    case "$link" in
      /*) path=$link ;;
      *)
        parent=${path%/*}
        [ -n "$parent" ] || parent=/
        path=$parent/$link
        ;;
    esac
  done
  if [ "$path" = "/" ]; then
    printf /
    return
  fi
  parent=${path%/*}
  name=${path##*/}
  [ -n "$parent" ] || parent=/
  physical_parent=$(cd -P "$parent" && pwd -P) || exit 72
  if [ "$physical_parent" = "/" ]; then
    printf '/%s' "$name"
  else
    printf '%s/%s' "$physical_parent" "$name"
  fi
}
root=$(resolve_path "$1") || exit 72
if [ -e "$2" ] || [ -L "$2" ]; then
  target=$(resolve_path "$2") || exit 72
else
  parent=${2%/*}
  name=${2##*/}
  [ -n "$parent" ] || parent=/
  resolved_parent=$(resolve_path "$parent") || exit 72
  if [ "$resolved_parent" = "/" ]; then
    target=/$name
  else
    target=$resolved_parent/$name
  fi
fi
printf '%s\0%s\0' "$root" "$target""#;
    format!(
        "sh -c {} sh {} {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(linux_project_path),
        crate::ssh::shell_quote_posix(linux_path)
    )
}

fn ensure_resolved_wsl_path_allowed(
    resolved_path: &str,
    resolved_root: &str,
    allow_project_root: bool,
) -> Result<(), String> {
    let path = normalize_wsl_path(resolved_path);
    let root = normalize_wsl_path(resolved_root);
    if path == root {
        return if allow_project_root {
            Ok(())
        } else {
            Err("Cannot modify the WSL project root".to_string())
        };
    }
    let root_prefix = if root == "/" {
        "/".to_string()
    } else {
        format!("{root}/")
    };
    if !path.starts_with(&root_prefix) {
        return Err("WSL path resolves outside the project root".to_string());
    }
    if let Some(first) = path[root_prefix.len()..].split('/').next() {
        if first == ".git" || first == ".aeroric" {
            return Err(format!("Cannot modify protected WSL directory: {}", first));
        }
    }
    Ok(())
}

fn run_wsl_output(distribution: &str, wsl_command: String) -> Result<Vec<u8>, String> {
    let mut cmd = crate::wsl::std_wsl_shell_command(distribution, wsl_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

fn resolve_wsl_path_allowed(
    distribution: &str,
    linux_path: &str,
    linux_project_path: Option<&str>,
    allow_project_root: bool,
) -> Result<String, String> {
    ensure_wsl_path_allowed(linux_path, linux_project_path, allow_project_root)?;
    let Some(linux_project_path) = linux_project_path else {
        return Ok(normalize_wsl_path(linux_path));
    };
    let output = run_wsl_output(
        distribution,
        build_wsl_resolve_path_command(linux_path, linux_project_path),
    )?;
    let mut fields = output.split(|byte| *byte == 0);
    let resolved_root = fields
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Failed to resolve WSL project root".to_string())?;
    let resolved_path = fields
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Failed to resolve WSL path".to_string())?;
    ensure_resolved_wsl_path_allowed(resolved_path, resolved_root, allow_project_root)?;
    Ok(resolved_path.to_string())
}

fn parse_wsl_dir_entries(linux_path: &str, raw: &str) -> Vec<WslFsEntry> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?;
            let kind = parts.next()?;
            let modified_at_ms = parts
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .map(|seconds| seconds.saturating_mul(1000));
            let is_dir = kind == "d";
            let extension = if is_dir {
                None
            } else {
                name.rsplit_once('.')
                    .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
                    .map(|(_, ext)| ext.to_string())
            };
            Some(WslFsEntry {
                name: name.to_string(),
                path: format!("{}/{}", linux_path.trim_end_matches('/'), name),
                is_dir,
                extension,
                modified_at_ms,
                is_gitignored: false,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn wsl_read_dir_entries(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<Vec<WslFsEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            true,
        )?;
        let stdout = run_wsl_output(&distribution, build_wsl_read_dir_command(&resolved_path))?;
        let raw = String::from_utf8_lossy(&stdout);
        Ok(parse_wsl_dir_entries(&linux_path, &raw))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_read_file_content(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            true,
        )?;
        let stdout = run_wsl_output(&distribution, build_wsl_read_file_command(&resolved_path))?;
        String::from_utf8(stdout).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_write_file_content(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
    content: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        let mut cmd = crate::wsl::std_wsl_shell_command(
            &distribution,
            build_wsl_write_file_command(&resolved_path),
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
pub async fn wsl_read_image_preview(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<WslImagePreviewData, String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            true,
        )?;
        let mime_type = wsl_image_mime_type(&linux_path)
            .ok_or_else(|| "Unsupported image format".to_string())?;
        let stdout = run_wsl_output(
            &distribution,
            build_wsl_image_preview_command(&resolved_path),
        )?;
        let encoded = String::from_utf8_lossy(&stdout)
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(WslImagePreviewData {
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
pub async fn wsl_create_file(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        run_wsl_output(&distribution, build_wsl_create_file_command(&resolved_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_create_directory(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        run_wsl_output(
            &distribution,
            build_wsl_create_directory_command(&resolved_path),
        )
        .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_delete_path(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        run_wsl_output(&distribution, build_wsl_delete_path_command(&resolved_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_rename_path(
    distribution: String,
    linux_path: String,
    new_name: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        let command = build_wsl_rename_path_command(&resolved_path, new_name.trim())?;
        run_wsl_output(&distribution, command).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_copy_paths_to_directory(
    distribution: String,
    source_paths: Vec<String>,
    target_directory: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_target = resolve_wsl_path_allowed(
            &distribution,
            &target_directory,
            linux_project_path.as_deref(),
            true,
        )?;
        let resolved_sources = source_paths
            .iter()
            .map(|source| {
                resolve_wsl_path_allowed(
                    &distribution,
                    source,
                    linux_project_path.as_deref(),
                    false,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let command = build_wsl_copy_paths_command(&resolved_sources, &resolved_target)?;
        run_wsl_output(&distribution, command).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_file_commands_quote_paths_and_enforce_limits() {
        assert_eq!(
            build_wsl_read_file_command("/home/me/a file.txt"),
            "size=$(wc -c < '/home/me/a file.txt') && [ \"$size\" -le 2097152 ] && cat -- '/home/me/a file.txt'"
        );
        assert_eq!(
            build_wsl_write_file_command("/home/me/a file.txt"),
            "cat > '/home/me/a file.txt'"
        );
    }

    #[test]
    fn wsl_paths_stay_inside_project_root() {
        assert!(ensure_wsl_path_allowed("/home/me/app", Some("/home/me/app"), true).is_ok());
        assert!(
            ensure_wsl_path_allowed("/home/me/app/../secret", Some("/home/me/app"), false).is_err()
        );
        assert!(ensure_wsl_path_allowed("/etc/passwd", Some("/home/me/app"), false).is_err());
        // 项目内的 .git / .aeroric 元数据目录不允许被文件操作触碰。
        assert!(
            ensure_wsl_path_allowed("/home/me/app/.git/config", Some("/home/me/app"), false)
                .is_err()
        );
        assert!(ensure_wsl_path_allowed(
            "/home/me/app/.aeroric/config.toml",
            Some("/home/me/app"),
            false
        )
        .is_err());
        assert!(ensure_wsl_path_allowed("/home/me/app", Some("/home/me/app"), false).is_err());
        assert!(ensure_wsl_path_allowed("home/me/app", Some("/home/me/app"), false).is_err());
    }

    #[test]
    fn wsl_entry_names_reject_traversal_and_separators() {
        assert!(validate_wsl_entry_name("main.rs").is_ok());
        assert!(validate_wsl_entry_name("").is_err());
        assert!(validate_wsl_entry_name("..").is_err());
        assert!(validate_wsl_entry_name("a/b").is_err());
        assert!(validate_wsl_entry_name("a\0b").is_err());
    }

    #[test]
    fn wsl_dir_entries_parse_names_types_and_extensions() {
        let entries = parse_wsl_dir_entries(
            "/home/me/app/",
            "src\td\t1700000000\nmain.rs\tf\t1700000001\n.env\tf\t0\nbroken-line\n",
        );
        assert_eq!(entries.len(), 3);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].path, "/home/me/app/src");
        assert_eq!(entries[0].extension, None);
        assert_eq!(entries[1].extension.as_deref(), Some("rs"));
        assert_eq!(entries[1].modified_at_ms, Some(1_700_000_001_000));
        // 以点开头的隐藏文件不应被当作扩展名。
        assert_eq!(entries[2].extension, None);
    }

    #[test]
    fn wsl_rename_and_copy_commands_quote_and_guard_destinations() {
        assert_eq!(
            build_wsl_rename_path_command("/home/me/app/old name.txt", "new name.txt").unwrap(),
            "[ ! -e '/home/me/app/new name.txt' ] && mv -- '/home/me/app/old name.txt' '/home/me/app/new name.txt'"
        );
        assert!(build_wsl_rename_path_command("/home/me/app/a.txt", "../b.txt").is_err());
        let copy = build_wsl_copy_paths_command(&["/home/me/a b.txt".to_string()], "/home/me/app")
            .unwrap();
        assert!(copy.contains("'/home/me/a b.txt'"));
        assert!(copy.contains("target='/home/me/app'"));
        assert_eq!(
            build_wsl_copy_paths_command(&[], "/home/me/app").unwrap(),
            ":"
        );
    }

    #[test]
    fn wsl_image_preview_limits_size_and_maps_mime_types() {
        assert_eq!(
            build_wsl_image_preview_command("/home/me/a b.png"),
            "size=$(wc -c < '/home/me/a b.png') && [ \"$size\" -le 10485760 ] && base64 < '/home/me/a b.png'"
        );
        assert_eq!(wsl_image_mime_type("/a/b.PNG"), Some("image/png"));
        assert_eq!(wsl_image_mime_type("/a/b.jpeg"), Some("image/jpeg"));
        assert_eq!(wsl_image_mime_type("/a/b.txt"), None);
    }
}
