//! Windows / Linux:没有 macOS TCC 那样的逐项前置授权模型。屏幕录制、辅助功能、
//! 目录访问在这些平台上由进程本身的权限(和 Linux 桌面的 portal 会话)决定,不存在
//! "应用级开关"可查可改,所以描述表为空,报告以 `supported: false` 返回。

use super::{PermissionDescriptor, PermissionProbe, PermissionStatus};

pub(crate) const SUPPORTED: bool = false;

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
