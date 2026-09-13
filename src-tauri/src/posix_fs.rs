//! WSL 与 SSH 远端共用的 POSIX 文件操作层。
//!
//! `wsl_fs.rs`(经 wsl.exe 进 Linux 侧)与 `remote_fs.rs`(经 ssh)面对的是同一套
//! POSIX shell,过去各自维护一份逐字相同的命令构造与路径校验 —— 其中文件名校验、
//! 项目根围栏和符号解析脚本是注入防御边界,两份拷贝曾经一处修了另一处会漏。
//! 现在命令构造、路径围栏与目录解析收敛到这里;两个调用方只剩各自的执行壳
//! (run_wsl_output / run_ssh_output)、上传通道与 Tauri command。
//!
//! 错误文案按原样保留:WSL 侧是「WSL path …」,远端侧是「Remote path …」,
//! 但「project root / protected directory」两处历史上就是小写 `remote`,
//! 由 [`PathFlavor`] 的两个拼写位分别提供,不在这里做统一改写。

use serde::Serialize;

use crate::path_guard::remote_path_has_relative_components;

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

/// `wsl_read_dir_entries` / `remote_read_dir_entries` 共用的目录项。
/// 两边前端消费的 JSON 形状本就相同(下划线字段名,未做 camelCase 改写)。
#[derive(Serialize)]
pub(crate) struct PosixFsEntry {
    name: String,
    path: String,
    is_dir: bool,
    extension: Option<String>,
    modified_at_ms: Option<u64>,
    is_gitignored: bool,
}

/// `wsl_read_image_preview` / `remote_read_image_preview` 共用的预览负载。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PosixImagePreviewData {
    pub(crate) data_url: String,
    pub(crate) mime_type: String,
    pub(crate) byte_length: u64,
}

/// 错误文案里的路径称谓。`heading` 用于句首(大写),`inline` 用于句中 ——
/// 历史上 WSL 侧两处都用 `WSL`,远端侧句首 `Remote`、句中 `remote`。
#[derive(Clone, Copy)]
pub(crate) enum PathFlavor {
    Wsl,
    Remote,
}

impl PathFlavor {
    fn heading(self) -> &'static str {
        match self {
            PathFlavor::Wsl => "WSL",
            PathFlavor::Remote => "Remote",
        }
    }

    fn inline(self) -> &'static str {
        match self {
            PathFlavor::Wsl => "WSL",
            PathFlavor::Remote => "remote",
        }
    }
}

pub(crate) fn build_read_dir_command(path: &str) -> String {
    // 逐字保持 POSIX 兼容:`-printf` 是 GNU 扩展,`cd --` 在某些 sh 里不合法,
    // glob 必须在 sh 内展开(路径里可能有远端才会展开的通配符)。
    let script = "cd \"$1\" && for p in ./* ./.[!.]* ./..?*; do [ -e \"$p\" ] || continue; name=${p#./}; if [ \"$name\" = \".\" ] || [ \"$name\" = \"..\" ]; then continue; fi; if [ -d \"$p\" ]; then type=d; else type=f; fi; mtime=$(stat -c %Y \"$p\" 2>/dev/null || stat -f %m \"$p\" 2>/dev/null || echo 0); printf '%s\\t%s\\t%s\\n' \"$name\" \"$type\" \"$mtime\"; done";
    format!(
        "sh -c {} sh {}",
        crate::ssh::shell_quote_posix(script),
        crate::ssh::shell_quote_posix(path)
    )
}

pub(crate) fn build_read_file_command(path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(path);
    format!("size=$(wc -c < {path}) && [ \"$size\" -le {MAX_FILE_BYTES} ] && cat -- {path}")
}

pub(crate) fn build_write_file_command(path: &str) -> String {
    format!("cat > {}", crate::ssh::shell_quote_posix(path))
}

pub(crate) fn build_create_file_command(path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(path);
    format!("test ! -e {path} && : > {path}")
}

pub(crate) fn build_create_directory_command(path: &str) -> String {
    format!("mkdir -- {}", crate::ssh::shell_quote_posix(path))
}

pub(crate) fn build_delete_path_command(path: &str) -> String {
    format!("rm -rf -- {}", crate::ssh::shell_quote_posix(path))
}

