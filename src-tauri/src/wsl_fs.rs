//! WSL 侧的文件操作执行壳。
//!
//! 命令构造、路径围栏(绝对路径/项目根/`.git` 与 `.aeroric` 保护)与目录解析
//! 都在 [`crate::posix_fs`] 与 SSH 远端共用;这里只负责经 `wsl.exe` 执行、
//! 把结果包装成 Tauri command。错误文案保持「WSL …」的历史拼写。

use std::io::Write;
use std::process::Stdio;

use base64::Engine;

use crate::posix_fs::{
    build_copy_paths_command, build_create_directory_command, build_create_file_command,
    build_delete_path_command, build_image_preview_command, build_read_dir_command,
    build_read_file_command, build_rename_path_command, build_resolve_path_command,
    build_write_file_command, ensure_path_allowed, image_mime_type, normalize_path,
    parse_dir_entries, parse_resolved_output, PathFlavor, PosixFsEntry, PosixImagePreviewData,
};

fn run_wsl_output(distribution: &str, wsl_command: String) -> Result<Vec<u8>, String> {
    let mut cmd = crate::wsl::std_wsl_shell_command(distribution, wsl_command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

fn resolve_wsl_path_allowed(
    distribution: &str,
    linux_path: &str,
    linux_project_path: Option<&str>,
    allow_project_root: bool,
) -> Result<String, String> {
    ensure_path_allowed(
        PathFlavor::Wsl,
        linux_path,
        linux_project_path,
        allow_project_root,
    )?;
    let Some(linux_project_path) = linux_project_path else {
        return Ok(normalize_path(linux_path));
    };
    let output = run_wsl_output(
        distribution,
        build_resolve_path_command(linux_path, linux_project_path),
    )?;
    parse_resolved_output(PathFlavor::Wsl, &output, allow_project_root)
}

#[tauri::command]
pub async fn wsl_read_dir_entries(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<Vec<PosixFsEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            true,
        )?;
        let stdout = run_wsl_output(&distribution, build_read_dir_command(&resolved_path))?;
        let raw = String::from_utf8_lossy(&stdout);
        Ok(parse_dir_entries(&linux_path, &raw))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_read_file_content(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            true,
        )?;
        let stdout = run_wsl_output(&distribution, build_read_file_command(&resolved_path))?;
        String::from_utf8(stdout).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_write_file_content(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
    content: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        let mut cmd = crate::wsl::std_wsl_shell_command(
            &distribution,
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
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_read_image_preview(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<PosixImagePreviewData, String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            true,
        )?;
        let mime_type =
            image_mime_type(&linux_path).ok_or_else(|| "Unsupported image format".to_string())?;
        let stdout = run_wsl_output(&distribution, build_image_preview_command(&resolved_path))?;
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
pub async fn wsl_create_file(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        run_wsl_output(&distribution, build_create_file_command(&resolved_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_create_directory(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        run_wsl_output(
            &distribution,
            build_create_directory_command(&resolved_path),
        )
        .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_delete_path(
    distribution: String,
    linux_path: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        run_wsl_output(&distribution, build_delete_path_command(&resolved_path)).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_rename_path(
    distribution: String,
    linux_path: String,
    new_name: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_path = resolve_wsl_path_allowed(
            &distribution,
            &linux_path,
            linux_project_path.as_deref(),
            false,
        )?;
        let command = build_rename_path_command(&resolved_path, new_name.trim())?;
        run_wsl_output(&distribution, command).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wsl_copy_paths_to_directory(
    distribution: String,
    source_paths: Vec<String>,
    target_directory: String,
    linux_project_path: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let resolved_target = resolve_wsl_path_allowed(
            &distribution,
            &target_directory,
            linux_project_path.as_deref(),
            true,
        )?;
        let resolved_sources = source_paths
            .iter()
            .map(|source| {
                resolve_wsl_path_allowed(
                    &distribution,
                    source,
                    linux_project_path.as_deref(),
                    false,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let command = build_copy_paths_command(&resolved_sources, &resolved_target)?;
        run_wsl_output(&distribution, command).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WSL 侧的围栏走共享实现,但错误文案必须保持「WSL …」的历史拼写,
    /// 项目根与 .git/.aeroric 保护一个都不能少。
    #[test]
    fn wsl_paths_stay_inside_project_root_with_wsl_wording() {
        assert!(
            ensure_path_allowed(PathFlavor::Wsl, "/home/me/app", Some("/home/me/app"), true)
                .is_ok()
        );
        assert!(ensure_path_allowed(
            PathFlavor::Wsl,
            "/home/me/app/../secret",
            Some("/home/me/app"),
            false
        )
        .is_err());
        assert!(
            ensure_path_allowed(PathFlavor::Wsl, "/etc/passwd", Some("/home/me/app"), false)
                .is_err()
        );
        assert!(ensure_path_allowed(
            PathFlavor::Wsl,
            "/home/me/app/.git/config",
            Some("/home/me/app"),
            false
        )
        .is_err());
        assert!(ensure_path_allowed(
            PathFlavor::Wsl,
            "/home/me/app/.aeroric/config.toml",
            Some("/home/me/app"),
            false
        )
        .is_err());
        assert!(
            ensure_path_allowed(PathFlavor::Wsl, "/home/me/app", Some("/home/me/app"), false)
                .is_err()
        );
        assert!(
            ensure_path_allowed(PathFlavor::Wsl, "home/me/app", Some("/home/me/app"), false)
                .is_err()
        );
    }
}
