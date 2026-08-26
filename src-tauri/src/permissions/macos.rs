//! macOS TCC 权限的检测与请求,全部走公开 C API,不引入 objc 绑定 crate。
//!
//! 检测口径:只回答"现在有没有",不区分"被拒"与"没问过"——对用户来说都是同一个
//! 动作(去授权)。查询接口本身都不弹框,唯一会弹框的是目录类权限(枚举受保护目录
//! 会触发系统询问),所以它们标了 `probe_prompts`,由 `mod.rs` 决定何时探测。
//!
//! 注意:TCC 判定按**签名后的 app bundle** 记账。`tauri dev` 跑的是未打包二进制,
//! 授权结果会记在终端/IDE 身上,与正式安装的 Aeroric 不共享。

use std::ffi::c_void;
use std::path::PathBuf;
use std::process::Command;

use super::{PermissionDescriptor, PermissionProbe, PermissionStatus};

const SCREEN_RECORDING: &str = "screen-recording";
const ACCESSIBILITY: &str = "accessibility";
const INPUT_MONITORING: &str = "input-monitoring";
const AUTOMATION: &str = "automation";
const FULL_DISK_ACCESS: &str = "full-disk-access";
const MICROPHONE: &str = "microphone";
const CAMERA: &str = "camera";
const LOCAL_NETWORK: &str = "local-network";
const FOLDER_DESKTOP: &str = "folder-desktop";
const FOLDER_DOCUMENTS: &str = "folder-documents";
const FOLDER_DOWNLOADS: &str = "folder-downloads";

pub(crate) const SUPPORTED: bool = true;

/// 顺序即界面顺序:先放 agent 终端最常撞上的几项。
pub(crate) const DESCRIPTORS: &[PermissionDescriptor] = &[
    PermissionDescriptor {
        id: SCREEN_RECORDING,
        can_request_in_app: true,
        can_open_settings: true,
        needs_restart: true,
        probe_prompts: false,
    },
    PermissionDescriptor {
        id: ACCESSIBILITY,
        can_request_in_app: true,
        can_open_settings: true,
        needs_restart: true,
        probe_prompts: false,
    },
    PermissionDescriptor {
        id: INPUT_MONITORING,
        can_request_in_app: true,
        can_open_settings: true,
        needs_restart: true,
        probe_prompts: false,
    },
    PermissionDescriptor {
        id: AUTOMATION,
        can_request_in_app: true,
        can_open_settings: true,
        needs_restart: false,
        probe_prompts: false,
    },
    PermissionDescriptor {
        id: FULL_DISK_ACCESS,
        can_request_in_app: false,
        can_open_settings: true,
        needs_restart: true,
        probe_prompts: false,
    },
    PermissionDescriptor {
        id: MICROPHONE,
        can_request_in_app: false,
        can_open_settings: true,
        needs_restart: false,
        probe_prompts: false,
    },
    PermissionDescriptor {
        id: CAMERA,
        can_request_in_app: false,
        can_open_settings: true,
        needs_restart: false,
        probe_prompts: false,
    },
    PermissionDescriptor {
        id: LOCAL_NETWORK,
        can_request_in_app: false,
        can_open_settings: true,
        needs_restart: true,
        probe_prompts: false,
    },
    PermissionDescriptor {
        id: FOLDER_DESKTOP,
        can_request_in_app: true,
        can_open_settings: true,
        needs_restart: false,
        probe_prompts: true,
    },
    PermissionDescriptor {
        id: FOLDER_DOCUMENTS,
        can_request_in_app: true,
        can_open_settings: true,
        needs_restart: false,
        probe_prompts: true,
    },
    PermissionDescriptor {
        id: FOLDER_DOWNLOADS,
        can_request_in_app: true,
        can_open_settings: true,
        needs_restart: false,
        probe_prompts: true,
    },
];

// ── 原生接口声明 ─────────────────────────────────────────────────────────────

type CFTypeRef = *const c_void;
type Boolean = u8;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// 10.15+。只读查询,不弹框。
    fn CGPreflightScreenCaptureAccess() -> bool;
    /// 首次调用会弹一次系统询问;之前拒绝过则直接返回 false(不再弹)。
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> Boolean;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> Boolean;
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
}

const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    /// 10.15+。返回 granted / denied / unknown(= 没问过),不弹框。
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDRequestAccess(request_type: u32) -> bool;
}

