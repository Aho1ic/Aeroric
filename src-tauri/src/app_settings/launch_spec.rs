//! Agent 启动规格(`AgentLaunchSpec`)的路径解析。
//!
//! 从 `app_settings.rs` 拆出来的一段:从用户配置里的一个路径字符串,推导出真正要执行
//! 的 program + args + env。这段逻辑独立于设置的读写与持久化 —— 它不碰
//! `SETTINGS_LOCK`、不读盘上的 settings.json,只做纯粹的路径推导,所以能整块搬走。
//!
//! 拆分依据(搬之前逐个数过引用):簇内 18 个符号里 14 个在 crate 内零外部引用。
//! 留在父文件的是 `AgentLaunchSpec`(19 处外部引用)与 `detect_path`(31 处) ——
//! 它们是对外的接口面,搬走会牵动一大片 `use`。
//!
//! **`#[cfg(windows)]` 很密**:`resolve_agent_launch_spec_from_path` 有 not(windows)
//! 与 windows 两份,另有 8 个 Windows 专用 helper。改这里必须逐目标编译,只验 macOS
//! 会漏掉 Windows 侧的 dead-code 与 cfg 门控错误。
//!
//! 注意 `resolve_agent_launch_spec_from_path` 会在持有 `SETTINGS_LOCK` 时被调用
//! (设置归一化路径上),所以它内部不能调 `agent_family()` —— 那个会重新加载设置并
//! 递归获取同一把锁。原注释保留在函数体里。

use std::fs;
use std::path::{Path, PathBuf};

// 这份清单不是手挑的 —— 手挑漏了 6 个,靠编译器枚举出来的。
use super::{
    agent_scripts_dir, append_agent_credential_env, append_agent_proxy_env,
    append_builtin_agent_api_env, append_local_router_env, configured_agent_family, detect_path,
    get_agent_configured_path, normalize_config_path, AgentFamily, AgentLaunchSpec, AppSettings,
};
// 只有 Windows 分支的 `prepend_to_path` 用它。不加 cfg 门控的话,非 Windows 目标上
// 是个 unused import;删掉则 Windows 编译不过。
#[cfg(windows)]
use super::get_login_shell_path;

fn resolve_input_path(path: &str, binary: &str) -> String {
    let normalized = normalize_config_path(path.to_string());
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        let detected = detect_path(binary);
        return if detected.is_empty() {
            binary.to_string()
        } else {
            detected
        };
    }

    let detected = detect_path(trimmed);
    if detected.is_empty() {
        trimmed.to_string()
    } else {
        detected
    }
}

pub(super) fn normalize_agent_configured_path(agent: &str, path: &str) -> String {
    let resolved = resolve_input_path(path, agent);
    // Preserve a DSH source checkout in settings. The launch spec converts it
    // to `pnpm --dir <checkout> dsh` at execution time; storing only `pnpm`
    // here would lose the checkout path and make subsequent launches fall
    // back to the global command.
    if dsh_source_root(&resolved).is_some() {
        return resolved;
    }
    #[cfg(windows)]
    if crate::platform::agent_script_command(Path::new(&resolved)).is_some() {
        return resolved;
    }
    resolve_agent_launch_spec_from_path(agent, &resolved).program
}

fn dsh_source_root(path: &str) -> Option<PathBuf> {
    let candidate = Path::new(path);
    if !candidate.is_dir()
        || !candidate.join("package.json").is_file()
        || !candidate.join("apps").join("cli").is_dir()
    {
        return None;
    }
    Some(candidate.to_path_buf())
}

#[cfg(not(windows))]
fn resolve_agent_launch_spec_from_path(agent: &str, path: &str) -> AgentLaunchSpec {
    let program = resolve_input_path(path, agent);
    // This function is also called while `SETTINGS_LOCK` is held during
    // settings normalization. Do not call `agent_family(agent)` here: that
    // helper reloads settings and would recursively acquire the same lock.
    let is_dsh_path = agent == "dsh"
        || inferred_agent_family(&program) == Some(AgentFamily::Dsh)
        || dsh_source_root(&program).is_some();
    if is_dsh_path {
        if let Some(root) = dsh_source_root(&program) {
            return AgentLaunchSpec {
                program: "pnpm".to_string(),
                args: vec![
                    "--dir".to_string(),
                    root.to_string_lossy().into_owned(),
                    "dsh".to_string(),
                ],
                working_dir: Some(root),
                family: AgentFamily::Dsh,
                ..Default::default()
            };
        }
    }
    if Path::new(&program).is_absolute() {
        let _ = ensure_user_agent_script_executable(Path::new(&program));
    }
    AgentLaunchSpec {
        program,
        ..Default::default()
    }
}

