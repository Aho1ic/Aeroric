//! E15 step 2 (2026-11-22): how far Aeroric's own Context server got, step
//! by step, so a session whose Aeroric tools never came up says where it
//! stopped — for every harness, also DSH and OMP, which report nothing of
//! their MCP servers' start.
//!
//! `aeroric-cli mcp-context` writes its steps to
//! `<data>/context-stages/<first 16 hex of sha256(token)>.json` (never the
//! token itself): when the process started, when `initialize` came and was
//! answered (answering asks the host what this project has), when
//! `tools/list` was answered, and the first `tools/call`. A new token is
//! issued per session start, so each start has its own file; a resumed
//! process writes it afresh.
//!
//! At the end of a turn the executor reads the file of a session that has
//! the server ([`ContextStageWatch`]); until the tools were listed once, a
//! missing step is said in the session, once per token.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_protocol::session_live::{SessionUpdate, SessionUpdateKind};
use aeroric_protocol::task::{ProtocolFamily, TaskStatus};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::context_server::ContextTokens;
use super::{DriverRegistry, LiveStatusSink};

pub const DIR: &str = "context-stages";
/// Stage files older than this are removed when a server starts.
pub const KEEP_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// One server process's steps, as epoch milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stages {
    pub started_ms: u64,
    pub pid: u32,
    /// The MCP client's name from `initialize` (`codex-mcp-client`…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialize_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialized_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_list_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_call_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_call: Option<String>,
}

/// Where the server with `token` writes its steps.
pub fn path(data_dir: &Path, token: &str) -> PathBuf {
    let digest = Sha256::digest(token.as_bytes());
    let name: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    data_dir.join(DIR).join(format!("{name}.json"))
}

pub fn read(path: &Path) -> Option<Stages> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Write the steps whole (a temporary file renamed over), so a reader never
/// sees half of them. Failing to write is not the server's business.
pub fn write(path: &Path, stages: &Stages) {
    let Some(dir) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    if let Ok(bytes) = serde_json::to_vec(stages) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// Remove stage files not touched for [`KEEP_MS`].
pub fn prune(data_dir: &Path, now_ms: u64) {
    let Ok(entries) = std::fs::read_dir(data_dir.join(DIR)) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .is_some_and(|t| now_ms.saturating_sub(t.as_millis() as u64) > KEEP_MS);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn secs(ms: u64) -> String {
    let tenths = ms / 100;
    if tenths % 10 == 0 {
        format!("{} 秒", tenths / 10)
    } else {
        format!("{}.{} 秒", tenths / 10, tenths % 10)
    }
}

const LABEL: &str = "Aeroric Context 工具（aeroric_context）";
const COST: &str = "；这一轮用不了记忆、委派、黑板等 Aeroric 工具";

/// What to say at a turn's end, or `None` once the tools were listed.
/// `given_ms`: when the session was given the server.
pub fn verdict(stages: Option<&Stages>, given_ms: u64, now_ms: u64) -> Option<String> {
    let Some(s) = stages.filter(|s| s.started_ms >= given_ms.saturating_sub(1000)) else {
        return Some(format!(
            "{LABEL}没有启动：给会话配上它 {}后，这一轮结束时 Agent 还没有启动 aeroric-cli mcp-context{COST}",
            secs(now_ms.saturating_sub(given_ms))
        ));
    };
    if s.tools_list_ms.is_some() {
        return None;
    }
    let mut steps = vec![format!(
        "进程在配上它 {}后启动",
        secs(s.started_ms.saturating_sub(given_ms))
    )];
    let stuck = match (s.initialize_ms, s.initialized_ms) {
        (None, _) => format!(
            "没有收到 initialize（已过 {}）",
            secs(now_ms.saturating_sub(s.started_ms))
        ),
        (Some(at), None) => {
            steps.push(format!(
                "启动后 {}收到 initialize",
                secs(at.saturating_sub(s.started_ms))
            ));
            format!(
                "initialize 还没答完：卡在向 Aeroric 宿主查询（已等 {}）",
                secs(now_ms.saturating_sub(at))
            )
        }
        (Some(at), Some(done)) => {
            steps.push(format!(
                "启动后 {}收到 initialize，用 {}答完",
                secs(at.saturating_sub(s.started_ms)),
                secs(done.saturating_sub(at))
            ));
            format!(
                "Agent 没有来要工具清单（tools/list，initialize 答完后已过 {}）",
                secs(now_ms.saturating_sub(done))
            )
        }
    };
    let client = s
        .client
        .as_deref()
        .map(|c| format!("（客户端 {c}）"))
        .unwrap_or_default();
    Some(format!(
        "{LABEL}没有接上{client}：{}；{stuck}{COST}",
        steps.join("，")
    ))
}

/// A sink layer: at each turn's end of a session given the server, read
/// its steps; once the tools were listed the session is left alone, and a
/// missing step is said once per token.
pub struct ContextStageWatch {
    inner: Arc<dyn LiveStatusSink>,
    tokens: Arc<ContextTokens>,
    registry: Arc<DriverRegistry>,
    /// The executor's `(program, data_dir)` for the server.
    program: Arc<Mutex<Option<(String, String)>>>,
    settled: Mutex<HashSet<String>>,
}

impl ContextStageWatch {
    pub fn new(
        inner: Arc<dyn LiveStatusSink>,
        tokens: Arc<ContextTokens>,
        registry: Arc<DriverRegistry>,
        program: Arc<Mutex<Option<(String, String)>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner,
            tokens,
            registry,
            program,
            settled: Mutex::default(),
        })
    }

    fn check(&self, task_id: &str, now_ms: u64) {
        let Some((token, given_ms)) = self.tokens.issued(task_id) else {
            return;
        };
        if self.settled.lock().contains(&token) {
            return;
        }
        let Some(dir) = self.program.lock().as_ref().map(|(_, d)| PathBuf::from(d)) else {
            return;
        };
        let stages = read(&path(&dir, &token));
        let said = verdict(stages.as_ref(), given_ms, now_ms);
        self.settled.lock().insert(token);
        let Some(message) = said else {
            return;
        };
        // Not under the driver's push: the log is written from a thread of
        // its own, after this update has been handed on.
        let registry = Arc::clone(&self.registry);
        let task = task_id.to_string();
        std::thread::spawn(move || {
            if let Some(log) = registry.log(&task) {
                log.push(
                    SessionUpdateKind::Notice {
                        level: "warning".into(),
                        message,
                    },
                    now_ms,
                );
            }
        });
    }
}

impl LiveStatusSink for ContextStageWatch {
    fn status(
        &self,
        project_id: &str,
        task_id: &str,
        generation: u64,
        status: TaskStatus,
        reason: Option<String>,
    ) {
        self.inner
            .status(project_id, task_id, generation, status, reason)
    }

    fn session(
        &self,
        project_id: &str,
        task_id: &str,
        family: ProtocolFamily,
        agent: &str,
        session_id: &str,
        session_path: Option<&str>,
    ) {
        self.inner
            .session(project_id, task_id, family, agent, session_id, session_path)
    }

    fn update(&self, update: &SessionUpdate) {
        self.inner.update(update);
        if matches!(update.kind, SessionUpdateKind::TurnEnded { .. }) {
            self.check(&update.task_id, update.at_ms);
        }
    }
}

#[cfg(test)]
#[path = "context_stages_tests.rs"]
mod tests;
