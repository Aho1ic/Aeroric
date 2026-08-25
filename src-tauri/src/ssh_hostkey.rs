//! 首次连接的 host key 信任(TOFU)。
//!
//! `ssh.rs` 给每一次调用都强制 `StrictHostKeyChecking=yes`,所以未登记过的主机
//! 必然连不上。命令行 ssh 遇到这种情况会弹
//! `Are you sure you want to continue connecting (yes/no/[fingerprint])?`,
//! 而 App 走的是 PTY,没有这个交互出口,只会甩一行
//! `No ED25519 host key is known for <host> ...`。
//!
//! 这个模块把那次交互搬进 App:先**只读**地判断主机在不在 known_hosts,
//! 不在就把服务端提供的 key 指纹交给前端展示,用户确认后才落盘。
//! 三条不退让的底线:
//!   * 判断阶段绝不写文件,也绝不放宽 `StrictHostKeyChecking`;
//!   * 写盘前重新扫一次,指纹集合必须与用户确认过的完全一致(TOCTOU 防护);
//!   * key 变更(已有记录但对不上)不在这里处理 —— 那是 MITM 信号,交给 ssh 自己
//!     报错,只补一段补救说明,不提供任何"照样信任"的按钮。

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ssh::SshConnection;

/// 服务端提供的一把 host key。`known_hosts_line` 不给用户看,只在信任时落盘。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SshHostKey {
    #[serde(rename = "keyType")]
    pub key_type: String,
    pub fingerprint: String,
    #[serde(rename = "knownHostsLine")]
    pub known_hosts_line: String,
}

/// 连接前的 host key 判定结果。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SshHostKeyStatus {
    /// known_hosts 里已有该主机(或无法判定),交给 ssh 自己校验。
    Trusted,
    /// known_hosts 里没有该主机,需要用户确认指纹。
    Unknown {
        target: String,
        keys: Vec<SshHostKey>,
    },
    /// 扫不到 key(主机不通 / DNS 失败)。不是 host key 问题,
    /// 让 ssh 去报真实的连接错误,不要在这里编一个。
    Unreachable { target: String },
}

fn program(binary: &str) -> String {
    let detected = crate::platform::detect_path(binary);
    if detected.is_empty() {
        binary.to_string()
    } else {
        detected
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// `ssh -G` 已经把 `~` 展开成绝对路径,但配置文件里也能写 `~/...`,
/// 而且 Windows 下行为不一致,所以这里再兜一层。
fn expand_tilde(value: &str) -> PathBuf {
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}

fn default_known_hosts() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".ssh").join("known_hosts"))
}

/// known_hosts 记录主机的形式:默认端口是裸主机名,非默认端口是 `[host]:port`。
/// `ssh-keygen -F` 必须用同一种写法才能命中,写错就会把已登记的主机误判成未知。
pub(crate) fn known_hosts_target(connection: &SshConnection) -> String {
    if connection.port == 22 {
        connection.host.clone()
    } else {
        format!("[{}]:{}", connection.host, connection.port)
    }
}

/// 读 `ssh -G` 得到该主机真正生效的配置。用户可能在 `~/.ssh/config` 里改过
/// `UserKnownHostsFile`,硬编码 `~/.ssh/known_hosts` 会判断错。
fn ssh_config_values(connection: &SshConnection, keys: &[&str]) -> Vec<(String, String)> {
    let mut cmd = Command::new(program("ssh"));
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(["-G", "-p", &connection.port.to_string()]);
    cmd.arg(format!("{}@{}", connection.username, connection.host));
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    let Ok(output) = cmd.output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(' ')?;
            let name = name.trim().to_ascii_lowercase();
            keys.contains(&name.as_str())
                .then(|| (name, value.trim().to_string()))
        })
        .collect()
}