#[cfg(not(windows))]
pub(crate) fn ensure_user_agent_script_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() {
        return Ok(());
    }
    let mode = metadata.permissions().mode();
    // Scripts we generate under ~/.aeroric/agents read an owner-only provider
    // API-key sidecar at runtime, so force owner-only 0o700 for the wrapper as
    // well. For an arbitrary user-provided
    // program path we only add the execute bit and leave its other bits alone,
    // so we never silently tighten permissions on the user's own binaries.
    let is_managed_agent_script = agent_scripts_dir()
        .ok()
        .and_then(|dir| dir.canonicalize().ok())
        .zip(path.canonicalize().ok())
        .map(|(dir, resolved)| resolved.starts_with(&dir))
        .unwrap_or(false);
    let target_mode = if is_managed_agent_script {
        0o700
    } else {
        mode | 0o100
    };
    if mode == target_mode {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(target_mode))
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn path_file_name_eq(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

#[cfg(windows)]
fn find_scoped_package_root(path: &Path, scope: &str, package: &str) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(dir) = current {
        let parent = dir.parent()?;
        if path_file_name_eq(dir, package) && path_file_name_eq(parent, scope) {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

#[cfg(windows)]
fn npm_package_root_from_shim(path: &Path, scope: &str, package: &str) -> Option<PathBuf> {
    let shim_dir = path.parent()?;
    let candidate = shim_dir.join("node_modules").join(scope).join(package);
    candidate.is_dir().then_some(candidate)
}

#[cfg(windows)]
fn candidate_from_ancestors(
    path: &Path,
    scope: &str,
    package: &str,
    relative: &[&str],
) -> Option<PathBuf> {
    let package_root = find_scoped_package_root(path, scope, package)
        .or_else(|| npm_package_root_from_shim(path, scope, package))?;
    let mut candidate = package_root;
    for segment in relative {
        candidate.push(segment);
    }
    candidate.is_file().then_some(candidate)
}

#[cfg(windows)]
fn codex_vendor_artifact_from_vendor_root(
    vendor_root: &Path,
) -> Option<(PathBuf, Option<PathBuf>)> {
    if !vendor_root.is_dir() {
        return None;
    }

    let mut arch_roots = fs::read_dir(vendor_root)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    arch_roots.sort();

    for arch_root in arch_roots {
        let exe = arch_root.join("codex").join("codex.exe");
        if exe.is_file() {
            let path_dir = arch_root.join("path");
            return Some((exe, path_dir.is_dir().then_some(path_dir)));
        }
    }

    None
}

#[cfg(windows)]
fn resolve_codex_vendor_artifact(path: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    if path_file_name_eq(path, "codex.exe")
        && path
            .parent()
            .is_some_and(|parent| path_file_name_eq(parent, "codex"))
    {
        let arch_root = path.parent()?.parent()?;
        let path_dir = arch_root.join("path");
        return Some((path.to_path_buf(), path_dir.is_dir().then_some(path_dir)));
    }

    if let Some(package_root) = find_scoped_package_root(path, "@openai", "codex")
        .or_else(|| npm_package_root_from_shim(path, "@openai", "codex"))
    {
        if let Some(found) = codex_vendor_artifact_from_vendor_root(&package_root.join("vendor")) {
            return Some(found);
        }

        let openai_dir = package_root.join("node_modules").join("@openai");
        if openai_dir.is_dir() {
            let mut package_dirs = fs::read_dir(&openai_dir)
                .ok()?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|candidate| {
                    candidate.is_dir()
                        && candidate
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("codex-win32-"))
                })
                .collect::<Vec<_>>();
            package_dirs.sort();

            for package_dir in package_dirs {
                if let Some(found) =
                    codex_vendor_artifact_from_vendor_root(&package_dir.join("vendor"))
                {
                    return Some(found);
                }
            }
        }
    }

    None
}

#[cfg(windows)]
fn prepend_to_path(entries: &[PathBuf]) -> Option<String> {
    let prefixes = entries
        .iter()
        .filter(|path| path.is_dir())
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if prefixes.is_empty() {
        return None;
    }

    let existing = get_login_shell_path();
    let mut combined = prefixes.join(";");
    if !existing.is_empty() {
        combined.push(';');
        combined.push_str(existing);
    }
    Some(combined)
}

#[cfg(windows)]
fn windows_script_launch(path: &Path) -> Option<AgentLaunchSpec> {
    crate::platform::agent_script_command(path).map(|command| AgentLaunchSpec {
        program: command.program,
        args: command.args,
        ..Default::default()
    })
}

#[cfg(windows)]
fn resolve_agent_launch_spec_from_path(agent: &str, path: &str) -> AgentLaunchSpec {
    let resolved = resolve_input_path(path, agent);
    let resolved_path = Path::new(&resolved);

    // Keep path resolution lock-free; this function is reached from settings
    // normalization while `SETTINGS_LOCK` is already held.
    let is_dsh_path = agent == "dsh"
        || inferred_agent_family(&resolved) == Some(AgentFamily::Dsh)
        || dsh_source_root(&resolved).is_some();
    if is_dsh_path {
        if let Some(root) = dsh_source_root(&resolved) {
            // 用裸名而不是硬编码 `pnpm.cmd`:corepack / Scoop 装出来的可能是
            // `pnpm.ps1` 或 `pnpm.bat`,写死 `.cmd` 在那些机器上直接找不到。
            // `detect_path` 会按 PATHEXT 依次尝试后缀。
            return AgentLaunchSpec {
                program: "pnpm".to_string(),
                args: vec![
                    "--dir".to_string(),
                    root.to_string_lossy().into_owned(),
                    "dsh".to_string(),
                ],
                working_dir: Some(root),
                family: AgentFamily::Dsh,
                ..Default::default()
            };
        }
    }

    match agent {
        "claude" => {
            if let Some(exe) = candidate_from_ancestors(
                resolved_path,
                "@anthropic-ai",
                "claude-code",
                &["bin", "claude.exe"],
            ) {
                AgentLaunchSpec {
                    program: exe.to_string_lossy().into_owned(),
                    ..Default::default()
                }
            } else if let Some(spec) = windows_script_launch(resolved_path) {
                spec
            } else {
                AgentLaunchSpec {
                    program: resolved,
                    ..Default::default()
                }
            }
        }
        "codex" => {
            if let Some((program, path_dir)) = resolve_codex_vendor_artifact(resolved_path) {
                let mut extra_env = Vec::new();
                if let Some(path_value) = prepend_to_path(&path_dir.into_iter().collect::<Vec<_>>())
                {
                    extra_env.push(("PATH".to_string(), path_value));
                }
                extra_env.push(("CODEX_MANAGED_BY_NPM".to_string(), "1".to_string()));
                AgentLaunchSpec {
                    program: program.to_string_lossy().into_owned(),
                    extra_env,
                    ..Default::default()
                }
            } else if let Some(spec) = windows_script_launch(resolved_path) {
                spec
            } else {
                AgentLaunchSpec {
                    program: resolved,
                    ..Default::default()
                }
            }
        }
        _ => windows_script_launch(resolved_path).unwrap_or_else(|| AgentLaunchSpec {
            program: resolved,
            ..Default::default()
        }),
    }
}

fn inferred_agent_codex_like(program: &str) -> Option<bool> {
    let file_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase);
    match file_name.as_deref() {
        Some("codex" | "codex.exe" | "codex.cmd" | "codex.js") => return Some(true),
        Some("claude" | "claude.exe" | "claude.cmd") => return Some(false),
        _ => {}
    }

    if fs::metadata(program).ok()?.len() > 256 * 1024 {
        return None;
    }
    let content = fs::read_to_string(program).ok()?;
    if content.contains("export CODEX_HOME=")
        && content.contains("model_catalog_json = \"model-catalog.json\"")
    {
        return Some(true);
    }
    if content.contains("export CLAUDE_CONFIG_DIR=")
        && content.contains("CLAUDE_CODE_SESSION_ENV_DIR")
    {
        return Some(false);
    }
    None
}