/// AppleEvents 的目标地址描述符。
#[repr(C)]
struct AEDesc {
    descriptor_type: u32,
    data_handle: *mut c_void,
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn AECreateDesc(
        type_code: u32,
        data_ptr: *const c_void,
        data_size: isize,
        result: *mut AEDesc,
    ) -> i16;
    fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
    /// 10.14+。`ask_user_if_needed = false` 时纯查询,不弹框。
    fn AEDeterminePermissionToAutomateTarget(
        target: *const AEDesc,
        event_class: u32,
        event_id: u32,
        ask_user_if_needed: bool,
    ) -> i32;
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeAudio: *const c_void;
    static AVMediaTypeVideo: *const c_void;
}

#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn objc_getClass(name: *const std::ffi::c_char) -> *const c_void;
    fn sel_registerName(name: *const std::ffi::c_char) -> *const c_void;
    fn objc_msgSend();
}

const TYPE_APPLICATION_BUNDLE_ID: u32 = four_cc(*b"bund");
const TYPE_WILDCARD: u32 = four_cc(*b"****");
const NO_ERR: i32 = 0;
/// 用户明确拒绝过这对 (client, target)。
const ERR_AE_EVENT_NOT_PERMITTED: i32 = -1743;
/// 还没问过用户。
const ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT: i32 = -1744;
/// 目标进程没在运行——查不出结果,与授权状态无关。
const ERR_PROC_NOT_FOUND: i32 = -600;

const fn four_cc(code: [u8; 4]) -> u32 {
    u32::from_be_bytes(code)
}

// ── 逐项检测 ─────────────────────────────────────────────────────────────────

pub(crate) fn check(id: &str) -> PermissionProbe {
    match id {
        SCREEN_RECORDING => {
            PermissionProbe::plain(bool_status(unsafe { CGPreflightScreenCaptureAccess() }))
        }
        ACCESSIBILITY => PermissionProbe::plain(bool_status(unsafe { AXIsProcessTrusted() } != 0)),
        INPUT_MONITORING => PermissionProbe::plain(input_monitoring_status()),
        AUTOMATION => automation_probe(false),
        FULL_DISK_ACCESS => PermissionProbe::plain(full_disk_access_status()),
        MICROPHONE => media_probe(unsafe { AVMediaTypeAudio }),
        CAMERA => media_probe(unsafe { AVMediaTypeVideo }),
        // 本地网络没有公开查询接口(macOS 15 起才有这个开关),只能给设置入口。
        LOCAL_NETWORK => PermissionProbe::plain(PermissionStatus::Unknown),
        FOLDER_DESKTOP => folder_probe("Desktop"),
        FOLDER_DOCUMENTS => folder_probe("Documents"),
        FOLDER_DOWNLOADS => folder_probe("Downloads"),
        _ => PermissionProbe::plain(PermissionStatus::Unknown),
    }
}

fn bool_status(granted: bool) -> PermissionStatus {
    if granted {
        PermissionStatus::Granted
    } else {
        PermissionStatus::NotGranted
    }
}

fn input_monitoring_status() -> PermissionStatus {
    match unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) } {
        K_IOHID_ACCESS_TYPE_GRANTED => PermissionStatus::Granted,
        // denied(1) 与 unknown(2,= 没问过)对用户都是同一个动作:去授权。
        _ => PermissionStatus::NotGranted,
    }
}

/// 自动化权限按 (调用方, 目标 app) 成对记账,这里以 System Events 为代表目标——
/// agent 跑 AppleScript 控制 UI 时几乎都经过它。
fn automation_probe(ask_user: bool) -> PermissionProbe {
    let target = b"com.apple.systemevents";
    let mut desc = AEDesc {
        descriptor_type: 0,
        data_handle: std::ptr::null_mut(),
    };
    let created = unsafe {
        AECreateDesc(
            TYPE_APPLICATION_BUNDLE_ID,
            target.as_ptr().cast(),
            target.len() as isize,
            &mut desc,
        )
    };
    if created != 0 {
        return PermissionProbe::unknown(format!("AECreateDesc failed ({created})"));
    }
    let status = unsafe {
        AEDeterminePermissionToAutomateTarget(&desc, TYPE_WILDCARD, TYPE_WILDCARD, ask_user)
    };
    unsafe { AEDisposeDesc(&mut desc) };

    match status {
        NO_ERR => PermissionProbe::plain(PermissionStatus::Granted),
        ERR_AE_EVENT_NOT_PERMITTED | ERR_AE_EVENT_WOULD_REQUIRE_USER_CONSENT => {
            PermissionProbe::plain(PermissionStatus::NotGranted)
        }
        // System Events 没在跑就查不出来,报 Unknown 而不是假装未授权。
        ERR_PROC_NOT_FOUND => PermissionProbe::unknown("System Events is not running"),
        other => PermissionProbe::unknown(format!("AppleEvents status {other}")),
    }
}

