use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatterCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatFileResult {
    pub file_path: String,
    pub command: String,
}

fn validate_project_root(project_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    if !canonical.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    Ok(canonical)
}

fn validate_file_path(project_root: &Path, file_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(file_path);
    if !path.is_absolute() {
        return Err("File path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve file path: {e}"))?;
    if !canonical.starts_with(project_root) {
        return Err("File path is outside the project".to_string());
    }
    if !canonical.is_file() {
        return Err("File path is not a file".to_string());
    }
    Ok(canonical)
}

pub fn formatter_for_extension(extension: &str, file_path: &str) -> Option<FormatterCommand> {
    let ext = extension.trim_start_matches('.').to_ascii_lowercase();
    match ext.as_str() {
        "js" | "jsx" | "ts" | "tsx" | "css" | "scss" | "json" | "jsonc" | "md" | "mdx" | "html"
        | "yaml" | "yml" => Some(FormatterCommand {
            program: "pnpm".to_string(),
            args: vec![
                "exec".to_string(),
                "prettier".to_string(),
                "--write".to_string(),
                file_path.to_string(),
            ],
        }),
        "rs" => Some(FormatterCommand {
            program: "rustfmt".to_string(),
            args: vec![file_path.to_string()],
        }),
        "go" => Some(FormatterCommand {
            program: "gofmt".to_string(),
            args: vec!["-w".to_string(), file_path.to_string()],
        }),
        "py" => Some(FormatterCommand {
            program: "ruff".to_string(),
            args: vec!["format".to_string(), file_path.to_string()],
        }),
        _ => None,
    }
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
