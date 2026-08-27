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

/// macOS 把 TCC 判定缓存在进程里,所以只有换进程才能问到系统当前记的账。
pub(crate) const FRESH_PROBE_HELPS: bool = true;

/// 除了没有 TCC 服务名的本地网络,每一项都能 `tccutil reset`,面板也一律能跳设置。
/// 所以这里只需要描述"能不能在应用内请求"「改完要不要重启」「检测会不会弹框」。
const fn tcc(
    id: &'static str,
    can_request_in_app: bool,
    needs_restart: bool,
    probe_prompts: bool,
) -> PermissionDescriptor {
    PermissionDescriptor {
        id,
        can_request_in_app,
        can_open_settings: true,
        can_reset: true,
        needs_restart,
        probe_prompts,
        report_only: false,
    }
}

/// 顺序即界面顺序:先放 agent 终端最常撞上的几项。
pub(crate) const DESCRIPTORS: &[PermissionDescriptor] = &[
    tcc(SCREEN_RECORDING, true, true, false),
    tcc(ACCESSIBILITY, true, true, false),
    tcc(INPUT_MONITORING, true, true, false),
    tcc(AUTOMATION, true, false, false),
    tcc(FULL_DISK_ACCESS, false, true, false),
    tcc(MICROPHONE, false, false, false),
    tcc(CAMERA, false, false, false),
    // 本地网络没有 TCC 服务名,`tccutil reset` 无从下手。
    PermissionDescriptor {
        can_reset: false,
        ..tcc(LOCAL_NETWORK, false, true, false)
    },
    tcc(FOLDER_DESKTOP, true, false, true),
    tcc(FOLDER_DOCUMENTS, true, false, true),
    tcc(FOLDER_DOWNLOADS, true, false, true),
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

// ── 代码签名身份 ─────────────────────────────────────────────────────────────
//
// TCC 按「签名身份」记账,不只按 bundle id。ad-hoc 签名的 designated requirement 绑
// cdhash,每次重新构建都变:系统设置里开关看着还在,实际已对不上 —— 这就是"设置里
// 显示已开放、应用报未获取"的主因,必须能检测出来并告诉用户。

const K_SEC_CS_SIGNING_INFORMATION: u32 = 1 << 1;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CF_NUMBER_SINT64_TYPE: isize = 4;
/// `CS_ADHOC`:没有真实签名主体。
const CS_ADHOC: i64 = 0x0000_0002;
/// `CS_LINKER_SIGNED`:链接器自动加的那份签名,连 Info.plist 都没覆盖。
const CS_LINKER_SIGNED: i64 = 0x0002_0000;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFBundleGetMainBundle() -> CFTypeRef;
    fn CFBundleGetIdentifier(bundle: CFTypeRef) -> CFTypeRef;
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        c_str: *const std::ffi::c_char,
        encoding: u32,
    ) -> CFTypeRef;
    fn CFStringGetCString(
        string: CFTypeRef,
        buffer: *mut std::ffi::c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> Boolean;
    fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, number_type: isize, value_ptr: *mut c_void) -> Boolean;
}

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecCodeCopySelf(flags: u32, code: *mut CFTypeRef) -> i32;
    fn SecCodeCopySigningInformation(
        code: CFTypeRef,
        flags: u32,
        information: *mut CFTypeRef,
    ) -> i32;
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

// ── 身份 ─────────────────────────────────────────────────────────────────────

fn cf_string_to_rust(value: CFTypeRef) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut buffer = [0_i8; 512];
    let ok = unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8(bytes).ok()
}

/// 以 `CFStringRef` 取字典键。这些键的值就是文档写明的字面量("identifier" 等),
/// 直接建字符串比把一堆 extern static 拉进来更省事。
fn dictionary_string(dict: CFTypeRef, key: &std::ffi::CStr) -> Option<String> {
    let key_ref = unsafe {
        CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    };
    if key_ref.is_null() {
        return None;
    }
    let value = unsafe { CFDictionaryGetValue(dict, key_ref) };
    let result = cf_string_to_rust(value);
    unsafe { CFRelease(key_ref) };
    result
}

fn dictionary_i64(dict: CFTypeRef, key: &std::ffi::CStr) -> Option<i64> {
    let key_ref = unsafe {
        CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    };
    if key_ref.is_null() {
        return None;
    }
    let value = unsafe { CFDictionaryGetValue(dict, key_ref) };
    let mut number: i64 = 0;
    let ok = if value.is_null() {
        0
    } else {
        unsafe {
            CFNumberGetValue(
                value,
                K_CF_NUMBER_SINT64_TYPE,
                (&mut number as *mut i64).cast(),
            )
        }
    };
    unsafe { CFRelease(key_ref) };
    (ok != 0).then_some(number)
}

/// 只有以 app bundle 运行时才有 bundle id。`tauri dev` 跑的裸二进制拿不到,而那正是
/// "授权记在终端/IDE 身上"的情形,值得区分。
fn bundle_identifier() -> Option<String> {
    let bundle = unsafe { CFBundleGetMainBundle() };
    if bundle.is_null() {
        return None;
    }
    cf_string_to_rust(unsafe { CFBundleGetIdentifier(bundle) })
}

/// 本进程自己的签名信息:(签名 identifier, csflags, team id)。
fn signing_information() -> Option<(Option<String>, i64, Option<String>)> {
    let mut code: CFTypeRef = std::ptr::null();
    if unsafe { SecCodeCopySelf(0, &mut code) } != 0 || code.is_null() {
        return None;
    }
    let mut info: CFTypeRef = std::ptr::null();
    let status =
        unsafe { SecCodeCopySigningInformation(code, K_SEC_CS_SIGNING_INFORMATION, &mut info) };
    unsafe { CFRelease(code) };
    if status != 0 || info.is_null() {
        return None;
    }
    let identifier = dictionary_string(info, c"identifier");
    let flags = dictionary_i64(info, c"flags").unwrap_or(0);
    let team = dictionary_string(info, c"teamid");
    unsafe { CFRelease(info) };
    Some((identifier, flags, team))
}