/// 完全磁盘访问没有查询 API。读 TCC 自己的数据库是社区通用探针:有 FDA 才能打开它,
/// 且这次读取不会新建任何 TCC 记录、也不弹框。
fn full_disk_access_status() -> PermissionStatus {
    match std::fs::File::open("/Library/Application Support/com.apple.TCC/TCC.db") {
        Ok(_) => PermissionStatus::Granted,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            PermissionStatus::NotGranted
        }
        // 文件不存在等其他情况说明探针本身不成立,不下结论。
        Err(_) => PermissionStatus::Unknown,
    }
}

fn media_probe(media_type: *const c_void) -> PermissionProbe {
    // AVAuthorizationStatus: 0 notDetermined / 1 restricted / 2 denied / 3 authorized
    match media_authorization_status(media_type) {
        Some(3) => PermissionProbe::plain(PermissionStatus::Granted),
        Some(_) => PermissionProbe::plain(PermissionStatus::NotGranted),
        None => PermissionProbe::unknown("AVCaptureDevice is unavailable"),
    }
}

fn media_authorization_status(media_type: *const c_void) -> Option<i64> {
    if media_type.is_null() {
        return None;
    }
    let class = unsafe { objc_getClass(c"AVCaptureDevice".as_ptr()) };
    if class.is_null() {
        return None;
    }
    let selector = unsafe { sel_registerName(c"authorizationStatusForMediaType:".as_ptr()) };
    if selector.is_null() {
        return None;
    }
    // objc_msgSend 的真实签名由调用点决定,必须先转成具体函数指针再调。
    let send: unsafe extern "C" fn(*const c_void, *const c_void, *const c_void) -> i64 =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    Some(unsafe { send(class, selector, media_type) })
}

/// 目录类权限:枚举受保护目录会触发系统询问,所以只在用户显式请求时调用。
fn folder_probe(name: &str) -> PermissionProbe {
    let Some(path) = home_subdirectory(name) else {
        return PermissionProbe::unknown("Home directory is unknown");
    };
    if !path.is_dir() {
        return PermissionProbe::unknown(format!("{} does not exist", path.display()));
    }
    match std::fs::read_dir(&path) {
        Ok(mut entries) => match entries.next() {
            Some(Err(error)) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                PermissionProbe::plain(PermissionStatus::NotGranted)
            }
            _ => PermissionProbe::plain(PermissionStatus::Granted),
        },
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            PermissionProbe::plain(PermissionStatus::NotGranted)
        }
        Err(error) => PermissionProbe::unknown(error.to_string()),
    }
}

fn home_subdirectory(name: &str) -> Option<PathBuf> {
    crate::platform::home_dir().map(|home| home.join(name))
}

// ── 请求 ─────────────────────────────────────────────────────────────────────

pub(crate) fn request(id: &str) -> PermissionProbe {
    match id {
        SCREEN_RECORDING => {
            // 只在"从未询问"时弹框;拒绝过则原样返回 false,此时用户得走系统设置。
            PermissionProbe::plain(bool_status(unsafe { CGRequestScreenCaptureAccess() }))
        }
        ACCESSIBILITY => PermissionProbe::plain(bool_status(request_accessibility())),
        INPUT_MONITORING => {
            let granted = unsafe { IOHIDRequestAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) };
            if granted {
                PermissionProbe::plain(PermissionStatus::Granted)
            } else {
                PermissionProbe::plain(input_monitoring_status())
            }
        }
        AUTOMATION => automation_probe(true),
        FOLDER_DESKTOP => folder_probe("Desktop"),
        FOLDER_DOCUMENTS => folder_probe("Documents"),
        FOLDER_DOWNLOADS => folder_probe("Downloads"),
        // 其余项目 `can_request_in_app: false`,不会走到这里。
        other => check(other),
    }
}

