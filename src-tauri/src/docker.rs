use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DockerImageSummary {
    id: String,
    repository: String,
    tag: String,
    digest: String,
    created_since: String,
    size: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DockerContainerSummary {
    id: String,
    image: String,
    names: String,
    state: String,
    status: String,
    ports: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DockerResources {
    images: Vec<DockerImageSummary>,
    containers: Vec<DockerContainerSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DockerTarget {
    remote: Option<crate::ssh::SshConnection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDockerImage {
    #[serde(rename = "ID")]
    id: Option<String>,
    repository: Option<String>,
    tag: Option<String>,
    digest: Option<String>,
    created_since: Option<String>,
    size: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDockerContainer {
    #[serde(rename = "ID")]
    id: Option<String>,
    image: Option<String>,
    names: Option<String>,
    state: Option<String>,
    status: Option<String>,
    ports: Option<String>,
    created_at: Option<String>,
}

fn docker_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from("/opt/homebrew/bin/docker"));
    candidates.push(PathBuf::from("/usr/local/bin/docker"));
    candidates.push(PathBuf::from("docker"));
    candidates
}

fn command_error(label: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("{} exited with status {}", label, output.status)
    } else {
        stderr
    }
}

fn should_retry_docker_with_sudo(stderr: &str) -> bool {
    let normalized = stderr.to_lowercase();
    normalized.contains("authorized users only")
        || normalized.contains("permission denied")
        || normalized.contains("access denied")
        || normalized.contains("operation not permitted")
        || normalized.contains("a password is required")
        || normalized.contains("no tty present")
        || normalized.contains("a terminal is required")
        || (normalized.contains("connection to ") && normalized.contains(" closed"))
}

fn run_local_docker_command(
    docker: &PathBuf,
    args: &[String],
    sudo: bool,
) -> std::io::Result<Output> {
    let mut command = if sudo {
        let mut command = Command::new("sudo");
        command.arg("-n").arg(docker);
        command
    } else {
        Command::new(docker)
    };
    crate::subprocess::configure_background_command(&mut command);
    command.args(args).output()
}

fn run_local_docker_args(args: &[String]) -> Result<String, String> {
    let mut last_error = None;
    for docker in docker_candidates() {
        let output = match run_local_docker_command(&docker, args, false) {
            Ok(output) => output,
            Err(err) => {
                last_error = Some(err.to_string());
                continue;
            }
        };
        if output.status.success() {
            return String::from_utf8(output.stdout).map_err(|err| err.to_string());
        }
        let error = command_error("docker", &output);
        if should_retry_docker_with_sudo(&error) {
            match run_local_docker_command(&docker, args, true) {
                Ok(sudo_output) if sudo_output.status.success() => {
                    return String::from_utf8(sudo_output.stdout).map_err(|err| err.to_string());
                }
                Ok(sudo_output) => {
                    last_error = Some(command_error("sudo docker", &sudo_output));
                    continue;
                }
                Err(err) => {
                    last_error = Some(err.to_string());
                    continue;
                }
            }
        }
        last_error = Some(error);
    }
    Err(last_error.unwrap_or_else(|| "Docker CLI not found".to_string()))
}

fn build_remote_docker_command(args: &[String]) -> String {
    let docker_command = std::iter::once("docker".to_string())
        .chain(args.iter().cloned())
        .map(|arg| crate::ssh::shell_word_posix(&arg))
        .collect::<Vec<_>>()
        .join(" ");
    format!("sh -lc {}", crate::ssh::shell_word_posix(&docker_command))
}

fn build_remote_docker_sudo_no_password_command(args: &[String]) -> String {
    let docker_command = std::iter::once("docker".to_string())
        .chain(args.iter().cloned())
        .map(|arg| crate::ssh::shell_word_posix(&arg))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "sudo -n sh -lc {}",
        crate::ssh::shell_word_posix(&docker_command)
    )
}

fn build_remote_docker_sudo_password_command(args: &[String]) -> String {
    let docker_command = std::iter::once("docker".to_string())
        .chain(args.iter().cloned())
        .map(|arg| crate::ssh::shell_word_posix(&arg))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "sudo -S -p '' sh -lc {}",
        crate::ssh::shell_word_posix(&docker_command)
    )
}