/// 把 csflags 翻成一个人能读的签名种类,并回答"授权能不能跨升级保留"。
fn classify_signature(flags: i64, team: Option<&str>) -> (&'static str, bool) {
    if flags & CS_LINKER_SIGNED != 0 {
        // 链接器自动加的签名:连 Info.plist 都没覆盖,requirement 就是一个裸 cdhash。
        ("linker-signed", false)
    } else if flags & CS_ADHOC != 0 {
        ("adhoc", false)
    } else if team.is_some_and(|value| !value.is_empty()) {
        ("developer-id", true)
    } else {
        ("signed", true)
    }
}

pub(crate) fn identity() -> super::AppIdentity {
    let bundle = bundle_identifier();
    let info = signing_information();
    let (signing_id, signature, stable) = match &info {
        Some((identifier, flags, team)) => {
            let (kind, stable) = classify_signature(*flags, team.as_deref());
            (identifier.clone(), kind, stable)
        }
        None => (None, "unsigned", false),
    };
    let subject = bundle
        .clone()
        .or_else(|| signing_id.clone())
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|path| path.display().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    // 没跑在 bundle 里是更要紧的解释(dev 构建的授权根本不记在本应用名下),
    // 所以它优先于签名不稳定。
    let warning = if bundle.is_none() {
        Some(super::IdentityWarning::NotBundled)
    } else if !stable {
        Some(super::IdentityWarning::UnstableSignature)
    } else {
        None
    };

    super::AppIdentity {
        subject,
        signature: signature.to_string(),
        stable_across_updates: stable,
        warning,
    }
}

// ── 重置授权 ─────────────────────────────────────────────────────────────────

/// `tccutil` 的服务名。与 `settings_anchor` 不同,这是另一套命名。
fn tcc_service(id: &str) -> Option<&'static str> {
    Some(match id {
        SCREEN_RECORDING => "ScreenCapture",
        ACCESSIBILITY => "Accessibility",
        INPUT_MONITORING => "ListenEvent",
        AUTOMATION => "AppleEvents",
        FULL_DISK_ACCESS => "SystemPolicyAllFiles",
        MICROPHONE => "Microphone",
        CAMERA => "Camera",
        FOLDER_DESKTOP => "SystemPolicyDesktopFolder",
        FOLDER_DOCUMENTS => "SystemPolicyDocumentsFolder",
        FOLDER_DOWNLOADS => "SystemPolicyDownloadsFolder",
        // 本地网络不在 TCC 服务表里。
        _ => return None,
    })
}

/// 清掉本应用该项的 TCC 记录,让下一次请求重新记一条(绑当前 cdhash)。
///
/// 这是 ad-hoc 签名升级后唯一的修法:旧记录绑的是上一版的 cdhash,系统设置里再点
/// 开关也不会让它重新对上。
pub(crate) fn reset(id: &str) -> Result<(), String> {
    let service = tcc_service(id).ok_or_else(|| format!("{id} has no TCC service to reset"))?;
    let bundle = bundle_identifier().ok_or_else(|| {
        "Not running from an app bundle, so there is no TCC record under this app's name"
            .to_string()
    })?;
    let mut command = Command::new("tccutil");
    command.args(["reset", service, &bundle]);
    crate::subprocess::configure_background_command(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("Failed to run tccutil: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!(
            "tccutil reset {service} failed (exit {:?})",
            output.status.code()
        )
    } else {
        stderr
    })
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

    /// 每个可重置的项目都得有 TCC 服务名,否则按钮点下去必然报错。
    #[test]
    fn resettable_descriptors_map_to_a_tcc_service() {
        for descriptor in DESCRIPTORS {
            assert_eq!(
                descriptor.can_reset,
                tcc_service(descriptor.id).is_some(),
                "{} disagrees about being resettable",
                descriptor.id
            );
        }
    }

    /// ad-hoc / linker 签名必须判为"授权不跨升级保留"——这条判断是给用户的解释所依赖的。
    #[test]
    fn adhoc_and_linker_signatures_are_unstable() {
        assert_eq!(classify_signature(CS_ADHOC, None), ("adhoc", false));
        assert_eq!(
            classify_signature(CS_ADHOC | CS_LINKER_SIGNED, None),
            ("linker-signed", false)
        );
        assert_eq!(
            classify_signature(0, Some("ABCDE12345")),
            ("developer-id", true)
        );
        // 有真实签名但没 team id(自签 / 企业签):当作稳定,requirement 不绑 cdhash。
        assert_eq!(classify_signature(0, None), ("signed", true));
        assert_eq!(classify_signature(0, Some("")), ("signed", true));
    }

    /// 身份必须能在测试进程里查出来(裸测试二进制是 linker-signed,不是 bundle)。
    #[test]
    fn identity_describes_the_running_code() {
        let identity = identity();
        assert!(!identity.subject.is_empty());
        assert!(!identity.signature.is_empty());
        // 测试二进制不在 app bundle 里,必须报出来而不是假装一切正常。
        assert_eq!(
            identity.warning,
            Some(super::super::IdentityWarning::NotBundled)
        );
    }

    /// 不在 bundle 里跑时重置必须报错而不是对着空 bundle id 调 tccutil。
    #[test]
    fn reset_without_a_bundle_is_rejected() {
        assert!(reset(LOCAL_NETWORK).is_err());
        if bundle_identifier().is_none() {
            assert!(reset(SCREEN_RECORDING).is_err());
        }
    }
}
