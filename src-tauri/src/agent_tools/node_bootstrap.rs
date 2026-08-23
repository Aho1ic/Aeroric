//! 私有 Node 运行时引导。
//!
//! dsh 是纯 JS 的 npm 包(`@deepseek-ai/dsh`),必须有 Node 才能运行,而且它的
//! tarball 不打包依赖(20 个文件、零 bundled node_modules,却声明 20+ 运行时
//! 依赖),所以托管安装必须做真实依赖解析 = 必须 npm。
//!
//! Node 官方发行包自带 npm(`lib/node_modules/npm/bin/npm-cli.js`),所以在缺
//! Node 的机器上下载一份私有 Node 就同时解决了"没有 Node"和"没有 npm"两件事,
//! 且全程装在 `~/.aeroric/tools/node` 下,不碰系统环境、不需要管理员权限。

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use super::{
    download_small_bytes, download_to_file, extract_archive, tools_dir, ActivatedDir,
    AgentInstallErrorCode, CleanupDir, DownloadProgress, InstallError, InstallResult,
    MAX_METADATA_BYTES,
};

/// Node 官方发行站。索引、压缩包与 SHASUMS256.txt 都从这里取。
const NODE_DIST_HOST: &str = "nodejs.org";
pub(super) const NODE_DIST_HOSTS: &[&str] = &["nodejs.org"];
const NODE_DIST_INDEX: &str = "https://nodejs.org/dist/index.json";
/// Node 发行包解包后约 60-110 MB,留足余量。
const MAX_NODE_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Deserialize)]
pub(super) struct NodeDistRelease {
    version: String,
    #[serde(default)]
    lts: serde_json::Value,
    #[serde(default)]
    files: Vec<String>,
}

impl NodeDistRelease {
    /// dist 索引里 `lts` 对非 LTS 版本是 `false`,对 LTS 版本是代号字符串。
    fn is_lts(&self) -> bool {
        self.lts.as_str().is_some_and(|name| !name.is_empty())
    }
}

/// 一份可用的 Node 运行时:`node` 可执行文件 + 随包自带的 npm 入口脚本。
#[derive(Clone, Debug)]
pub(super) struct NodeRuntime {
    pub(super) node: PathBuf,
    /// `lib/node_modules/npm/bin/npm-cli.js`,用 `node <npm_cli> ...` 调用,
    /// 不依赖 PATH 上的 npm shim。
    pub(super) npm_cli: Option<PathBuf>,
    pub(super) managed: bool,
}

/// `<os>-<arch>` → Node 发行包的平台标识。musl 不在官方发行矩阵内。
pub(super) fn node_dist_target(os: &str, arch: &str, musl: bool) -> InstallResult<&'static str> {
    if musl {
        return Err(InstallError::new(
            AgentInstallErrorCode::UnsupportedPlatform,
            "Node.js does not publish official musl builds; install Node.js from your distribution and retry",
        ));
    }
    match (os, arch) {
        ("macos", "aarch64") => Ok("darwin-arm64"),
        ("macos", "x86_64") => Ok("darwin-x64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        ("linux", "powerpc64") => Ok("linux-ppc64le"),
        ("linux", "s390x") => Ok("linux-s390x"),
        ("windows", "aarch64") => Ok("win-arm64"),
        ("windows", "x86_64") => Ok("win-x64"),
        ("windows", "x86") => Ok("win-x86"),
        _ => Err(InstallError::new(
            AgentInstallErrorCode::UnsupportedPlatform,
            format!("Node.js does not publish an official build for {os}/{arch}"),
        )),
    }
}

/// Windows 发行包是 zip,其余是 tar.gz。
pub(super) fn node_archive_extension(target: &str) -> &'static str {
    if target.starts_with("win-") {
        "zip"
    } else {
        "tar.gz"
    }
}

pub(super) fn node_archive_name(version: &str, target: &str) -> String {
    format!("node-{version}-{target}.{}", node_archive_extension(target))
}

pub(super) fn node_archive_url(version: &str, target: &str) -> String {
    format!(
        "https://{NODE_DIST_HOST}/dist/{version}/{}",
        node_archive_name(version, target)
    )
}

pub(super) fn node_shasums_url(version: &str) -> String {
    format!("https://{NODE_DIST_HOST}/dist/{version}/SHASUMS256.txt")
}

/// 从 `SHASUMS256.txt` 里挑出目标文件的摘要。格式是 `<sha256>  <filename>`。
pub(super) fn sha256_from_shasums(shasums: &str, file_name: &str) -> Option<String> {
    shasums.lines().find_map(|line| {
        let (digest, name) = line.trim().split_once("  ")?;
        (name.trim() == file_name).then(|| digest.trim().to_ascii_lowercase())
    })
}

