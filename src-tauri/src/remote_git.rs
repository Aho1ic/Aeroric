use std::{
    collections::HashSet,
    io::Write,
    process::{Output, Stdio},
};

use crate::ssh::SshConnection;

const WORKTREE_DIFF_LIMIT: usize = 200 * 1024;
const COMMIT_DIFF_LIMIT: usize = 500 * 1024;
const REMOTE_CONFLICT_FILE_LIMIT: usize = 2 * 1024 * 1024;

fn normalize_remote_project_path(remote_project_path: &str) -> Result<String, String> {
    let trimmed = remote_project_path.trim();
    if !trimmed.starts_with('/') {
        return Err("Remote project path must be absolute".to_string());
    }
    if trimmed
        .split('/')
        .any(|component| component == "." || component == "..")
    {
        return Err("Remote project path cannot contain . or .. components".to_string());
    }
    if trimmed == "/" {
        Ok("/".to_string())
    } else {
        Ok(trimmed.trim_end_matches('/').to_string())
    }
}

fn validate_remote_git_relative_path(file_path: &str) -> Result<(), String> {
    if file_path.is_empty() {
        return Err("File path must not be empty".to_string());
    }
    if file_path.starts_with('/') {
        return Err("File path must be relative".to_string());
    }
    if file_path.split('/').any(|part| part == "." || part == "..") {
        return Err("File path must stay inside the remote git worktree".to_string());
    }
    if file_path.contains('\0') {
        return Err("File path must not contain NUL bytes".to_string());
    }
    Ok(())
}

fn is_protected_remote_git_relative_path(file_path: &str) -> bool {
    file_path
        .split('/')
        .find(|component| !component.is_empty())
        .map(|component| {
            component.eq_ignore_ascii_case(".git") || component.eq_ignore_ascii_case(".aeroric")
        })
        .unwrap_or(false)
}

fn validate_remote_git_discard_path(file_path: &str) -> Result<(), String> {
    validate_remote_git_relative_path(file_path)?;
    if is_protected_remote_git_relative_path(file_path) {
        return Err("Refusing to delete protected project metadata".to_string());
    }
    Ok(())
}

fn validate_remote_git_revision(revision: &str) -> Result<(), String> {
    if revision.is_empty() {
        return Err("Git revision must not be empty".to_string());
    }
    if revision.starts_with('-') || revision.contains('\0') {
        return Err("Invalid git revision".to_string());
    }
    Ok(())
}

fn build_remote_git_command(remote_project_path: &str, args: &[String]) -> String {
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

fn run_remote_git_output(
    connection: &SshConnection,
    remote_project_path: &str,
    args: &[String],
) -> Result<Output, String> {
    let remote_root = normalize_remote_project_path(remote_project_path)?;
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_git_command(&remote_root, args),
    );
    crate::subprocess::configure_background_command(&mut cmd);
    cmd.output().map_err(|e| e.to_string())
}

fn output_error(output: &Output, fallback: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = format!("{}{}", stderr, stdout).trim().to_string();
    if message.is_empty() {
        fallback.to_string()
    } else {
        message
    }
}

