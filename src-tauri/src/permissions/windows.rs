//! Windows:各项隐私开关记在注册表的 CapabilityAccessManager\ConsentStore 下,可读。
//!
//! 和 macOS 的差别有两处,决定了这里能做什么:
//!
//! - **没有应用内授权流程**。UWP / 打包应用调 WinRT 的 `RequestAccessAsync` 会弹一次
//!   系统框;本应用是非打包的 Win32 程序,系统不会为它弹框,只能引导用户去
//!   「设置 > 隐私和安全性」。所以所有项目都是 `can_request_in_app: false`。
//! - **没有进程内缓存那回事**。ConsentStore 是注册表,每次读都是当前值,所以
//!   `FRESH_PROBE_HELPS = false`——不必为了拿新鲜状态去 spawn 子进程。
//!
//! 非打包应用在 ConsentStore 里以**可执行文件路径**为键(反斜杠换成 `#`)。这也意味着
//! 移动或重装到别的路径后,先前的授权不再命中——`identity()` 如实报告这个主体。

use std::path::PathBuf;

use super::{AppIdentity, PermissionDescriptor, PermissionProbe, PermissionStatus};

const SCREEN_RECORDING: &str = "screen-recording";
const MICROPHONE: &str = "microphone";
const CAMERA: &str = "camera";
const FULL_DISK_ACCESS: &str = "full-disk-access";

pub(crate) const SUPPORTED: bool = true;

/// 注册表每次读都是当前值,没有进程内缓存要绕开。
pub(crate) const FRESH_PROBE_HELPS: bool = false;

pub(crate) const DESCRIPTORS: &[PermissionDescriptor] = &[
    // 改完开关后需要重新打开设备/捕获会话,重启应用是最省事的说法。
    PermissionDescriptor::settings_only(SCREEN_RECORDING, true),
    PermissionDescriptor::settings_only(MICROPHONE, false),
    PermissionDescriptor::settings_only(CAMERA, false),
    PermissionDescriptor::settings_only(FULL_DISK_ACCESS, true),
];

/// ConsentStore 里的能力名。与前端 id 不同,这是 Windows 自己的一套命名。
fn capability(id: &str) -> Option<&'static str> {
    Some(match id {
        // Windows 11 的「应用可以录制屏幕」总开关。
        SCREEN_RECORDING => "graphicsCaptureProgrammatic",
        MICROPHONE => "microphone",
        CAMERA => "webcam",
        FULL_DISK_ACCESS => "broadFileSystemAccess",
        _ => return None,
    })
}

fn settings_page(id: &str) -> Option<&'static str> {
    Some(match id {
        SCREEN_RECORDING => "ms-settings:privacy-graphicscaptureprogrammatic",
        MICROPHONE => "ms-settings:privacy-microphone",
        CAMERA => "ms-settings:privacy-webcam",
        FULL_DISK_ACCESS => "ms-settings:privacy-broadfilesystemaccess",
        _ => return None,
    })
}

// ── 注册表读取 ───────────────────────────────────────────────────────────────

const CONSENT_STORE: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 读一个 `REG_SZ`。键或值不存在时返回 None——调用方据此区分"没记过"和"记着拒绝"。
fn read_string(subkey: &str, value_name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, RegGetValueW, RRF_RT_REG_SZ,
    };

    let subkey = wide(subkey);
    let name = wide(value_name);
    let mut size: u32 = 0;
    // 先问长度:0 长度调用只填 size,不写 buffer。
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status != 0 || size == 0 {
        return None;
    }
    let mut buffer = vec![0_u16; size as usize / 2 + 1];
    let mut byte_len = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut byte_len,
        )
    };
    if status != 0 {
        return None;
    }
    let chars = (byte_len as usize / 2).min(buffer.len());
    let text: String = String::from_utf16_lossy(&buffer[..chars]);
    Some(text.trim_end_matches('\0').to_string())
}

/// 非打包应用的键名就是可执行文件路径,反斜杠换成 `#`。
fn app_key() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.to_string_lossy().replace('\\', "#"))
}

