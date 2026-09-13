//! 远端主机的操作系统探测,以及跨 POSIX / Windows 的远端路径模型。
//!
//! 为什么需要这个模块:Aeroric 的远端文件浏览不走 SFTP 协议,而是把 POSIX `sh` 脚本
//! 通过 ssh 送到对端执行(见 `sftp.rs` 的 `build_remote_*`)。Windows OpenSSH 的默认
//! shell 是 cmd.exe,`sh` / `stat` / `wc` / `find` 一个都没有,于是整条链路必须先知道
//! 对端是什么系统,才能决定发 `sh -c` 还是发 PowerShell。
//!
//! 路径模型与前端 `sftpTypes.ts` 共用同一套约定,跨 Tauri 边界的规范形式一律用正斜杠:
//! POSIX 是 `/home/user`,根为 `/`;Windows 是 `C:/Users/Administrator`,盘根为 `C:/`
//! (盘符大写),UNC 为 `//server/share`。这里接受反斜杠输入并归一,所以配置里已经存着的
//! `C:\Users\...` 不需要数据迁移就能继续用。

use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;

use crate::ssh::SshConnection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteOs {
    Posix,
    Windows,
}

/// 探测结果按 `user@host:port` 缓存 —— 每次目录跳转都多跑一次 ssh 往返是不能接受的,
/// 而同一台机器的内核不会在一个会话里变。缓存不持久化:重启后重新探一次,成本是一次往返。
static REMOTE_OS_CACHE: LazyLock<Mutex<HashMap<String, RemoteOs>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_key(connection: &SshConnection) -> String {
    format!(
        "{}@{}:{}",
        connection.username, connection.host, connection.port
    )
}

/// `uname -s` 在 POSIX 上必然输出内核名;cmd.exe 与 PowerShell 都不认识这个命令,
/// 于是"跑通且输出可识别内核名"就是 POSIX 的判据。MINGW/MSYS/Cygwin 也算 POSIX ——
/// 它们的 `sh` 就在 PATH 上,发 POSIX 脚本是对的。
const POSIX_KERNELS: &[&str] = &[
    "linux",
    "darwin",
    "freebsd",
    "openbsd",
    "netbsd",
    "dragonfly",
    "sunos",
    "aix",
    "mingw",
    "msys",
    "cygwin",
];

pub(crate) fn build_posix_probe_command() -> String {
    "uname -s".to_string()
}

fn classify_uname(stdout: &str) -> Option<RemoteOs> {
    let text = stdout.trim().to_ascii_lowercase();
    if text.is_empty() {
        return None;
    }
    if POSIX_KERNELS
        .iter()
        .any(|kernel| text.starts_with(kernel) || text.contains(kernel))
    {
        return Some(RemoteOs::Posix);
    }
    None
}

/// PowerShell 脚本一律走 `-EncodedCommand` + base64(UTF-16LE)。
///
/// 这不是为了省事,而是唯一能穿过多层解析的办法:命令串先被本地 ssh 客户端交给远端
/// sshd,sshd 再交给默认 shell(可能是 cmd.exe,可能是 PowerShell),cmd.exe 的引号
/// 规则与 POSIX 完全不同且无法安全转义(见 `platform/mod.rs::validate_cmd_value`)。
/// base64 的字符集里没有任何一层会特殊对待的字符,于是引号问题整体消失。
pub(crate) fn encode_powershell_command(script: &str) -> String {
    use base64::Engine as _;
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(&utf16)
}

/// 只给测试用:把 `-EncodedCommand` 的载荷还原回脚本正文,让断言落在真实内容上而不是
/// base64 串上。
#[cfg(test)]
pub(crate) fn tests_decode_base64(encoded: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .expect("payload is valid base64")
}

/// 显式指名 `powershell`,不依赖远端默认 shell 是什么 —— cmd.exe 和 PowerShell 都能
/// 把这一行作为外部程序启动。`-NoProfile` 避免用户 profile 往 stdout 里吐东西污染解析,
/// 与 `platform/mod.rs:60-63` 本地那套参数约定保持一致。
pub(crate) fn build_powershell_command(script: &str) -> String {
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        encode_powershell_command(script)
    )
}

pub(crate) const WINDOWS_PROBE_MARKER: &str = "AERORIC_REMOTE_IS_WINDOWS";

