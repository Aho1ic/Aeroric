//! P8-05 切片一百零八（ide 刀三）：运行面——run config 进程注册表（start/
//! stop/read_run_process verbatim 上收）。有状态面：run_id→child+snapshot 的
//! 注册表、输出读取线程、退出观察线程、完成进程棘轮（保留 64 个）与清理
//! 线程。两处 Tauri 适配注入：shell 启动器（Tauri 平台 shell／宿主 `sh -c`）
//! 与 login shell 环境。remote 孪生留 Tauri SSH 适配层。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use uuid::Uuid;

use super::run_config::{
    legacy_cwd_candidate, validate_legacy_run_config, RunConfig, RunProcessSnapshot,
    RunProcessStatus,
};
use crate::runtime::subprocess::{
    configure_background_command, configure_terminable_process_tree, terminate_process_tree,
};

const MAX_OUTPUT_CHARS: usize = 200_000;
const MAX_RETAINED_FINISHED_PROCESSES: usize = 64;

/// 宿主默认启动器：POSIX shell（Tauri 的平台 shell 适配——powershell/cmd
/// 分支——由 Tauri 壳注入等价闭包）。
pub fn posix_shell_adapter(command: &str) -> (String, Vec<String>) {
    (
        "sh".to_string(),
        vec!["-c".to_string(), command.to_string()],
    )
}

#[derive(Clone)]
struct RunProcessHandle {
    child: Arc<Mutex<Child>>,
    snapshot: Arc<Mutex<RunProcessSnapshot>>,
}

/// 切片一百零八：进程注册表（Tauri `RunConfigState` verbatim——宿主每实例
/// 一份，三个 handler 共享；Clone 共享同一注册表——Embedded 的桌面壳把一份
/// 挂在应用句柄上，三次调用经由 clone 落到同一张表）。
#[derive(Clone, Default)]
pub struct RunProcessState {
    processes: Arc<Mutex<HashMap<String, RunProcessHandle>>>,
}

pub fn finished_process_ids_to_remove(snapshots: &[(String, RunProcessSnapshot)]) -> Vec<String> {
    let mut finished = snapshots
        .iter()
        .filter(|(_, snapshot)| snapshot.status != RunProcessStatus::Running)
        .map(|(id, snapshot)| {
            (
                id.clone(),
                snapshot.finished_at.unwrap_or(snapshot.started_at),
            )
        })
        .collect::<Vec<_>>();
    if finished.len() <= MAX_RETAINED_FINISHED_PROCESSES {
        return Vec::new();
    }
    finished.sort_by_key(|(_, finished_at)| *finished_at);
    let remove_count = finished.len() - MAX_RETAINED_FINISHED_PROCESSES;
    finished
        .into_iter()
        .take(remove_count)
        .map(|(id, _)| id)
        .collect()
}

fn prune_finished_processes(processes: &mut HashMap<String, RunProcessHandle>) {
    let snapshots = processes
        .iter()
        .map(|(id, handle)| (id.clone(), handle.snapshot.lock().clone()))
        .collect::<Vec<_>>();
    for id in finished_process_ids_to_remove(&snapshots) {
        processes.remove(&id);
    }
}

fn spawn_process_cleanup(
    processes: Arc<Mutex<HashMap<String, RunProcessHandle>>>,
    snapshot: Arc<Mutex<RunProcessSnapshot>>,
) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(250));
        if snapshot.lock().status == RunProcessStatus::Running {
            continue;
        }
        prune_finished_processes(&mut processes.lock());
        break;
    });
}

pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub fn resolve_run_cwd(root: &Path, cwd: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let candidate = legacy_cwd_candidate(&root, cwd)?;
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Cannot resolve run cwd: {e}"))?;
    if !canonical.is_dir() {
        return Err("Run cwd is not a directory".to_string());
    }
    if !canonical.starts_with(&root) {
        return Err("Run cwd is outside project root".to_string());
    }
    Ok(canonical)
}

fn trim_output(output: &mut String) {
    if output.len() <= MAX_OUTPUT_CHARS {
        return;
    }
    let excess = output.len() - MAX_OUTPUT_CHARS;
    let boundary = output
        .char_indices()
        .find_map(|(index, _)| (index >= excess).then_some(index))
        .unwrap_or(excess);
    output.drain(..boundary);
}

fn append_output(snapshot: &Arc<Mutex<RunProcessSnapshot>>, chunk: &str) {
    let mut snapshot = snapshot.lock();
    snapshot.output.push_str(chunk);
    trim_output(&mut snapshot.output);
}

pub fn spawn_output_reader<R>(mut reader: R, snapshot: Arc<Mutex<RunProcessSnapshot>>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = [0; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let chunk = String::from_utf8_lossy(&buffer[..count]);
                    append_output(&snapshot, &chunk);
                }
                Err(error) => {
                    append_output(&snapshot, &format!("\n[run output read failed: {error}]\n"));
                    break;
                }
            }
        }
    });
}

pub fn spawn_exit_watcher(child: Arc<Mutex<Child>>, snapshot: Arc<Mutex<RunProcessSnapshot>>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(150));
        let status = {
            let mut child = child.lock();
            match child.try_wait() {
                Ok(Some(status)) => Some(Ok(status)),
                Ok(None) => None,
                Err(error) => Some(Err(error.to_string())),
            }
        };
        let Some(status) = status else {
            continue;
        };
        let mut snapshot = snapshot.lock();
        if snapshot.status == RunProcessStatus::Stopped {
            if snapshot.finished_at.is_none() {
                snapshot.finished_at = Some(now_millis());
            }
            break;
        }
        match status {
            Ok(status) => {
                snapshot.exit_code = status.code();
                snapshot.status = if status.success() {
                    RunProcessStatus::Exited
                } else {
                    RunProcessStatus::Failed
                };
            }
            Err(error) => {
                snapshot.status = RunProcessStatus::Failed;
                snapshot
                    .output
                    .push_str(&format!("\n[run wait failed: {error}]\n"));
                trim_output(&mut snapshot.output);
            }
        }
        snapshot.finished_at = Some(now_millis());
        break;
    });
}

