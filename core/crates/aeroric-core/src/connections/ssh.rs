//! P7-01 Action 1：SSH 连接配置与 host key（core 独占；保留 credential owner
//! 与连接配置格式——与 `src-tauri/src/ssh.rs`/`ssh_hostkey.rs` 同源语义
//! verbatim 上收）。
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// SSH 连接配置（与 Tauri `SshConnection` 字段对齐；明文密码**只在反序列化
/// 方向有值**——`skip_serializing` 让它永远不被写出去：既不进 IPC 返回值，
/// 也不进 `ssh-connections.json`。结构性事实，不是可被忘掉的步骤）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshConnection {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(rename = "identityFile", skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    /// 明文密码（skip_serializing——明文出不去）。
    #[serde(default, skip_serializing)]
    pub password: Option<String>,
    /// 这条连接存了密码（keyring 或 legacy 任一来源）。
    #[serde(rename = "hasPassword", default)]
    pub has_password: bool,
}

/// 服务端提供的一把 host key（`known_hosts_line` 不给用户看，只在信任时
/// 落盘；密码/私钥 passphrase 不进 snapshot/log——trust 只落 known_hosts 行）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SshHostKey {
    #[serde(rename = "keyType")]
    pub key_type: String,
    pub fingerprint: String,
    #[serde(rename = "knownHostsLine")]
    pub known_hosts_line: String,
}

/// 连接前的 host key 判定结果（首连/主机 key 变化分别提示）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SshHostKeyStatus {
    /// known_hosts 里已有该主机（或无法判定），交给 ssh 自己校验。
    Trusted,
    /// known_hosts 里没有该主机，需要用户确认指纹（**拒绝不可自动重试为
    /// 信任**——hostkey changed 必须失败等待确认）。
    Unknown {
        target: String,
        keys: Vec<SshHostKey>,
    },
    /// 扫不到 key（主机不通/DNS 失败）。不是 host key 问题，让 ssh 去报
    /// 真实的连接错误，不要在这里编一个。
    Unreachable { target: String },
}

/// known_hosts 行 → host key（verbatim 上收：`@cert-authority` 标记占第一位，
/// key_type 前缀过滤，SHA256 指纹不带 `=` 填充）。
pub fn fingerprint_of(line: &str) -> Option<SshHostKey> {
    let mut fields = line.split_whitespace();
    let host_field = fields.next()?;
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
    let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
    Some(SshHostKey {
        key_type: key_type.to_string(),
        fingerprint: format!("SHA256:{encoded}"),
        known_hosts_line: line.trim_end().to_string(),
    })
}

/// ssh-keyscan 输出解析（verbatim；按 key_type 去重）。
pub fn parse_scan_output(stdout: &str) -> Vec<SshHostKey> {
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

/// 远程路径校验（Action 5：非 ASCII/空格/前导 dash 远程路径不注入命令）。
pub fn validate_remote_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("remote path is empty".to_string());
    }
    if path.contains('\0') {
        return Err("remote path contains NUL".to_string());
    }
    if path.starts_with('-') {
        return Err("remote path must not start with '-'".to_string());
    }
    Ok(())
}

/// remote cwd 与 local cwd 分开显示（Action 4：防止 native 文件动作误落
/// 本机——判定纯函数）。
pub fn cwd_targets_are_distinct(local_cwd: &str, remote_cwd: &str) -> bool {
    local_cwd != remote_cwd
}
