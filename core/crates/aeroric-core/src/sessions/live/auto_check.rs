//! A05: after a turn changed files, run the project's own tests and
//! diagnostics in the task's working directory and append the verdict to the
//! session's log as an `autoCheck` update.
//!
//! Off unless the project opted in ([`AutoCheckPolicy`]). Checks run one at a
//! time on their own thread; a new turn cancels the running check. Every
//! command gets its own process group so cancel / timeout can kill the whole
//! tree (`terminate_process_tree` signals that group only).
//!
//! The checker also remembers each task's latest failed run, so the host can
//! list them for the "需要我处理" queue without reading every session's log.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::update::{AutoCheckItem, SessionUpdateKind};
use super::{now_ms, DriverRegistry, LiveStatusSink};
use crate::ide::{diagnostics_face, tests_face};

/// Per check; a suite slower than this is reported as timed out.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(600);
pub const MAX_FAILURES: usize = 20;
pub const MAX_FAILURE_CHARS: usize = 300;
pub const OUTPUT_TAIL_CHARS: usize = 4000;
const POLL: Duration = Duration::from_millis(100);

/// What a project opted into. Both off by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoCheckSelection {
    pub tests: bool,
    pub diagnostics: bool,
}

impl AutoCheckSelection {
    pub fn any(self) -> bool {
        self.tests || self.diagnostics
    }
}

/// The host reads this from preferences; tests use a fixed answer.
pub trait AutoCheckPolicy: Send + Sync {
    fn selection(&self, project_id: &str) -> AutoCheckSelection;
}

/// Never checks anything (until the host installs its policy).
pub struct NoAutoChecks;

impl AutoCheckPolicy for NoAutoChecks {
    fn selection(&self, _project_id: &str) -> AutoCheckSelection {
        AutoCheckSelection::default()
    }
}

/// One command to run: a test or diagnostics profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckSpec {
    pub kind: &'static str,
    pub profile: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

/// Diagnostics first (fast), then tests.
pub fn discover(cwd: &Path, selection: AutoCheckSelection) -> Vec<CheckSpec> {
    let mut specs = Vec::new();
    if selection.diagnostics {
        for (profile, label) in diagnostics_face::discover_diagnostic_profiles(cwd) {
            if let Ok((program, args)) = diagnostics_face::diagnostic_command(cwd, profile) {
                specs.push(CheckSpec {
                    kind: "diagnostics",
                    profile: profile.into(),
                    label: label.into(),
                    program,
                    args,
                });
            }
        }
    }
    if selection.tests {
        for profile in tests_face::discover_test_profiles_from_root(cwd).unwrap_or_default() {
            let command = match profile.id.as_str() {
                "cargo" => tests_face::cargo_command(cwd, None),
                "python" => {
                    tests_face::python_command(cwd, None, &tests_face::default_python_program)
                }
                "vitest" => tests_face::vitest_command(cwd, None, false),
                _ => continue,
            };
            if let Ok((program, args, _)) = command {
                specs.push(CheckSpec {
                    kind: "tests",
                    profile: profile.id,
                    label: profile.label,
                    program,
                    args,
                });
            }
        }
    }
    specs
}

enum Exit {
    Finished {
        success: bool,
        stdout: String,
        stderr: String,
    },
    TimedOut(String),
    Cancelled,
    SpawnFailed(String),
}

