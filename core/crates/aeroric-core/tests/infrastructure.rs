//! P7-03 验收：fake CLI 覆盖 unavailable/permission denied/输出异常/取消；
//! 端口 PID 身份判定；Conda 环境校验。
use aeroric_core::infrastructure::{conda, docker, ports};

#[test]
fn docker_parsers_and_output_anomalies() {
    // 正常 JSON。
    let stdout = r#"[{"ID":"abc123","Repository":"nginx","Tag":"latest","Digest":"sha256:x","CreatedSince":"2 days","Size":"100MB"}]"#;
    let images = docker::parse_docker_images_json(stdout);
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].repository, "nginx");
    // 输出异常（非法 JSON）→ 空列表不 panic。
    assert!(docker::parse_docker_images_json("not json").is_empty());
    assert!(docker::parse_docker_containers_json("not json").is_empty());
    // 空输出。
    assert!(docker::parse_docker_containers_json("[]").is_empty());
}

#[test]
fn docker_action_readonly_vs_destructive() {
    // Action 3：显式区分只读/破坏操作。
    assert!(!docker::DockerAction::Start.is_destructive());
    assert!(!docker::DockerAction::Stop.is_destructive());
    assert!(!docker::DockerAction::Restart.is_destructive());
    assert!(docker::DockerAction::Remove.is_destructive());
}

#[test]
fn docker_daemon_unavailable_and_permission_denied() {
    // fake CLI 覆盖 unavailable/permission denied。
    let reason = docker::daemon_unavailable_reason(
        "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?",
    );
    assert_eq!(
        reason.as_deref(),
        Some("Docker daemon 不可用"),
        "无 daemon 显示不可用，不弹 sudo"
    );
    let reason = docker::daemon_unavailable_reason(
        "permission denied while trying to connect to the Docker daemon socket",
    );
    assert!(
        reason.as_ref().unwrap().contains("权限不足"),
        "permission denied 有因提示"
    );
    assert!(docker::daemon_unavailable_reason("all good").is_none());
}

#[test]
fn long_operation_registry_lifecycle() {
    let mut registry = docker::OperationRegistry::new();
    registry.register("op-1");
    assert_eq!(
        registry.status("op-1"),
        Some(docker::OperationStatus::Pending)
    );
    registry.update("op-1", docker::OperationStatus::InProgress);
    assert_eq!(
        registry.status("op-1"),
        Some(docker::OperationStatus::InProgress)
    );
    // 取消（状态查询支持；Cancelled 后不可改）。
    assert!(registry.cancel("op-1"));
    assert_eq!(
        registry.status("op-1"),
        Some(docker::OperationStatus::Cancelled)
    );
    assert!(!registry.cancel("op-1"), "Cancelled 后不可改");
    assert_eq!(
        registry.status("op-1"),
        Some(docker::OperationStatus::Cancelled)
    );
    assert_eq!(registry.status("op-missing"), None);
}

#[test]
fn port_pid_identity_guard() {
    // 端口释放前确认 PID 启动身份，避免 PID 重用误杀。
    assert!(ports::pid_matches_process(1234, "nginx", "nginx"));
    assert!(
        ports::pid_matches_process(1234, "nginx", "/usr/sbin/nginx"),
        "路径形式后缀匹配"
    );
    assert!(
        !ports::pid_matches_process(1234, "nginx", "python3"),
        "PID 重用误杀防护"
    );
    // stop 判定（owner/stop 显式区分；PID 身份确认）。
    assert!(ports::can_stop_port(true, true).is_ok());
    assert!(
        ports::can_stop_port(true, false).is_err(),
        "PID 身份不匹配拒绝"
    );
    assert!(
        ports::can_stop_port(false, true).is_err(),
        "owner 未确认拒绝"
    );
}

#[test]
fn conda_environment_validation_and_spawn_policy() {
    assert!(conda::validate_environment_path("/opt/miniconda3/envs/dev").is_ok());
    assert!(conda::validate_environment_path("").is_err());
    assert!(
        conda::validate_environment_path("relative/path").is_err(),
        "绝对路径要求"
    );
    assert!(conda::validate_environment_path("bad\0path").is_err());
    // env 名从路径推导。
    assert_eq!(
        conda::env_name_from_path("/opt/miniconda3/envs/dev/bin/conda"),
        "dev"
    );
    assert_eq!(
        conda::env_name_from_path("/opt/miniconda3/bin/conda"),
        "base"
    );
    // spawn 环境变量（不能污染全局 shell rc——变量进子进程环境）。
    let env = conda::conda_spawn_env("/opt/miniconda3/envs/dev");
    assert!(env.contains(&(
        "CONDA_PREFIX".to_string(),
        "/opt/miniconda3/envs/dev".to_string()
    )));
    assert!(env.contains(&(
        "VIRTUAL_ENV".to_string(),
        "/opt/miniconda3/envs/dev".to_string()
    )));
}
