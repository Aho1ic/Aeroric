//! Legacy directory listing: seven snake_case keys and absolute paths.
//! The caller validates the directory; native read_write::read_dir_entries has
//! a distinct relative/camelCase contract and is deliberately not an alias.

use std::path::Path;
use std::time::UNIX_EPOCH;

/// 目录项。字段名一律下划线,前端 `src/components/file-explorer/types.ts` 的
/// `FsEntry` 逐字段镜像这份形状 —— 结构体上没有 `rename_all`,所以 serde 发出去的
/// 就是这里的名字。改名/加字段必须两边一起改,`fs_entry_wire_field_names_match_the_frontend_type`
/// 会锁住这份键集。
#[derive(serde::Serialize)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub extension: Option<String>,
    pub modified_at_ms: Option<u64>,
    pub is_gitignored: bool,
    /// 条目本身是符号链接。`is_dir` 走的是跟随链接后的类型,所以指向目录的链接
    /// 两个标记同时为真 —— 前端图标按「先看 is_symlink」判。
    pub is_symlink: bool,
}

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".next",
    ".nuxt",
    "dist",
    "build",
    "target",
    "__pycache__",
    ".cache",
    "coverage",
    ".turbo",
    ".expo",
    "out",
    ".output",
    ".venv",
    "venv",
    ".tox",
];

/// Read an already validated directory. Preserve the original project string
/// for best-effort git cwd; this helper does not acquire project/root authority.
pub fn read_dir_entries(directory: &Path, project_path: &str) -> Result<Vec<FsEntry>, String> {
    let entries = std::fs::read_dir(directory).map_err(|e| e.to_string())?;
    let mut result: Vec<FsEntry> = entries
        .flatten()
        .filter(|entry| {
            let p = entry.path();
            if p.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                !IGNORED_DIRS.contains(&name_str.as_ref())
            } else {
                true
            }
        })
        .map(|entry| {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = p.is_dir();
            let extension = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase());
            let modified_at_ms = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64);
            // Keep DirEntry metadata unchanged; is_dir above follows links,
            // while file_type flags the entry itself, including dangling links.
            let is_symlink = entry
                .file_type()
                .map(|file_type| file_type.is_symlink())
                .unwrap_or(false);
            FsEntry {
                name,
                path: p.to_string_lossy().into_owned(),
                is_dir,
                extension,
                modified_at_ms,
                is_gitignored: false,
                is_symlink,
            }
        })
        .collect();
    result.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    // Mark gitignored entries via `git check-ignore --stdin`
    if !result.is_empty() {
        let ignored_set: std::collections::HashSet<String> = {
            use std::io::Write;
            let mut cmd = std::process::Command::new("git");
            crate::runtime::subprocess::configure_background_command(&mut cmd);
            cmd.args(["check-ignore", "--stdin"])
                .current_dir(project_path)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            match cmd.spawn() {
                Ok(mut child) => {
                    if let Some(ref mut stdin) = child.stdin {
                        for entry in &result {
                            let _ = writeln!(stdin, "{}", entry.path);
                        }
                    }
                    match child.wait_with_output() {
                        Ok(output) => String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .filter(|l| !l.is_empty())
                            .map(|l| l.to_string())
                            .collect(),
                        Err(_) => std::collections::HashSet::new(),
                    }
                }
                Err(_) => std::collections::HashSet::new(),
            }
        };
        for entry in &mut result {
            entry.is_gitignored = ignored_set.contains(&entry.path);
        }
    }

    Ok(result)
}
