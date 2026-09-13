//! 三套 git 面(本地 worktree / WSL / SSH 远端)共用的路径与 revision 防御。
//!
//! `git.rs`、`wsl_git.rs`、`remote_git.rs` 过去各存一份同形校验,实现已经漂移:
//! 本地版用 `Path::components()` 判 `..` 且不拦 NUL,WSL/远端版用 `split('/')` 且拦
//! NUL —— 同一威胁模型三套判定,一处修了另外两处会漏。这些校验是真实的攻击面
//! (见 `git.rs` 里 `validate_git_url` 的 RCE 实测注释),所以收敛为这一份,
//! 各调用方以 [`GitFlavor`] 区分文案。
//!
//! `wsl_git.rs` / `remote_git.rs` / `git.rs` 保留同名的一行委托别名,调用点与
//! 测试不动;真正的判定只在这里。

use std::collections::HashSet;
use std::path::Path;
use std::process::Output;

/// 错误文案里的 git 面称谓:"git" / "WSL git" / "remote git"。
#[derive(Clone, Copy)]
pub(crate) enum GitFlavor {
    Local,
    Wsl,
    Remote,
}

impl GitFlavor {
    fn label(self) -> &'static str {
        match self {
            GitFlavor::Local => "git",
            GitFlavor::Wsl => "WSL git",
            GitFlavor::Remote => "remote git",
        }
    }
}

/// git 相对路径防御:非空、非绝对(含 Windows 盘符,本地面在 Windows 上跑)、
/// 无 `.`/`..` 相对段、无 NUL。三种实现过去最漂移的一处,取各家的并集:
/// WSL/远端的 split 判定 + 本地对绝对路径的 `Path` 判定 + NUL 检查。
pub(crate) fn validate_git_relative_path(flavor: GitFlavor, file_path: &str) -> Result<(), String> {
    if file_path.is_empty() {
        return Err("File path must not be empty".to_string());
    }
    if file_path.starts_with('/') || Path::new(file_path).is_absolute() {
        return Err("File path must be relative".to_string());
    }
    if file_path.split('/').any(|part| part == "." || part == "..") {
        return Err(format!(
            "File path must stay inside the {} worktree",
            flavor.label()
        ));
    }
    if file_path.contains('\0') {
        return Err("File path must not contain NUL bytes".to_string());
    }
    Ok(())
}

/// 受保护路径:仅当**第一段**目录是 `.git` / `.aeroric`(大小写不敏感)。
/// 用 `Path::components` 取首段,POSIX 输入上与 split 版逐字等价,
/// Windows 本地输入还能正确按 `\` 分段。
pub(crate) fn is_protected_git_relative_path(file_path: &str) -> bool {
    const PROTECTED_FIRST_SEGMENTS: &[&str] = &[".git", ".aeroric"];
    Path::new(file_path)
        .components()
        .find_map(|component| match component {
            std::path::Component::Normal(name) => name.to_str().map(|name| {
                PROTECTED_FIRST_SEGMENTS
                    .iter()
                    .any(|protected| name.eq_ignore_ascii_case(protected))
            }),
            _ => None,
        })
        .unwrap_or(false)
}

/// discard(丢改动)专用:常规校验之外,拒绝删到项目元数据目录。
pub(crate) fn validate_git_discard_path(flavor: GitFlavor, file_path: &str) -> Result<(), String> {
    validate_git_relative_path(flavor, file_path)?;
    if is_protected_git_relative_path(file_path) {
        return Err("Refusing to delete protected project metadata".to_string());
    }
    Ok(())
}

/// revision / 分支名防御:空值、`-` 开头(选项注入,CWE-88)、NUL 一律拒绝。
/// 调用方应按需追加 `--end-of-options`(本地 git 版本支持时)。
///
/// Reject a user-supplied revision / commit hash that git could parse as an
/// option. A malicious repository can carry a ref such as
/// `refs/heads/--output=/path` (creatable via `git update-ref`), which then
/// shows up in the branch list and flows back here as `branch`/`commit_hash`.
/// Without this guard, `git show`/`git log` would treat it as `--output=` and
/// write attacker-controlled content to an arbitrary path (option injection,
/// CWE-88).
pub(crate) fn validate_git_revision(revision: &str) -> Result<(), String> {
    if revision.is_empty() {
        return Err("Git revision must not be empty".to_string());
    }
    if revision.starts_with('-') || revision.contains('\0') {
        return Err("Invalid git revision".to_string());
    }
    Ok(())
}

pub(crate) fn str_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

/// 逐个校验后去重,保序。
pub(crate) fn unique_git_file_paths(
    flavor: GitFlavor,
    file_paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for file_path in file_paths {
        validate_git_relative_path(flavor, &file_path)?;
        if seen.insert(file_path.clone()) {
            unique.push(file_path);
        }
    }
    Ok(unique)
}

/// `base… -- path…` 形态的参数;路径为空时只返回 base。
pub(crate) fn git_path_args(
    flavor: GitFlavor,
    base: &[&str],
    file_paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let file_paths = unique_git_file_paths(flavor, file_paths)?;
    if file_paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = str_args(base);
    args.push("--".to_string());
    args.extend(file_paths);
    Ok(args)
}

