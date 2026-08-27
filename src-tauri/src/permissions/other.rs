//! macOS / Windows / Linux 之外的平台(BSD 等):没有可枚举的应用级权限清单,
//! 描述表为空,报告以 `supported: false` 返回。
//!
//! 三个主流平台各有自己的实现:`macos.rs`(TCC)、`windows.rs`(ConsentStore)、
//! `linux.rs`(会话 / 用户组 / 沙箱,只报告)。

use super::{AppIdentity, PermissionDescriptor, PermissionProbe, PermissionStatus};

pub(crate) const SUPPORTED: bool = false;

pub(crate) const FRESH_PROBE_HELPS: bool = false;

pub(crate) const DESCRIPTORS: &[PermissionDescriptor] = &[];

pub(crate) fn check(_id: &str) -> PermissionProbe {
    PermissionProbe::plain(PermissionStatus::Unknown)
}

pub(crate) fn request(_id: &str) -> PermissionProbe {
    PermissionProbe::plain(PermissionStatus::Unknown)
}

pub(crate) fn open_settings(_id: &str) -> Result<(), String> {
    Err("This platform has no per-permission settings pane".to_string())
}

pub(crate) fn reset(_id: &str) -> Result<(), String> {
    Err("This platform has no authorization record to reset".to_string())
}

pub(crate) fn identity() -> AppIdentity {
    AppIdentity::not_applicable(
        std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
    )
}
