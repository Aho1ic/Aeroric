//! The adapter half of ADR-010's measurement (windows only since P3-07 ②a).
//!
//! 分类表、unix `statfs` 测量与祖先回退已 verbatim 上收 core `location_probe`
//! （`aeroric-cli serve` 要拿写租约，不能依赖 Tauri crate）；本文件只剩两样
//! core 拿不了的东西：
//! - **Windows 测量**：`GetDriveTypeW`（windows-sys 不是 core 依赖——与
//!   `runtime::subprocess` 同一边界）与 ACE 回执（`windows_private_acl_receipt`，
//!   shipping client 的权限安装）；
//! - **装配**：[`PlatformLocationProbe`] = 平台测量 + core 的 sync-root 启发式。
//!   unix 上测量整体委托 core（行为与上收前逐字一致），windows 上本地测量＋回执。
//!   本文件对外只暴露 `PlatformLocationProbe`（唯一消费者 `core_adapter`），
//!   故不设 re-export 壳——上收的名字在宿主已无调用点。

use std::path::Path;

use aeroric_core::location::{with_cloud_sync_heuristic, StorageLocation, StorageLocationProbe};

#[cfg(windows)]
use aeroric_core::location::VolumeKind;
#[cfg(windows)]
use aeroric_core::location_probe::{classify_drive_type, ProbeVolume};
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
fn windows_drive_type(data_dir: &Path) -> ProbeVolume {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    let root = drive_root(data_dir);
    let wide: Vec<u16> = root
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
    match classify_drive_type(kind) {
        Some(volume) => ProbeVolume::Measured(volume),
        None => ProbeVolume::NotVerifiable,
    }
}

/// `GetDriveTypeW` wants a drive root like `C:\`, not a subdirectory.
#[cfg(windows)]
pub fn drive_root(path: &Path) -> PathBuf {
    let bytes: Vec<u8> = path
        .to_string_lossy()
        .as_bytes()
        .iter()
        .take(3)
        .copied()
        .collect();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return PathBuf::from(format!("{}:\\", (bytes[0] as char).to_ascii_uppercase()));
    }
    PathBuf::from(path)
}

/// The host's raw measurement: unix delegates wholesale to core (identical answer,
/// one implementation); windows measures locally and asks the ACL module for the
/// permission receipt — the thing a core without windows_acl cannot know.
struct HostRawPlatformProbe;

impl StorageLocationProbe for HostRawPlatformProbe {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        #[cfg(unix)]
        {
            aeroric_core::location_probe::RawPlatformProbe.probe(data_dir)
        }
        #[cfg(windows)]
        {
            let volume = match windows_drive_type(data_dir) {
                ProbeVolume::Measured(kind) => kind,
                ProbeVolume::NotVerifiable => VolumeKind::Unknown,
            };
            StorageLocation::with_permissions(
                data_dir,
                volume,
                crate::storage::windows_private_acl_receipt(data_dir),
            )
        }
    }
}

/// The adapter's probe. Delegates the measurement to [`HostRawPlatformProbe`] and then
/// lets core relabel a measured `Local` as `CloudSyncedRoot` where the path says so —
/// the heuristic can never turn `Unknown` into something eligible.
pub struct PlatformLocationProbe;

impl StorageLocationProbe for PlatformLocationProbe {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        with_cloud_sync_heuristic(&HostRawPlatformProbe, data_dir)
    }
}

#[cfg(test)]
mod tests {
    // `use super::*` 会随下面那条 macOS-only 用例一起变成 non-macOS 上的 unused import
    // (clippy `-D warnings`)。只在真正需要的 cfg 下引入。
    #[cfg(any(windows, target_os = "macos"))]
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_windows_path_becomes_the_root_getdrivetype_needs() {
        assert_eq!(
            drive_root(Path::new(r"C:\Users\me\AppData\Roaming")),
            PathBuf::from(r"C:\")
        );
        assert_eq!(drive_root(Path::new(r"d:\data")), PathBuf::from(r"D:\"));
        // A UNC path is not a drive root: it is passed through and `GetDriveTypeW` answers
        // `DRIVE_NO_ROOT_DIR`, which is a refusal rather than a guess.
        assert_eq!(
            drive_root(Path::new(r"\\nas\share\x")),
            PathBuf::from(r"\\nas\share\x")
        );
    }

    // 这条钉的是「本机」——Linux runner 的 /tmp 卷被探成 `Unknown` ⇒ `VolumeUnverified`。
    // 名字里的 on_this_machine 就是它自己的适用范围(ADR-039 §4 S2)。
    #[cfg(target_os = "macos")]
    #[test]
    fn the_wired_probe_grants_the_writer_role_on_this_machine() {
        // "接不上就是红的": the point of the exercise is that `CoreContext::acquire` gets a
        // probe whose answer is eligible, so an unwired adapter cannot pass for a wired one.
        // 装配半边（sync-root 启发式）在 core `location_probe` 有同形用例；这里钉的是
        // **宿主接的那条线**——windows 上它比 core 版多一张 ACE 回执。
        let location = PlatformLocationProbe.probe(&std::env::temp_dir());
        assert_eq!(
            location.writer_eligible(),
            Ok(()),
            "probe answered {:?}",
            location
        );
    }
}