pub fn start_run_config(
    root: &Path,
    config: RunConfig,
    state: &RunProcessState,
    shell_adapter: &dyn Fn(&str) -> (String, Vec<String>),
    login_envs: &[(String, String)],
) -> Result<RunProcessSnapshot, String> {
    validate_legacy_run_config(root, &config)?;
    let RunConfig::Shell {
        id,
        name,
        command,
        cwd,
        env,
    } = config
    else {
        return Err("Debug run configs must be started with the debug launcher".to_string());
    };
    let cwd = resolve_run_cwd(root, &cwd)?;
    let run_id = format!("run-{}", Uuid::new_v4());
    let started_at = now_millis();
    let (shell_program, shell_args) = shell_adapter(&command);
    let mut shell = Command::new(shell_program);
    configure_background_command(&mut shell);
    shell.args(shell_args);
    configure_terminable_process_tree(&mut shell);
    let mut child = shell
        .current_dir(&cwd)
        .envs(login_envs.iter().cloned())
        .envs(env.iter())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start run config: {e}"))?;

    let snapshot = Arc::new(Mutex::new(RunProcessSnapshot {
        run_id: run_id.clone(),
        config_id: id,
        name,
        command,
        cwd: cwd.to_string_lossy().into_owned(),
        status: RunProcessStatus::Running,
        output: String::new(),
        exit_code: None,
        started_at,
        finished_at: None,
    }));

    if let Some(stdout) = child.stdout.take() {
        spawn_output_reader(stdout, Arc::clone(&snapshot));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_output_reader(stderr, Arc::clone(&snapshot));
    }

    let child = Arc::new(Mutex::new(child));
    spawn_exit_watcher(Arc::clone(&child), Arc::clone(&snapshot));
    state.processes.lock().insert(
        run_id.clone(),
        RunProcessHandle {
            child,
            snapshot: Arc::clone(&snapshot),
        },
    );
    spawn_process_cleanup(Arc::clone(&state.processes), Arc::clone(&snapshot));

    let snapshot = snapshot.lock().clone();
    Ok(snapshot)
}

pub fn stop_run_config(
    run_id: String,
    state: &RunProcessState,
) -> Result<RunProcessSnapshot, String> {
    let handle = state
        .processes
        .lock()
        .get(&run_id)
        .cloned()
        .ok_or_else(|| "Run process not found".to_string())?;
    {
        let mut child = handle.child.lock();
        terminate_process_tree(&mut child)
            .map_err(|error| format!("Failed to stop run process tree: {error}"))?;
    }
    let mut snapshot = handle.snapshot.lock();
    snapshot.status = RunProcessStatus::Stopped;
    snapshot.finished_at = Some(now_millis());
    Ok(snapshot.clone())
}

pub fn read_run_process(
    run_id: String,
    state: &RunProcessState,
) -> Result<RunProcessSnapshot, String> {
    let handle = state
        .processes
        .lock()
        .get(&run_id)
        .cloned()
        .ok_or_else(|| "Run process not found".to_string())?;
    let snapshot = handle.snapshot.lock().clone();
    Ok(snapshot)
}

/// 切片一百零八：remote 孪生的注册口（SSH 子进程不经本地 start——由 Tauri
/// 适配层 spawn 后把 child+snapshot 交给同一张注册表；清理线程随之启动）。
pub fn register_running_process(
    state: &RunProcessState,
    run_id: String,
    child: Arc<Mutex<Child>>,
    snapshot: Arc<Mutex<RunProcessSnapshot>>,
) {
    state.processes.lock().insert(
        run_id,
        RunProcessHandle {
            child,
            snapshot: Arc::clone(&snapshot),
        },
    );
    spawn_process_cleanup(Arc::clone(&state.processes), snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prunes_only_the_oldest_finished_run_processes() {
        let mut snapshots = (0..MAX_RETAINED_FINISHED_PROCESSES + 3)
            .map(|id| {
                (
                    format!("run-{id}"),
                    RunProcessSnapshot {
                        run_id: format!("run-{id}"),
                        config_id: format!("c{id}"),
                        name: format!("n{id}"),
                        command: String::new(),
                        cwd: String::new(),
                        status: RunProcessStatus::Exited,
                        output: String::new(),
                        exit_code: Some(0),
                        started_at: id as u128,
                        finished_at: Some(id as u128),
                    },
                )
            })
            .collect::<Vec<_>>();
        snapshots.push((
            "run-999".to_string(),
            RunProcessSnapshot {
                run_id: "run-999".to_string(),
                config_id: "c999".to_string(),
                name: "n999".to_string(),
                command: String::new(),
                cwd: String::new(),
                status: RunProcessStatus::Running,
                output: String::new(),
                exit_code: None,
                started_at: 999,
                finished_at: None,
            },
        ));

        let removed = finished_process_ids_to_remove(&snapshots);

        assert_eq!(removed, vec!["run-0", "run-1", "run-2"]);
        assert!(!removed.contains(&"run-999".to_string()));
    }
}
