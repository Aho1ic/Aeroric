//! DSH 的一键安装与升级。
//!
//! 之前 dsh 的升级完全依赖 `build_agent_upgrade_commands` 的包管理器检测,
//! 只要活动安装不是 npm 全局 / Homebrew,就直接抛
//! "Cannot upgrade the active DeepSeek Harness installation ... (detected
//! channel: standalone)"。源码 checkout 恰好命中这个空档:它被刻意原样保留在
//! `dsh_path` 里(否则会丢掉 checkout 路径),但目录本身没有任何包管理器渠道。
//!
//! 这里给出一条在任何平台上都有结果的路径:
//!
//! | 活动安装 | 策略 |
//! |---|---|
//! | 源码 checkout | `git pull --ff-only` + `pnpm install` + build,失败自动降级为托管 |
//! | `~/.aeroric/tools/dsh` 下 | 托管重装(**必须优先于包管理器检测**) |
//! | npm 全局 / Homebrew 且能精确定位归属 | 沿用既有包管理器升级 |
//! | 其余(standalone / 未知 / 未安装) | 托管安装 |

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tokio::process::Command;

use super::node_bootstrap::{ensure_node_runtime, NodeRuntime};
use super::{
    command_output, tools_dir, ActivatedDir, AgentInstallErrorCode, AgentInstallStage, CleanupDir,
    DownloadProgress, InstallError, InstallResult, ProgressSink,
};

pub(super) const DSH_NPM_PACKAGE: &str = "@deepseek-ai/dsh";

/// 活动 dsh 安装该怎么升级。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DshStrategy {
    /// 源码 checkout:原地 git 升级 + 重新构建。
    SourceCheckout { root: PathBuf },
    /// Aeroric 托管副本:重装到私有前缀。
    Managed,
    /// npm 全局 / Homebrew:沿用既有包管理器升级命令。
    PackageManager,
}

pub(super) fn managed_dsh_root() -> InstallResult<PathBuf> {
    Ok(tools_dir()?.join("dsh"))
}

fn managed_dsh_current() -> InstallResult<PathBuf> {
    Ok(managed_dsh_root()?.join("current"))
}

/// 我们自己写的启动器文件名。不用 npm 生成的 shim:那个 shim 是
/// `#!/usr/bin/env node`,在只装了私有 Node 的机器上 PATH 里没有 node,直接跑不起来。
pub(super) fn managed_launcher_name() -> &'static str {
    if cfg!(windows) {
        "dsh.cmd"
    } else {
        "dsh"
    }
}

/// 托管安装对外暴露的可执行文件(写进 `dsh_path` 的就是这个)。
#[cfg(test)]
pub(super) fn managed_launcher_path() -> Option<PathBuf> {
    managed_dsh_root()
        .ok()
        .map(|root| root.join("bin").join(managed_launcher_name()))
}

/// 某个程序路径是否属于 Aeroric 托管的 dsh 副本。
pub(super) fn is_managed_program(program: &str) -> bool {
    let Ok(root) = managed_dsh_root() else {
        return false;
    };
    if program.trim().is_empty() {
        return false;
    }
    let normalized = |path: &Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
    };
    let root = normalized(&root);
    let program = normalized(Path::new(program));
    program == root || program.starts_with(&format!("{root}/"))
}

/// dsh 源码 checkout 的判定与 `app_settings` 保持一致:`package.json` + `apps/cli`。
fn source_checkout_root(path: &str) -> Option<PathBuf> {
    let candidate = Path::new(path);
    (candidate.is_dir()
        && candidate.join("package.json").is_file()
        && candidate.join("apps").join("cli").is_dir())
    .then(|| candidate.to_path_buf())
}

/// 纯函数版策略解析,便于测试:调用方先把探测结果算好传进来。
pub(super) fn resolve_strategy_from(
    configured_path: &str,
    active_program: &str,
    checkout_root: Option<PathBuf>,
    package_manager_upgradable: bool,
) -> DshStrategy {
    if let Some(root) = checkout_root {
        return DshStrategy::SourceCheckout { root };
    }
    // 托管判断必须排在包管理器检测之前:托管 shim 会经由 symlink 命中
    // `/node_modules/`,被 `detected_upgrade_manager` 判成 npm,而
    // `matching_npm_program` 拿全局 `npm prefix -g` 比对私有前缀必然不匹配,
    // 于是又退化成本 bug 的空命令列表。
    if is_managed_program(active_program) || is_managed_program(configured_path) {
        return DshStrategy::Managed;
    }
    if package_manager_upgradable {
        return DshStrategy::PackageManager;
    }
    DshStrategy::Managed
}