fn collect(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn execute(
    spec: &CheckSpec,
    cwd: &Path,
    envs: &[(String, String)],
    cancel: &AtomicBool,
    timeout: Duration,
) -> Exit {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(cwd)
        .envs(envs.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::runtime::subprocess::configure_terminable_process_tree(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Exit::SpawnFailed(format!("无法运行 {}：{error}", spec.program)),
    };
    let stdout = collect(child.stdout.take());
    let stderr = collect(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        let cancelled = cancel.load(Ordering::SeqCst);
        if cancelled || Instant::now() >= deadline {
            let _ = crate::runtime::subprocess::terminate_process_tree(&mut child);
            let _ = child.wait();
            let output = format!(
                "{}{}",
                stdout.join().unwrap_or_default(),
                stderr.join().unwrap_or_default()
            );
            return if cancelled {
                Exit::Cancelled
            } else {
                Exit::TimedOut(output)
            };
        }
        std::thread::sleep(POLL);
    };
    Exit::Finished {
        success: status.is_some_and(|s| s.success()),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    }
}

/// Run one check to a verdict.
pub fn run_check(
    spec: &CheckSpec,
    cwd: &Path,
    envs: &[(String, String)],
    cancel: &AtomicBool,
    timeout: Duration,
) -> AutoCheckItem {
    let item = |state: &str, summary: String, failures: Vec<String>, output: &str| AutoCheckItem {
        kind: spec.kind.into(),
        profile: spec.profile.clone(),
        label: spec.label.clone(),
        state: state.into(),
        summary,
        failures: failures
            .into_iter()
            .take(MAX_FAILURES)
            .map(|f| head(&f, MAX_FAILURE_CHARS))
            .collect(),
        output_tail: tail(output, OUTPUT_TAIL_CHARS),
    };
    let (success, stdout, stderr) = match execute(spec, cwd, envs, cancel, timeout) {
        Exit::Finished {
            success,
            stdout,
            stderr,
        } => (success, stdout, stderr),
        Exit::TimedOut(output) => {
            let summary = match timeout.as_secs() {
                secs if secs >= 60 => format!("超过 {} 分钟未结束，已停止", secs / 60),
                secs => format!("超过 {secs} 秒未结束，已停止"),
            };
            return item("timedOut", summary, Vec::new(), &output);
        }
        Exit::Cancelled => return item("cancelled", "新一轮开始，已取消".into(), Vec::new(), ""),
        Exit::SpawnFailed(message) => return item("error", message, Vec::new(), ""),
    };
    let raw = format!("{stdout}{stderr}");
    if spec.kind == "diagnostics" {
        let result =
            diagnostics_face::parse_diagnostic_run_result(cwd, &spec.profile, &stdout, &stderr);
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, diagnostics_face::DiagnosticSeverity::Error))
            .map(|d| format!("{}:{}:{} {}", d.file, d.line, d.column, d.message))
            .collect();
        let warnings = result.diagnostics.len() - errors.len();
        return if !errors.is_empty() {
            let summary = format!("{} 个错误，{warnings} 个警告", errors.len());
            item("failed", summary, errors, &raw)
        } else if !success && result.diagnostics.is_empty() {
            item("error", "命令失败，未解析出诊断".into(), Vec::new(), &raw)
        } else if warnings > 0 {
            item(
                "passed",
                format!("无错误，{warnings} 个警告"),
                Vec::new(),
                &raw,
            )
        } else {
            item("passed", "无错误".into(), Vec::new(), &raw)
        };
    }
    let result = match spec.profile.as_str() {
        "cargo" => tests_face::parse_cargo_test_output(cwd, &raw),
        "python" => tests_face::parse_pytest_output(cwd, &raw),
        "vitest" => tests_face::parse_vitest_json(cwd, &stdout),
        // A profile with no parser is judged by its exit status.
        _ if success => return item("passed", "通过".into(), Vec::new(), &raw),
        _ => return item("failed", "命令以非零状态退出".into(), Vec::new(), &raw),
    };
    let failures: Vec<String> = result
        .failures
        .iter()
        .map(|f| format!("{}: {}", f.name, f.message))
        .collect();
    if result.failed > 0 || !failures.is_empty() {
        let failed = result.failed.max(failures.len());
        let summary = format!("{} 项中 {failed} 项失败", result.total.max(failed));
        item("failed", summary, failures, &raw)
    } else if !success {
        item(
            "error",
            "测试命令失败（可能没编译通过）".into(),
            Vec::new(),
            &raw,
        )
    } else {
        item(
            "passed",
            format!("{} 项全部通过", result.total),
            Vec::new(),
            &raw,
        )
    }
}

/// The overall state of a finished run.
pub fn overall(items: &[AutoCheckItem]) -> &'static str {
    if items.iter().any(|i| i.state == "cancelled") {
        "cancelled"
    } else if items.iter().all(|i| i.state == "passed") {
        "passed"
    } else {
        "failed"
    }
}

fn head(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(chars).collect();
    cut.push('…');
    cut
}

fn tail(text: &str, chars: usize) -> String {
    let count = text.chars().count();
    if count <= chars {
        return text.to_string();
    }
    text.chars().skip(count - chars).collect()
}

struct Running {
    generation: u64,
    cancel: Arc<AtomicBool>,
}

/// A task whose latest check run failed, for the attention queue.
#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckFailure {
    pub task_id: String,
    pub turn_id: String,
    /// e.g. `测试 3 项失败，诊断 2 个错误`.
    pub summary: String,
    pub at_ms: u64,
}

/// One line for a failed run: failing tests, diagnostic errors, and checks
/// that did not finish (error / timeout) told apart.
pub fn failure_summary(items: &[AutoCheckItem]) -> String {
    let count = |kind: &str| -> Option<usize> {
        let failed: Vec<&AutoCheckItem> = items
            .iter()
            .filter(|i| i.kind == kind && i.state == "failed")
            .collect();
        (!failed.is_empty()).then(|| failed.iter().map(|i| i.failures.len().max(1)).sum())
    };
    let mut parts = Vec::new();
    if let Some(n) = count("tests") {
        parts.push(format!("测试 {n} 项失败"));
    }
    if let Some(n) = count("diagnostics") {
        parts.push(format!("诊断 {n} 个错误"));
    }
    if items
        .iter()
        .any(|i| i.state == "error" || i.state == "timedOut")
    {
        parts.push("检查没跑完".to_string());
    }
    if parts.is_empty() {
        "检查没通过".to_string()
    } else {
        parts.join("，")
    }
}

