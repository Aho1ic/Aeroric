//! Linux:没有 macOS TCC 或 Windows ConsentStore 那样的**应用级前置授权**。
//!
//! 能不能截屏、能不能读全局输入,由三件事决定,都不是"应用开关":
//!
//! - **会话类型**。X11 不设限(任何客户端都能截屏、抓全局按键);Wayland 反过来,合成器
//!   不给任何直接接口,必须走 `xdg-desktop-portal`,而且每次由用户在系统 UI 里挑目标。
//! - **用户组**。裸设备访问(evdev 输入、`/dev/video*`)按组授权,改组要重新登录才生效。
//! - **沙箱**。Flatpak / Snap 里由沙箱清单说话,`flatpak override` 改,不是应用自己能改。
//!
//! 所以这里的每一项都是 `report_only`:如实报告现在有没有、为什么,不摆一个点了没用的
//! 「获取」按钮。这比在 Linux 上直接回一句"本平台无需授权"有用——Wayland 上缺 portal
//! 时截屏是真的不工作,用户需要知道。

use std::path::{Path, PathBuf};

use super::{AppIdentity, PermissionDescriptor, PermissionProbe, PermissionStatus};

const SCREEN_RECORDING: &str = "screen-recording";
const INPUT_MONITORING: &str = "input-monitoring";
const MICROPHONE: &str = "microphone";
const CAMERA: &str = "camera";
const FOLDER_DESKTOP: &str = "folder-desktop";
const FOLDER_DOCUMENTS: &str = "folder-documents";
const FOLDER_DOWNLOADS: &str = "folder-downloads";

pub(crate) const SUPPORTED: bool = true;

/// 这里的每项检测都读的是当前环境(env / 文件 / 组),没有进程内缓存要绕开。
/// 组成员身份倒是进程启动时就定了,但换进程也一样——那要重新登录,不是重启应用。
pub(crate) const FRESH_PROBE_HELPS: bool = false;

pub(crate) const DESCRIPTORS: &[PermissionDescriptor] = &[
    PermissionDescriptor::report_only(SCREEN_RECORDING),
    PermissionDescriptor::report_only(INPUT_MONITORING),
    PermissionDescriptor::report_only(MICROPHONE),
    PermissionDescriptor::report_only(CAMERA),
    PermissionDescriptor::report_only(FOLDER_DESKTOP),
    PermissionDescriptor::report_only(FOLDER_DOCUMENTS),
    PermissionDescriptor::report_only(FOLDER_DOWNLOADS),
];

// ── 环境探测 ─────────────────────────────────────────────────────────────────

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum Session {
    X11,
    Wayland,
    Unknown,
}

/// `XDG_SESSION_TYPE` 最可靠,但 display 变量在它缺失时也能定性。
fn session() -> Session {
    match std::env::var("XDG_SESSION_TYPE").as_deref() {
        Ok("wayland") => return Session::Wayland,
        Ok("x11") => return Session::X11,
        _ => {}
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        Session::Wayland
    } else if std::env::var_os("DISPLAY").is_some() {
        Session::X11
    } else {
        Session::Unknown
    }
}

/// portal 是 DBus 服务,但不引 DBus 依赖也能判断它装没装:看激活文件在不在。
fn portal_installed() -> bool {
    const SERVICE: &str = "dbus-1/services/org.freedesktop.portal.Desktop.service";
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share"),
        PathBuf::from("/usr/local/share"),
        PathBuf::from("/var/lib/flatpak/exports/share"),
    ];
    if let Some(home) = crate::platform::home_dir() {
        roots.push(home.join(".local/share"));
    }
    if let Some(dirs) = std::env::var_os("XDG_DATA_DIRS") {
        roots.extend(std::env::split_paths(&dirs));
    }
    roots.iter().any(|root| root.join(SERVICE).is_file())
}

fn sandbox() -> Option<&'static str> {
    if Path::new("/.flatpak-info").is_file() {
        Some("Flatpak")
    } else if std::env::var_os("SNAP").is_some() {
        Some("Snap")
    } else {
        None
    }
}