/// dist 索引的 `files` 用的是另一套命名(`osx-arm64-tar`、`win-x64-zip`),
/// 和下载文件名里的平台标识(`darwin-arm64`、`win-x64`)并不相同,必须显式映射,
/// 否则可用性判断会永远落空。
pub(super) fn dist_index_key(target: &str) -> &'static str {
    match target {
        "darwin-arm64" => "osx-arm64-tar",
        "darwin-x64" => "osx-x64-tar",
        "linux-arm64" => "linux-arm64",
        "linux-x64" => "linux-x64",
        "linux-ppc64le" => "linux-ppc64le",
        "linux-s390x" => "linux-s390x",
        "win-arm64" => "win-arm64-zip",
        "win-x64" => "win-x64-zip",
        "win-x86" => "win-x86-zip",
        _ => "",
    }
}

/// 在 dist 索引里选出最新的、且发布了目标平台压缩包的 LTS 版本。
pub(super) fn pick_latest_lts(releases: &[NodeDistRelease], target: &str) -> Option<String> {
    let key = dist_index_key(target);
    // 索引本身按版本从新到旧排列,直接取第一个满足条件的即可。
    releases
        .iter()
        .find(|release| {
            release.is_lts()
                && (release.files.is_empty()
                    || key.is_empty()
                    || release.files.iter().any(|file| file == key))
        })
        .map(|release| release.version.clone())
}

pub(super) fn managed_node_root() -> InstallResult<PathBuf> {
    Ok(tools_dir()?.join("node"))
}

fn managed_node_current() -> InstallResult<PathBuf> {
    Ok(managed_node_root()?.join("current"))
}

/// 私有 Node 解包后 `node`/`npm-cli.js` 的位置在 Windows 与 Unix 上不同。
pub(super) fn node_executable_in(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("node.exe")
    } else {
        root.join("bin").join("node")
    }
}

pub(super) fn npm_cli_in(root: &Path) -> PathBuf {
    root.join("lib")
        .join("node_modules")
        .join("npm")
        .join("bin")
        .join("npm-cli.js")
}

/// Windows 的 npm 在包根目录下(`node_modules/npm`),Unix 在 `lib/` 下。
fn npm_cli_candidates(root: &Path) -> Vec<PathBuf> {
    vec![
        npm_cli_in(root),
        root.join("node_modules")
            .join("npm")
            .join("bin")
            .join("npm-cli.js"),
    ]
}

fn resolve_runtime_in(root: &Path, managed: bool) -> Option<NodeRuntime> {
    let node = node_executable_in(root);
    if !node.is_file() {
        return None;
    }
    Some(NodeRuntime {
        node,
        npm_cli: npm_cli_candidates(root)
            .into_iter()
            .find(|candidate| candidate.is_file()),
        managed,
    })
}

/// 已装好的私有 Node(若存在)。
pub(super) fn existing_managed_runtime() -> Option<NodeRuntime> {
    let current = managed_node_current().ok()?;
    resolve_runtime_in(&current, true)
}

/// 系统 Node(若存在)。npm 入口从 Node 安装前缀里推断,失败也不致命——
/// 调用方会退回 PATH 上的 npm。
fn system_runtime() -> Option<NodeRuntime> {
    let node = crate::node_runtime::detect_node()?;
    // `<prefix>/bin/node` → `<prefix>`;Windows 是 `<prefix>/node.exe`。
    let prefix = if cfg!(windows) {
        node.parent().map(Path::to_path_buf)
    } else {
        node.parent().and_then(Path::parent).map(Path::to_path_buf)
    };
    let npm_cli = prefix
        .as_deref()
        .and_then(|prefix| {
            npm_cli_candidates(prefix)
                .into_iter()
                .find(|candidate| candidate.is_file())
        })
        // Homebrew 等把 node 与 npm 分开放时,退回按 PATH 找 npm 的安装位置。
        .or_else(|| {
            let npm = crate::platform::detect_path("npm");
            if npm.is_empty() {
                return None;
            }
            let npm = std::fs::canonicalize(&npm).unwrap_or_else(|_| PathBuf::from(npm));
            npm.ancestors()
                .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "npm"))
                .map(|npm_root| npm_root.join("bin").join("npm-cli.js"))
                .filter(|candidate| candidate.is_file())
        });
    Some(NodeRuntime {
        node,
        npm_cli,
        managed: false,
    })
}

