//! P6-01 Action 2/3：文件路径策略（core 独占的 server 端校验——不依赖客户端
//! 过滤 `..`）。native root策略：绝对＋存在＋是目录＋canonicalize；legacy
//! root入口保持旧Tauri错误顺序。relative path 拒 `.`/`..` 组件；symlink 落根内判定。
//!
//! 边界说明：Aeroric 数据目录的项目记录（`AppPaths::project_dir`）不是用户
//! 仓库根；用户仓库根在 project record 的 `path` 字段，由调用方（宿主服务）
//! 读出后传入本模块。本模块不读 storage；root/resolve校验会访问给定文件系统。

/// 相对路径组件校验：拒 `.`/`..`、空段、分隔符、NUL（server 端权威，不
/// 依赖客户端过滤）。
pub fn validate_relative_path(relative: &str) -> Result<(), String> {
    if relative.is_empty() {
        return Err("relative path is empty".to_string());
    }
    if relative.contains('\0') {
        return Err("relative path contains NUL".to_string());
    }
    let normalized = relative.replace('\\', "/");
    for component in normalized.split('/') {
        match component {
            "" => return Err("relative path contains an empty component".to_string()),
            "." => return Err("relative path contains '.'".to_string()),
            ".." => return Err("relative path contains '..'".to_string()),
            _ => {}
        }
    }
    Ok(())
}

/// 项目根校验：绝对＋存在＋是目录＋canonicalize（`..` 在返回值中解掉）。
/// Native policy; legacy callers use `validate_legacy_project_root` for the
/// original canonicalization-first error order and diagnostics.
pub fn validate_project_root(project_path: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    if !path.exists() {
        return Err("Project path does not exist".to_string());
    }
    if !path.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    path.canonicalize().map_err(|error| error.to_string())
}

/// Exact legacy root guard shared with the retained desktop adapters. Use the
/// returned canonical root for subsequent joins. This is not a project ACL.
pub fn validate_legacy_project_root(project_path: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    if !canonical.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    Ok(canonical)
}

/// 拼接 relative path 到已 canonicalize 的根下，并确认结果仍落根内
/// （symlink 落根内判定：canonicalize 后前缀比对）。TOCTOU 界面：调用方在
/// open 之前调用，且 open 用返回值（不重拼字符串）。
pub fn resolve_within(
    root: &std::path::Path,
    relative: &str,
) -> Result<std::path::PathBuf, String> {
    validate_relative_path(relative)?;
    let target = root.join(relative);
    let canonical = target
        .canonicalize()
        .map_err(|error| format!("cannot resolve path: {error}"))?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve project root: {error}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("path escapes the project root".to_string());
    }
    Ok(canonical)
}

/// 写入/重命名/删除前的父目录判定（TOCTOU 界面：父 canonicalize 落根内，
/// 末段允许不存在——新建文件场景）。返回要打开的父目录（调用方 open 父
/// 目录句柄后再操作，避免 canonicalize 与 open 之间被替换）。
pub fn resolve_parent_within(
    root: &std::path::Path,
    relative: &str,
) -> Result<(std::path::PathBuf, String), String> {
    validate_relative_path(relative)?;
    let normalized = relative.replace('\\', "/");
    let (parent, name) = normalized
        .rsplit_once('/')
        .map(|(parent, name)| (parent.to_string(), name.to_string()))
        .unwrap_or_else(|| (String::new(), normalized.clone()));
    if name.is_empty() {
        return Err("relative path ends with a separator".to_string());
    }
    let parent_path = if parent.is_empty() {
        root.to_path_buf()
    } else {
        let joined = root.join(&parent);
        let canonical = joined
            .canonicalize()
            .map_err(|error| format!("cannot resolve parent: {error}"))?;
        let canonical_root = root
            .canonicalize()
            .map_err(|error| format!("cannot resolve project root: {error}"))?;
        if !canonical.starts_with(&canonical_root) {
            return Err("path escapes the project root".to_string());
        }
        canonical
    };
    Ok((parent_path, name))
}

/// Parent containment does not authorize a leaf symlink. Existing leaves must
/// resolve inside the project; unresolved links must not create their referent.
/// An ordinary absent leaf is allowed under the already-checked parent.
/// Like resolve_within, this is point-in-time, not descriptor-relative IO.
pub fn resolve_file_write_destination(
    root: &std::path::Path,
    relative: &str,
) -> Result<std::path::PathBuf, String> {
    let (parent, name) = resolve_parent_within(root, relative)?;
    let target = parent.join(name);
    match std::fs::symlink_metadata(&target) {
        Ok(_) => {
            let canonical = target
                .canonicalize()
                .map_err(|error| format!("cannot resolve write path: {error}"))?;
            let canonical_root = root
                .canonicalize()
                .map_err(|error| format!("cannot resolve project root: {error}"))?;
            if !canonical.starts_with(&canonical_root) {
                return Err("path escapes the project root".to_string());
            }
            Ok(canonical)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(target),
        Err(error) => Err(format!("cannot inspect write path: {error}")),
    }
}

/// Validate a possibly missing destination directory without creating it.
/// A dangling symlink is an existing entry, not a missing ancestor to skip.
/// Keep the caller's spelling: an in-root directory alias remains valid.
/// This is a point-in-time check, not descriptor-relative protection from races.
pub fn validate_directory_destination(
    root: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("Cannot resolve project root: {error}"))?;
    let mut ancestor = target;
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let resolved = ancestor
                    .canonicalize()
                    .map_err(|error| format!("Cannot resolve copy destination: {error}"))?;
                if !resolved.starts_with(&canonical_root) {
                    return Err("copy destination escapes the project root".to_string());
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| "Cannot resolve copy destination ancestor".to_string())?;
            }
            Err(error) => return Err(format!("Cannot inspect copy destination: {error}")),
        }
    }
}

/// 文件大小上限（与 `posix_fs.rs::MAX_FILE_BYTES` 同值）。
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// 内容大小守卫（写入/读回共用；超限拒绝，不静默截断）。
pub fn validate_content_size(content: &str) -> Result<(), String> {
    let bytes = content.len() as u64;
    if bytes > MAX_FILE_BYTES {
        return Err(format!(
            "file content exceeds the {MAX_FILE_BYTES} byte limit"
        ));
    }
    Ok(())
}

/// 删除策略：进废纸篓（trash）/既有策略——**不默认永久删除**（Action 3）。
pub const DELETE_USES_TRASH: bool = true;