pub(crate) fn build_windows_probe_command() -> String {
    build_powershell_command(&format!("[Console]::Out.Write('{WINDOWS_PROBE_MARKER}')"))
}

/// 两步探测:先 `uname -s`,失败再问 PowerShell。顺序不能反 —— 绝大多数远端是 POSIX,
/// 让常见情况只花一次往返。
pub(crate) fn detect_remote_os_with<F>(
    connection: &SshConnection,
    mut run: F,
) -> Result<RemoteOs, String>
where
    F: FnMut(&SshConnection, String) -> Result<Vec<u8>, String>,
{
    let key = cache_key(connection);
    if let Some(cached) = REMOTE_OS_CACHE.lock().get(&key).copied() {
        return Ok(cached);
    }
    let detected = probe_remote_os(connection, &mut run)?;
    REMOTE_OS_CACHE.lock().insert(key, detected);
    Ok(detected)
}

fn probe_remote_os<F>(connection: &SshConnection, run: &mut F) -> Result<RemoteOs, String>
where
    F: FnMut(&SshConnection, String) -> Result<Vec<u8>, String>,
{
    if let Ok(stdout) = run(connection, build_posix_probe_command()) {
        if let Some(os) = classify_uname(&String::from_utf8_lossy(&stdout)) {
            return Ok(os);
        }
    }
    let stdout = run(connection, build_windows_probe_command())
        .map_err(|error| format!("Cannot determine the remote operating system: {error}"))?;
    if String::from_utf8_lossy(&stdout).contains(WINDOWS_PROBE_MARKER) {
        return Ok(RemoteOs::Windows);
    }
    Err(
        "Cannot determine the remote operating system: neither `uname` nor PowerShell answered"
            .to_string(),
    )
}

// ---------------------------------------------------------------------------
// 路径模型
// ---------------------------------------------------------------------------

/// `C:` / `c:` 开头即视为 Windows 盘路径;`//` 开头是 UNC。判据与前端
/// `sftpTypes.ts::isWindowsRemotePath` 必须一致,否则两侧会对同一个路径给出不同的根。
///
/// 检测前先 trim:配置里的 `  C:\Users` 仍应被认成 Windows。trim 只用于识别,POSIX
/// 路径的空白是合法文件名字符,canonicalize 时不会 trim。
pub(crate) fn is_windows_remote_path(path: &str) -> bool {
    let path = path.trim();
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return bytes.len() == 2 || bytes[2] == b'/' || bytes[2] == b'\\';
    }
    path.starts_with("//") || path.starts_with("\\\\")
}

/// 归一到跨边界的规范形式。Windows 与 POSIX 规则不同,混用会删错文件:
/// POSIX 上 `\` 与首尾空白都是合法文件名字符,套 Windows 规则会把 `/home/u/a\b`
/// 改写成 `/home/u/a/b`,再交给 `rm -rf` 就是另一个目标。
pub(crate) fn canonicalize_remote_path(path: &str) -> String {
    if is_windows_remote_path(path) {
        canonicalize_windows_path(path)
    } else {
        canonicalize_posix_path(path)
    }
}

fn canonicalize_windows_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return "/".to_string();
    }
    let unc = trimmed.starts_with("//") || trimmed.starts_with("\\\\");
    let mut normalized = trimmed.replace('\\', "/");
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    if unc {
        normalized.insert(0, '/');
    }
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let upper = bytes[0].to_ascii_uppercase() as char;
        normalized.replace_range(0..1, &upper.to_string());
        // `C:` 单独出现时补成盘根 `C:/`,避免下游把它当成相对路径。
        if normalized.len() == 2 {
            normalized.push('/');
        }
    }
    let root = remote_root_of(&normalized);
    if normalized.len() > root.len() {
        normalized = normalized.trim_end_matches('/').to_string();
        if normalized.len() < root.len() {
            normalized = root;
        }
    } else {
        normalized = root;
    }
    normalized
}

fn canonicalize_posix_path(path: &str) -> String {
    // 空串是"还没选路径"的哨兵,回落到根。除此之外不动空白、不动反斜杠。
    if path.is_empty() {
        return "/".to_string();
    }
    let mut normalized = path.to_string();
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    if normalized.len() > 1 {
        normalized = normalized.trim_end_matches('/').to_string();
        if normalized.is_empty() {
            normalized = "/".to_string();
        }
    }
    normalized
}

