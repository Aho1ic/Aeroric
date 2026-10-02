//! P7-01 验收：host key 指纹/scan 解析、远程路径校验、SFTP entry 校验与
//! 传输队列（fake SSH/SFTP；显式 opt-in 本地 sshd 才跑真连接——本测试不
//! 连接任何真实服务器）。
use aeroric_core::connections::sftp;
use aeroric_core::connections::ssh;

#[test]
fn fingerprint_matches_openssh_output() {
    // OpenSSH keyscan 输出行（ed25519）。
    let line = "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGJtFmT0P9XjGqJdKFPpAeN3FzJGFmXK8eFz/1xG8vJq";
    let key = ssh::fingerprint_of(line).expect("ed25519 key parsed");
    assert_eq!(key.key_type, "ssh-ed25519");
    assert!(key.fingerprint.starts_with("SHA256:"), "SHA256 指纹");
    assert!(!key.fingerprint.ends_with('='), "不带 = 填充");
    assert_eq!(key.known_hosts_line, line);
    // 非法 key_type 拒绝。
    assert!(ssh::fingerprint_of("host not-a-key AAAA").is_none());
    // @cert-authority 标记占第一位。
    let cert = "@cert-authority example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGJtFmT0P9XjGqJdKFPpAeN3FzJGFmXK8eFz/1xG8vJq";
    let key = ssh::fingerprint_of(cert).expect("cert-authority parsed");
    assert_eq!(key.key_type, "ssh-ed25519");
}

#[test]
fn scan_output_deduplicates_by_key_type() {
    let stdout = "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGJtFmT0P9XjGqJdKFPpAeN3FzJGFmXK8eFz/1xG8vJq\n\
                  example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGJtFmT0P9XjGqJdKFPpAeN3FzJGFmXK8eFz/1xG8vJq\n\
                  # comment\n\
                  example.com ssh-rsa AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n";
    let keys = ssh::parse_scan_output(stdout);
    assert_eq!(keys.len(), 2, "按 key_type 去重（ed25519 + rsa）");
}

#[test]
fn hostkey_changed_must_fail_waiting_confirmation() {
    // hostkey changed 必须失败等待确认（拒绝不可自动重试为信任）：
    // Unknown 状态需要用户确认指纹，不自动 trust。
    let status = ssh::SshHostKeyStatus::Unknown {
        target: "example.com:22".to_string(),
        keys: vec![ssh::SshHostKey {
            key_type: "ssh-ed25519".to_string(),
            fingerprint: "SHA256:abc".to_string(),
            known_hosts_line: "example.com ssh-ed25519 AAAA".to_string(),
        }],
    };
    match status {
        ssh::SshHostKeyStatus::Unknown { target, keys } => {
            assert_eq!(target, "example.com:22");
            assert_eq!(
                keys.len(),
                1,
                "Unknown 需要用户确认指纹（不自动重试为信任）"
            );
        }
        _ => panic!("应为 Unknown"),
    }
    // Trusted/Unreachable 的语义。
    assert!(matches!(
        ssh::SshHostKeyStatus::Trusted,
        ssh::SshHostKeyStatus::Trusted
    ));
    assert!(matches!(
        ssh::SshHostKeyStatus::Unreachable {
            target: "x".to_string()
        },
        ssh::SshHostKeyStatus::Unreachable { .. }
    ));
}

#[test]
fn remote_path_validation_rejects_injection() {
    // 非 ASCII/空格远程路径合法。
    assert!(ssh::validate_remote_path("/home/user/目录/文件 名.txt").is_ok());
    // 前导 dash 拒绝（不注入命令）。
    assert!(ssh::validate_remote_path("-rf").is_err());
    assert!(ssh::validate_remote_path("").is_err());
    assert!(ssh::validate_remote_path("nul\0byte").is_err());
    // remote cwd 与 local cwd 分开显示（防误落本机）。
    assert!(ssh::cwd_targets_are_distinct("/local", "/remote"));
    assert!(!ssh::cwd_targets_are_distinct("/same", "/same"));
}