/// 确保有一个能用的 Node:优先系统的(沿用用户 registry/代理配置),其次已装好
/// 的私有副本,最后才下载。`progress` 用于把下载进度透传给前端。
pub(super) async fn ensure_node_runtime(
    progress: &DownloadProgress<'_>,
    cancelled: &AtomicBool,
) -> InstallResult<NodeRuntime> {
    if let Some(runtime) = system_runtime() {
        return Ok(runtime);
    }
    if let Some(runtime) = existing_managed_runtime() {
        return Ok(runtime);
    }
    install_managed_node(progress, cancelled).await
}

/// 下载并原子激活一份私有 Node 到 `~/.aeroric/tools/node/current`。
async fn install_managed_node(
    progress: &DownloadProgress<'_>,
    cancelled: &AtomicBool,
) -> InstallResult<NodeRuntime> {
    let target = node_dist_target(
        std::env::consts::OS,
        std::env::consts::ARCH,
        super::current_libc_is_musl(),
    )?;
    let client = super::http_client(NODE_DIST_HOSTS)?;
    let index_bytes =
        download_small_bytes(&client, NODE_DIST_INDEX, MAX_METADATA_BYTES, cancelled).await?;
    let releases: Vec<NodeDistRelease> = serde_json::from_slice(&index_bytes).map_err(|error| {
        InstallError::new(
            AgentInstallErrorCode::DownloadFailed,
            format!("Invalid Node.js release index: {error}"),
        )
    })?;
    let version = pick_latest_lts(&releases, target).ok_or_else(|| {
        InstallError::new(
            AgentInstallErrorCode::UnsupportedPlatform,
            format!("Node.js has no LTS build for {target}"),
        )
    })?;

    let archive_name = node_archive_name(&version, target);
    let shasums = download_small_bytes(
        &client,
        &node_shasums_url(&version),
        MAX_METADATA_BYTES,
        cancelled,
    )
    .await?;
    let shasums = String::from_utf8_lossy(&shasums).into_owned();
    let expected_sha256 = sha256_from_shasums(&shasums, &archive_name).ok_or_else(|| {
        InstallError::new(
            AgentInstallErrorCode::ChecksumFailed,
            format!("Node.js SHASUMS256.txt has no entry for {archive_name}"),
        )
    })?;

    let root = managed_node_root()?;
    let staging = root.join(format!(".install-{}", uuid::Uuid::new_v4()));
    let _cleanup = CleanupDir::new(staging.clone());
    let archive_path = staging.join(&archive_name);
    let download = download_to_file(
        &client,
        &node_archive_url(&version, target),
        &archive_path,
        None,
        MAX_NODE_ARCHIVE_BYTES,
        cancelled,
        progress,
    )
    .await?;
    super::verify_sha256(&download.sha256, &expected_sha256)?;

    let extracted = staging.join("extracted");
    extract_archive(&archive_path, &extracted)?;
    // 官方压缩包只有一个顶层目录 `node-<version>-<target>/`。
    let extracted_root = extracted.join(format!("node-{version}-{target}"));
    let staged_current = if extracted_root.is_dir() {
        extracted_root
    } else {
        single_child_dir(&extracted)?
    };
    if !node_executable_in(&staged_current).is_file() {
        return Err(InstallError::new(
            AgentInstallErrorCode::ArchiveInvalid,
            "The downloaded Node.js archive has no node executable",
        ));
    }
    super::make_executable(&node_executable_in(&staged_current))?;

    let current = managed_node_current()?;
    let activated = ActivatedDir::activate(&staged_current, &current)?;
    let runtime = resolve_runtime_in(&current, true).ok_or_else(|| {
        InstallError::new(
            AgentInstallErrorCode::VerificationFailed,
            "The activated Node.js runtime has no node executable",
        )
    })?;
    activated.commit();
    Ok(runtime)
}

