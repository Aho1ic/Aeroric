//! SSH 远端侧的文件操作执行壳。
//!
//! 命令构造、路径围栏(绝对路径/项目根/`.git` 与 `.aeroric` 保护)与目录解析
//! 都在 [`crate::posix_fs`] 与 WSL 侧共用;这里只负责经 ssh 执行、把结果包装成
//! Tauri command,外加远端独有的本地上传通道(冲突检查 + scp)。错误文案保持
//! 「Remote/remote …」的历史拼写。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;

use base64::Engine;

use crate::posix_fs::{
    build_copy_paths_command, build_create_directory_command, build_create_file_command,
    build_delete_path_command, build_image_preview_command, build_read_dir_command,
    build_read_file_command, build_rename_path_command, build_resolve_path_command,
    build_write_file_command, ensure_path_allowed, image_mime_type, normalize_path,
    parse_dir_entries, parse_resolved_output, validate_entry_name, PathFlavor, PosixFsEntry,
    PosixImagePreviewData,
};
use crate::ssh::SshConnection;

/// 把连接记录在盘上存的明文密码回填进来。
///
/// 与 `sftp.rs::hydrate_endpoint_credentials` 同一动机:连接记录里的 `password` 带
/// `skip_serializing`,前端手里的对象恒为 `None`,所以这条链路原先只能靠公钥认证 ——
/// 对"只有密码"的主机,每次目录刷新都要先拿本机默认密钥(`~/.ssh/id_rsa`、
/// `id_ed25519`、agent 里的身份)去撞一遍服务端,直到 `MaxAuthTries` 耗尽,报
/// `Too many authentication failures`。终端那条链路一直有回填,这里补齐是为了让同一个
/// 连接在两条链路上走同一种认证方式,而不是"刚编辑过连接时能用、重启后就失效"。
///
/// 回填出密码后 [`crate::ssh::ssh_command_spec_from_args`] 会追加
/// `PubkeyAuthentication=no` —— 这正是想要的:不让公钥探测抢在密码之前把重试次数用光。
fn hydrate_connection(mut connection: SshConnection) -> Result<SshConnection, String> {
    crate::ssh::hydrate_ssh_password(&mut connection)?;
    Ok(connection)
}

fn run_ssh_output(connection: &SshConnection, remote_command: String) -> Result<Vec<u8>, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, remote_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(crate::ssh::annotate_ssh_error(
            connection,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    Ok(output.stdout)
}

fn resolve_remote_path_allowed(
    connection: &SshConnection,
    remote_path: &str,
    remote_project_path: Option<&str>,
    allow_project_root: bool,
) -> Result<String, String> {
    ensure_path_allowed(
        PathFlavor::Remote,
        remote_path,
        remote_project_path,
        allow_project_root,
    )?;
    let Some(remote_project_path) = remote_project_path else {
        return Ok(normalize_path(remote_path));
    };
    let output = run_ssh_output(
        connection,
        build_resolve_path_command(remote_path, remote_project_path),
    )?;
    parse_resolved_output(PathFlavor::Remote, &output, allow_project_root)
}

/// 上传前的同名冲突检查:本地源文件的 basename 在远端目标目录里逐一探测。
/// 与目录复制的检查同文案,但源是**本地**路径,basename 要用本地规则取。
fn build_remote_upload_conflict_check_command(
    local_source_paths: &[String],
    target_directory: &str,
) -> Result<String, String> {
    if local_source_paths.is_empty() {
        return Ok(":".to_string());
    }
    let target = crate::ssh::shell_quote_posix(target_directory);
    let names = local_source_paths
        .iter()
        .map(|source| {
            let name = Path::new(source)
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| "Invalid source file name".to_string())?;
            validate_entry_name(name)?;
            Ok(crate::ssh::shell_quote_posix(name))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!(
        "target={target}; [ -d \"$target\" ] && for name in {names}; do [ ! -e \"$target/$name\" ] || {{ echo \"A file or folder with that name already exists\" >&2; exit 1; }}; done",
        target = target,
        names = names.join(" ")
    ))
}

fn std_scp_upload_command(
    connection: &SshConnection,
    source_paths: &[String],
    target_directory: &str,
) -> Command {
    let password = connection
        .password
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    let mut cmd = if password.is_some() {
        let detected = crate::platform::detect_path("sshpass");
        let program = if detected.is_empty() {
            "sshpass".to_string()
        } else {
            detected
        };
        let mut command = Command::new(program);
        crate::subprocess::configure_background_command(&mut command);
        command.arg("-e").arg("scp");
        command
    } else {
        let mut command = Command::new("scp");
        crate::subprocess::configure_background_command(&mut command);
        command
    };

    cmd.arg("-P").arg(connection.port.to_string()).arg("-r");
    if let Some(identity_file) = connection
        .identity_file
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        cmd.arg("-i").arg(identity_file);
    }
    for source in source_paths {
        cmd.arg(source);
    }
    cmd.arg(format!(
        "{}@{}:{}",
        connection.username,
        connection.host,
        crate::ssh::shell_quote_posix(target_directory)
    ));
    if let Some(password) = password {
        cmd.env("SSHPASS", password);
    }
    cmd.env("PATH", crate::app_settings::get_login_shell_path());
    crate::subprocess::configure_background_command(&mut cmd);
    cmd
}

