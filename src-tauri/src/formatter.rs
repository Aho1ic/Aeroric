use crate::path_guard::validate_project_root;
use aeroric_core::files::format::validate_file_path;
pub use aeroric_core::files::format::{FormatFileResult, FormatterCommand};
use std::process::Command;

pub fn formatter_for_extension(extension: &str, file_path: &str) -> Option<FormatterCommand> {
    aeroric_core::files::format::formatter_for_extension(extension, file_path)
}

#[tauri::command]
pub async fn format_file(
    project_path: String,
    file_path: String,
) -> Result<FormatFileResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = validate_project_root(&project_path)?;
        let file = validate_file_path(&root, &file_path)?;
        let file_string = file.to_string_lossy().into_owned();
        crate::local_history::record_snapshot_before_write(&project_path, &file_string, "")?;
        let extension = file
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| "No formatter is configured for this file type.".to_string())?;
        let formatter = formatter_for_extension(extension, &file_string)
            .ok_or_else(|| "No formatter is configured for this file type.".to_string())?;
        let mut cmd = Command::new(&formatter.program);
        crate::subprocess::configure_background_command(&mut cmd);
        let output = cmd
            .args(&formatter.args)
            .current_dir(&root)
            .envs(crate::app_settings::get_login_shell_env().iter().cloned())
            .output()
            .map_err(|e| format!("Failed to run formatter: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Formatter failed: {stderr}"));
        }
        Ok(FormatFileResult {
            file_path: file_string,
            command: format!("{} {}", formatter.program, formatter.args.join(" ")),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_prettier_for_typescript_files() {
        let command = formatter_for_extension("tsx", "/repo/src/App.tsx").unwrap();

        assert_eq!(command.program, "pnpm");
        assert_eq!(
            command.args,
            vec!["exec", "prettier", "--write", "/repo/src/App.tsx"]
        );
    }

    #[test]
    fn selects_rustfmt_for_rust_files() {
        let command = formatter_for_extension("rs", "/repo/src/lib.rs").unwrap();

        assert_eq!(command.program, "rustfmt");
        assert_eq!(command.args, vec!["/repo/src/lib.rs"]);
    }

    #[test]
    fn returns_none_for_unknown_extensions() {
        assert!(formatter_for_extension("png", "/repo/image.png").is_none());
    }
}

#[cfg(test)]
#[cfg(unix)]
mod legacy_formatter_tests;