fn single_child_dir(root: &Path) -> InstallResult<PathBuf> {
    let mut directories = std::fs::read_dir(root)
        .map_err(|error| InstallError::from_io(error, "Read extracted archive failed"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir());
    let first = directories.next().ok_or_else(|| {
        InstallError::new(
            AgentInstallErrorCode::ArchiveInvalid,
            "The downloaded archive has no top-level directory",
        )
    })?;
    if directories.next().is_some() {
        return Err(InstallError::new(
            AgentInstallErrorCode::ArchiveInvalid,
            "The downloaded archive has multiple top-level directories",
        ));
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_supported_platform_to_an_official_node_build() {
        for (os, arch, expected) in [
            ("macos", "aarch64", "darwin-arm64"),
            ("macos", "x86_64", "darwin-x64"),
            ("linux", "aarch64", "linux-arm64"),
            ("linux", "x86_64", "linux-x64"),
            ("windows", "aarch64", "win-arm64"),
            ("windows", "x86_64", "win-x64"),
        ] {
            assert_eq!(node_dist_target(os, arch, false).unwrap(), expected);
        }
    }

    #[test]
    fn refuses_musl_because_nodejs_publishes_no_official_musl_build() {
        let error = node_dist_target("linux", "x86_64", true)
            .expect_err("musl has no official Node.js build");
        assert_eq!(error.code, AgentInstallErrorCode::UnsupportedPlatform);
        assert!(error.message.contains("musl"));
    }

    #[test]
    fn windows_uses_zip_and_every_other_platform_uses_tar_gz() {
        assert_eq!(node_archive_extension("win-x64"), "zip");
        assert_eq!(node_archive_extension("darwin-arm64"), "tar.gz");
        assert_eq!(
            node_archive_name("v24.19.0", "win-x64"),
            "node-v24.19.0-win-x64.zip"
        );
        assert_eq!(
            node_archive_name("v24.19.0", "linux-x64"),
            "node-v24.19.0-linux-x64.tar.gz"
        );
    }

    #[test]
    fn builds_archive_and_checksum_urls_on_the_official_dist_host() {
        assert_eq!(
            node_archive_url("v24.19.0", "darwin-arm64"),
            "https://nodejs.org/dist/v24.19.0/node-v24.19.0-darwin-arm64.tar.gz"
        );
        assert_eq!(
            node_shasums_url("v24.19.0"),
            "https://nodejs.org/dist/v24.19.0/SHASUMS256.txt"
        );
    }

    #[test]
    fn reads_the_matching_digest_out_of_shasums256() {
        let shasums = concat!(
            "1111111111111111111111111111111111111111111111111111111111111111  node-v24.19.0-linux-x64.tar.gz\n",
            "2222222222222222222222222222222222222222222222222222222222222222  node-v24.19.0-darwin-arm64.tar.gz\n"
        );
        assert_eq!(
            sha256_from_shasums(shasums, "node-v24.19.0-darwin-arm64.tar.gz").as_deref(),
            Some("2222222222222222222222222222222222222222222222222222222222222222")
        );
        assert!(sha256_from_shasums(shasums, "node-v24.19.0-win-x64.zip").is_none());
    }

    /// dist 索引的 files 命名与下载文件名不同,映射错了会让可用性判断永远落空。
    #[test]
    fn translates_archive_targets_into_dist_index_keys() {
        assert_eq!(dist_index_key("darwin-arm64"), "osx-arm64-tar");
        assert_eq!(dist_index_key("darwin-x64"), "osx-x64-tar");
        assert_eq!(dist_index_key("win-x64"), "win-x64-zip");
        assert_eq!(dist_index_key("linux-x64"), "linux-x64");
    }

    #[test]
    fn picks_the_newest_lts_release_that_ships_the_target() {
        let releases: Vec<NodeDistRelease> = serde_json::from_str(
            r#"[
                {"version":"v26.7.0","lts":false,"files":["osx-arm64-tar","linux-x64"]},
                {"version":"v24.19.0","lts":"Krypton","files":["osx-arm64-tar","linux-x64"]},
                {"version":"v22.22.0","lts":"Jod","files":["osx-arm64-tar"]}
            ]"#,
        )
        .unwrap();
        assert_eq!(
            pick_latest_lts(&releases, "darwin-arm64").as_deref(),
            Some("v24.19.0")
        );
    }

    #[test]
    fn skips_an_lts_release_that_does_not_publish_the_target() {
        let releases: Vec<NodeDistRelease> = serde_json::from_str(
            r#"[
                {"version":"v24.19.0","lts":"Krypton","files":["linux-x64"]},
                {"version":"v22.22.0","lts":"Jod","files":["win-arm64-zip","linux-x64"]}
            ]"#,
        )
        .unwrap();
        assert_eq!(
            pick_latest_lts(&releases, "win-arm64").as_deref(),
            Some("v22.22.0")
        );
    }

    #[test]
    fn locates_npm_and_node_using_the_platform_archive_layout() {
        let root = Path::new("/tools/node/current");
        if cfg!(windows) {
            assert_eq!(node_executable_in(root), root.join("node.exe"));
        } else {
            assert_eq!(node_executable_in(root), root.join("bin").join("node"));
        }
        assert_eq!(
            npm_cli_in(root),
            root.join("lib")
                .join("node_modules")
                .join("npm")
                .join("bin")
                .join("npm-cli.js")
        );
    }
}