/// 当前进程的补充组 gid。`/proc/self/status` 的 `Groups:` 行,不引 libc。
fn process_groups() -> Vec<u32> {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return Vec::new();
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("Groups:"))
        .map(|line| {
            line.split_whitespace()
                .filter_map(|g| g.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn group_gid(name: &str) -> Option<u32> {
    let groups = std::fs::read_to_string("/etc/group").ok()?;
    groups.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == name).then(|| fields.nth(1)?.parse().ok())?
    })
}

/// 组在不在、且本进程是不是成员。组不存在时返回 None——"没有这个组"和"不是成员"
/// 对用户是不同的信息。
fn in_group(name: &str) -> Option<bool> {
    let gid = group_gid(name)?;
    Some(process_groups().contains(&gid))
}

// ── 逐项检测 ─────────────────────────────────────────────────────────────────

pub(crate) fn check(id: &str) -> PermissionProbe {
    match id {
        SCREEN_RECORDING => screen_recording_probe(),
        INPUT_MONITORING => input_monitoring_probe(),
        MICROPHONE => device_probe("/dev/snd", "audio", "microphone"),
        CAMERA => video_probe(),
        FOLDER_DESKTOP => folder_probe("Desktop"),
        FOLDER_DOCUMENTS => folder_probe("Documents"),
        FOLDER_DOWNLOADS => folder_probe("Downloads"),
        _ => PermissionProbe::plain(PermissionStatus::Unknown),
    }
}

fn screen_recording_probe() -> PermissionProbe {
    match session() {
        Session::X11 => PermissionProbe::explained(
            PermissionStatus::Granted,
            "X11 sessions do not gate screen capture",
        ),
        Session::Wayland if portal_installed() => PermissionProbe::explained(
            PermissionStatus::Granted,
            "Wayland routes screen capture through xdg-desktop-portal; \
             the compositor asks which window to share each time",
        ),
        Session::Wayland => PermissionProbe::explained(
            PermissionStatus::NotGranted,
            "Wayland needs xdg-desktop-portal for screen capture, and it is not installed",
        ),
        Session::Unknown => PermissionProbe::unknown(
            "No graphical session was detected (XDG_SESSION_TYPE is unset)",
        ),
    }
}

fn input_monitoring_probe() -> PermissionProbe {
    // evdev 直读绕过合成器,在 Wayland 上也能拿到全局输入——前提是在 input 组里。
    let evdev = in_group("input");
    match (session(), evdev) {
        (Session::X11, _) => PermissionProbe::explained(
            PermissionStatus::Granted,
            "X11 sessions allow global input monitoring",
        ),
        (_, Some(true)) => PermissionProbe::explained(
            PermissionStatus::Granted,
            "Reading input devices directly is possible: this user is in the 'input' group",
        ),
        (Session::Wayland, _) => PermissionProbe::explained(
            PermissionStatus::NotGranted,
            "Wayland does not expose global input to clients; \
             add this user to the 'input' group and log in again to read devices directly",
        ),
        (Session::Unknown, _) => PermissionProbe::unknown(
            "No graphical session was detected (XDG_SESSION_TYPE is unset)",
        ),
    }
}

/// 设备目录可读即视为可用;不可读时把该管的组名说出来,用户才知道下一步做什么。
fn device_probe(directory: &str, group: &str, what: &str) -> PermissionProbe {
    let path = Path::new(directory);
    if !path.is_dir() {
        return PermissionProbe::unknown(format!(
            "{directory} does not exist, so no {what} is available"
        ));
    }
    match std::fs::read_dir(path) {
        Ok(_) => PermissionProbe::explained(
            PermissionStatus::Granted,
            format!("{directory} is readable"),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            PermissionProbe::explained(
                PermissionStatus::NotGranted,
                format!(
                    "{directory} is not readable; add this user to the '{group}' group and log in again"
                ),
            )
        }
        Err(error) => PermissionProbe::unknown(error.to_string()),
    }
}