pub(crate) fn check(id: &str) -> PermissionProbe {
    let Some(capability) = capability(id) else {
        return PermissionProbe::plain(PermissionStatus::Unknown);
    };
    let global = read_string(&format!(r"{CONSENT_STORE}\{capability}"), "Value");
    // 全局关掉的话逐应用开关不起作用,直接是未授权。
    if global.as_deref() == Some("Deny") {
        return PermissionProbe::explained(
            PermissionStatus::NotGranted,
            format!("{capability} is turned off for all apps"),
        );
    }
    let per_app = app_key().and_then(|key| {
        read_string(
            &format!(r"{CONSENT_STORE}\{capability}\NonPackaged\{key}"),
            "Value",
        )
    });
    match (per_app.as_deref(), global.as_deref()) {
        (Some("Allow"), _) => PermissionProbe::plain(PermissionStatus::Granted),
        (Some("Deny"), _) => PermissionProbe::plain(PermissionStatus::NotGranted),
        // 没有逐应用记录:跟随全局开关。Allow 时系统默认放行。
        (_, Some("Allow")) => PermissionProbe::explained(
            PermissionStatus::Granted,
            format!("{capability} follows the system-wide setting"),
        ),
        // 键根本不存在:这个 Windows 版本没有这项开关,别下结论。
        _ => PermissionProbe::unknown(format!("{capability} is not present in ConsentStore")),
    }
}

/// Windows 不为非打包应用弹授权框,所以"请求"只能是打开设置再读一遍。
pub(crate) fn request(id: &str) -> PermissionProbe {
    if let Err(error) = open_settings(id) {
        return PermissionProbe::unknown(error);
    }
    check(id)
}

pub(crate) fn open_settings(id: &str) -> Result<(), String> {
    let page = settings_page(id).ok_or_else(|| format!("{id} has no settings page to open"))?;
    // `explorer.exe <uri>` 是打开 ms-settings: 的标准方式;它对 URI 一律返回非零退出码,
    // 所以只看 spawn 成功与否。
    let mut command = std::process::Command::new("explorer.exe");
    command.arg(page);
    crate::subprocess::configure_background_command(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Failed to open Windows Settings: {error}"))
}

/// Windows 没有可清除的"授权记录":开关就是注册表里的值,由用户在设置里改。
pub(crate) fn reset(id: &str) -> Result<(), String> {
    Err(format!("{id} cannot be reset on Windows"))
}

/// 非打包应用在 ConsentStore 里以 exe 路径为键,所以换了安装路径授权就不再命中。
pub(crate) fn identity() -> AppIdentity {
    let exe: Option<PathBuf> = std::env::current_exe().ok();
    let subject = exe
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    AppIdentity::not_applicable(subject)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_descriptor_maps_to_a_capability_and_a_settings_page() {
        for descriptor in DESCRIPTORS {
            assert!(
                capability(descriptor.id).is_some(),
                "{} has no ConsentStore capability",
                descriptor.id
            );
            assert!(
                settings_page(descriptor.id).is_some(),
                "{} has no settings page",
                descriptor.id
            );
        }
    }

    #[test]
    fn unknown_ids_report_unknown_without_panicking() {
        assert_eq!(
            check("not-a-real-permission").status,
            PermissionStatus::Unknown
        );
        assert!(open_settings("not-a-real-permission").is_err());
        assert!(reset(MICROPHONE).is_err());
    }

    #[test]
    fn app_key_escapes_backslashes() {
        let key = app_key().expect("current exe is known");
        assert!(!key.contains('\\'), "unescaped path: {key}");
        assert!(key.contains('#'), "path was not escaped: {key}");
    }

    /// 只读检测必须能跑完,不弹框不挂起。授权与否都算通过。
    #[test]
    fn read_only_checks_complete() {
        for descriptor in DESCRIPTORS {
            let probe = check(descriptor.id);
            if probe.status == PermissionStatus::Unknown {
                assert!(
                    probe.detail.is_some(),
                    "{} reported Unknown without a reason",
                    descriptor.id
                );
            }
        }
    }

    #[test]
    fn missing_registry_value_is_none() {
        assert!(read_string(r"Software\Aeroric-does-not-exist-9f2b", "Value").is_none());
    }

    #[test]
    fn identity_reports_the_executable_path() {
        let identity = identity();
        assert_eq!(identity.signature, "not-applicable");
        assert!(identity.stable_across_updates);
        assert!(identity.warning.is_none());
        assert!(!identity.subject.is_empty());
    }
}