/// 盘根 / UNC 共享根 / POSIX 根。父目录上溯到这里就停。
pub(crate) fn remote_root_of(path: &str) -> String {
    let canonical = path.replace('\\', "/");
    let bytes = canonical.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return format!("{}:/", bytes[0].to_ascii_uppercase() as char);
    }
    if let Some(rest) = canonical.strip_prefix("//") {
        let mut parts = rest.splitn(3, '/');
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        if server.is_empty() {
            return "/".to_string();
        }
        if share.is_empty() {
            return format!("//{server}");
        }
        return format!("//{server}/{share}");
    }
    "/".to_string()
}

/// 相对分量检查。分隔符集合按 OS 取:Windows 必须把 `\` 也当分隔符,否则
/// `C:\app\..\secret` 能带着 `..` 蒙过校验;POSIX 必须**不**把它当分隔符,否则
/// 一个合法文件名 `a\..\b` 会被误判成穿越而拒掉。
pub(crate) fn remote_path_has_relative_components(path: &str, os: RemoteOs) -> bool {
    let parts: Vec<&str> = match os {
        RemoteOs::Windows => path.split(['/', '\\']).collect(),
        RemoteOs::Posix => path.split('/').collect(),
    };
    parts.iter().any(|part| *part == "." || *part == "..")
}

/// OS 感知的绝对路径校验。POSIX 仍旧要求以 `/` 开头(与既有行为逐字一致),
/// Windows 要求盘根或 UNC。
pub(crate) fn validate_remote_path(path: &str, os: RemoteOs) -> Result<String, String> {
    if path.contains('\0') {
        return Err("Remote path contains forbidden characters".to_string());
    }
    match os {
        RemoteOs::Posix => {
            if !path.starts_with('/') {
                return Err("Remote path must be absolute".to_string());
            }
        }
        RemoteOs::Windows => {
            if !is_windows_remote_path(path) {
                return Err(
                    "Remote path must be absolute, e.g. C:/Users/Public or //server/share"
                        .to_string(),
                );
            }
        }
    }
    if remote_path_has_relative_components(path, os) {
        return Err("Remote path cannot contain . or .. components".to_string());
    }
    Ok(match os {
        RemoteOs::Windows => canonicalize_windows_path(path),
        RemoteOs::Posix => canonicalize_posix_path(path),
    })
}

/// 转成远端原生形式。只在拼 PowerShell 脚本时用 —— 状态里、跨边界的一律是正斜杠。
pub(crate) fn to_windows_native_path(path: &str) -> String {
    canonicalize_remote_path(path).replace('/', "\\")
}

pub(crate) fn remote_parent_of(path: &str) -> String {
    let canonical = canonicalize_remote_path(path);
    let root = remote_root_of(&canonical);
    if canonical == root {
        return root;
    }
    match canonical.rsplit_once('/') {
        Some((parent, _)) => {
            if parent.len() < root.len() || parent.is_empty() {
                root
            } else {
                let parent = parent.to_string();
                // `C:/x` 的 rsplit 会给出 `C:`,补回盘根。
                if parent.len() == 2 && parent.as_bytes()[1] == b':' {
                    root
                } else {
                    parent
                }
            }
        }
        None => root,
    }
}