/// 从设置里解析当前该走哪条策略。
pub(super) fn resolve_strategy(configured_path: &str, active_program: &str) -> DshStrategy {
    let checkout_root = source_checkout_root(configured_path);
    // 只有在既不是 checkout 也不是托管时才值得付出探测包管理器的进程开销。
    let package_manager_upgradable = checkout_root.is_none()
        && !is_managed_program(active_program)
        && !is_managed_program(configured_path)
        && crate::app_settings::agent_upgrade_channel_available("dsh", active_program);
    resolve_strategy_from(
        configured_path,
        active_program,
        checkout_root,
        package_manager_upgradable,
    )
}

/// Unix 用 `#!/bin/sh` 包一层,Windows 用 `.cmd`,两者都写绝对路径,
/// 因此不依赖 PATH 上是否有 node。
pub(super) fn launcher_script(node: &Path, entry: &Path) -> String {
    if cfg!(windows) {
        format!(
            "@echo off\r\n\"{}\" \"{}\" %*\r\n",
            node.display(),
            entry.display()
        )
    } else {
        format!(
            "#!/bin/sh\nexec \"{}\" \"{}\" \"$@\"\n",
            node.display(),
            entry.display()
        )
    }
}

/// npm 全局前缀布局在两个平台上不同:Unix 是 `<prefix>/lib/node_modules`,
/// Windows 是 `<prefix>/node_modules`。两个都试,谁在就用谁。
fn installed_package_entry(prefix: &Path) -> Option<PathBuf> {
    for modules in [
        prefix.join("lib").join("node_modules"),
        prefix.join("node_modules"),
    ] {
        let package = modules.join("@deepseek-ai").join("dsh");
        let entry = package.join("lib").join("bin.js");
        if entry.is_file() {
            return Some(entry);
        }
        // 兜底:从 package.json 的 bin 字段解析入口。
        if let Some(entry) = package_json_bin(&package) {
            if entry.is_file() {
                return Some(entry);
            }
        }
    }
    None
}

fn package_json_bin(package: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(package.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let bin = value.get("bin")?;
    let relative = match bin {
        serde_json::Value::String(path) => path.clone(),
        serde_json::Value::Object(map) => map
            .get("dsh")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)?,
        _ => return None,
    };
    Some(package.join(relative))
}

/// 装到私有前缀:`npm install -g --prefix <staging> @deepseek-ai/dsh@<version>`。
///
/// 用私有前缀而不是全局目录,是为了绕开 Linux/macOS 上 npm 全局目录权限这个最
/// 常见的失败源——不需要 sudo,也不会动系统里已有的 dsh。
fn npm_install_command(runtime: &NodeRuntime, prefix: &Path, version: &str) -> Command {
    let mut command = match runtime.npm_cli.as_ref() {
        Some(npm_cli) => {
            let mut command = Command::new(&runtime.node);
            crate::subprocess::configure_background_tokio_command(&mut command);
            command.arg(npm_cli);
            command
        }
        // 系统 Node 找不到随包 npm 时退回 PATH 上的 npm shim。
        None => tool_command("npm"),
    };
    command.arg("install").arg("-g");
    command.arg("--prefix").arg(prefix);
    // npm 的用户级 min-release-age 会让刚发布的版本静默解析到旧版本却仍然退出 0。
    command.arg("--min-release-age=0");
    command.arg("--no-fund").arg("--no-audit");
    command.arg(format!("{DSH_NPM_PACKAGE}@{version}"));
    // `--prefix` 在部分 npm 版本上对 -g 的处理不一致,同时设环境变量兜底。
    command.env("npm_config_prefix", prefix);
    command.env("npm_config_global", "true");
    command.env("npm_config_fund", "false");
    command.env("npm_config_audit", "false");
    // 让 npm 的子进程也能找到同一个 node。基线用登录 shell 的 PATH:GUI 进程自身的
    // PATH 常常缺少 Homebrew 等前缀。
    let base = crate::app_settings::get_login_shell_path();
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(parent) = runtime.node.parent() {
        paths.push(parent.to_path_buf());
    }
    paths.extend(std::env::split_paths(base));
    if let Ok(joined) = std::env::join_paths(paths) {
        command.env("PATH", joined);
    }
    command
}

pub(super) struct ManagedInstall {
    pub(super) version: String,
    pub(super) launcher: PathBuf,
    pub(super) node_managed: bool,
}