fn run_command_with_stdin(mut command: Command, input: &str) -> Result<Output, String> {
    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    child.wait_with_output().map_err(|err| err.to_string())
}

fn should_retry_remote_docker_with_sudo(stderr: &str) -> bool {
    should_retry_docker_with_sudo(stderr)
}

fn run_remote_docker_args(
    connection: crate::ssh::SshConnection,
    args: &[String],
) -> Result<String, String> {
    let mut command = crate::ssh::std_ssh_command_for_remote_command(
        &connection,
        build_remote_docker_command(args),
    );
    let output = command.output().map_err(|err| err.to_string())?;
    if output.status.success() {
        return String::from_utf8(output.stdout).map_err(|err| err.to_string());
    }
    let stderr = command_error("remote docker", &output);
    if should_retry_remote_docker_with_sudo(&stderr) {
        let sudo_no_password_output = crate::ssh::std_ssh_command_for_remote_command(
            &connection,
            build_remote_docker_sudo_no_password_command(args),
        )
        .output()
        .map_err(|err| err.to_string())?;
        if sudo_no_password_output.status.success() {
            return String::from_utf8(sudo_no_password_output.stdout)
                .map_err(|err| err.to_string());
        }
        let sudo_no_password_error = command_error("remote sudo docker", &sudo_no_password_output);

        let password = connection
            .password
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        if let Some(password) = password {
            let sudo_password_command = crate::ssh::std_ssh_command_for_remote_command(
                &connection,
                build_remote_docker_sudo_password_command(args),
            );
            let sudo_password_output =
                run_command_with_stdin(sudo_password_command, &format!("{password}\n"))?;
            if sudo_password_output.status.success() {
                return String::from_utf8(sudo_password_output.stdout)
                    .map_err(|err| err.to_string());
            }
            return Err(command_error("remote sudo docker", &sudo_password_output));
        }
        return Err(sudo_no_password_error);
    }
    Err(stderr)
}

fn run_docker_args(target: DockerTarget, args: Vec<String>) -> Result<String, String> {
    match target.remote {
        Some(connection) => run_remote_docker_args(connection, &args),
        None => run_local_docker_args(&args),
    }
}

fn docker_delete_image_args(image: &str) -> Vec<String> {
    vec![
        "image".to_string(),
        "rm".to_string(),
        "--force".to_string(),
        image.to_string(),
    ]
}

fn parse_json_lines<T>(raw: &str, label: &str) -> Result<Vec<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    raw.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str::<T>(line).map_err(|err| {
                format!(
                    "Failed to parse Docker {} line {}: {}",
                    label,
                    index + 1,
                    err
                )
            })
        })
        .collect()
}

fn value_or_dash(value: Option<String>) -> String {
    value
        .filter(|item| !item.trim().is_empty())
        .unwrap_or_else(|| "-".to_string())
}

fn strip_docker_created_timezone(value: &str) -> String {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    if parts.len() >= 4
        && parts[2].len() == 5
        && (parts[2].starts_with('+') || parts[2].starts_with('-'))
        && parts[2][1..].chars().all(|ch| ch.is_ascii_digit())
    {
        return parts[..2].join(" ");
    }
    value.to_string()
}

fn parse_images(raw: &str) -> Result<Vec<DockerImageSummary>, String> {
    parse_json_lines::<RawDockerImage>(raw, "image").map(|items| {
        items
            .into_iter()
            .map(|item| DockerImageSummary {
                id: value_or_dash(item.id),
                repository: value_or_dash(item.repository),
                tag: value_or_dash(item.tag),
                digest: value_or_dash(item.digest),
                created_since: value_or_dash(item.created_since),
                size: value_or_dash(item.size),
            })
            .collect()
    })
}