#[tauri::command]
pub async fn remote_read_dir_entries(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<Vec<PosixFsEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_path = resolve_remote_path_allowed(
            &connection,
            &remote_path,
            remote_project_path.as_deref(),
            true,
        )?;
        let stdout = run_ssh_output(&connection, build_read_dir_command(&resolved_path))?;
        let raw = String::from_utf8_lossy(&stdout);
        Ok(parse_dir_entries(&remote_path, &raw))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_read_file_content(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_path = resolve_remote_path_allowed(
            &connection,
            &remote_path,
            remote_project_path.as_deref(),
            true,
        )?;
        let stdout = run_ssh_output(&connection, build_read_file_command(&resolved_path))?;
        String::from_utf8(stdout).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_write_file_content(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
    content: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_path = resolve_remote_path_allowed(
            &connection,
            &remote_path,
            remote_project_path.as_deref(),
            false,
        )?;
        let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
            &connection,
            build_write_file_command(&resolved_path),
        );
        crate::subprocess::configure_background_command(&mut cmd);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| "Failed to open ssh stdin".to_string())?;
            stdin
                .write_all(content.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(crate::ssh::annotate_ssh_error(
                &connection,
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_read_image_preview(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<PosixImagePreviewData, String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_path = resolve_remote_path_allowed(
            &connection,
            &remote_path,
            remote_project_path.as_deref(),
            true,
        )?;
        let mime_type =
            image_mime_type(&remote_path).ok_or_else(|| "Unsupported image format".to_string())?;
        let stdout = run_ssh_output(&connection, build_image_preview_command(&resolved_path))?;
        let encoded = String::from_utf8_lossy(&stdout)
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(PosixImagePreviewData {
            data_url: format!(
                "data:{};base64,{}",
                mime_type,
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            ),
            mime_type: mime_type.to_string(),
            byte_length: bytes.len() as u64,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_create_file(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_path = resolve_remote_path_allowed(
            &connection,
            &remote_path,
            remote_project_path.as_deref(),
            false,
        )?;
        run_ssh_output(&connection, build_create_file_command(&resolved_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_create_directory(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_path = resolve_remote_path_allowed(
            &connection,
            &remote_path,
            remote_project_path.as_deref(),
            false,
        )?;
        run_ssh_output(&connection, build_create_directory_command(&resolved_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_delete_path(
    connection: SshConnection,
    remote_path: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_path = resolve_remote_path_allowed(
            &connection,
            &remote_path,
            remote_project_path.as_deref(),
            false,
        )?;
        run_ssh_output(&connection, build_delete_path_command(&resolved_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_rename_path(
    connection: SshConnection,
    remote_path: String,
    new_name: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_path = resolve_remote_path_allowed(
            &connection,
            &remote_path,
            remote_project_path.as_deref(),
            false,
        )?;
        let command = build_rename_path_command(&resolved_path, new_name.trim())?;
        run_ssh_output(&connection, command).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_copy_paths_to_directory(
    connection: SshConnection,
    source_paths: Vec<String>,
    target_directory: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_target = resolve_remote_path_allowed(
            &connection,
            &target_directory,
            remote_project_path.as_deref(),
            true,
        )?;
        let resolved_sources = source_paths
            .iter()
            .map(|source| {
                resolve_remote_path_allowed(
                    &connection,
                    source,
                    remote_project_path.as_deref(),
                    false,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let command = build_copy_paths_command(&resolved_sources, &resolved_target)?;
        run_ssh_output(&connection, command).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_upload_local_paths_to_directory(
    connection: SshConnection,
    local_source_paths: Vec<String>,
    target_directory: String,
    remote_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let connection = hydrate_connection(connection)?;
        let resolved_target = resolve_remote_path_allowed(
            &connection,
            &target_directory,
            remote_project_path.as_deref(),
            true,
        )?;
        if local_source_paths.is_empty() {
            return Ok(());
        }
        let mut validated_sources = Vec::with_capacity(local_source_paths.len());
        for source in local_source_paths {
            let source_path = PathBuf::from(&source);
            if !source_path.is_absolute() {
                return Err("Source path must be absolute".to_string());
            }
            if !source_path.exists() {
                return Err(format!("Source path does not exist: {}", source));
            }
            let name = source_path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| "Invalid source file name".to_string())?;
            validate_entry_name(name)?;
            validated_sources.push(source);
        }
        run_ssh_output(
            &connection,
            build_remote_upload_conflict_check_command(&validated_sources, &resolved_target)?,
        )?;
        let output = std_scp_upload_command(&connection, &validated_sources, &resolved_target)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(crate::ssh::annotate_ssh_error(
                &connection,
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_connection() -> SshConnection {
        SshConnection {
            id: "conn-win-laptop".to_string(),
            name: "Windows笔记本".to_string(),
            group: None,
            host: "192.168.0.106".to_string(),
            port: 22,
            username: "administrator".to_string(),
            identity_file: None,
            password: None,
            has_password: true,
            remote_path: None,
            auto_sudo_with_password: false,
            use_proxy: false,
            created_at: 1,
            last_connected_at: None,
        }
    }

    /// 回填对"已经带着明文"的连接是恒等变换 —— 新建/编辑对话框里「测试连接」用的就是
    /// 这种入参,用户刚敲进去的密码必须赢过盘上存的那份。
    ///
    /// 断言刻意只覆盖这条提前返回的路径:走到读盘分支的用例会依赖真实的
    /// `~/.aeroric/ssh-passwords.json`,在同一台机器上跑就成了不可复现的测试。
    #[test]
    fn hydrate_connection_keeps_an_already_supplied_password() {
        let connection = hydrate_connection(SshConnection {
            password: Some("typed-just-now".to_string()),
            ..test_connection()
        })
        .expect("hydrate");

        assert_eq!(connection.password.as_deref(), Some("typed-just-now"));
    }

    #[test]
    fn upload_conflict_command_checks_local_basenames_on_remote_target() {
        assert_eq!(
            build_remote_upload_conflict_check_command(
                &[
                    "/Users/me/Desktop/a file.txt".to_string(),
                    "/Users/me/Desktop/folder".to_string(),
                ],
                "/srv/app/target dir",
            )
            .unwrap(),
            "target='/srv/app/target dir'; [ -d \"$target\" ] && for name in 'a file.txt' 'folder'; do [ ! -e \"$target/$name\" ] || { echo \"A file or folder with that name already exists\" >&2; exit 1; }; done"
        );
        assert_eq!(
            build_remote_upload_conflict_check_command(&[], "/srv/app").unwrap(),
            ":"
        );
    }

    /// 远端侧的围栏文案走「Remote/remote」拼写;相对段判定用 path_guard 的实现。
    #[test]
    fn remote_fence_rejects_escaping_paths_with_remote_wording() {
        use crate::path_guard::remote_path_has_relative_components;

        assert!(
            ensure_path_allowed(PathFlavor::Remote, "/srv/app", Some("/srv/app"), true).is_ok()
        );
        assert!(ensure_path_allowed(
            PathFlavor::Remote,
            "/srv/app/../outside.txt",
            Some("/srv/app"),
            false
        )
        .is_err());
        assert!(remote_path_has_relative_components(
            "/srv/app/../outside.txt"
        ));
        assert!(
            ensure_path_allowed(PathFlavor::Remote, "srv/app", Some("/srv/app"), false).is_err()
        );
        assert_eq!(
            ensure_path_allowed(PathFlavor::Remote, "/srv/app", Some("/srv/app"), false)
                .unwrap_err(),
            "Cannot modify the remote project root"
        );
    }
}
