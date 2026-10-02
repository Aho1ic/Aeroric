//! P6-01 Action 2/3：文件路径策略（core 独占的 server 端校验——不依赖客户端
//! 过滤 `..`）。校验面与 `src-tauri/src/path_guard.rs` 同源语义：绝对＋存在＋
//! 是目录＋canonicalize；relative path 拒 `.`/`..` 组件；symlink 落根内判定。
//!
//! 边界说明：Aeroric 数据目录的项目记录（`AppPaths::project_dir`）不是用户
//! 仓库根；用户仓库根在 project record 的 `path` 字段，由调用方（宿主服务）
//! 读出后传入本模块。本模块不读 storage——路径策略是纯函数。

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
/// 与 `path_guard.rs::validate_project_root` 同语义。
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