fn parse_containers(raw: &str) -> Result<Vec<DockerContainerSummary>, String> {
    parse_json_lines::<RawDockerContainer>(raw, "container").map(|items| {
        items
            .into_iter()
            .map(|item| DockerContainerSummary {
                id: value_or_dash(item.id),
                image: value_or_dash(item.image),
                names: value_or_dash(item.names),
                state: value_or_dash(item.state),
                status: value_or_dash(item.status),
                ports: value_or_dash(item.ports),
                created_at: strip_docker_created_timezone(&value_or_dash(item.created_at)),
            })
            .collect()
    })
}

fn list_docker_resources_sync(target: DockerTarget) -> Result<DockerResources, String> {
    let images = run_docker_args(
        target.clone(),
        vec![
            "image".to_string(),
            "ls".to_string(),
            "--all".to_string(),
            "--digests".to_string(),
            "--format".to_string(),
            "{{json .}}".to_string(),
        ],
    )
    .and_then(|raw| parse_images(&raw))?;
    let containers = run_docker_args(
        target,
        vec![
            "container".to_string(),
            "ls".to_string(),
            "--all".to_string(),
            "--size".to_string(),
            "--format".to_string(),
            "{{json .}}".to_string(),
        ],
    )
    .and_then(|raw| parse_containers(&raw))?;

    Ok(DockerResources { images, containers })
}

