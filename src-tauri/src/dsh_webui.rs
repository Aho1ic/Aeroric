use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tokio::process::{Child, Command};
use tokio::time::{sleep, Duration};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebUiStatus {
    Starting,
    Running,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct DshWebUiState {
    pub agent: String,
    pub port: u16,
    pub url: Option<String>,
    pub pid: Option<u32>,
    pub status: WebUiStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct WebUiProcess {
    child: Child,
    state: DshWebUiState,
}

pub struct DshWebUiManager {
    processes: Arc<RwLock<HashMap<String, WebUiProcess>>>,
    port_allocator: Arc<Mutex<PortAllocator>>,
}

struct PortAllocator {
    next_port: u16,
    used_ports: Vec<u16>,
}

impl PortAllocator {
    fn new() -> Self {
        Self {
            next_port: 15800,
            used_ports: Vec::new(),
        }
    }

    fn allocate(&mut self) -> Result<u16, String> {
        for _ in 0..100 {
            let port = self.next_port;
            self.next_port += 1;
            if self.next_port > 15900 {
                self.next_port = 15800;
            }
            if !self.used_ports.contains(&port) {
                self.used_ports.push(port);
                return Ok(port);
            }
        }
        Err("No available ports in range 15800-15900".to_string())
    }

    fn release(&mut self, port: u16) {
        self.used_ports.retain(|&p| p != port);
    }
}

impl DshWebUiManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            port_allocator: Arc::new(Mutex::new(PortAllocator::new())),
        }
    }

    pub async fn shutdown_all(&self) {
        let keys: Vec<String> = {
            let processes = self.processes.read();
            processes.keys().cloned().collect()
        };

        for agent in keys {
            let process_opt = {
                let mut processes = self.processes.write();
                processes.remove(&agent)
            };

            if let Some(mut process) = process_opt {
                let _ = Self::stop_process(&mut process.child).await;
            }
        }
    }

    async fn stop_process(child: &mut Child) -> Result<(), String> {
        #[cfg(unix)]
        {
            if let Some(pid) = child.id() {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }

                for _ in 0..10 {
                    match child.try_wait() {
                        Ok(Some(_)) => return Ok(()),
                        Ok(None) => sleep(Duration::from_millis(500)).await,
                        Err(_) => break,
                    }
                }

                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                let _ = child.wait().await;
            }
        }

        #[cfg(windows)]
        {
            let _ = child.kill().await;
        }

        Ok(())
    }
}

#[tauri::command]
pub async fn start_dsh_webui(
    agent: String,
    state: State<'_, DshWebUiManager>,
) -> Result<DshWebUiState, String> {
    let mut stale_process = {
        let mut processes = state.processes.write();
        if let Some(process) = processes.get(&agent) {
            if process.state.status == WebUiStatus::Running {
                return Ok(process.state.clone());
            }
        }
        processes.remove(&agent)
    };
    if let Some(mut process) = stale_process.take() {
        state.port_allocator.lock().release(process.state.port);
        let _ = DshWebUiManager::stop_process(&mut process.child).await;
    }

    let port = state.port_allocator.lock().allocate()?;
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let launch = crate::app_settings::get_agent_launch_spec(&agent);

    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args);
    for patch in [
        crate::dsh_home::managed_patch_path_in(&home),
        crate::dsh_home::plugins_patch_path_in(&home),
    ] {
        if patch.is_file() {
            cmd.arg("--patch").arg(patch);
        }
    }
    cmd.arg("web")
        .arg("--port")
        .arg(port.to_string())
        .envs(launch.extra_env)
        .env("DSH_HOME", &home)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            state.port_allocator.lock().release(port);
            return Err(format!("Failed to spawn dsh web: {error}"));
        }
    };

    let pid = child.id();
    let url = format!("http://localhost:{}", port);

    let mut initial_state = DshWebUiState {
        agent: agent.clone(),
        port,
        url: Some(url.clone()),
        pid,
        status: WebUiStatus::Starting,
        error: None,
    };

    {
        let mut processes = state.processes.write();
        processes.insert(
            agent.clone(),
            WebUiProcess {
                child,
                state: initial_state.clone(),
            },
        );
    }

    let health_check_result = check_health(&url, 10).await;

    match health_check_result {
        Ok(_) => {
            let mut processes = state.processes.write();
            if let Some(process) = processes.get_mut(&agent) {
                process.state.status = WebUiStatus::Running;
                initial_state.status = WebUiStatus::Running;
            }
        }
        Err(e) => {
            initial_state.status = WebUiStatus::Error;
            initial_state.error = Some(e);
            let mut process_opt = {
                let mut processes = state.processes.write();
                processes.remove(&agent)
            };

            if let Some(mut process) = process_opt.take() {
                let _ = DshWebUiManager::stop_process(&mut process.child).await;
            }
            state.port_allocator.lock().release(port);
        }
    }

    Ok(initial_state)
}

#[tauri::command]
pub async fn stop_dsh_webui(
    agent: String,
    state: State<'_, DshWebUiManager>,
) -> Result<(), String> {
    let process_opt = {
        let mut processes = state.processes.write();
        processes.remove(&agent)
    };

    if let Some(mut process) = process_opt {
        state.port_allocator.lock().release(process.state.port);
        DshWebUiManager::stop_process(&mut process.child).await?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_dsh_webui_status(
    agent: String,
    state: State<'_, DshWebUiManager>,
) -> Result<DshWebUiState, String> {
    let processes = state.processes.read();

    if let Some(process) = processes.get(&agent) {
        Ok(process.state.clone())
    } else {
        Ok(DshWebUiState {
            agent,
            port: 0,
            url: None,
            pid: None,
            status: WebUiStatus::Stopped,
            error: None,
        })
    }
}

async fn check_health(url: &str, max_attempts: u32) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    for attempt in 1..=max_attempts {
        sleep(Duration::from_millis(500)).await;

        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                return Ok(());
            }
            Ok(_) => {}
            Err(_) if attempt < max_attempts => continue,
            Err(e) => {
                return Err(format!(
                    "Health check failed after {} attempts: {}",
                    max_attempts, e
                ));
            }
        }
    }

    Err(format!(
        "Health check timed out after {} attempts",
        max_attempts
    ))
}
