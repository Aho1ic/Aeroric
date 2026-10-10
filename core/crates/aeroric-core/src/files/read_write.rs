//! P6-01 Action 1：文件读取/列目录的业务主体（core 独占；宿主 command 只做
//! adapter）。操作上限与 symlink 判定在 path_policy。
pub use super::path_policy::{validate_content_size, MAX_FILE_BYTES};

/// Native directory entry: six camelCase keys and a project-relative path.
/// Legacy `legacy_directory::FsEntry` has seven snake_case keys and absolute
/// paths; the two established wire contracts must not be aliased.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_gitignored: bool,
    pub is_symlink: bool,
    pub modified_at_ms: Option<u64>,
}

/// 列目录（业务主体）：symlink 落根内的子目录可列；拒绝列根外。
pub fn read_dir_entries(root: &std::path::Path, relative: &str) -> Result<Vec<FileEntry>, String> {
    let canonical = if relative.is_empty() {
        root.to_path_buf()
    } else {
        super::path_policy::resolve_within(root, relative)?
    };
    let mut entries: Vec<FileEntry> = Vec::new();
    let read =
        std::fs::read_dir(&canonical).map_err(|error| format!("cannot read directory: {error}"))?;
    for item in read {
        let item = item.map_err(|error| error.to_string())?;
        let name = item.file_name().to_string_lossy().to_string();
        let metadata = item.metadata().ok();
        let file_type = item.file_type().ok();
        entries.push(FileEntry {
            path: format!("{}/{}", relative.trim_end_matches('/'), name)
                .trim_matches('/')
                .to_string(),
            is_dir: metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
            is_gitignored: false,
            is_symlink: file_type.as_ref().map(|t| t.is_symlink()).unwrap_or(false),
            modified_at_ms: metadata
                .and_then(|m| {
                    m.modified()
                        .ok()?
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                })
                .map(|d| d.as_millis() as u64),
            name,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    Ok(entries)
}

/// 读文件内容（业务主体）：resolve_within（symlink 落根内）＋大小上限；
/// 超限拒绝不静默截断。
pub fn read_file_content(root: &std::path::Path, relative: &str) -> Result<String, String> {
    let canonical = super::path_policy::resolve_within(root, relative)?;
    let metadata =
        std::fs::metadata(&canonical).map_err(|error| format!("cannot stat file: {error}"))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!("file exceeds the {MAX_FILE_BYTES} byte limit"));
    }
    std::fs::read_to_string(&canonical).map_err(|error| format!("cannot read file: {error}"))
}

/// 写文件内容（业务主体）：内容大小守卫先行，父目录与既存叶子均须落根内；
/// 使用解析出的目标写入，保留有效根内链接。路径检查仍有 TOCTOU 窗口。
pub fn write_file_content(
    root: &std::path::Path,
    relative: &str,
    content: &str,
) -> Result<(), String> {
    super::path_policy::validate_content_size(content)?;
    let target = super::path_policy::resolve_file_write_destination(root, relative)?;
    std::fs::write(&target, content).map_err(|error| format!("cannot write file: {error}"))
}

/// 新建文件（业务主体）：父目录判定后独占创建；任意已有叶子（含悬空链接）拒绝。
pub fn create_file(root: &std::path::Path, relative: &str) -> Result<(), String> {
    let (parent, name) = super::path_policy::resolve_parent_within(root, relative)?;
    let target = parent.join(&name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map(|_| ())
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "file already exists".to_string()
            } else {
                format!("cannot create file: {error}")
            }
        })
}

/// 新建目录（业务主体）：已存在拒绝。
pub fn create_directory(root: &std::path::Path, relative: &str) -> Result<(), String> {
    let (parent, name) = super::path_policy::resolve_parent_within(root, relative)?;
    let target = parent.join(&name);
    if target.exists() {
        return Err("directory already exists".to_string());
    }
    std::fs::create_dir(&target).map_err(|error| format!("cannot create directory: {error}"))
}

/// 重命名（业务主体）：目标名经 validate_relative_path 组件校验；同目录内。
pub fn rename_path(root: &std::path::Path, relative: &str, new_name: &str) -> Result<(), String> {
    super::path_policy::validate_relative_path(new_name)?;
    let canonical = super::path_policy::resolve_within(root, relative)?;
    let parent = canonical
        .parent()
        .ok_or_else(|| "cannot determine parent".to_string())?;
    let target = parent.join(
        std::path::Path::new(new_name)
            .file_name()
            .ok_or_else(|| "invalid new name".to_string())?,
    );
    let canonical_root = super::path_policy::validate_project_root(
        root.to_str()
            .ok_or_else(|| "root is not utf-8".to_string())?,
    )?;
    if !target.starts_with(&canonical_root) {
        return Err("path escapes the project root".to_string());
    }
    std::fs::rename(&canonical, &target).map_err(|error| format!("cannot rename: {error}"))
}

/// 删除（业务主体）：进废纸篓（trash crate；DELETE_USES_TRASH）——**不默认
/// 永久删除**；trash 不可用时拒绝（不回退 rm）。
pub fn delete_path(root: &std::path::Path, relative: &str) -> Result<(), String> {
    delete_path_with_trash(root, relative, |path| {
        trash::delete(path).map_err(|error| format!("cannot trash path: {error}"))
    })
}

/// Keep OS trash access at the boundary. Tests inject a scratch trash directory
/// rather than moving fixture files into the real user's ~/.Trash.
pub fn delete_path_with_trash(
    root: &std::path::Path,
    relative: &str,
    trash: impl FnOnce(&std::path::Path) -> Result<(), String>,
) -> Result<(), String> {
    let canonical = super::path_policy::resolve_within(root, relative)?;
    trash(&canonical)
}

/// Existing hosted project-scoped copy, not legacy absolute/external-source
/// clipboard copy. The host checks its current writer before calling this
/// path-only helper. A later bad source preserves earlier copies (no transaction).
pub fn copy_project_paths_to_directory(
    root: &std::path::Path,
    target_dir: &str,
    sources: &[String],
) -> Result<Vec<String>, String> {
    super::path_policy::validate_relative_path(target_dir)?;
    let target = root.join(target_dir);
    super::path_policy::validate_directory_destination(root, &target)?;
    std::fs::create_dir_all(&target).map_err(|error| format!("Create target failed: {error}"))?;
    let mut copied = Vec::new();
    for source in sources {
        let from =
            super::path_policy::resolve_within(root, source).map_err(|error| error.to_string())?;
        let name = from
            .file_name()
            .ok_or_else(|| format!("Source has no file name: {source}"))?;
        let to = target.join(name);
        // exists() follows symlinks and mistakes a dangling leaf for a free name.
        match std::fs::symlink_metadata(&to) {
            Ok(_) => {
                return Err(format!(
                    "Refusing to overwrite {}: the target already exists",
                    to.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Cannot inspect copy target: {error}")),
        }
        std::fs::copy(&from, &to).map_err(|error| format!("Copy failed: {error}"))?;
        copied.push(to.to_string_lossy().into_owned());
    }
    Ok(copied)
}