/// 带 prompt 选项的辅助功能检查:未授权时弹出"打开系统设置"的系统提示。
fn request_accessibility() -> bool {
    let key = unsafe { kAXTrustedCheckOptionPrompt };
    let value = unsafe { kCFBooleanTrue };
    if key.is_null() {
        return unsafe { AXIsProcessTrustedWithOptions(std::ptr::null()) } != 0;
    }
    let options = unsafe {
        CFDictionaryCreate(
            std::ptr::null(),
            &key,
            &value,
            1,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    let trusted = unsafe { AXIsProcessTrustedWithOptions(options) } != 0;
    if !options.is_null() {
        unsafe { CFRelease(options) };
    }
    trusted
}

// ── 系统设置跳转 ─────────────────────────────────────────────────────────────

/// 隐私与安全性各面板的 anchor。`x-apple.systempreferences:` 在 Ventura 之后仍然
/// 是打开具体面板的唯一稳定入口。
fn settings_anchor(id: &str) -> Option<&'static str> {
    Some(match id {
        SCREEN_RECORDING => "Privacy_ScreenCapture",
        ACCESSIBILITY => "Privacy_Accessibility",
        INPUT_MONITORING => "Privacy_ListenEvent",
        AUTOMATION => "Privacy_Automation",
        FULL_DISK_ACCESS => "Privacy_AllFiles",
        MICROPHONE => "Privacy_Microphone",
        CAMERA => "Privacy_Camera",
        LOCAL_NETWORK => "Privacy_LocalNetwork",
        FOLDER_DESKTOP => "Privacy_DesktopFolder",
        FOLDER_DOCUMENTS => "Privacy_DocumentsFolder",
        FOLDER_DOWNLOADS => "Privacy_DownloadsFolder",
        _ => return None,
    })
}

pub(crate) fn open_settings(id: &str) -> Result<(), String> {
    let anchor = settings_anchor(id).ok_or_else(|| format!("{id} has no settings pane to open"))?;
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    let mut command = Command::new("open");
    command.arg(&url);
    crate::subprocess::configure_background_command(&mut command);
    let status = command
        .status()
        .map_err(|error| format!("Failed to open System Settings: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "System Settings did not open (exit {:?})",
            status.code()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_descriptor_has_a_settings_anchor() {
        for descriptor in DESCRIPTORS {
            assert!(
                settings_anchor(descriptor.id).is_some(),
                "{} has no settings anchor",
                descriptor.id
            );
        }
    }

    #[test]
    fn four_cc_packs_big_endian() {
        assert_eq!(TYPE_APPLICATION_BUNDLE_ID, 0x62756E64);
        assert_eq!(TYPE_WILDCARD, 0x2A2A2A2A);
    }

    #[test]
    fn unknown_ids_report_unknown_without_panicking() {
        let probe = check("not-a-real-permission");
        assert_eq!(probe.status, PermissionStatus::Unknown);
        assert!(settings_anchor("not-a-real-permission").is_none());
        assert!(open_settings("not-a-real-permission").is_err());
    }

    /// 只读查询必须能在测试进程里跑完(不弹框、不挂起)。授权与否都算通过。
    #[test]
    fn read_only_checks_complete() {
        for id in [
            SCREEN_RECORDING,
            ACCESSIBILITY,
            INPUT_MONITORING,
            AUTOMATION,
            FULL_DISK_ACCESS,
            MICROPHONE,
            CAMERA,
            LOCAL_NETWORK,
        ] {
            let probe = check(id);
            if probe.status == PermissionStatus::Unknown {
                // Unknown 必须带上原因,否则界面只能显示一个没有下文的"未知"。
                // 本地网络是唯一例外:系统根本没有查询接口。
                assert!(
                    probe.detail.is_some() || id == LOCAL_NETWORK,
                    "{id} reported Unknown without a reason"
                );
            }
        }
    }

    #[test]
    fn media_status_maps_authorized_only() {
        // 空指针走不进 objc,必须报 Unknown 而不是崩。
        let probe = media_probe(std::ptr::null());
        assert_eq!(probe.status, PermissionStatus::Unknown);
    }

    #[test]
    fn missing_folder_is_unknown_not_denied() {
        let probe = folder_probe("Aeroric-does-not-exist-9f2b");
        assert_eq!(probe.status, PermissionStatus::Unknown);
        assert!(probe.detail.is_some());
    }
}