/// 所有可能登记该主机的文件,顺序与 ssh 自己的查找顺序一致。
fn known_hosts_files(connection: &SshConnection) -> Vec<PathBuf> {
    let values = ssh_config_values(connection, &["userknownhostsfile", "globalknownhostsfile"]);
    let mut files: Vec<PathBuf> = values
        .iter()
        .flat_map(|(_, value)| value.split_whitespace())
        .filter(|value| !value.is_empty() && *value != "none")
        .map(expand_tilde)
        .collect();
    if files.is_empty() {
        files.extend(default_known_hosts());
    }
    files
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lookup {
    Found,
    Absent,
    /// 跑不动 `ssh-keygen`。绝不能当成"未登记" —— 那会把一台本来连得上的主机
    /// 拦在弹窗后面。
    Inconclusive,
}

fn lookup_in_file(target: &str, file: &Path) -> Lookup {
    // 文件不存在是全新机器的正常状态,算"没登记",不是错误。
    if !file.exists() {
        return Lookup::Absent;
    }
    let mut cmd = Command::new(program("ssh-keygen"));
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(["-F", target, "-f"]);
    cmd.arg(file);
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    let Ok(output) = cmd.output() else {
        return Lookup::Inconclusive;
    };
    match output.status.code() {
        Some(0) => {
            // `-F` 命中时会连注释行一起打印,只有非注释行才算真正的记录。
            let has_entry = String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
            if has_entry {
                Lookup::Found
            } else {
                Lookup::Absent
            }
        }
        Some(1) => Lookup::Absent,
        _ => Lookup::Inconclusive,
    }
}

/// `Some(true)` 已登记,`Some(false)` 确定未登记,`None` 无法判定。
pub(crate) fn is_host_known(connection: &SshConnection) -> Option<bool> {
    let target = known_hosts_target(connection);
    let mut inconclusive = false;
    for file in known_hosts_files(connection) {
        match lookup_in_file(&target, &file) {
            Lookup::Found => return Some(true),
            Lookup::Inconclusive => inconclusive = true,
            Lookup::Absent => {}
        }
    }
    (!inconclusive).then_some(false)
}

/// 从一条 known_hosts 行算出 ssh 展示的那个 `SHA256:...` 指纹。
///
/// 自己算而不是再 spawn 一次 `ssh-keygen -l`:指纹是用户唯一会逐字核对的东西,
/// 它必须可单元测试,不能依赖外部进程的输出格式。
fn fingerprint_of(line: &str) -> Option<SshHostKey> {
    let mut fields = line.split_whitespace();
    let host_field = fields.next()?;
    // `@cert-authority` / `@revoked` 标记会占掉第一个字段,主机名往后挪一位。
    if host_field.starts_with('@') {
        fields.next()?;
    }
    let key_type = fields.next()?;
    let blob = fields.next()?;
    if !key_type.starts_with("ssh-")
        && !key_type.starts_with("ecdsa-")
        && !key_type.starts_with("sk-")
    {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .ok()?;
    let digest = Sha256::digest(&decoded);
    // ssh 打印的 base64 指纹不带 `=` 填充。
    let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
    Some(SshHostKey {
        key_type: key_type.to_string(),
        fingerprint: format!("SHA256:{encoded}"),
        known_hosts_line: line.trim_end().to_string(),
    })
}

fn parse_scan_output(stdout: &str) -> Vec<SshHostKey> {
    let mut keys: Vec<SshHostKey> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(key) = fingerprint_of(line) {
            if !keys
                .iter()
                .any(|existing| existing.key_type == key.key_type)
            {
                keys.push(key);
            }
        }
    }
    keys
}

/// 扫服务端提供的 host key。空结果表示扫不到(主机不通),而不是"没有 key"。
///
/// `hash_names` 对应用户的 `HashKnownHosts` 设置:开了就让 keyscan 直接输出
/// 哈希过的主机名。哈希只作用于主机名字段,key blob 不变,所以指纹照样能核对。
fn scan_host_keys(connection: &SshConnection, hash_names: bool) -> Vec<SshHostKey> {
    let mut cmd = Command::new(program("ssh-keyscan"));
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.args(["-T", "10"]);
    if hash_names {
        cmd.arg("-H");
    }
    cmd.args(["-p", &connection.port.to_string(), &connection.host]);
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    let Ok(output) = cmd.output() else {
        return Vec::new();
    };
    // keyscan 扫不到时退出码非 0 且 stdout 为空,不需要另外判断 stderr。
    if !output.status.success() {
        return Vec::new();
    }
    parse_scan_output(&String::from_utf8_lossy(&output.stdout))
}

fn hash_known_hosts_enabled(connection: &SshConnection) -> bool {
    ssh_config_values(connection, &["hashknownhosts"])
        .first()
        .is_some_and(|(_, value)| value.eq_ignore_ascii_case("yes"))
}

/// 该写哪个文件:`UserKnownHostsFile` 的第一项(ssh 自己也写第一项),
/// 拿不到就退回 `~/.ssh/known_hosts`。GlobalKnownHostsFile 不碰,那是系统级的。
fn writable_known_hosts(connection: &SshConnection) -> Result<PathBuf, String> {
    let configured = ssh_config_values(connection, &["userknownhostsfile"])
        .into_iter()
        .flat_map(|(_, value)| {
            value
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .find(|value| !value.is_empty() && value != "none")
        .map(|value| expand_tilde(&value));
    configured
        .or_else(default_known_hosts)
        .ok_or_else(|| "Cannot locate a known_hosts file to write".to_string())
}

fn append_known_hosts_lines(path: &Path, lines: &[String]) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        // `~/.ssh` 权限过宽时 ssh 会拒绝使用里面的私钥,新建目录必须收紧。
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    // 末尾缺换行时直接 append 会把新记录粘到最后一行上,先补一个。
    let needs_leading_newline = match fs::read(path) {
        Ok(existing) => !existing.is_empty() && !existing.ends_with(b"\n"),
        Err(_) => false,
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut payload = String::new();
    if needs_leading_newline {
        payload.push('\n');
    }
    for line in lines {
        payload.push_str(line.trim_end());
        payload.push('\n');
    }
    file.write_all(payload.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    crate::storage::ensure_private_file_permissions(path)
}

/// 连接前判断是否需要弹窗确认 host key。只读,不写任何文件。
#[tauri::command]
pub async fn check_ssh_host_key(connection: SshConnection) -> Result<SshHostKeyStatus, String> {
    tokio::task::spawn_blocking(move || {
        // 无法判定时按"已信任"走,让 ssh 自己去校验 —— 宁可多一次真实报错,
        // 也不要把连得上的主机拦在弹窗后面。
        if is_host_known(&connection) != Some(false) {
            return SshHostKeyStatus::Trusted;
        }
        let target = known_hosts_target(&connection);
        let keys = scan_host_keys(&connection, false);
        if keys.is_empty() {
            return SshHostKeyStatus::Unreachable { target };
        }
        SshHostKeyStatus::Unknown { target, keys }
    })
    .await
    .map_err(|e| e.to_string())
}

/// 把用户确认过的 host key 写进 known_hosts。
///
/// `approved_fingerprints` 是用户在弹窗里实际看到的那一组指纹。写盘前重新扫一次,
/// 只有集合完全一致才落盘:这样即便"展示"和"确认"之间服务端换了 key,
/// 也不会把用户没看过的 key 写成受信任。
#[tauri::command]
pub async fn trust_ssh_host_key(
    connection: SshConnection,
    approved_fingerprints: Vec<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if approved_fingerprints.is_empty() {
            return Err("No approved host key fingerprint was provided".to_string());
        }
        let hash_names = hash_known_hosts_enabled(&connection);
        let scanned = scan_host_keys(&connection, hash_names);
        if scanned.is_empty() {
            return Err(format!(
                "Cannot reach {} to confirm its host key",
                known_hosts_target(&connection)
            ));
        }
        let mut scanned_prints: Vec<&str> =
            scanned.iter().map(|key| key.fingerprint.as_str()).collect();
        scanned_prints.sort_unstable();
        let mut approved: Vec<&str> = approved_fingerprints
            .iter()
            .map(|value| value.trim())
            .collect();
        approved.sort_unstable();
        approved.dedup();
        if scanned_prints != approved {
            return Err(format!(
                "{} now presents a different host key than the one you confirmed. \
Nothing was trusted. Verify the host before retrying.",
                known_hosts_target(&connection)
            ));
        }
        // 已经登记过就别再追加一遍(并发点两次弹窗时会遇到)。
        if is_host_known(&connection) == Some(true) {
            return Ok(());
        }
        let path = writable_known_hosts(&connection)?;
        let lines: Vec<String> = scanned
            .into_iter()
            .map(|key| key.known_hosts_line)
            .collect();
        append_known_hosts_lines(&path, &lines)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(host: &str, port: u16) -> SshConnection {
        SshConnection {
            id: "conn-1".to_string(),
            name: "test".to_string(),
            group: None,
            host: host.to_string(),
            port,
            username: "deploy".to_string(),
            identity_file: None,
            password: None,
            remote_path: None,
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        }
    }

    /// 指纹是用户唯一会逐字核对的东西。这里用真实的 Oracle Linux host key 固定住
    /// 算法:ssh 打印的是无填充 base64 的 SHA256(key blob)。
    #[test]
    fn fingerprint_matches_openssh_output() {
        let line = "217.142.187.92 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKv0Q9IEEfXXgt6SiDTAyeAQyYm0QYTljtem2ftd/o7B";

        let key = fingerprint_of(line).expect("parses");

        assert_eq!(key.key_type, "ssh-ed25519");
        assert_eq!(
            key.fingerprint,
            "SHA256:QWYSzHIlnZ3CjHkjh54wW7fcxwtUu7cCq7u4kSrJang"
        );
        assert_eq!(key.known_hosts_line, line);
    }

    #[test]
    fn known_hosts_target_brackets_only_non_default_ports() {
        assert_eq!(
            known_hosts_target(&connection("example.com", 22)),
            "example.com"
        );
        assert_eq!(
            known_hosts_target(&connection("example.com", 2200)),
            "[example.com]:2200"
        );
    }

    #[test]
    fn scan_output_skips_comments_and_dedupes_key_types() {
        let stdout = "# 217.142.187.92:22 SSH-2.0-OpenSSH_9.9\n\
217.142.187.92 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKv0Q9IEEfXXgt6SiDTAyeAQyYm0QYTljtem2ftd/o7B\n\
\n\
# another banner\n\
217.142.187.92 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKv0Q9IEEfXXgt6SiDTAyeAQyYm0QYTljtem2ftd/o7B\n\
217.142.187.92 ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEo++PC1Lgq+qoWYVdw4L5uEQS28drKaJGDgudTPw4K3GBehvP2c04wAJ25N1l+4ZLmYdbZAUzgjQu+v/htAanw=\n";

        let keys = parse_scan_output(stdout);

        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].key_type, "ssh-ed25519");
        assert_eq!(keys[1].key_type, "ecdsa-sha2-nistp256");
    }

    #[test]
    fn garbage_lines_are_ignored() {
        assert!(fingerprint_of("host not-a-key-type AAAA").is_none());
        assert!(fingerprint_of("host ssh-ed25519 !!!not-base64!!!").is_none());
        assert!(fingerprint_of("").is_none());
        assert!(fingerprint_of("host ssh-ed25519").is_none());
    }

    /// `@cert-authority` 行的第一个字段被标记占掉,不跳过就会把标记当主机名、
    /// 把主机名当 key 类型,整行解析错位。
    #[test]
    fn cert_authority_marker_is_skipped() {
        let key = fingerprint_of(
            "@cert-authority *.example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKv0Q9IEEfXXgt6SiDTAyeAQyYm0QYTljtem2ftd/o7B",
        )
        .expect("parses");

        assert_eq!(key.key_type, "ssh-ed25519");
    }

    #[test]
    fn missing_known_hosts_file_counts_as_absent() {
        let missing = std::env::temp_dir().join("aeroric-absent-known-hosts-does-not-exist");
        let _ = fs::remove_file(&missing);

        assert_eq!(lookup_in_file("example.com", &missing), Lookup::Absent);
    }

    #[test]
    fn append_adds_missing_trailing_newline_before_new_entries() {
        let dir = std::env::temp_dir().join(format!("aeroric-kh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("known_hosts");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "old.example.com ssh-ed25519 AAAA").unwrap();

        append_known_hosts_lines(&path, &["new.example.com ssh-ed25519 BBBB".to_string()]).unwrap();

        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(
            written,
            "old.example.com ssh-ed25519 AAAA\nnew.example.com ssh-ed25519 BBBB\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
