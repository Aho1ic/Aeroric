//! 更新簿记（自 `src-tauri/src/notification.rs` 的 update 半边上收）。
//!
//! 计划 Action 5 把更新拆成两段：**准备/下载/校验**与**实际替换**。
//! 这里只有前一段里与平台无关、可断言的部分——版本比较、资产选择、文件名守卫、
//! 待装记录的路径守卫与 sha256 校验。`hdiutil attach`/`ditto`/`mv`、
//! Windows/Linux 的 update helper、`app.request_restart()` 全是宿主的活，
//! 因为它们要求"替换正在运行的那个 app"，而 core 不是那个 app。
//!
//! 一条硬约束写在这里而不是注释里：**本计划实施期间 preview app 不覆盖旧
//! Aeroric**。所以 [`ReleaseUpdatePrepareResult::ready_to_restart`] 永远来自
//! 校验结果，而不是"下载完了就算准备好"。

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::notifications::{sanitize_text, GitHubReleaseAsset};

/// 待装记录里的 helper 状态（旧值一字不改：Windows/Linux 的 update helper 会回写它）。
pub const UPDATE_HELPER_READY: &str = "ready";

/// 选择安装资产时的机器形状。旧代码直接读 `std::env::consts`，收成参数之后
/// "这台机器上有没有 arm64 的 dmg"才是一个可以断言的问题。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetPlatform<'a> {
    pub os: &'a str,
    pub arch: &'a str,
    /// Linux 包型：`Some("deb")`/`Some("rpm")`；其它平台传 `None`。
    pub linux_kind: Option<&'a str>,
}

/// 准备完成的待装更新（旧 `ReleaseUpdatePrepareResult`，字段名逐个显式）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReleaseUpdatePrepareResult {
    #[serde(rename = "tagName")]
    pub tag_name: String,
    #[serde(rename = "assetName")]
    pub asset_name: String,
    #[serde(rename = "installerPath")]
    pub installer_path: String,
    #[serde(rename = "readyToRestart")]
    pub ready_to_restart: bool,
    #[serde(rename = "checksumVerified")]
    pub checksum_verified: bool,
    #[serde(rename = "helperStatus")]
    pub helper_status: String,
    pub error: Option<String>,
}

/// 安装结果（旧 `ReleaseInstallResult`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReleaseInstallResult {
    #[serde(rename = "tagName")]
    pub tag_name: String,
    #[serde(rename = "assetName")]
    pub asset_name: String,
    #[serde(rename = "installedAppPath")]
    pub installed_app_path: String,
    pub restarted: bool,
}

/// 落盘的待装记录（旧 `PendingReleaseUpdate`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReleaseUpdate {
    pub tag_name: String,
    pub asset_name: String,
    pub installer_path: String,
    pub checksum_path: String,
    pub checksum_verified: bool,
    pub helper_status: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// 旧盘上可能缺字段的变体（`PendingReleaseUpdatePayload`）：`checksumPath` 缺省是
/// 空串、`helperStatus` 缺省回 `ready`，与旧 `load_pending_update` 的兼容分支同义。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReleaseUpdatePayload {
    pub tag_name: String,
    pub asset_name: String,
    pub installer_path: String,
    #[serde(default)]
    pub checksum_path: String,
    #[serde(default)]
    pub checksum_verified: bool,
    #[serde(default)]
    pub helper_status: String,
    #[serde(default)]
    pub error: Option<String>,
}

impl PendingReleaseUpdatePayload {
    /// 补齐缺省字段；`helperStatus` 空串视作 `ready`（旧行为）。
    pub fn normalize(self) -> PendingReleaseUpdate {
        PendingReleaseUpdate {
            tag_name: self.tag_name,
            asset_name: self.asset_name,
            installer_path: self.installer_path,
            checksum_path: self.checksum_path,
            checksum_verified: self.checksum_verified,
            helper_status: if self.helper_status.is_empty() {
                UPDATE_HELPER_READY.to_string()
            } else {
                self.helper_status
            },
            error: self.error,
        }
    }
}

/// `v1.2.3` / `V1.2.3` → `1.2.3`。
pub fn release_version(tag: &str) -> String {
    tag.trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

/// `major.minor.patch` 逐段数值比较；缺段补 0，非数字段按 0（旧行为，别改）。
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    let max_len = va.len().max(vb.len());
    for index in 0..max_len {
        let a_part = va.get(index).copied().unwrap_or(0);
        let b_part = vb.get(index).copied().unwrap_or(0);
        match a_part.cmp(&b_part) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// tag → 目录名。只留 `[A-Za-z0-9._-]`，其余压成 `_`，长度 80（旧
/// `release_update_dir` 的 sanitize；路径穿越在这里就死掉）。
pub fn safe_tag(tag: &str) -> String {
    sanitize_text(tag, 80)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// `updates/<safe_tag>`。
pub fn release_update_dir(updates_root: &Path, tag_name: &str) -> PathBuf {
    updates_root.join(safe_tag(tag_name))
}

/// macOS dmg 选择（`aeroric_*_aarch64.dmg` / `_x64.dmg`；未知架构退 `.dmg`）。
pub fn select_macos_dmg_asset<'a>(
    assets: &'a [GitHubReleaseAsset],
    arch: &str,
) -> Option<&'a GitHubReleaseAsset> {
    let arch_token = match arch {
        "aarch64" | "arm64" => "_aarch64.dmg",
        "x86_64" | "x64" => "_x64.dmg",
        _ => ".dmg",
    };
    assets.iter().find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        name.starts_with("aeroric_") && name.ends_with(arch_token)
    })
}