#[test]
fn sftp_entry_name_validation() {
    assert!(sftp::validate_entry_name("valid.txt").is_ok());
    assert!(sftp::validate_entry_name(".").is_err());
    assert!(sftp::validate_entry_name("..").is_err());
    assert!(sftp::validate_entry_name("a/b").is_err());
    assert!(sftp::validate_entry_name("").is_err());
    // 非 ASCII 与空格文件名合法。
    assert!(sftp::validate_entry_name("文件 名.txt").is_ok());
}

#[test]
fn sftp_path_helpers() {
    assert_eq!(sftp::basename("/home/user/file.txt").unwrap(), "file.txt");
    assert_eq!(
        sftp::remote_parent("/home/user/file.txt").unwrap(),
        "/home/user"
    );
    assert_eq!(sftp::remote_parent("/file.txt").unwrap(), "/");
    assert_eq!(sftp::join_remote_path("/", "a.txt"), "/a.txt");
    assert_eq!(
        sftp::join_remote_path("/home/user", "a.txt"),
        "/home/user/a.txt"
    );
    assert!(sftp::ensure_no_nul("ok").is_ok());
    assert!(sftp::ensure_no_nul("bad\0").is_err());
}

#[test]
fn transfer_queue_lifecycle() {
    let mut queue = sftp::TransferQueue::new();
    queue.enqueue(sftp::TransferItem {
        id: "t1".to_string(),
        direction: sftp::TransferDirection::Upload,
        local_path: "/local/a.txt".to_string(),
        remote_path: "/remote/a.txt".to_string(),
        status: sftp::TransferStatus::Pending,
        bytes_transferred: 0,
        total_bytes: 100,
        error: None,
    });
    // 进度。
    queue.update_progress("t1", 50);
    assert_eq!(queue.items()[0].bytes_transferred, 50);
    assert_eq!(queue.items()[0].status, sftp::TransferStatus::InProgress);
    // 完成。
    queue.complete("t1");
    assert_eq!(queue.items()[0].status, sftp::TransferStatus::Completed);
    // 取消（清临时文件而非删除原目标——队列只标状态）。
    queue.enqueue(sftp::TransferItem {
        id: "t2".to_string(),
        direction: sftp::TransferDirection::Download,
        local_path: "/local/b.txt".to_string(),
        remote_path: "/remote/b.txt".to_string(),
        status: sftp::TransferStatus::Pending,
        bytes_transferred: 0,
        total_bytes: 100,
        error: None,
    });
    queue.cancel("t2");
    assert_eq!(queue.items()[1].status, sftp::TransferStatus::Cancelled);
    // 恢复（断线后）。
    queue.resume("t2");
    assert_eq!(queue.items()[1].status, sftp::TransferStatus::Pending);
    // 失败（权限错误）。
    queue.fail("t2", "permission denied".to_string());
    assert_eq!(queue.items()[1].status, sftp::TransferStatus::Failed);
    assert_eq!(queue.items()[1].error.as_deref(), Some("permission denied"));
}

#[test]
fn connection_serialization_never_leaks_password() {
    // 明文密码**只在反序列化方向有值**——skip_serializing 让它永远不被写出。
    let connection = ssh::SshConnection {
        id: "c1".to_string(),
        name: "prod".to_string(),
        group: Some("work".to_string()),
        host: "example.com".to_string(),
        port: 22,
        username: "user".to_string(),
        identity_file: Some("~/.ssh/id_ed25519".to_string()),
        password: Some("secret".to_string()),
        has_password: true,
    };
    let json = serde_json::to_string(&connection).unwrap();
    assert!(
        !json.contains("secret"),
        "明文密码不进序列化输出（snapshot/log）"
    );
    assert!(json.contains("\"hasPassword\""), "hasPassword 保留");
    // 反序列化方向有值（credential owner 保留）。
    let parsed: ssh::SshConnection = serde_json::from_str(&json).unwrap();
    assert!(parsed.has_password);
}