#[tauri::command]
pub async fn list_docker_resources(
    remote: Option<crate::ssh::SshConnection>,
) -> Result<DockerResources, String> {
    tauri::async_runtime::spawn_blocking(move || {
        list_docker_resources_sync(DockerTarget { remote })
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn docker_container_action(
    remote: Option<crate::ssh::SshConnection>,
    action: String,
    container_id: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let args = match action.as_str() {
            "start" => vec!["container", "start", container_id.as_str()],
            "restart" => vec!["container", "restart", container_id.as_str()],
            "stop" => vec!["container", "stop", container_id.as_str()],
            "delete" => vec!["container", "rm", "--force", container_id.as_str()],
            _ => return Err(format!("Unsupported Docker container action: {}", action)),
        }
        .into_iter()
        .map(str::to_string)
        .collect();
        run_docker_args(DockerTarget { remote }, args).map(|_| ())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn docker_container_logs(
    remote: Option<crate::ssh::SshConnection>,
    container_id: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_docker_args(
            DockerTarget { remote },
            vec![
                "container".to_string(),
                "logs".to_string(),
                "--tail".to_string(),
                "500".to_string(),
                container_id,
            ],
        )
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn docker_delete_image(
    remote: Option<crate::ssh::SshConnection>,
    image: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_docker_args(DockerTarget { remote }, docker_delete_image_args(&image)).map(|_| ())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn docker_tag_image(
    remote: Option<crate::ssh::SshConnection>,
    source: String,
    target: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_docker_args(
            DockerTarget { remote },
            vec!["tag".to_string(), source, target],
        )
        .map(|_| ())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{
        build_remote_docker_command, build_remote_docker_sudo_no_password_command,
        build_remote_docker_sudo_password_command, docker_delete_image_args, parse_containers,
        parse_images, should_retry_remote_docker_with_sudo, strip_docker_created_timezone,
    };

    #[test]
    fn parses_docker_image_json_lines() {
        let raw = r#"{"Containers":"N/A","CreatedAt":"2026-06-01 10:00:00 +0800 CST","CreatedSince":"2 weeks ago","Digest":"sha256:abc","ID":"sha256:1234567890ab","Repository":"qwen","SharedSize":"N/A","Size":"8.2GB","Tag":"latest","UniqueSize":"N/A","VirtualSize":"8.2GB"}"#;
        let images = parse_images(raw).expect("images parse");

        assert_eq!(images.len(), 1);
        assert_eq!(images[0].repository, "qwen");
        assert_eq!(images[0].tag, "latest");
        assert_eq!(images[0].id, "sha256:1234567890ab");
        assert_eq!(images[0].digest, "sha256:abc");
        assert_eq!(images[0].created_since, "2 weeks ago");
        assert_eq!(images[0].size, "8.2GB");
    }

    #[test]
    fn parses_docker_container_json_lines() {
        let raw = r#"{"Command":"python app.py","CreatedAt":"2026-06-15 10:00:00 +0800 CST","ID":"a1b2c3d4e5f6","Image":"qwen:latest","Labels":"","LocalVolumes":"0","Mounts":"/tmp","Names":"qwen-api","Networks":"bridge","Ports":"0.0.0.0:8000->8000/tcp","RunningFor":"1 hour ago","Size":"12MB (virtual 8.2GB)","State":"running","Status":"Up 1 hour"}"#;
        let containers = parse_containers(raw).expect("containers parse");

        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, "a1b2c3d4e5f6");
        assert_eq!(containers[0].image, "qwen:latest");
        assert_eq!(containers[0].names, "qwen-api");
        assert_eq!(containers[0].state, "running");
        assert_eq!(containers[0].status, "Up 1 hour");
        assert_eq!(containers[0].ports, "0.0.0.0:8000->8000/tcp");
        assert_eq!(containers[0].created_at, "2026-06-15 10:00:00");
    }

    #[test]
    fn strips_timezone_suffix_from_docker_created_at() {
        assert_eq!(
            strip_docker_created_timezone("2026-06-15 10:00:00 +0800 CST"),
            "2026-06-15 10:00:00"
        );
        assert_eq!(strip_docker_created_timezone("2 weeks ago"), "2 weeks ago");
    }

    #[test]
    fn builds_remote_docker_command_with_shell_quoting() {
        let command = build_remote_docker_command(&[
            "tag".to_string(),
            "repo/app:old tag".to_string(),
            "repo/app:new tag".to_string(),
        ]);

        assert!(command.starts_with("sh -lc "));
        assert!(command.contains("docker tag"));
        assert!(command.contains("repo/app"));
        assert!(command.contains("'"));
    }

    #[test]
    fn builds_remote_docker_sudo_no_password_command_with_shell_quoting() {
        let command =
            build_remote_docker_sudo_no_password_command(&["ps".to_string(), "--all".to_string()]);

        assert!(command.starts_with("sudo -n sh -lc "));
        assert!(command.contains("docker ps"));
    }

    #[test]
    fn builds_remote_docker_sudo_password_command_with_shell_quoting() {
        let command =
            build_remote_docker_sudo_password_command(&["ps".to_string(), "--all".to_string()]);

        assert!(command.contains("sudo -S -p ''"));
        assert!(command.contains("docker ps"));
        assert!(!command.contains("pass word"));
    }

    #[test]
    fn builds_remote_docker_sudo_password_command_for_image_delete() {
        let command = build_remote_docker_sudo_password_command(&[
            "image".to_string(),
            "rm".to_string(),
            "repo/app:latest".to_string(),
        ]);

        assert!(command.contains("sudo -S -p ''"));
        assert!(command.contains("docker image rm repo/app:latest"));
    }

    #[test]
    fn deletes_images_with_force_to_remove_container_references() {
        assert_eq!(
            docker_delete_image_args("repo/app:latest"),
            vec![
                "image".to_string(),
                "rm".to_string(),
                "--force".to_string(),
                "repo/app:latest".to_string(),
            ]
        );
    }

    #[test]
    fn detects_remote_docker_permission_errors_for_sudo_retry() {
        assert!(should_retry_remote_docker_with_sudo(
            "Authorized users only. All activities may be monitored and reported."
        ));
        assert!(should_retry_remote_docker_with_sudo(
            "Got permission denied while trying to connect to the Docker daemon socket"
        ));
        assert!(should_retry_remote_docker_with_sudo(
            "dial unix /var/run/docker.sock: connect: permission denied"
        ));
        assert!(should_retry_remote_docker_with_sudo(
            "sudo: a password is required"
        ));
        assert!(should_retry_remote_docker_with_sudo(
            "sudo: a terminal is required to read the password"
        ));
        assert!(should_retry_remote_docker_with_sudo(
            "Connection to 192.168.10.100 closed."
        ));
        assert!(!should_retry_remote_docker_with_sudo(
            "Cannot connect to the Docker daemon"
        ));
        assert!(!should_retry_remote_docker_with_sudo(
            "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"
        ));
    }
}