/// 按平台选安装包；选不出就是"这台机器没有可装资产"，不是错误。
pub fn select_release_installer_asset<'a>(
    assets: &'a [GitHubReleaseAsset],
    os: &str,
    arch: &str,
    linux_kind: Option<&str>,
) -> Option<&'a GitHubReleaseAsset> {
    match os {
        "macos" => select_macos_dmg_asset(assets, arch),
        "windows" => {
            let suffix = match arch {
                "aarch64" | "arm64" => "_arm64-setup.exe",
                "x86_64" | "x64" => "_x64-setup.exe",
                _ => return None,
            };
            assets.iter().find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name.starts_with("aeroric_") && name.ends_with(suffix)
            })
        }
        "linux" if matches!(arch, "x86_64" | "x64") => {
            let suffix = match linux_kind {
                Some("deb") => "_amd64.deb",
                Some("rpm") => "-1.x86_64.rpm",
                _ => return None,
            };
            assets.iter().find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name.starts_with("aeroric") && name.ends_with(suffix)
            })
        }
        _ => None,
    }
}

/// 安装包的校验文件叫什么；不认识的扩展名返回 `None`（不是"没有校验"）。
pub fn expected_release_digest_asset_name(asset_name: &str) -> Option<String> {
    [".dmg", ".exe", ".deb", ".rpm"]
        .iter()
        .any(|suffix| asset_name.to_ascii_lowercase().ends_with(suffix))
        .then(|| "SHA256SUMS.txt".to_string())
}

/// 在发布资产表里找校验文件；找不到就报错（缺校验＝拒绝准备，旧行为）。
pub fn find_checksum_for_asset(
    asset: &GitHubReleaseAsset,
    assets: &[GitHubReleaseAsset],
) -> Result<String, String> {
    let expected_name = expected_release_digest_asset_name(&asset.name)
        .ok_or_else(|| "Unsupported installer asset name".to_string())?;
    if assets
        .iter()
        .any(|candidate| candidate.name == expected_name)
    {
        return Ok(expected_name);
    }
    Err(format!(
        "Missing checksum asset for {}. Expected a {expected_name} release asset.",
        asset.name
    ))
}

/// 发布资产名只能是文件名（来自网络的字符串要拼进本地路径之前先过这里）。
pub fn validate_release_asset_filename(name: &str) -> Result<(), String> {
    let path = Path::new(name);
    if name.is_empty()
        || path.is_absolute()
        || path.components().count() != 1
        || path.file_name() != Some(std::ffi::OsStr::new(name))
    {
        return Err("Invalid release asset filename".to_string());
    }
    Ok(())
}

/// 待装安装包必须落在 `updates/<safe_tag>/<assetName>` 上（canonical 之后比父目录，
/// 符号链接与 `..` 都在这一关死掉——旧实现同款，因为这是"被替换的文件从哪来"的分界）。
pub fn validate_pending_installer_path(
    updates_root: &Path,
    tag_name: &str,
    asset_name: &str,
    installer_path: &Path,
) -> Result<PathBuf, String> {
    let canonical_root = updates_root
        .canonicalize()
        .map_err(|error| format!("Resolve updates directory failed: {error}"))?;
    let canonical_installer = installer_path
        .canonicalize()
        .map_err(|error| format!("Resolve installer path failed: {error}"))?;
    let expected_dir = canonical_root.join(safe_tag(tag_name));

    if canonical_installer.parent() != Some(expected_dir.as_path()) {
        return Err("Prepared installer is outside the update directory.".to_string());
    }
    if canonical_installer.file_name() != Some(std::ffi::OsStr::new(asset_name)) {
        return Err("Prepared installer does not match the selected release asset.".to_string());
    }
    Ok(canonical_installer)
}

/// 待装文件自校验：`checksum_verified` 为假直接拒；校验文件必须是同一目录下的
/// `SHA256SUMS.txt`，且安装包现在的哈希仍与它记录的一致。**实读盘而不是信记录**——
/// 记录里的 `checksum_verified: true` 只说明下载那一刻对过。
pub fn verify_pending_update_files(
    pending: &PendingReleaseUpdate,
    installer_path: &Path,
) -> Result<(), String> {
    if !pending.checksum_verified {
        return Err("Prepared update has not passed checksum verification.".to_string());
    }
    let checksum_path = Path::new(&pending.checksum_path)
        .canonicalize()
        .map_err(|error| format!("Resolve checksum path failed: {error}"))?;
    let installer_path = installer_path
        .canonicalize()
        .map_err(|error| format!("Resolve installer path failed: {error}"))?;
    if checksum_path.parent() != installer_path.parent()
        || checksum_path.file_name() != Some(std::ffi::OsStr::new("SHA256SUMS.txt"))
    {
        return Err("Prepared checksum is outside the selected update directory.".to_string());
    }
    let checksum_text = std::fs::read_to_string(&checksum_path)
        .map_err(|error| format!("Read checksum failed: {error}"))?;
    verify_downloaded_checksum(&installer_path, &pending.asset_name, &checksum_text)
}

/// `SHA256SUMS.txt` 里找 `expected_file_name` 那一行，比对实读哈希。
pub fn verify_downloaded_checksum(
    asset_path: &Path,
    expected_file_name: &str,
    checksum_text: &str,
) -> Result<(), String> {
    let expected = checksum_text
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mut parts = trimmed.split_whitespace();
            let hash = parts.next()?;
            let file_name = parts.next()?;
            if file_name.ends_with(expected_file_name) {
                Some(hash.trim().to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| "Checksum entry for the downloaded installer was not found.".to_string())?;
    let raw =
        std::fs::read(asset_path).map_err(|error| format!("Read installer failed: {error}"))?;
    let mut hasher = Sha256::new();
    hasher.update(raw);
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err("Downloaded installer checksum does not match.".to_string());
    }
    Ok(())
}