/// 入口名防御:空名、超长(>255 字节)、`.`/`..`、路径分隔符与 NUL 一律拒绝。
pub(crate) fn validate_entry_name(name: &str) -> Result<(), String> {
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

pub(crate) fn parent_path(path: &str) -> Result<&str, String> {
    let trimmed = path.trim_end_matches('/');
    let Some((parent, _)) = trimmed.rsplit_once('/') else {
        return Err("Cannot resolve parent directory".to_string());
    };
    if parent.is_empty() {
        Ok("/")
    } else {
        Ok(parent)
    }
}

pub(crate) fn basename(path: &str) -> Result<&str, String> {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit_once('/')
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Invalid file name".to_string())
}

pub(crate) fn build_rename_path_command(path: &str, new_name: &str) -> Result<String, String> {
    validate_entry_name(new_name)?;
    let parent = parent_path(path)?;
    let destination = if parent == "/" {
        format!("/{}", new_name)
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), new_name)
    };
    let source = crate::ssh::shell_quote_posix(path);
    let dest = crate::ssh::shell_quote_posix(&destination);
    Ok(format!(
        "[ ! -e {dest} ] && mv -- {source} {dest}",
        source = source,
        dest = dest
    ))
}

pub(crate) fn build_copy_paths_command(
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
            validate_entry_name(basename(source)?)?;
            Ok(crate::ssh::shell_quote_posix(source))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && for src in {sources}; do name=${{src##*/}}; [ ! -e \"$target/$name\" ] || {{ echo \"A file or folder with that name already exists\" >&2; exit 1; }}; done && cp -R -- {sources} \"$target/\"",
        target = target,
        sources = sources.join(" ")
    ))
}

pub(crate) fn build_image_preview_command(path: &str) -> String {
    let path = crate::ssh::shell_quote_posix(path);
    format!(
        "size=$(wc -c < {path}) && [ \"$size\" -le {MAX_IMAGE_PREVIEW_BYTES} ] && base64 < {path}"
    )
}

pub(crate) fn image_mime_type(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
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

pub(crate) fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    }
}

/// 符号解析脚本:逐级 readlink(至多 40 跳),最后取物理父目录拼出真实路径,
/// 以 NUL 分隔输出 `root\0target\0`。`exit 72` 表示解析失败,由调用方映射成错误文案。
pub(crate) fn build_resolve_path_command(path: &str, project_path: &str) -> String {
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
        crate::ssh::shell_quote_posix(project_path),
        crate::ssh::shell_quote_posix(path)
    )
}

/// 静态围栏:绝对路径、无 `.`/`..` 相对段、落在项目根内、且不触 `.git`/`.aeroric`。
pub(crate) fn ensure_path_allowed(
    flavor: PathFlavor,
    path: &str,
    project_path: Option<&str>,
    allow_project_root: bool,
) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err(format!("{} path must be absolute", flavor.heading()));
    }
    if remote_path_has_relative_components(path) {
        return Err(format!(
            "{} path cannot contain . or .. components",
            flavor.heading()
        ));
    }
    let Some(project_path) = project_path else {
        return Ok(());
    };
    if !project_path.starts_with('/') {
        return Err(format!(
            "{} project path must be absolute",
            flavor.heading()
        ));
    }
    if remote_path_has_relative_components(project_path) {
        return Err(format!(
            "{} project path cannot contain . or .. components",
            flavor.heading()
        ));
    }
    let path = normalize_path(path);
    let root = normalize_path(project_path);
    if path == root {
        if allow_project_root {
            return Ok(());
        }
        return Err(format!(
            "Cannot modify the {} project root",
            flavor.inline()
        ));
    }
    check_inside_project_root(flavor, &path, &root, "is outside")
}

/// 符号解析后的二次围栏:同 [`ensure_path_allowed`],但作用于解析出的真实路径。
pub(crate) fn ensure_resolved_path_allowed(
    flavor: PathFlavor,
    resolved_path: &str,
    resolved_root: &str,
    allow_project_root: bool,
) -> Result<(), String> {
    let path = normalize_path(resolved_path);
    let root = normalize_path(resolved_root);
    if path == root {
        return if allow_project_root {
            Ok(())
        } else {
            Err(format!(
                "Cannot modify the {} project root",
                flavor.inline()
            ))
        };
    }
    check_inside_project_root(flavor, &path, &root, "resolves outside")
}

fn check_inside_project_root(
    flavor: PathFlavor,
    path: &str,
    root: &str,
    verb: &str,
) -> Result<(), String> {
    let root_prefix = if root == "/" {
        "/".to_string()
    } else {
        format!("{root}/")
    };
    if !path.starts_with(&root_prefix) {
        return Err(format!(
            "{} path {} the project root",
            flavor.heading(),
            verb
        ));
    }
    if let Some(first) = path[root_prefix.len()..].split('/').next() {
        if first == ".git" || first == ".aeroric" {
            return Err(format!(
                "Cannot modify protected {} directory: {}",
                flavor.inline(),
                first
            ));
        }
    }
    Ok(())
}

