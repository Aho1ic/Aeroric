//! P7-02 验收：平台能力（macOS wsl=false——不得由 client OS 直接推断 host
//! 能力）、WSL settings roundtrip（不丢 distribution/linuxPath）、wsl git
//! 命令构建、路径转换。
use aeroric_core::connections::wsl;
use aeroric_core::connections::wsl_files;
use aeroric_core::connections::wsl_git;
use aeroric_core::platform;

#[test]
fn capabilities_macos_wsl_false() {
    // Action 3：core capability 在 macOS 为 wsl=false。
    let caps = platform::capabilities();
    if platform::current_platform() == platform::Platform::MacOS {
        assert!(!caps.wsl, "macOS wsl=false");
        assert!(caps.wsl_reason.contains("Windows host"), "诊断原因");
    }
    // client OS 推断守卫（不回答远程 host 能力）。
    let client_declares = platform::client_os_declares_wsl();
    assert_eq!(
        client_declares,
        platform::current_platform() == platform::Platform::Windows
    );
}

#[test]
fn wsl_project_is_local_guard() {
    // Action 2：在 macOS 打开 WSL 项目时禁止当作本地 path。
    assert!(
        !wsl::wsl_project_is_local(true, "macos"),
        "macOS 打开 WSL 项目＝非本地"
    );
    assert!(
        wsl::wsl_project_is_local(true, "windows"),
        "Windows 打开 WSL 项目＝本地"
    );
    assert!(
        wsl::wsl_project_is_local(false, "macos"),
        "非 WSL 项目＝本地"
    );
}

#[test]
fn wsl_settings_roundtrip_keeps_linux_path() {
    // 旧 WSL JSON 读写不丢 distribution/linuxPath。
    let json = r#"{"defaultDistribution":"Ubuntu","distributions":[{"distribution":"Ubuntu","linuxPath":"/home/user/project"}],"environment":{}}"#;
    let settings = wsl_files::settings_roundtrip(json).unwrap();
    assert_eq!(settings.default_distribution.as_deref(), Some("Ubuntu"));
    assert_eq!(settings.distributions.len(), 1);
    assert_eq!(settings.distributions[0].distribution, "Ubuntu");
    assert_eq!(
        settings.distributions[0].linux_path.as_deref(),
        Some("/home/user/project")
    );
    // roundtrip 后 linuxPath 保留。
    let roundtripped = serde_json::to_string(&settings).unwrap();
    assert!(roundtripped.contains("linuxPath"), "linuxPath 不丢");
    assert!(roundtripped.contains("/home/user/project"));
}

#[test]
fn wsl_path_to_windows_unc() {
    assert_eq!(
        wsl_files::wsl_path_to_windows_unc("Ubuntu", "/home/user/project"),
        r"\\wsl$\Ubuntu\home\user\project"
    );
    assert_eq!(
        wsl_files::wsl_path_to_windows_unc("Ubuntu", "/"),
        r"\\wsl$\Ubuntu"
    );
}

#[test]
fn wsl_git_command_build() {
    let command =
        wsl_git::build_wsl_git_command("Ubuntu", "/home/user/project", &["status", "--porcelain"]);
    assert_eq!(command[0], "wsl.exe");
    assert_eq!(command[1], "-d");
    assert_eq!(command[2], "Ubuntu");
    assert!(command.contains(&"--cd".to_string()));
    assert!(command.contains(&"--".to_string()));
    assert!(command.contains(&"git".to_string()));
    assert!(command.contains(&"status".to_string()));
    // 无 cwd 的构建。
    let command = wsl_git::build_wsl_git_command("Ubuntu", "", &["log"]);
    assert!(!command.contains(&"--cd".to_string()));
}