pub(crate) fn join_remote_path(parent: &str, name: &str) -> String {
    let parent = canonicalize_remote_path(parent);
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

/// PowerShell 单引号字面量:内部的 `'` 写成 `''`。与
/// `app_settings/agent_scripts.rs::powershell_quote` 同一套规则,但那个是
/// `#[cfg(any(windows, test))]` 且在错误的模块里,远端场景要在 macOS/Linux 上也能用。
pub(crate) fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例传自己的 `host`:探测结果按 `user@host:port` 缓存,共用一个 host 会让
    /// 并行跑的用例互相看见对方的缓存条目。给每个用例独立键之后,这些用例之间再无共享
    /// 状态,于是既不需要全局重置、也不会因 cargo 默认并行而互相污染。
    fn connection_on(host: &str) -> SshConnection {
        SshConnection {
            id: format!("conn-{host}"),
            name: "win".to_string(),
            group: None,
            host: host.to_string(),
            port: 22,
            username: "administrator".to_string(),
            identity_file: None,
            password: None,
            has_password: false,
            remote_path: None,
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        }
    }

    #[test]
    fn uname_output_classifies_posix_kernels() {
        assert_eq!(classify_uname("Linux\n"), Some(RemoteOs::Posix));
        assert_eq!(classify_uname("Darwin\n"), Some(RemoteOs::Posix));
        assert_eq!(classify_uname("MINGW64_NT-10.0\n"), Some(RemoteOs::Posix));
        assert_eq!(classify_uname(""), None);
        assert_eq!(
            classify_uname("'uname' is not recognized as an internal or external command"),
            None
        );
    }

    #[test]
    fn probe_prefers_posix_and_only_falls_back_to_powershell() {
        let mut commands = Vec::new();
        let os = detect_remote_os_with(&connection_on("posix-first.test"), |_, command| {
            commands.push(command);
            Ok(b"Linux\n".to_vec())
        })
        .unwrap();
        assert_eq!(os, RemoteOs::Posix);
        assert_eq!(
            commands.len(),
            1,
            "POSIX host must cost a single round trip"
        );
        assert_eq!(commands[0], "uname -s");
    }

    #[test]
    fn probe_detects_windows_when_uname_fails() {
        let mut calls = 0;
        let os = detect_remote_os_with(&connection_on("windows-fallback.test"), |_, command| {
            calls += 1;
            if command == "uname -s" {
                return Err("command not found".to_string());
            }
            Ok(WINDOWS_PROBE_MARKER.as_bytes().to_vec())
        })
        .unwrap();
        assert_eq!(os, RemoteOs::Windows);
        assert_eq!(calls, 2);
    }

    #[test]
    fn probe_result_is_cached_per_connection() {
        let mut calls = 0;
        for _ in 0..5 {
            detect_remote_os_with(&connection_on("cached.test"), |_, _| {
                calls += 1;
                Ok(b"Linux\n".to_vec())
            })
            .unwrap();
        }
        assert_eq!(calls, 1, "cache must collapse repeated probes");
    }

    #[test]
    fn probe_errors_when_neither_shell_answers() {
        let error =
            detect_remote_os_with(
                &connection_on("silent.test"),
                |_, _| Err("boom".to_string()),
            )
            .unwrap_err();
        assert!(error.contains("Cannot determine the remote operating system"));
    }

    #[test]
    fn encoded_command_is_base64_of_utf16le() {
        // "hi" => 68 00 69 00 => aABpAA==
        assert_eq!(encode_powershell_command("hi"), "aABpAA==");
    }

    #[test]
    fn powershell_command_never_needs_shell_quoting() {
        let command = build_powershell_command("Get-ChildItem 'C:\\Users'");
        assert!(command.starts_with("powershell -NoProfile -NonInteractive -EncodedCommand "));
        let encoded = command.rsplit(' ').next().unwrap();
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
            "encoded payload must stay inside the base64 alphabet: {encoded}"
        );
    }

    #[test]
    fn canonicalize_converts_the_configured_windows_path() {
        assert_eq!(
            canonicalize_remote_path("C:\\Users\\Administrator\\Documents"),
            "C:/Users/Administrator/Documents"
        );
    }

    #[test]
    fn canonicalize_normalizes_separators_case_and_duplicates() {
        // 盘符大写,其余大小写原样保留。
        assert_eq!(canonicalize_remote_path("c:/users"), "C:/users");
        assert_eq!(canonicalize_remote_path("C:\\"), "C:/");
        assert_eq!(canonicalize_remote_path("C:"), "C:/");
        assert_eq!(
            canonicalize_remote_path("C:/Users//Public/"),
            "C:/Users/Public"
        );
        assert_eq!(canonicalize_remote_path("/srv//app/"), "/srv/app");
        assert_eq!(canonicalize_remote_path("/"), "/");
        assert_eq!(canonicalize_remote_path(""), "/");
        assert_eq!(
            canonicalize_remote_path("\\\\server\\share\\dir"),
            "//server/share/dir"
        );
    }

    #[test]
    fn posix_canonicalize_keeps_backslashes_and_spaces_verbatim() {
        // POSIX 上 `\` 与首尾空白都是合法文件名字符。套 Windows 规则会把这些路径改写成
        // **另一个**目标,而 `validate_remote_path` 的返回值会直接进 `rm -rf`。
        assert_eq!(canonicalize_remote_path("/home/u/a\\b"), "/home/u/a\\b");
        assert_eq!(canonicalize_remote_path("/home/u/tail "), "/home/u/tail ");
        assert_eq!(canonicalize_remote_path("/home/u/ lead"), "/home/u/ lead");
        // 反斜杠不是分隔符,所以夹在文件名里的 `..` 段不算穿越。
        assert!(!remote_path_has_relative_components(
            "/home/u/a\\..\\b",
            RemoteOs::Posix
        ));
        // Windows 上同一个串必须按穿越拒掉。
        assert!(remote_path_has_relative_components(
            "C:\\app\\..\\secret",
            RemoteOs::Windows
        ));
    }

    #[test]
    fn validate_posix_path_does_not_rewrite_the_delete_target() {
        // 端到端:`validate_remote_path` 的返回值就是发给 `rm -rf` 的那个串。
        assert_eq!(
            validate_remote_path("/home/u/a\\b", RemoteOs::Posix).expect("valid"),
            "/home/u/a\\b"
        );
        assert_eq!(
            validate_remote_path("/home/u/tail ", RemoteOs::Posix).expect("valid"),
            "/home/u/tail "
        );
    }

    #[test]
    fn roots_stop_parent_traversal() {
        assert_eq!(remote_root_of("C:/Users/Public"), "C:/");
        assert_eq!(remote_root_of("//server/share/dir"), "//server/share");
        assert_eq!(remote_root_of("/home/user"), "/");
        assert_eq!(remote_parent_of("C:/"), "C:/");
        assert_eq!(remote_parent_of("C:/Users"), "C:/");
        assert_eq!(remote_parent_of("C:/Users/Public"), "C:/Users");
        assert_eq!(remote_parent_of("//server/share"), "//server/share");
        assert_eq!(remote_parent_of("/"), "/");
        assert_eq!(remote_parent_of("/home"), "/");
    }

    #[test]
    fn join_never_doubles_the_drive_root_slash() {
        assert_eq!(join_remote_path("C:/", "Users"), "C:/Users");
        assert_eq!(join_remote_path("C:/Users", "Public"), "C:/Users/Public");
        assert_eq!(join_remote_path("/", "home"), "/home");
        assert_eq!(join_remote_path("/home", "user"), "/home/user");
    }

    #[test]
    fn windows_paths_are_recognized() {
        assert!(is_windows_remote_path("C:/Users"));
        assert!(is_windows_remote_path("c:\\Users"));
        assert!(is_windows_remote_path("C:"));
        assert!(is_windows_remote_path("//server/share"));
        assert!(!is_windows_remote_path("/home/user"));
        assert!(!is_windows_remote_path("relative/path"));
    }

    #[test]
    fn validation_is_os_aware() {
        assert_eq!(
            validate_remote_path("C:\\Users\\Administrator\\Documents", RemoteOs::Windows).unwrap(),
            "C:/Users/Administrator/Documents"
        );
        assert_eq!(
            validate_remote_path("/home/user/", RemoteOs::Posix).unwrap(),
            "/home/user"
        );
        // POSIX 分支的既有行为逐字保留。
        assert_eq!(
            validate_remote_path("relative", RemoteOs::Posix).unwrap_err(),
            "Remote path must be absolute"
        );
        assert!(validate_remote_path("relative", RemoteOs::Windows)
            .unwrap_err()
            .contains("must be absolute"));
    }

    #[test]
    fn validation_blocks_relative_components_through_both_separators() {
        assert!(
            validate_remote_path("C:\\app\\..\\secret", RemoteOs::Windows)
                .unwrap_err()
                .contains("cannot contain")
        );
        assert!(validate_remote_path("/app/../secret", RemoteOs::Posix)
            .unwrap_err()
            .contains("cannot contain"));
        assert!(remote_path_has_relative_components(
            "C:\\app\\..\\x",
            RemoteOs::Windows
        ));
        assert!(remote_path_has_relative_components(
            "/app/./x",
            RemoteOs::Posix
        ));
        assert!(!remote_path_has_relative_components(
            "C:/app/x",
            RemoteOs::Windows
        ));
    }

    #[test]
    fn native_path_is_only_for_powershell_scripts() {
        assert_eq!(
            to_windows_native_path("C:/Users/Administrator/Documents"),
            "C:\\Users\\Administrator\\Documents"
        );
    }

    #[test]
    fn powershell_quote_doubles_single_quotes() {
        assert_eq!(powershell_quote("plain"), "'plain'");
        assert_eq!(powershell_quote("it's"), "'it''s'");
    }
}