/// 解析命令输出的公共尾部:拆 NUL 字段并做解析后围栏,返回解析后的目标路径。
pub(crate) fn parse_resolved_output(
    flavor: PathFlavor,
    output: &[u8],
    allow_project_root: bool,
) -> Result<String, String> {
    let mut fields = output.split(|byte| *byte == 0);
    let resolved_root = fields
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Failed to resolve {} project root", flavor.inline()))?;
    let resolved_path = fields
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Failed to resolve {} path", flavor.inline()))?;
    ensure_resolved_path_allowed(flavor, resolved_path, resolved_root, allow_project_root)?;
    Ok(resolved_path.to_string())
}

/// 解析 `build_read_dir_command` 的输出为目录项列表。
pub(crate) fn parse_dir_entries(root: &str, raw: &str) -> Vec<PosixFsEntry> {
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
            Some(PosixFsEntry {
                name: name.to_string(),
                path: format!("{}/{}", root.trim_end_matches('/'), name),
                is_dir,
                extension,
                modified_at_ms,
                is_gitignored: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_commands_quote_paths_and_enforce_limits() {
        assert_eq!(
            build_read_file_command("/srv/app's repo/README.md"),
            "size=$(wc -c < '/srv/app'\\''s repo/README.md') && [ \"$size\" -le 2097152 ] && cat -- '/srv/app'\\''s repo/README.md'"
        );
        assert_eq!(
            build_write_file_command("/srv/app/config value.txt"),
            "cat > '/srv/app/config value.txt'"
        );
        assert_eq!(
            build_create_file_command("/srv/app/new file.txt"),
            "test ! -e '/srv/app/new file.txt' && : > '/srv/app/new file.txt'"
        );
        assert_eq!(
            build_create_directory_command("/srv/app/new folder"),
            "mkdir -- '/srv/app/new folder'"
        );
        assert_eq!(
            build_delete_path_command("/srv/app/old file.txt"),
            "rm -rf -- '/srv/app/old file.txt'"
        );
    }

    #[test]
    fn read_dir_command_is_posix_compatible_without_gnu_printf() {
        let command = build_read_dir_command("/srv/app");
        assert!(!command.contains("-printf"));
        assert!(!command.contains("cd --"));
        assert!(command.contains("printf"));
        assert!(command.contains("%s\\t%s\\t%s\\n"));
    }

    #[test]
    fn read_dir_command_runs_globs_inside_posix_sh() {
        let command = build_read_dir_command("/Users/lyx/Documents");

        assert!(command.starts_with("sh -c "));
        assert!(command.contains("'cd \"$1\" && for p in ./* ./.[!.]* ./..?*;"));
        assert!(command.ends_with(" sh '/Users/lyx/Documents'"));
    }

    #[test]
    fn entry_names_reject_traversal_and_separators() {
        assert!(validate_entry_name("main.rs").is_ok());
        assert!(validate_entry_name("").is_err());
        assert!(validate_entry_name("..").is_err());
        assert!(validate_entry_name("a/b").is_err());
        assert!(validate_entry_name("a\0b").is_err());
    }

    #[test]
    fn rename_command_validates_basename_and_quotes_destination() {
        assert_eq!(
            build_rename_path_command("/srv/app/old file.txt", "new file.txt").unwrap(),
            "[ ! -e '/srv/app/new file.txt' ] && mv -- '/srv/app/old file.txt' '/srv/app/new file.txt'"
        );
        assert!(build_rename_path_command("/srv/app/old", "../new").is_err());
    }

    #[test]
    fn copy_paths_command_quotes_sources_and_target() {
        assert_eq!(
            build_copy_paths_command(
                &[
                    "/srv/app/a file.txt".to_string(),
                    "/srv/app/folder".to_string(),
                ],
                "/srv/app/target dir",
            )
            .unwrap(),
            "target='/srv/app/target dir'; [ -d \"$target\" ] && for src in '/srv/app/a file.txt' '/srv/app/folder'; do name=${src##*/}; [ ! -e \"$target/$name\" ] || { echo \"A file or folder with that name already exists\" >&2; exit 1; }; done && cp -R -- '/srv/app/a file.txt' '/srv/app/folder' \"$target/\""
        );
        assert_eq!(build_copy_paths_command(&[], "/srv/app").unwrap(), ":");
    }

    #[test]
    fn image_preview_command_encodes_with_size_limit() {
        assert_eq!(
            build_image_preview_command("/srv/app/logo.png"),
            "size=$(wc -c < '/srv/app/logo.png') && [ \"$size\" -le 10485760 ] && base64 < '/srv/app/logo.png'"
        );
        assert_eq!(image_mime_type("/a/b.PNG"), Some("image/png"));
        assert_eq!(image_mime_type("/a/b.jpeg"), Some("image/jpeg"));
        assert_eq!(image_mime_type("/a/b.txt"), None);
    }

    #[test]
    fn static_fence_rejects_escaping_paths() {
        let flavor = PathFlavor::Remote;
        assert!(ensure_path_allowed(flavor, "/srv/app", Some("/srv/app"), true).is_ok());
        assert!(ensure_path_allowed(flavor, "/srv/app", Some("/srv/app"), false).is_err());
        assert!(
            ensure_path_allowed(flavor, "/srv/app/../outside.txt", Some("/srv/app"), false)
                .is_err()
        );
        assert!(
            ensure_path_allowed(flavor, "/srv/app/./file.txt", Some("/srv/app"), false).is_err()
        );
        assert!(ensure_path_allowed(flavor, "srv/app", Some("/srv/app"), false).is_err());
        // 项目内的 .git / .aeroric 元数据目录不允许被文件操作触碰。
        assert!(
            ensure_path_allowed(flavor, "/srv/app/.git/config", Some("/srv/app"), false).is_err()
        );
        assert!(ensure_path_allowed(
            flavor,
            "/srv/app/.aeroric/config.toml",
            Some("/srv/app"),
            false
        )
        .is_err());
        assert!(ensure_path_allowed(flavor, "/etc/passwd", Some("/srv/app"), false).is_err());
    }

    #[test]
    fn fence_error_wording_matches_each_flavor_verbatim() {
        // 两族的既有文案大小写不同(WSL 全大写 / remote 句中小写),逐字保留。
        assert_eq!(
            ensure_path_allowed(PathFlavor::Wsl, "home/me/app", Some("/home/me/app"), false)
                .unwrap_err(),
            "WSL path must be absolute"
        );
        assert_eq!(
            ensure_path_allowed(PathFlavor::Remote, "srv/app", Some("/srv/app"), false)
                .unwrap_err(),
            "Remote path must be absolute"
        );
        assert_eq!(
            ensure_path_allowed(PathFlavor::Wsl, "/home/me/app", Some("/home/me/app"), false)
                .unwrap_err(),
            "Cannot modify the WSL project root"
        );
        assert_eq!(
            ensure_path_allowed(PathFlavor::Remote, "/srv/app", Some("/srv/app"), false)
                .unwrap_err(),
            "Cannot modify the remote project root"
        );
        assert_eq!(
            ensure_path_allowed(
                PathFlavor::Wsl,
                "/home/me/app/.git/config",
                Some("/home/me/app"),
                false
            )
            .unwrap_err(),
            "Cannot modify protected WSL directory: .git"
        );
        assert_eq!(
            ensure_path_allowed(
                PathFlavor::Remote,
                "/srv/app/.aeroric/config.toml",
                Some("/srv/app"),
                false
            )
            .unwrap_err(),
            "Cannot modify protected remote directory: .aeroric"
        );
        assert_eq!(
            ensure_resolved_path_allowed(
                PathFlavor::Remote,
                "/etc/aeroric.conf",
                "/srv/app",
                false
            )
            .unwrap_err(),
            "Remote path resolves outside the project root"
        );
        assert!(ensure_resolved_path_allowed(
            PathFlavor::Remote,
            "/srv/app/src/main.rs",
            "/srv/app",
            false
        )
        .is_ok());
    }

    #[test]
    fn dir_entries_parse_names_types_and_extensions() {
        let entries = parse_dir_entries(
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

    #[cfg(unix)]
    #[test]
    fn path_resolver_exposes_symlink_escape_for_boundary_check() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("aeroric-posix-path-{}", uuid::Uuid::new_v4()));
        let project = root.join("project");
        let outside = root.join("outside");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, project.join("link")).unwrap();

        let command = build_resolve_path_command(
            &project.join("link/file.txt").to_string_lossy(),
            &project.to_string_lossy(),
        );
        assert!(!command.contains("readlink --"));
        assert!(!command.contains("cd -P --"));
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .unwrap();
        assert!(output.status.success());
        let mut fields = output.stdout.split(|byte| *byte == 0);
        let resolved_root = std::str::from_utf8(fields.next().unwrap()).unwrap();
        let resolved_path = std::str::from_utf8(fields.next().unwrap()).unwrap();
        assert_eq!(
            resolved_root,
            project.canonicalize().unwrap().to_string_lossy()
        );
        assert_eq!(
            resolved_path,
            outside
                .canonicalize()
                .unwrap()
                .join("file.txt")
                .to_string_lossy()
        );
        assert!(ensure_resolved_path_allowed(
            PathFlavor::Remote,
            resolved_path,
            resolved_root,
            false
        )
        .is_err());

        let _ = std::fs::remove_dir_all(root);
    }
}