/// 托管安装/升级:准备 Node → npm 装进 staging → 写启动器 → 原子激活 → 验版本。
pub(super) async fn install_managed(
    sink: &ProgressSink<'_>,
    target_version: &str,
    cancelled: &AtomicBool,
) -> InstallResult<ManagedInstall> {
    sink.emit(
        AgentInstallStage::PreparingEnvironment,
        8,
        "Preparing the Node.js runtime",
    );
    let runtime = ensure_node_runtime(
        &DownloadProgress {
            app: sink.app,
            operation_id: sink.operation_id,
            agent: sink.agent.to_string(),
            start: 10,
            end: 45,
            message: "Downloading the Node.js runtime".to_string(),
        },
        cancelled,
    )
    .await?;

    let root = managed_dsh_root()?;
    let staging = root.join(format!(".install-{}", uuid::Uuid::new_v4()));
    let _cleanup = CleanupDir::new(staging.clone());
    let staged_current = staging.join("current");
    std::fs::create_dir_all(&staged_current)
        .map_err(|error| InstallError::from_io(error, "Create install directory failed"))?;

    sink.emit(
        AgentInstallStage::Installing,
        55,
        format!("Installing {DSH_NPM_PACKAGE}@{target_version}"),
    );
    let output = command_output(
        npm_install_command(&runtime, &staged_current, target_version),
        cancelled,
        "npm install failed",
    )
    .await?;
    if !output.status.success() {
        let detail = super::combined_output(&output);
        return Err(InstallError::new(
            AgentInstallErrorCode::InstallFailed,
            if detail.is_empty() {
                format!("npm exited with {}", output.status)
            } else {
                detail
            },
        ));
    }

    let entry = installed_package_entry(&staged_current).ok_or_else(|| {
        InstallError::new(
            AgentInstallErrorCode::InstallFailed,
            format!("npm reported success but {DSH_NPM_PACKAGE} was not installed"),
        )
    })?;

    // 启动器要引用激活后的最终路径,所以先把入口相对 staging 的位置算出来。
    // strip_prefix 失败意味着 `installed_package_entry` 返回了 staging 之外的
    // 路径,那是内部不一致,必须显式报错 —— 否则拼出来的绝对路径会指向马上就要
    // 被清理掉的 staging 目录,装完立刻失效。
    let relative_entry = entry.strip_prefix(&staged_current).map_err(|_| {
        InstallError::new(
            AgentInstallErrorCode::Internal,
            "The installed DSH entry point is outside the staging directory",
        )
    })?;
    let final_current = managed_dsh_current()?;
    let final_entry = final_current.join(relative_entry);

    sink.emit(
        AgentInstallStage::VerifyingInstall,
        88,
        "Verifying the installation",
    );
    // 先原子激活 `current`,再写稳定启动器 `<root>/bin/dsh`,最后用启动器验版本。
    // 验证失败时提前返回,`ActivatedDir` 的 Drop 会把旧副本回滚回来。
    let activated = ActivatedDir::activate(&staged_current, &final_current)?;
    let stable_bin = root.join("bin");
    std::fs::create_dir_all(&stable_bin)
        .map_err(|error| InstallError::from_io(error, "Create launcher directory failed"))?;
    let stable_launcher = stable_bin.join(managed_launcher_name());
    std::fs::write(
        &stable_launcher,
        launcher_script(&runtime.node, &final_entry),
    )
    .map_err(|error| InstallError::from_io(error, "Write launcher failed"))?;
    super::make_executable(&stable_launcher)?;
    let version = super::detect_version(&stable_launcher, cancelled).await?;
    activated.commit();

    Ok(ManagedInstall {
        version,
        launcher: stable_launcher,
        node_managed: runtime.managed,
    })
}

/// 源码 checkout 原地升级的前置检查结果。不满足就降级为托管,而不是报错。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SourceUpgradeBlock {
    NotAGitRepository,
    DirtyWorktree,
    NoUpstream,
    MissingGit,
    MissingPnpm,
}

impl SourceUpgradeBlock {
    pub(super) fn reason(&self) -> &'static str {
        match self {
            Self::NotAGitRepository => "the DSH source checkout is not a git repository",
            Self::DirtyWorktree => {
                "the DSH source checkout has uncommitted changes, so it cannot fast-forward"
            }
            Self::NoUpstream => "the DSH source checkout has no upstream branch",
            Self::MissingGit => "git is not available",
            Self::MissingPnpm => "pnpm is not available",
        }
    }
}