fn inferred_agent_family(program: &str) -> Option<AgentFamily> {
    let file_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(
        file_name.as_deref(),
        Some("dsh" | "dsh.exe" | "dsh.cmd" | "dsh.js" | "dsh.ps1")
    ) {
        return Some(AgentFamily::Dsh);
    }
    inferred_agent_codex_like(program).map(AgentFamily::from_codex_like)
}

pub(super) fn build_agent_launch_spec(
    settings: &AppSettings,
    agent: &str,
    router_listening: bool,
) -> AgentLaunchSpec {
    let configured_path = get_agent_configured_path(settings, agent);
    let mut spec = resolve_agent_launch_spec_from_path(agent, &configured_path);
    spec.family = inferred_agent_family(&configured_path)
        .unwrap_or_else(|| configured_agent_family(settings, agent));
    spec.codex_like = spec.family.is_codex_like();
    append_agent_credential_env(settings, agent, &mut spec.extra_env);
    append_builtin_agent_api_env(settings, agent, &mut spec.extra_env);
    append_agent_proxy_env(settings, agent, &mut spec.extra_env);
    append_local_router_env(settings, agent, router_listening, &mut spec.extra_env);
    spec
}

pub(super) fn get_agent_launch_spec_from_settings(
    settings: &AppSettings,
    agent: &str,
) -> AgentLaunchSpec {
    let router_listening =
        crate::local_router::is_listening_on(settings.local_router_settings.listen_port);
    build_agent_launch_spec(settings, agent, router_listening)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsh_source_directory_resolves_to_package_manager_launch() {
        let root =
            std::env::temp_dir().join(format!("aeroric-dsh-source-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("apps").join("cli")).unwrap();
        std::fs::write(root.join("package.json"), "{}\n").unwrap();

        let launch = resolve_agent_launch_spec_from_path("dsh", &root.to_string_lossy());
        assert_eq!(launch.working_dir, Some(root.clone()));
        assert_eq!(launch.args.last().map(String::as_str), Some("dsh"));
        // 两个平台都用裸名:Windows 侧交给 PATHEXT 去匹配 .cmd/.ps1/.bat,
        // 写死 `pnpm.cmd` 会在 corepack / Scoop 装的 pnpm 上找不到。
        assert_eq!(launch.program, "pnpm");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_empty_agent_path_to_binary_name_when_path_detection_fails() {
        let resolved = resolve_input_path("", "__aeroric_missing_agent_binary__");
        assert_eq!(resolved, "__aeroric_missing_agent_binary__");
    }

    #[cfg(windows)]
    #[test]
    fn windows_shell_agent_uses_an_interpreter_without_rewriting_configured_path() {
        let path = r"C:\Users\test\.aeroric\agents\mimo.sh";
        let launch = resolve_agent_launch_spec_from_path("mimo", path);

        assert_ne!(launch.program, path);
        assert!(launch.args.iter().any(|arg| arg.contains(path)));
        assert_eq!(normalize_agent_configured_path("mimo", path), path);
    }

    #[cfg(windows)]
    #[test]
    fn windows_powershell_agent_is_never_passed_directly_to_create_process() {
        let path = r"C:\Users\Test User\.aeroric\agents\mimo.ps1";
        let launch = resolve_agent_launch_spec_from_path("mimo", path);

        assert_ne!(launch.program, path);
        assert!(launch
            .program
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| {
                name.eq_ignore_ascii_case("pwsh.exe")
                    || name.eq_ignore_ascii_case("pwsh")
                    || name.eq_ignore_ascii_case("powershell.exe")
                    || name.eq_ignore_ascii_case("powershell")
            }));
        assert!(launch.args.windows(2).any(|args| args == ["-File", path]));
        assert_eq!(normalize_agent_configured_path("mimo", path), path);
    }

    #[test]
    fn launch_spec_recognizes_aeroric_generated_wrapper_families() {
        let codex_path =
            std::env::temp_dir().join(format!("aeroric-codex-wrapper-{}.sh", uuid::Uuid::new_v4()));
        let claude_path = std::env::temp_dir().join(format!(
            "aeroric-claude-wrapper-{}.sh",
            uuid::Uuid::new_v4()
        ));
        fs::write(
            &codex_path,
            "#!/bin/sh\nexport CODEX_HOME=/tmp/codex\nmodel_catalog_json = \"model-catalog.json\"\n",
        )
        .unwrap();
        fs::write(
            &claude_path,
            "#!/bin/sh\nexport CLAUDE_CONFIG_DIR=/tmp/claude\nexport CLAUDE_CODE_SESSION_ENV_DIR=/tmp/sessions\n",
        )
        .unwrap();

        assert_eq!(
            inferred_agent_codex_like(codex_path.to_string_lossy().as_ref()),
            Some(true)
        );
        assert_eq!(
            inferred_agent_codex_like(claude_path.to_string_lossy().as_ref()),
            Some(false)
        );

        let _ = fs::remove_file(codex_path);
        let _ = fs::remove_file(claude_path);
    }

    /// 一个只存在于此次测试的临时目录,`Drop` 时整棵删掉。
    /// 这些用例要造「像 DSH checkout」和「不像」的目录结构,手写 remove_dir_all
    /// 会在断言失败时漏掉清理。
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("aeroric-{tag}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn as_str(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 造出一棵满足 `dsh_source_root` 三个条件的树。
    fn make_dsh_checkout(tag: &str) -> TempTree {
        let tree = TempTree::new(tag);
        fs::create_dir_all(tree.path().join("apps").join("cli")).unwrap();
        fs::write(tree.path().join("package.json"), "{}\n").unwrap();
        tree
    }

    #[test]
    fn dsh_source_root_requires_all_three_markers() {
        // 三个条件缺任意一个都必须落空 —— 否则 normalize_agent_configured_path
        // 会把一个普通目录当 checkout 存进设置,之后启动时 `pnpm --dir <dir> dsh` 必失败。
        let full = make_dsh_checkout("dsh-full");
        assert_eq!(
            dsh_source_root(&full.as_str()),
            Some(full.path().to_path_buf())
        );

        let no_pkg = TempTree::new("dsh-no-pkg");
        fs::create_dir_all(no_pkg.path().join("apps").join("cli")).unwrap();
        assert_eq!(dsh_source_root(&no_pkg.as_str()), None, "缺 package.json");

        let no_cli = TempTree::new("dsh-no-cli");
        fs::write(no_cli.path().join("package.json"), "{}\n").unwrap();
        assert_eq!(dsh_source_root(&no_cli.as_str()), None, "缺 apps/cli");

        let pkg_is_dir = TempTree::new("dsh-pkg-dir");
        fs::create_dir_all(pkg_is_dir.path().join("apps").join("cli")).unwrap();
        fs::create_dir_all(pkg_is_dir.path().join("package.json")).unwrap();
        assert_eq!(
            dsh_source_root(&pkg_is_dir.as_str()),
            None,
            "package.json 是目录而不是文件"
        );
    }

    #[test]
    fn dsh_source_root_rejects_a_file_and_a_missing_path() {
        let tree = TempTree::new("dsh-file");
        let file = tree.path().join("dsh");
        fs::write(&file, "#!/bin/sh\n").unwrap();
        assert_eq!(
            dsh_source_root(&file.to_string_lossy()),
            None,
            "是文件不是目录"
        );
        assert_eq!(dsh_source_root(""), None, "空路径");
        assert_eq!(
            dsh_source_root(&tree.path().join("nope").to_string_lossy()),
            None,
            "不存在的路径"
        );
    }

    #[test]
    fn inferred_agent_family_reads_dsh_from_the_file_name_alone() {
        // 这几个不看文件内容,连存在都不要求 —— 名字就够。
        for name in ["dsh", "dsh.exe", "dsh.cmd", "dsh.js", "dsh.ps1"] {
            assert_eq!(
                inferred_agent_family(&format!("/opt/tools/{name}")),
                Some(AgentFamily::Dsh),
                "{name} 应认成 Dsh"
            );
        }
        // 大小写不敏感(内部走 to_ascii_lowercase)。
        assert_eq!(
            inferred_agent_family("/opt/tools/DSH.EXE"),
            Some(AgentFamily::Dsh)
        );
        // 前缀相同但不是这几个名字的,不能误判。
        for name in ["dshx", "mydsh", "dsh.py", "dsh.sh"] {
            assert_ne!(
                inferred_agent_family(&format!("/opt/tools/{name}")),
                Some(AgentFamily::Dsh),
                "{name} 不该认成 Dsh"
            );
        }
    }

    #[test]
    fn inferred_agent_codex_like_reads_known_names_without_touching_the_disk() {
        // 路径都不存在。走的是文件名 match,在读盘之前就返回了。
        for name in ["codex", "codex.exe", "codex.cmd", "codex.js"] {
            assert_eq!(
                inferred_agent_codex_like(&format!("/nonexistent/{name}")),
                Some(true),
                "{name}"
            );
        }
        for name in ["claude", "claude.exe", "claude.cmd"] {
            assert_eq!(
                inferred_agent_codex_like(&format!("/nonexistent/{name}")),
                Some(false),
                "{name}"
            );
        }
        // 不认识的名字 + 读不到文件 = None(而不是猜一个)。
        assert_eq!(inferred_agent_codex_like("/nonexistent/mimo"), None);
    }

    #[test]
    fn a_wrapper_needs_both_markers_before_it_counts_as_a_family() {
        // 只有一半标记的脚本必须是 None。少了这条,一个恰好 export 了 CODEX_HOME
        // 的用户脚本会被当成 codex 系,进而套上错的模型目录。
        let tree = TempTree::new("half-marker");
        let half_codex = tree.path().join("half-codex.sh");
        fs::write(&half_codex, "#!/bin/sh\nexport CODEX_HOME=/tmp/codex\n").unwrap();
        assert_eq!(
            inferred_agent_codex_like(&half_codex.to_string_lossy()),
            None
        );

        let half_claude = tree.path().join("half-claude.sh");
        fs::write(&half_claude, "#!/bin/sh\nexport CLAUDE_CONFIG_DIR=/tmp/c\n").unwrap();
        assert_eq!(
            inferred_agent_codex_like(&half_claude.to_string_lossy()),
            None
        );
    }

    #[test]
    fn a_large_wrapper_is_not_scanned_for_markers() {
        // >256KiB 直接放弃(避免把一个大二进制整个读进内存)。造一个超限的、
        // 但带完整 codex 标记的文件:因为超限,结论必须是 None 而不是 Some(true)。
        let tree = TempTree::new("big-wrapper");
        let big = tree.path().join("big.sh");
        let mut content = String::from(
            "#!/bin/sh\nexport CODEX_HOME=/tmp/codex\nmodel_catalog_json = \"model-catalog.json\"\n",
        );
        content.push_str(&"# padding\n".repeat(30_000));
        assert!(content.len() > 256 * 1024, "构造的样本得真的超过 256KiB");
        fs::write(&big, &content).unwrap();

        assert_eq!(inferred_agent_codex_like(&big.to_string_lossy()), None);
        // 同样内容缩到限内就能认出来 —— 证明上面的 None 来自大小闸门,不是标记没写对。
        let small = tree.path().join("small.sh");
        fs::write(
            &small,
            "#!/bin/sh\nexport CODEX_HOME=/tmp/codex\nmodel_catalog_json = \"model-catalog.json\"\n",
        )
        .unwrap();
        assert_eq!(
            inferred_agent_codex_like(&small.to_string_lossy()),
            Some(true)
        );
    }

    #[test]
    fn normalize_keeps_a_dsh_checkout_but_resolves_a_plain_binary() {
        // checkout 要原样留住:设置里存 `pnpm` 就丢了 --dir 的那个路径。
        let checkout = make_dsh_checkout("normalize-checkout");
        assert_eq!(
            normalize_agent_configured_path("dsh", &checkout.as_str()),
            checkout.as_str()
        );

        // 非 checkout 的路径走 launch spec 的 program。用一个不存在的名字,
        // detect_path 落空,于是原样返回 —— 断言的是"没有被改写成 pnpm"。
        let plain = "/nonexistent/__aeroric_plain_agent__";
        assert_eq!(normalize_agent_configured_path("mimo", plain), plain);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_dsh_named_binary_that_is_not_a_checkout_launches_directly() {
        // 名字是 dsh 但不是源码树:不能拼出 `pnpm --dir`,否则 --dir 指向一个
        // 没有 apps/cli 的目录,启动必失败。
        let tree = TempTree::new("dsh-binary");
        let binary = tree.path().join("dsh");
        fs::write(&binary, "#!/bin/sh\necho dsh\n").unwrap();

        let launch = resolve_agent_launch_spec_from_path("dsh", &binary.to_string_lossy());
        assert_ne!(launch.program, "pnpm");
        assert!(
            launch.args.is_empty(),
            "不该有 --dir 参数,实际 {:?}",
            launch.args
        );
        assert_eq!(launch.working_dir, None);
    }
}