/// unstage:有 HEAD 用 `restore --staged`,没有用 `reset`。
pub(crate) fn git_unstage_args(
    flavor: GitFlavor,
    has_head: bool,
    file_paths: Vec<String>,
) -> Result<Vec<String>, String> {
    if has_head {
        git_path_args(flavor, &["restore", "--staged"], file_paths)
    } else {
        git_path_args(flavor, &["reset"], file_paths)
    }
}

/// 丢改动:tracked 走 `restore`,untracked 走 `clean -f`(后者还要过 discard 防御)。
pub(crate) fn git_discard_files_args(
    flavor: GitFlavor,
    file_paths: Vec<String>,
    untracked: bool,
) -> Result<Vec<String>, String> {
    let mut file_paths = unique_git_file_paths(flavor, file_paths)?;
    if untracked {
        for file_path in &file_paths {
            validate_git_discard_path(flavor, file_path)?;
        }
    }
    if file_paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = if untracked {
        str_args(&["clean", "-f"])
    } else {
        str_args(&["restore"])
    };
    args.push("--".to_string());
    args.append(&mut file_paths);
    Ok(args)
}

/// push:带分支时校验分支名再拼 `push origin <branch>`。校验文案与 git 面无关,
/// 所以不像其余构造器那样接 [`GitFlavor`]。
pub(crate) fn git_push_args(branch: Option<&str>) -> Result<Vec<String>, String> {
    let mut args = str_args(&["push"]);
    if let Some(branch) = branch.filter(|branch| !branch.is_empty()) {
        validate_git_revision(branch)?;
        args.push("origin".to_string());
        args.push(branch.to_string());
    }
    Ok(args)
}

pub(crate) fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_guard_rejects_traversal_for_every_flavor() {
        for flavor in [GitFlavor::Local, GitFlavor::Wsl, GitFlavor::Remote] {
            assert!(validate_git_relative_path(flavor, "src/main.rs").is_ok());
            assert!(validate_git_relative_path(flavor, "").is_err());
            assert!(validate_git_relative_path(flavor, "/abs/path").is_err());
            assert!(validate_git_relative_path(flavor, "../outside").is_err());
            assert!(validate_git_relative_path(flavor, "a/./b").is_err());
            assert!(validate_git_relative_path(flavor, "a\0b").is_err());
        }
        // 三家的文案差异只在称谓上,逐字保留。
        assert_eq!(
            validate_git_relative_path(GitFlavor::Local, "..").unwrap_err(),
            "File path must stay inside the git worktree"
        );
        assert_eq!(
            validate_git_relative_path(GitFlavor::Wsl, "..").unwrap_err(),
            "File path must stay inside the WSL git worktree"
        );
        assert_eq!(
            validate_git_relative_path(GitFlavor::Remote, "..").unwrap_err(),
            "File path must stay inside the remote git worktree"
        );
    }

    /// 首段保护:子目录里的 .git 不拦(那是子模块/嵌套库),`.gitignore` 不是 `.git`。
    #[test]
    fn protected_paths_cover_top_level_metadata_only() {
        for flavor in [GitFlavor::Local, GitFlavor::Wsl, GitFlavor::Remote] {
            assert!(validate_git_discard_path(flavor, ".git/config").is_err());
            assert!(validate_git_discard_path(flavor, ".AERORIC/config.toml").is_err());
            assert_eq!(
                validate_git_discard_path(flavor, ".git/config").unwrap_err(),
                "Refusing to delete protected project metadata"
            );
        }
        assert!(is_protected_git_relative_path(".git/config"));
        assert!(is_protected_git_relative_path(".aeroric/state.json"));
        assert!(!is_protected_git_relative_path("src/.git/config"));
        assert!(!is_protected_git_relative_path(".gitignore"));
        assert!(!is_protected_git_relative_path("src/git.rs"));
    }

    #[test]
    fn revision_guard_rejects_option_like_values() {
        assert!(validate_git_revision("main").is_ok());
        assert!(validate_git_revision("HEAD~1").is_ok());
        assert!(validate_git_revision("").is_err());
        assert!(validate_git_revision("--upload-pack=x").is_err());
        assert!(validate_git_revision("a\0b").is_err());
    }

    #[test]
    fn arg_builders_dedupe_quote_and_guard() {
        assert_eq!(str_args(&["status", "--short"]), vec!["status", "--short"]);
        assert_eq!(
            unique_git_file_paths(GitFlavor::Local, vec!["a".into(), "a".into(), "b".into()])
                .unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            git_path_args(GitFlavor::Local, &["add"], vec!["a b".into()]).unwrap(),
            vec!["add".to_string(), "--".to_string(), "a b".to_string()]
        );
        assert_eq!(
            git_path_args(GitFlavor::Local, &["add"], vec![]).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            git_unstage_args(GitFlavor::Wsl, true, vec!["a".into()]).unwrap(),
            vec!["restore", "--staged", "--", "a"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            git_unstage_args(GitFlavor::Wsl, false, vec!["a".into()]).unwrap(),
            vec!["reset", "--", "a"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            git_discard_files_args(GitFlavor::Remote, vec![".git/config".into()], true)
                .unwrap_err(),
            "Refusing to delete protected project metadata"
        );
        assert_eq!(
            git_push_args(Some("feature/x")).unwrap(),
            vec!["push", "origin", "feature/x"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            git_push_args(Some("--exec=x")).unwrap_err(),
            "Invalid git revision"
        );
        assert_eq!(git_push_args(None).unwrap(), vec!["push".to_string()]);
    }
}