/// GUI 进程继承的 PATH 往往只有 `/usr/bin:/bin`,不含 Homebrew 之类的前缀,
/// 所以工具一律按登录 shell 的 PATH 解析成绝对路径,并把该 PATH 传给子进程。
fn tool_command(binary: &str) -> Command {
    let resolved = crate::platform::detect_path(binary);
    let mut command = Command::new(if resolved.is_empty() {
        binary.to_string()
    } else {
        resolved
    });
    // npm / git / node 在 Windows 上多为 .cmd shim,安装流程会连着跑好几条,
    // 不加这个标志就是接连闪好几个控制台窗口。
    crate::subprocess::configure_background_tokio_command(&mut command);
    command.env("PATH", crate::app_settings::get_login_shell_path());
    command
}

async fn git_stdout(
    root: &Path,
    args: &[&str],
    cancelled: &AtomicBool,
) -> InstallResult<Option<String>> {
    let mut command = tool_command("git");
    command.current_dir(root).args(args);
    let output = command_output(command, cancelled, "git failed").await?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string()))
}

/// 原地升级前置检查。任何一项不满足都返回 Some(block),调用方据此降级。
pub(super) async fn source_upgrade_block(
    root: &Path,
    cancelled: &AtomicBool,
) -> InstallResult<Option<SourceUpgradeBlock>> {
    if crate::platform::detect_path("git").is_empty() {
        return Ok(Some(SourceUpgradeBlock::MissingGit));
    }
    if crate::platform::detect_path("pnpm").is_empty() {
        return Ok(Some(SourceUpgradeBlock::MissingPnpm));
    }
    if git_stdout(root, &["rev-parse", "--is-inside-work-tree"], cancelled)
        .await?
        .as_deref()
        != Some("true")
    {
        return Ok(Some(SourceUpgradeBlock::NotAGitRepository));
    }
    let status = git_stdout(root, &["status", "--porcelain"], cancelled).await?;
    if status.as_deref().is_none_or(|status| !status.is_empty()) {
        return Ok(Some(SourceUpgradeBlock::DirtyWorktree));
    }
    if git_stdout(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        cancelled,
    )
    .await?
    .is_none()
    {
        return Ok(Some(SourceUpgradeBlock::NoUpstream));
    }
    Ok(None)
}

async fn run_checked(
    mut command: Command,
    root: &Path,
    cancelled: &AtomicBool,
    context: &str,
) -> InstallResult<()> {
    command.current_dir(root);
    let output = command_output(command, cancelled, context).await?;
    if output.status.success() {
        return Ok(());
    }
    let detail = super::combined_output(&output);
    Err(InstallError::new(
        AgentInstallErrorCode::InstallFailed,
        if detail.is_empty() {
            format!("{context}: exited with {}", output.status)
        } else {
            format!("{context}: {detail}")
        },
    ))
}

fn has_build_script(root: &Path) -> bool {
    std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| {
            value
                .get("scripts")?
                .get("build")
                .map(serde_json::Value::is_string)
        })
        .unwrap_or(false)
}