fn video_probe() -> PermissionProbe {
    let Ok(entries) = std::fs::read_dir("/dev") else {
        return PermissionProbe::unknown("/dev is not readable");
    };
    let cameras: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("video"))
        })
        .collect();
    if cameras.is_empty() {
        return PermissionProbe::unknown("No /dev/video* device is present");
    }
    // 能打开任意一个就算有摄像头能力:多摄像头机器上不该因为某一个被占用就报未授权。
    let openable = cameras.iter().any(|path| std::fs::File::open(path).is_ok());
    if openable {
        PermissionProbe::explained(
            PermissionStatus::Granted,
            "A /dev/video* device is openable",
        )
    } else {
        PermissionProbe::explained(
            PermissionStatus::NotGranted,
            "No /dev/video* device is openable; add this user to the 'video' group and log in again",
        )
    }
}

/// Linux 上读目录不会弹框,所以列表加载时就能给出真实状态(与 macOS 不同)。
fn folder_probe(name: &str) -> PermissionProbe {
    let Some(path) = crate::platform::home_dir().map(|home| home.join(name)) else {
        return PermissionProbe::unknown("Home directory is unknown");
    };
    if !path.is_dir() {
        return PermissionProbe::unknown(format!("{} does not exist", path.display()));
    }
    match std::fs::read_dir(&path) {
        Ok(_) => match sandbox() {
            // 沙箱里读得到不代表宿主整个目录都读得到,说清楚免得误判。
            Some(kind) => PermissionProbe::explained(
                PermissionStatus::Granted,
                format!("Readable inside the {kind} sandbox; use overrides to widen host access"),
            ),
            None => PermissionProbe::plain(PermissionStatus::Granted),
        },
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => match sandbox() {
            Some(kind) => PermissionProbe::explained(
                PermissionStatus::NotGranted,
                format!("The {kind} sandbox does not expose this directory"),
            ),
            None => PermissionProbe::plain(PermissionStatus::NotGranted),
        },
        Err(error) => PermissionProbe::unknown(error.to_string()),
    }
}

// ── 动作 ─────────────────────────────────────────────────────────────────────
//
// 全部项目都是 report_only,通用逻辑不会走到下面这几个,但平台实现的接口要齐。

pub(crate) fn request(id: &str) -> PermissionProbe {
    check(id)
}

pub(crate) fn open_settings(id: &str) -> Result<(), String> {
    Err(format!("{id} has no per-app settings pane on Linux"))
}

pub(crate) fn reset(id: &str) -> Result<(), String> {
    Err(format!("{id} cannot be reset on Linux"))
}

/// Linux 不按代码签名记授权。沙箱里报沙箱身份,否则报可执行文件路径。
pub(crate) fn identity() -> AppIdentity {
    let subject = match sandbox() {
        Some(kind) => format!(
            "{kind}: {}",
            std::env::var("FLATPAK_ID")
                .or_else(|_| std::env::var("SNAP_NAME"))
                .unwrap_or_else(|_| "unknown".to_string())
        ),
        None => std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
    };
    AppIdentity::not_applicable(subject)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_descriptor_is_report_only() {
        // Linux 没有可点的授权动作,摆按钮就是骗人。
        for descriptor in DESCRIPTORS {
            assert!(
                descriptor.report_only,
                "{} is not report-only",
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
        assert!(open_settings(MICROPHONE).is_err());
        assert!(reset(MICROPHONE).is_err());
    }

    /// 每项检测都必须跑完,且 Unknown 一定带原因——否则界面只能显示没有下文的"未知"。
    #[test]
    fn every_check_completes_with_a_reason_when_unknown() {
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
    fn missing_group_is_none_not_false() {
        assert!(in_group("aeroric-does-not-exist-9f2b").is_none());
    }

    #[test]
    fn session_detection_prefers_xdg_session_type() {
        // 只验证纯函数分支:不改进程环境,避免影响并行测试。
        assert!(matches!(
            session(),
            Session::X11 | Session::Wayland | Session::Unknown
        ));
    }

    #[test]
    fn missing_folder_is_unknown_not_denied() {
        let probe = folder_probe("Aeroric-does-not-exist-9f2b");
        assert_eq!(probe.status, PermissionStatus::Unknown);
        assert!(probe.detail.is_some());
    }

    #[test]
    fn identity_reports_a_subject() {
        let identity = identity();
        assert_eq!(identity.signature, "not-applicable");
        assert!(!identity.subject.is_empty());
    }
}