fn run_remote_git(
    connection: &SshConnection,
    remote_project_path: &str,
    args: &[String],
) -> Result<String, String> {
    let output = run_remote_git_output(connection, remote_project_path, args)?;
    if !output.status.success() {
        return Err(output_error(&output, "Remote git command failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn trim_output(output: Vec<u8>, limit: usize) -> String {
    String::from_utf8_lossy(if output.len() > limit {
        &output[..limit]
    } else {
        &output
    })
    .into_owned()
}

fn str_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

fn unique_remote_git_file_paths(file_paths: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for file_path in file_paths {
        validate_remote_git_relative_path(&file_path)?;
        if seen.insert(file_path.clone()) {
            unique.push(file_path);
        }
    }
    Ok(unique)
}

fn remote_git_path_args(base: &[&str], file_paths: Vec<String>) -> Result<Vec<String>, String> {
    let file_paths = unique_remote_git_file_paths(file_paths)?;
    if file_paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = str_args(base);
    args.push("--".to_string());
    args.extend(file_paths);
    Ok(args)
}

fn remote_git_unstage_args(has_head: bool, file_paths: Vec<String>) -> Result<Vec<String>, String> {
    if has_head {
        remote_git_path_args(&["restore", "--staged"], file_paths)
    } else {
        remote_git_path_args(&["reset"], file_paths)
    }
}

fn remote_git_discard_files_args(
    file_paths: Vec<String>,
    untracked: bool,
) -> Result<Vec<String>, String> {
    let mut file_paths = unique_remote_git_file_paths(file_paths)?;
    if untracked {
        for file_path in &file_paths {
            validate_remote_git_discard_path(file_path)?;
        }
    }
    if file_paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = if untracked {
        str_args(&["clean", "-f"])
    } else {
        str_args(&["restore"])
    };
    args.push("--".to_string());
    args.append(&mut file_paths);
    Ok(args)
}

fn remote_git_push_args(branch: Option<&str>) -> Result<Vec<String>, String> {
    let mut args = str_args(&["push"]);
    if let Some(branch) = branch.filter(|branch| !branch.is_empty()) {
        validate_remote_git_revision(branch)?;
        args.push("origin".to_string());
        args.push(branch.to_string());
    }
    Ok(args)
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn remote_git_has_head(
    connection: &SshConnection,
    remote_project_path: &str,
) -> Result<bool, String> {
    let output = run_remote_git_output(
        connection,
        remote_project_path,
        &str_args(&["rev-parse", "--verify", "HEAD"]),
    )?;
    Ok(output.status.success())
}

fn remote_git_list_untracked_files(
    connection: &SshConnection,
    remote_project_path: &str,
) -> Result<Vec<String>, String> {
    let output = run_remote_git_output(
        connection,
        remote_project_path,
        &str_args(&["ls-files", "--others", "--exclude-standard", "-z"]),
    )?;
    if !output.status.success() {
        return Err(output_error(
            &output,
            "Failed to list remote untracked files",
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter(|path| validate_remote_git_discard_path(path).is_ok())
        .map(|path| path.to_string())
        .collect())
}

fn remote_git_absolute_file_path(
    remote_project_path: &str,
    file_path: &str,
) -> Result<String, String> {
    validate_remote_git_relative_path(file_path)?;
    if is_protected_remote_git_relative_path(file_path) {
        return Err("Refusing to access protected project metadata".to_string());
    }
    let remote_root = normalize_remote_project_path(remote_project_path)?;
    Ok(if remote_root == "/" {
        format!("/{}", file_path.trim_start_matches('/'))
    } else {
        format!("{}/{}", remote_root, file_path)
    })
}

fn remote_git_read_text_file(
    connection: &SshConnection,
    remote_project_path: &str,
    file_path: &str,
) -> Result<String, String> {
    let remote_path = remote_git_absolute_file_path(remote_project_path, file_path)?;
    let quoted_path = crate::ssh::shell_quote_posix(&remote_path);
    let command = format!(
        "size=$(wc -c < {quoted_path}) && [ \"$size\" -le {REMOTE_CONFLICT_FILE_LIMIT} ] && cat -- {quoted_path}"
    );
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, command);
    crate::subprocess::configure_background_command(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(output_error(&output, "Failed to read remote git file"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn remote_git_write_text_file(
    connection: &SshConnection,
    remote_project_path: &str,
    file_path: &str,
    content: &str,
) -> Result<(), String> {
    let remote_path = remote_git_absolute_file_path(remote_project_path, file_path)?;
    let command = format!("cat > {}", crate::ssh::shell_quote_posix(&remote_path));
    let mut cmd = crate::ssh::std_ssh_command_for_remote_command(connection, command);
    crate::subprocess::configure_background_command(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open remote git file writer".to_string())?;
        stdin
            .write_all(content.as_bytes())
            .map_err(|e| format!("Failed to write remote git file: {}", e))?;
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(output_error(&output, "Failed to write remote git file"));
    }
    Ok(())
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
            &str_args(&["status", "--short", "--branch"]),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_changes(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<Vec<crate::git::GitFileChange>, String> {
    tokio::task::spawn_blocking(move || {
        let args = str_args(&[
            "-c",
            "core.quotePath=false",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ]);
        let output = run_remote_git_output(&connection, &remote_project_path, &args)?;
        if !output.status.success() {
            return Err(output_error(&output, "Failed to get remote git status"));
        }
        Ok(crate::git::parse_porcelain_z_status(&output.stdout))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_list_branches(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<Vec<crate::git::GitBranchInfo>, String> {
    tokio::task::spawn_blocking(move || {
        let stdout = run_remote_git(
            &connection,
            &remote_project_path,
            &str_args(&["branch", "-a"]),
        )?;
        Ok(crate::git::parse_git_branch_list(&stdout))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_log(
    connection: SshConnection,
    remote_project_path: String,
    limit: u32,
    search: Option<String>,
    branch: Option<String>,
) -> Result<Vec<crate::git::GitCommit>, String> {
    tokio::task::spawn_blocking(move || {
        if let Some(branch) = branch.as_deref().filter(|value| !value.is_empty()) {
            validate_remote_git_revision(branch)?;
        }
        let args = crate::git::build_git_log_args(limit, search.as_deref(), branch.as_deref());
        let stdout = run_remote_git(&connection, &remote_project_path, &args)?;
        Ok(crate::git::parse_git_log_output(&stdout))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_commit_detail(
    connection: SshConnection,
    remote_project_path: String,
    commit_hash: String,
) -> Result<crate::git::GitCommitDetail, String> {
    tokio::task::spawn_blocking(move || {
        validate_remote_git_revision(&commit_hash)?;
        let info_out = run_remote_git(
            &connection,
            &remote_project_path,
            &["show".to_string(),
                "--no-patch".to_string(),
                "--format=HASH:%H%nSHORT:%h%nAUTHOR:%an%nDATE:%ar%nSUBJECT:%s".to_string(),
                commit_hash.clone()],
        )?;
        let name_status_out = run_remote_git(
            &connection,
            &remote_project_path,
            &["diff-tree".to_string(),
                "--no-commit-id".to_string(),
                "-r".to_string(),
                "--name-status".to_string(),
                commit_hash.clone()],
        )?;
        let numstat_out = run_remote_git(
            &connection,
            &remote_project_path,
            &["diff-tree".to_string(),
                "--no-commit-id".to_string(),
                "-r".to_string(),
                "--numstat".to_string(),
                commit_hash],
        )?;
        Ok(crate::git::parse_git_commit_detail(
            &info_out,
            &name_status_out,
            &numstat_out,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_remote_counts(
    connection: SshConnection,
    remote_project_path: String,
    branch: Option<String>,
) -> Result<crate::git::GitRemoteCounts, String> {
    tokio::task::spawn_blocking(move || {
        let branch = if let Some(branch) = branch.filter(|branch| !branch.is_empty()) {
            branch
        } else {
            run_remote_git(
                &connection,
                &remote_project_path,
                &str_args(&["rev-parse", "--abbrev-ref", "HEAD"]),
            )?
            .trim()
            .to_string()
        };

        let rev_str = format!("{}...@{{u}}", branch);
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &str_args(&["rev-list", "--count", "--left-right", &rev_str]),
        )?;
        let rev_stdout = if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        };
        Ok(crate::git::git_remote_counts_from_rev_list(
            branch,
            rev_stdout.as_deref(),
        ))
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
        let mut args = str_args(&["diff"]);
        if staged.unwrap_or(false) {
            args.push("--cached".to_string());
        }
        if let Some(ref file_path) = file_path {
            validate_remote_git_relative_path(file_path)?;
            args.push("--".to_string());
            args.push(file_path.clone());
        }
        run_remote_git(&connection, &remote_project_path, &args)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_show_commit_diff(
    connection: SshConnection,
    remote_project_path: String,
    commit_hash: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        validate_remote_git_revision(&commit_hash)?;
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &["show".to_string(), "--format=".to_string(), commit_hash],
        )?;
        if !output.status.success() {
            return Err(output_error(&output, "Failed to get remote commit diff"));
        }
        Ok(trim_output(output.stdout, COMMIT_DIFF_LIMIT))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_show_file_diff(
    connection: SshConnection,
    remote_project_path: String,
    commit_hash: String,
    file_path: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        validate_remote_git_revision(&commit_hash)?;
        validate_remote_git_relative_path(&file_path)?;
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &["show".to_string(),
                "--format=".to_string(),
                commit_hash,
                "--".to_string(),
                file_path],
        )?;
        if !output.status.success() {
            return Err(output_error(&output, "Failed to get remote file diff"));
        }
        Ok(trim_output(output.stdout, COMMIT_DIFF_LIMIT))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_file_diff(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
    staged: bool,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        validate_remote_git_relative_path(&file_path)?;
        let mut args = str_args(&["diff"]);
        if staged {
            args.push("--cached".to_string());
        }
        args.push("--".to_string());
        args.push(file_path.clone());

        let output = run_remote_git_output(&connection, &remote_project_path, &args)?;
        if !output.status.success() {
            return Err(output_error(&output, "Failed to get remote file diff"));
        }
        if !output.stdout.is_empty() || staged {
            return Ok(trim_output(output.stdout, WORKTREE_DIFF_LIMIT));
        }

        let fallback = run_remote_git_output(
            &connection,
            &remote_project_path,
            &["diff".to_string(),
                "--no-index".to_string(),
                "--".to_string(),
                "/dev/null".to_string(),
                file_path],
        )?;
        if !fallback.status.success() && fallback.status.code() != Some(1) {
            return Err(output_error(
                &fallback,
                "Failed to get remote untracked file diff",
            ));
        }
        Ok(trim_output(fallback.stdout, WORKTREE_DIFF_LIMIT))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_stage(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        validate_remote_git_relative_path(&file_path)?;
        run_remote_git(
            &connection,
            &remote_project_path,
            &["add".to_string(), "--".to_string(), file_path],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_unstage(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let has_head = remote_git_has_head(&connection, &remote_project_path)?;
        let args = remote_git_unstage_args(has_head, vec![file_path])?;
        if args.is_empty() {
            return Ok(());
        }
        run_remote_git(&connection, &remote_project_path, &args)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_stage_files(
    connection: SshConnection,
    remote_project_path: String,
    file_paths: Vec<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let args = remote_git_path_args(&["add"], file_paths)?;
        if args.is_empty() {
            return Ok(());
        }
        run_remote_git(&connection, &remote_project_path, &args)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_unstage_files(
    connection: SshConnection,
    remote_project_path: String,
    file_paths: Vec<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let has_head = remote_git_has_head(&connection, &remote_project_path)?;
        let args = remote_git_unstage_args(has_head, file_paths)?;
        if args.is_empty() {
            return Ok(());
        }
        run_remote_git(&connection, &remote_project_path, &args)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_stage_all(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        run_remote_git(&connection, &remote_project_path, &str_args(&["add", "-A"]))?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_unstage_all(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let has_head = remote_git_has_head(&connection, &remote_project_path)?;
        let args = if has_head {
            str_args(&["restore", "--staged", "."])
        } else {
            str_args(&["reset", "--", "."])
        };
        run_remote_git(&connection, &remote_project_path, &args)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_commit(
    connection: SshConnection,
    remote_project_path: String,
    message: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let message = message.trim().to_string();
        if message.is_empty() {
            return Err("Commit message must not be empty".to_string());
        }
        run_remote_git(
            &connection,
            &remote_project_path,
            &["commit".to_string(), "-m".to_string(), message],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_discard_file(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
    untracked: bool,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let args = remote_git_discard_files_args(vec![file_path], untracked)?;
        if args.is_empty() {
            return Ok(());
        }
        run_remote_git(&connection, &remote_project_path, &args)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_discard_files(
    connection: SshConnection,
    remote_project_path: String,
    file_paths: Vec<String>,
    untracked: bool,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let args = remote_git_discard_files_args(file_paths, untracked)?;
        if args.is_empty() {
            return Ok(());
        }
        run_remote_git(&connection, &remote_project_path, &args)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_discard_all(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if remote_git_has_head(&connection, &remote_project_path)? {
            run_remote_git(
                &connection,
                &remote_project_path,
                &str_args(&["restore", "--source=HEAD", "--staged", "--worktree", "."]),
            )?;
        } else {
            run_remote_git(
                &connection,
                &remote_project_path,
                &str_args(&["rm", "-r", "--cached", "--ignore-unmatch", "--", "."]),
            )?;
        }

        let untracked_files = remote_git_list_untracked_files(&connection, &remote_project_path)?;
        let args = remote_git_discard_files_args(untracked_files, true)?;
        if !args.is_empty() {
            run_remote_git(&connection, &remote_project_path, &args)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_push(
    connection: SshConnection,
    remote_project_path: String,
    branch: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let args = remote_git_push_args(branch.as_deref())?;
        let output = run_remote_git_output(&connection, &remote_project_path, &args)?;
        let combined = combined_output(&output);
        if !output.status.success() {
            return Err(combined);
        }
        Ok(combined.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_pull(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let output =
            run_remote_git_output(&connection, &remote_project_path, &str_args(&["pull"]))?;
        let combined = combined_output(&output);
        if !output.status.success() {
            return Err(combined);
        }
        Ok(combined.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_blame_file(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
) -> Result<crate::git::GitBlameResult, String> {
    tokio::task::spawn_blocking(move || {
        validate_remote_git_relative_path(&file_path)?;
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &["-c".to_string(),
                "core.quotePath=false".to_string(),
                "blame".to_string(),
                "--line-porcelain".to_string(),
                "--".to_string(),
                file_path.clone()],
        )?;
        if !output.status.success() {
            return Err(output_error(&output, "Failed to load remote git blame"));
        }
        Ok(crate::git::GitBlameResult {
            file_path,
            lines: crate::git::parse_blame_porcelain(&output.stdout),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_branch_graph(
    connection: SshConnection,
    remote_project_path: String,
    limit: Option<u32>,
) -> Result<crate::git::GitBranchGraphResult, String> {
    tokio::task::spawn_blocking(move || {
        if !remote_git_has_head(&connection, &remote_project_path)? {
            return Ok(crate::git::GitBranchGraphResult {
                commits: Vec::new(),
                truncated: false,
            });
        }
        let limit = limit.unwrap_or(80).clamp(1, 200);
        let fetch_limit = limit + 1;
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &["log".to_string(),
                "--all".to_string(),
                "--decorate=short".to_string(),
                "--date=relative".to_string(),
                "--pretty=format:%H%x1f%P%x1f%D%x1f%an%x1f%cr%x1f%s".to_string(),
                "-n".to_string(),
                fetch_limit.to_string()],
        )?;
        if !output.status.success() {
            return Err(output_error(
                &output,
                "Failed to load remote git branch graph",
            ));
        }
        let mut commits = crate::git::parse_branch_graph_log(&output.stdout);
        let truncated = commits.len() > limit as usize;
        if truncated {
            commits.truncate(limit as usize);
        }
        Ok(crate::git::GitBranchGraphResult { commits, truncated })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_stash_list(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<Vec<crate::git::GitStashEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &str_args(&["stash", "list", "--format=%gd%x1f%H%x1f%cr%x1f%s"]),
        )?;
        if !output.status.success() {
            return Err(output_error(&output, "Failed to list remote git stashes"));
        }
        Ok(crate::git::parse_stash_list(&output.stdout))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_stash_diff(
    connection: SshConnection,
    remote_project_path: String,
    stash_ref: String,
) -> Result<crate::git::GitStashDiff, String> {
    tokio::task::spawn_blocking(move || {
        crate::git::validate_stash_ref(&stash_ref)?;
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &["stash".to_string(),
                "show".to_string(),
                "--patch".to_string(),
                "--stat".to_string(),
                "--include-untracked".to_string(),
                "--no-ext-diff".to_string(),
                "--no-color".to_string(),
                stash_ref.clone()],
        )?;
        if !output.status.success() {
            return Err(output_error(
                &output,
                "Failed to load remote git stash diff",
            ));
        }
        let raw = String::from_utf8_lossy(&output.stdout).into_owned();
        let (diff, truncated) = crate::git::truncate_text(raw, crate::git::MAX_STASH_DIFF_CHARS);
        Ok(crate::git::GitStashDiff {
            stash_ref,
            diff,
            truncated,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_stash_push(
    connection: SshConnection,
    remote_project_path: String,
    message: Option<String>,
    include_untracked: bool,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let mut args = str_args(&["stash", "push"]);
        if include_untracked {
            args.push("--include-untracked".to_string());
        }
        if let Some(message) = message
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            args.push("-m".to_string());
            args.push(message);
        }
        let output = run_remote_git_output(&connection, &remote_project_path, &args)?;
        let combined = combined_output(&output).trim().to_string();
        if !output.status.success() {
            return Err(if combined.is_empty() {
                "Failed to create remote git stash".to_string()
            } else {
                combined
            });
        }
        Ok(combined)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_stash_apply(
    connection: SshConnection,
    remote_project_path: String,
    stash_ref: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        crate::git::validate_stash_ref(&stash_ref)?;
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &["stash".to_string(), "apply".to_string(), stash_ref],
        )?;
        let combined = combined_output(&output).trim().to_string();
        if !output.status.success() {
            return Err(if combined.is_empty() {
                "Failed to apply remote git stash".to_string()
            } else {
                combined
            });
        }
        Ok(combined)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_stash_drop(
    connection: SshConnection,
    remote_project_path: String,
    stash_ref: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        crate::git::validate_stash_ref(&stash_ref)?;
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &["stash".to_string(), "drop".to_string(), stash_ref],
        )?;
        let combined = combined_output(&output).trim().to_string();
        if !output.status.success() {
            return Err(if combined.is_empty() {
                "Failed to drop remote git stash".to_string()
            } else {
                combined
            });
        }
        Ok(combined)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_conflict_files(
    connection: SshConnection,
    remote_project_path: String,
) -> Result<Vec<crate::git::GitConflictFile>, String> {
    tokio::task::spawn_blocking(move || {
        let output = run_remote_git_output(
            &connection,
            &remote_project_path,
            &str_args(&["diff", "--name-only", "--diff-filter=U", "-z"]),
        )?;
        if !output.status.success() {
            return Err(output_error(
                &output,
                "Failed to list remote conflict files",
            ));
        }
        Ok(crate::git::parse_conflict_paths_z(&output.stdout))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_conflict_preview(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
) -> Result<crate::git::GitConflictPreview, String> {
    tokio::task::spawn_blocking(move || {
        let content = remote_git_read_text_file(&connection, &remote_project_path, &file_path)?;
        Ok(crate::git::GitConflictPreview {
            file_path,
            hunks: crate::git::parse_conflict_hunks(&content)?,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_git_resolve_conflict(
    connection: SshConnection,
    remote_project_path: String,
    file_path: String,
    resolution: crate::git::GitConflictResolution,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        validate_remote_git_relative_path(&file_path)?;
        if is_protected_remote_git_relative_path(&file_path) {
            return Err("Refusing to modify protected project metadata".to_string());
        }
        match resolution {
            crate::git::GitConflictResolution::Ours => {
                run_remote_git(
                    &connection,
                    &remote_project_path,
                    &["checkout".to_string(),
                        "--ours".to_string(),
                        "--".to_string(),
                        file_path.clone()],
                )?;
            }
            crate::git::GitConflictResolution::Theirs => {
                run_remote_git(
                    &connection,
                    &remote_project_path,
                    &["checkout".to_string(),
                        "--theirs".to_string(),
                        "--".to_string(),
                        file_path.clone()],
                )?;
            }
            crate::git::GitConflictResolution::Both => {
                let content =
                    remote_git_read_text_file(&connection, &remote_project_path, &file_path)?;
                let resolved = crate::git::resolve_conflict_markers_keep_both(&content)?;
                remote_git_write_text_file(
                    &connection,
                    &remote_project_path,
                    &file_path,
                    &resolved,
                )?;
            }
        }
        run_remote_git(
            &connection,
            &remote_project_path,
            &["add".to_string(), "--".to_string(), file_path],
        )?;
        Ok(())
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
            build_remote_git_command(
                "/srv/app's repo",
                &str_args(&["diff", "--", "src/main file.rs"])
            ),
            "cd -- '/srv/app'\\''s repo' && git diff -- 'src/main file.rs'"
        );
    }

    #[test]
    fn remote_project_path_must_be_absolute_and_normalized() {
        assert_eq!(
            normalize_remote_project_path("/srv/app/").unwrap(),
            "/srv/app"
        );
        assert!(normalize_remote_project_path("srv/app").is_err());
        assert!(normalize_remote_project_path("/srv/../app").is_err());
    }

    #[test]
    fn remote_git_relative_paths_stay_inside_worktree() {
        assert!(validate_remote_git_relative_path("src/main.rs").is_ok());
        assert!(validate_remote_git_relative_path("/srv/app/src/main.rs").is_err());
        assert!(validate_remote_git_relative_path("../secret").is_err());
        assert!(validate_remote_git_relative_path("src/./main.rs").is_err());
    }

    #[test]
    fn remote_git_advanced_file_paths_stay_inside_worktree() {
        assert_eq!(
            remote_git_absolute_file_path("/srv/app/", "src/main.rs").unwrap(),
            "/srv/app/src/main.rs"
        );
        assert_eq!(
            remote_git_absolute_file_path("/", "src/main.rs").unwrap(),
            "/src/main.rs"
        );
        assert!(remote_git_absolute_file_path("/srv/app", "../secret").is_err());
        assert!(remote_git_absolute_file_path("/srv/app", ".git/index").is_err());
        assert!(remote_git_absolute_file_path("/srv/app", ".aeroric/config.json").is_err());
    }

    #[test]
    fn remote_git_file_path_args_dedupe_and_quote_paths() {
        let args = remote_git_path_args(
            &["add"],
            vec![
                "src/main file.rs".to_string(),
                "src/main file.rs".to_string(),
                "README.md".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(
            build_remote_git_command("/srv/app", &args),
            "cd -- '/srv/app' && git add -- 'src/main file.rs' README.md"
        );
    }

    #[test]
    fn remote_git_unstage_uses_reset_before_first_commit() {
        assert_eq!(
            remote_git_unstage_args(true, vec!["src/main.rs".to_string()]).unwrap(),
            str_args(&["restore", "--staged", "--", "src/main.rs"])
        );
        assert_eq!(
            remote_git_unstage_args(false, vec!["src/main.rs".to_string()]).unwrap(),
            str_args(&["reset", "--", "src/main.rs"])
        );
    }

    #[test]
    fn remote_git_discard_rejects_protected_metadata_paths() {
        assert!(remote_git_discard_files_args(vec!["src/main.rs".to_string()], true).is_ok());
        assert!(
            remote_git_discard_files_args(vec![".aeroric/config.toml".to_string()], true).is_err()
        );
        assert!(remote_git_discard_files_args(vec![".git/index".to_string()], true).is_err());
    }

    #[test]
    fn remote_git_push_args_validate_branch_names() {
        assert_eq!(
            remote_git_push_args(Some("main")).unwrap(),
            str_args(&["push", "origin", "main"])
        );
        assert_eq!(remote_git_push_args(None).unwrap(), str_args(&["push"]));
        assert!(remote_git_push_args(Some("--force")).is_err());
    }

    #[test]
    fn remote_git_log_args_are_shell_quoted() {
        let args = crate::git::build_git_log_args(50, Some("fix prod"), Some("feature/remote ui"));
        assert_eq!(
            build_remote_git_command("/srv/app", &args),
            "cd -- '/srv/app' && git log --format=COMMIT:%H%nSHORT:%h%nAUTHOR:%an%nDATE:%ar%nSUBJECT:%s%nREFS:%D%nEND_RECORD -n 50 '--grep=fix prod' 'feature/remote ui'"
        );
    }
}