/// 源码 checkout 原地升级:fetch → ff-only merge → pnpm install → build。
pub(super) async fn upgrade_source_checkout(
    sink: &ProgressSink<'_>,
    root: &Path,
    cancelled: &AtomicBool,
) -> InstallResult<()> {
    sink.emit(
        AgentInstallStage::Downloading,
        20,
        "Fetching the latest DSH source",
    );
    let mut fetch = tool_command("git");
    fetch.args(["fetch", "--tags", "--prune"]);
    run_checked(fetch, root, cancelled, "git fetch failed").await?;

    sink.emit(
        AgentInstallStage::Installing,
        40,
        "Fast-forwarding the checkout",
    );
    let mut merge = tool_command("git");
    merge.args(["merge", "--ff-only", "@{upstream}"]);
    run_checked(merge, root, cancelled, "git merge --ff-only failed").await?;

    sink.emit(AgentInstallStage::Installing, 60, "Installing dependencies");
    let mut install = tool_command("pnpm");
    install.arg("install");
    run_checked(install, root, cancelled, "pnpm install failed").await?;

    if has_build_script(root) {
        sink.emit(AgentInstallStage::Installing, 78, "Building DSH");
        let mut build = tool_command("pnpm");
        build.args(["run", "build"]);
        run_checked(build, root, cancelled, "pnpm run build failed").await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GUI 进程的 PATH 常常只有 `/usr/bin:/bin`,pnpm 之类装在 Homebrew 前缀下就
    /// 找不到。工具必须解析成绝对路径,并把登录 shell 的 PATH 传给子进程。
    #[test]
    fn tools_resolve_to_an_absolute_path_and_carry_the_login_shell_path() {
        let command = tool_command("git");
        let command = command.as_std();
        let program = PathBuf::from(command.get_program());
        // 系统里没有 git 时退回裸名,此处只在解析成功时断言绝对路径。
        if !crate::platform::detect_path("git").is_empty() {
            assert!(
                program.is_absolute(),
                "expected an absolute program, got {}",
                program.display()
            );
        }
        let path_env = command
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new("PATH"))
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().to_string())
            .expect("the child gets an explicit PATH");
        assert_eq!(path_env, crate::app_settings::get_login_shell_path());
    }

    #[test]
    fn source_checkout_wins_over_every_other_strategy() {
        let strategy = resolve_strategy_from(
            "/src/deepseek-harness",
            "pnpm",
            Some(PathBuf::from("/src/deepseek-harness")),
            true,
        );
        assert_eq!(
            strategy,
            DshStrategy::SourceCheckout {
                root: PathBuf::from("/src/deepseek-harness")
            }
        );
    }

    /// 本次 bug 的回归:托管副本的 shim 会被 `detected_upgrade_manager` 判成 npm,
    /// 但全局 npm prefix 与私有前缀不匹配,若让包管理器路径优先就会重新抛出
    /// "detected channel" 错误。
    #[test]
    fn managed_copy_is_resolved_before_package_manager_detection() {
        let Some(launcher) = managed_launcher_path() else {
            return;
        };
        let strategy = resolve_strategy_from(
            &launcher.to_string_lossy(),
            &launcher.to_string_lossy(),
            None,
            true,
        );
        assert_eq!(strategy, DshStrategy::Managed);
    }

    #[test]
    fn keeps_the_package_manager_path_when_the_active_install_owns_a_channel() {
        assert_eq!(
            resolve_strategy_from("/opt/homebrew/bin/dsh", "/opt/homebrew/bin/dsh", None, true),
            DshStrategy::PackageManager
        );
    }

    /// 之前这条路径抛错;现在必须落到托管安装,才能做到"任意电脑都能一键升级"。
    #[test]
    fn falls_back_to_managed_for_standalone_and_missing_installs() {
        assert_eq!(
            resolve_strategy_from("/opt/custom/dsh", "/opt/custom/dsh", None, false),
            DshStrategy::Managed
        );
        assert_eq!(
            resolve_strategy_from("", "", None, false),
            DshStrategy::Managed
        );
    }

    #[test]
    fn launcher_uses_absolute_paths_so_it_works_without_node_on_path() {
        let script = launcher_script(
            Path::new("/tools/node/bin/node"),
            Path::new("/tools/dsh/bin.js"),
        );
        assert!(script.contains("/tools/node/bin/node"));
        assert!(script.contains("/tools/dsh/bin.js"));
        if cfg!(windows) {
            assert!(script.starts_with("@echo off"));
            assert!(script.contains("%*"));
        } else {
            assert!(script.starts_with("#!/bin/sh"));
            assert!(script.contains("\"$@\""));
        }
    }

    #[test]
    fn npm_install_targets_the_private_prefix_and_the_exact_version() {
        let runtime = NodeRuntime {
            node: PathBuf::from("/tools/node/bin/node"),
            npm_cli: Some(PathBuf::from(
                "/tools/node/lib/node_modules/npm/bin/npm-cli.js",
            )),
            managed: true,
        };
        let prefix = PathBuf::from("/tools/dsh/.install-x/current");
        let command = npm_install_command(&runtime, &prefix, "0.1.1-rc.2");
        let std_command = command.as_std();
        let args: Vec<String> = std_command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            std_command.get_program().to_string_lossy(),
            "/tools/node/bin/node"
        );
        assert!(args.contains(&"install".to_string()));
        assert!(args.contains(&"-g".to_string()));
        assert!(args.contains(&"--min-release-age=0".to_string()));
        assert!(args.contains(&"@deepseek-ai/dsh@0.1.1-rc.2".to_string()));
        assert!(args.iter().any(|arg| arg == &prefix.to_string_lossy()));
    }

    #[test]
    fn every_source_upgrade_block_explains_why_it_fell_back() {
        for block in [
            SourceUpgradeBlock::NotAGitRepository,
            SourceUpgradeBlock::DirtyWorktree,
            SourceUpgradeBlock::NoUpstream,
            SourceUpgradeBlock::MissingGit,
            SourceUpgradeBlock::MissingPnpm,
        ] {
            assert!(!block.reason().is_empty());
        }
    }
}
