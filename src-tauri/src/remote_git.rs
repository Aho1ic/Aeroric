use crate::ssh::SshConnection;

fn build_remote_git_command(remote_project_path: &str, args: &[&str]) -> String {
    let quoted_args = args
        .iter()
        .map(|arg| crate::ssh::shell_word_posix(arg))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "cd -- {} && git {}",
        crate::ssh::shell_quote_posix(remote_project_path),
        quoted_args
    )
}

fn run_remote_git(
    connection: &SshConnection,
    remote_project_path: &str,
    args: &[&str],
) -> Result<String, String> {
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_git_command(remote_project_path, args),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[tauri::command]
pub async fn remote_git_status(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        run_remote_git(
            &connection,
            &remote_project_path,
            &["status", "--short", "--branch"],
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_show_diff(
    connection: SshConnection,
    remote_project_path: String,
    file_path: Option<String>,
    staged: Option<bool>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let mut args = vec!["diff"];
        if staged.unwrap_or(false) {
            args.push("--staged");
        }
        if let Some(ref file_path) = file_path {
            args.push("--");
            args.push(file_path);
        }
        run_remote_git(&connection, &remote_project_path, &args)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_command_changes_directory_and_quotes_arguments() {
        assert_eq!(
            build_remote_git_command("/srv/app's repo", &["diff", "--", "src/main file.rs"]),
            "cd -- '/srv/app'\\''s repo' && git diff -- 'src/main file.rs'"
        );
    }
}
