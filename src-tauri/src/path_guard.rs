//! 项目根目录校验的**唯一**实现。
//!
//! 这段逻辑曾经在 9 个模块里各存一份逐字节相同的拷贝
//! (`diagnostics` / `ports` / `dap` / `formatter` / `run_config` /
//! `local_history` / `tests` / `fs` / `search`)。它是路径穿越的第一道闸门:
//! 前端传来的 `project_path` 会被当成后续所有文件操作的根,不校验就等于把
//! `../../../etc` 当项目目录用。
//!
//! 复制 9 份的真实代价是:将来要加强校验(比如拒绝 symlink 指向仓库外、
//! 或要求路径在已注册的项目列表里)就得同时改 9 处,漏一处就是一个活着的漏洞。
//! 所以这里收敛成一处,新增校验只改这个文件。

use std::path::{Path, PathBuf};

/// 校验并规范化项目根目录。
///
/// 通过条件:
/// 1. 必须是绝对路径 —— 相对路径的含义取决于进程 cwd,而 cwd 在 Tauri 里不可靠。
/// 2. 必须能 `canonicalize` —— 顺带解掉 `..` 与 symlink,得到真实位置。
/// 3. 必须是目录。
///
/// 返回规范化后的绝对路径,调用方应当用**返回值**做后续拼接,而不是原始入参 ——
/// 用原始入参就绕掉了 `..` 的解析。
pub(crate) fn validate_project_root(project_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(project_path);
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

/// 远端(SSH)路径里是否含 `.` 或 `..` 段。
///
/// 原先在 `ports` / `lsp` / `remote_fs` / `database::legacy_sqlite` 各存一份。
/// 远端路径不能走 `canonicalize`(那会解析**本机**文件系统),所以只能靠拒绝
/// 相对段来防穿越 —— 这是远端侧唯一的闸门,更不该有 4 份拷贝。
pub(crate) fn remote_path_has_relative_components(path: &str) -> bool {
    path.split('/')
        .any(|component| component == "." || component == "..")
}

/// 校验并规范化远端项目根路径:必须是绝对路径、不含相对段,尾部 `/` 去掉。
///
/// 原先在 `diagnostics` / `remote_git` / `tests` 各存一份逐字节相同的拷贝。
/// 注意错误文案会原样回到前端,改动前先确认没有测试或 UI 依赖具体措辞。
pub(crate) fn normalize_remote_project_path(remote_project_path: &str) -> Result<String, String> {
    let trimmed = remote_project_path.trim();
    if !trimmed.starts_with('/') {
        return Err("Remote project path must be absolute".to_string());
    }
    if remote_path_has_relative_components(trimmed) {
        return Err("Remote project path cannot contain . or .. components".to_string());
    }
    if trimmed == "/" {
        Ok("/".to_string())
    } else {
        Ok(trimmed.trim_end_matches('/').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_remote_project_path, remote_path_has_relative_components, validate_project_root,
    };
    use std::path::PathBuf;

    /// 项目里没有 `tempfile` dev-dependency,现有测试(`ssh_hostkey` / `session`)
    /// 一律用 `std::env::temp_dir()` + 进程 id 造唯一目录。这里沿用同一套。
    struct ScopedDir(PathBuf);

    impl ScopedDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "aeroric-path-guard-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create scoped dir");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }

        fn canonical(&self) -> PathBuf {
            self.0.canonicalize().expect("canonicalize scoped dir")
        }
    }

    impl Drop for ScopedDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rejects_relative_paths() {
        let error = validate_project_root("relative/dir").unwrap_err();
        assert_eq!(error, "Project path must be absolute");
    }

    #[test]
    fn rejects_empty_path() {
        let error = validate_project_root("").unwrap_err();
        assert_eq!(error, "Project path must be absolute");
    }

    #[test]
    fn rejects_missing_path() {
        let dir = ScopedDir::new("missing");
        let missing = dir.path().join("does-not-exist");
        let error = validate_project_root(missing.to_str().expect("utf8")).unwrap_err();
        assert!(
            error.starts_with("Cannot resolve project path:"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_file_that_is_not_a_directory() {
        let dir = ScopedDir::new("not-a-dir");
        let file = dir.path().join("a-file.txt");
        std::fs::write(&file, b"x").expect("write");
        let error = validate_project_root(file.to_str().expect("utf8")).unwrap_err();
        assert_eq!(error, "Project path is not a directory");
    }

    #[test]
    fn accepts_directory_and_returns_canonical_path() {
        let dir = ScopedDir::new("ok");
        let resolved = validate_project_root(dir.path().to_str().expect("utf8")).expect("ok");
        assert!(resolved.is_absolute());
        assert!(resolved.is_dir());
        assert_eq!(resolved, dir.canonical());
    }

    /// `..` 必须在返回值里被解掉 —— 这是调用方能安全拼接的前提。
    #[test]
    fn resolves_dot_dot_segments() {
        let dir = ScopedDir::new("dotdot");
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).expect("create nested");
        let sneaky = nested.join("..");
        let resolved = validate_project_root(sneaky.to_str().expect("utf8")).expect("ok");
        assert_eq!(resolved, dir.canonical());
        assert!(!resolved.to_string_lossy().contains(".."));
    }

    #[test]
    fn flags_dot_and_dot_dot_components() {
        assert!(remote_path_has_relative_components("/srv/../etc"));
        assert!(remote_path_has_relative_components("/srv/./app"));
        assert!(remote_path_has_relative_components(".."));
        assert!(remote_path_has_relative_components("a/../b"));
    }

    #[test]
    fn allows_paths_without_relative_components() {
        assert!(!remote_path_has_relative_components("/srv/app"));
        assert!(!remote_path_has_relative_components("/"));
        assert!(!remote_path_has_relative_components(""));
        // `..` 只在**整段**相等时算相对段。`..foo` 是合法文件名。
        assert!(!remote_path_has_relative_components("/srv/..foo"));
        assert!(!remote_path_has_relative_components("/srv/foo..bar"));
    }

    #[test]
    fn normalizes_remote_root_and_strips_trailing_slashes() {
        assert_eq!(normalize_remote_project_path("/").expect("ok"), "/");
        assert_eq!(
            normalize_remote_project_path("/srv/app/").expect("ok"),
            "/srv/app"
        );
        assert_eq!(
            normalize_remote_project_path("  /srv/app///  ").expect("ok"),
            "/srv/app"
        );
    }

    #[test]
    fn rejects_relative_and_traversing_remote_paths() {
        assert_eq!(
            normalize_remote_project_path("srv/app").unwrap_err(),
            "Remote project path must be absolute"
        );
        assert_eq!(
            normalize_remote_project_path("/srv/../etc").unwrap_err(),
            "Remote project path cannot contain . or .. components"
        );
    }
}