/// Keep `failures` in step with one `autoCheck` update: a failed run is
/// remembered, anything else (running, passed, cancelled, skipped) clears it.
fn remember(
    failures: &mut HashMap<String, CheckFailure>,
    task_id: &str,
    kind: &SessionUpdateKind,
    at_ms: u64,
) {
    let SessionUpdateKind::AutoCheck {
        turn_id,
        state,
        items,
    } = kind
    else {
        return;
    };
    if state == "failed" {
        failures.insert(
            task_id.to_string(),
            CheckFailure {
                task_id: task_id.to_string(),
                turn_id: turn_id.clone(),
                summary: failure_summary(items),
                at_ms,
            },
        );
    } else {
        failures.remove(task_id);
    }
}

/// Starts and cancels checks per task; results go to the session's log.
pub struct AutoChecker {
    inner: Arc<dyn LiveStatusSink>,
    registry: Arc<DriverRegistry>,
    policy: Mutex<Arc<dyn AutoCheckPolicy>>,
    envs: Vec<(String, String)>,
    timeout: Duration,
    running: Mutex<HashMap<String, Running>>,
    next: AtomicU64,
    /// task → its latest run, when that run failed.
    failures: Mutex<HashMap<String, CheckFailure>>,
}

impl AutoChecker {
    pub fn new(
        inner: Arc<dyn LiveStatusSink>,
        registry: Arc<DriverRegistry>,
        envs: Vec<(String, String)>,
        timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner,
            registry,
            policy: Mutex::new(Arc::new(NoAutoChecks)),
            envs,
            timeout,
            running: Mutex::default(),
            next: AtomicU64::new(1),
            failures: Mutex::default(),
        })
    }

    pub fn set_policy(&self, policy: Arc<dyn AutoCheckPolicy>) {
        *self.policy.lock() = policy;
    }

    fn push(&self, task_id: &str, kind: SessionUpdateKind) {
        let now = now_ms();
        remember(&mut self.failures.lock(), task_id, &kind, now);
        if let Some(log) = self.registry.log(task_id) {
            let update = log.push(kind, now);
            self.inner.update(&update);
        }
    }

    /// A new turn began: stop what is running for `task_id`.
    pub fn cancel(&self, task_id: &str) {
        if let Some(running) = self.running.lock().remove(task_id) {
            running.cancel.store(true, Ordering::SeqCst);
        }
        // The new turn may fix what failed; its own check decides again.
        self.failures.lock().remove(task_id);
    }

    /// Tasks whose latest check run failed, oldest first.
    pub fn failures(&self) -> Vec<CheckFailure> {
        let mut list: Vec<CheckFailure> = self.failures.lock().values().cloned().collect();
        list.sort_by(|a, b| a.at_ms.cmp(&b.at_ms).then(a.task_id.cmp(&b.task_id)));
        list
    }

    /// `turn_id` changed files and ended normally; returns whether a run
    /// started (the project may not have opted in).
    pub fn start(
        self: &Arc<Self>,
        task_id: &str,
        project_id: &str,
        turn_id: &str,
        cwd: &Path,
    ) -> bool {
        let selection = self.policy.lock().selection(project_id);
        if !selection.any() {
            return false;
        }
        self.cancel(task_id);
        let cancel = Arc::new(AtomicBool::new(false));
        let generation = self.next.fetch_add(1, Ordering::SeqCst);
        self.running.lock().insert(
            task_id.to_string(),
            Running {
                generation,
                cancel: Arc::clone(&cancel),
            },
        );
        let this = Arc::clone(self);
        let task_id = task_id.to_string();
        let turn_id = turn_id.to_string();
        let cwd: PathBuf = cwd.to_path_buf();
        std::thread::spawn(move || {
            let report = |state: &str, items: Vec<AutoCheckItem>| {
                this.push(
                    &task_id,
                    SessionUpdateKind::AutoCheck {
                        turn_id: turn_id.clone(),
                        state: state.into(),
                        items,
                    },
                );
            };
            let specs = discover(&cwd, selection);
            if specs.is_empty() {
                report("skipped", Vec::new());
            } else {
                report("running", Vec::new());
                let mut items = Vec::new();
                for spec in &specs {
                    let item = run_check(spec, &cwd, &this.envs, &cancel, this.timeout);
                    let stop = item.state == "cancelled";
                    items.push(item);
                    if stop {
                        break;
                    }
                }
                report(overall(&items), items);
            }
            this.finish(&task_id, generation);
        });
        true
    }

    fn finish(&self, task_id: &str, generation: u64) {
        let mut running = self.running.lock();
        if running
            .get(task_id)
            .is_some_and(|r| r.generation == generation)
        {
            running.remove(task_id);
        }
    }

    /// Whether a check is running for `task_id`.
    pub fn is_running(&self, task_id: &str) -> bool {
        self.running.lock().contains_key(task_id)
    }
}

#[cfg(test)]
#[path = "auto_check_tests.rs"]
mod tests;
