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

use super::update::{AutoCheckItem, SessionUpdate, SessionUpdateKind};
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
    /// B4: its failure does not fail the run (none are optional yet; the
    /// rule is in `overall`).
    pub optional: bool,
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
                    optional: false,
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
                    optional: false,
                });
            }
        }
    }
    specs
}

enum Exit {
    Finished {
        success: bool,
        /// B4: `None` when a signal ended it.
        code: Option<i32>,
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
        code: status.and_then(|s| s.code()),
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
    let started = Instant::now();
    let exit = execute(spec, cwd, envs, cancel, timeout);
    let duration_ms = Some(started.elapsed().as_millis() as u64);
    let exit_code = match &exit {
        Exit::Finished { code, .. } => *code,
        _ => None,
    };
    // B4: the raw verdict travels with the summary.
    let item =
        |state: &str, summary: String, failures: Vec<String>, output: &str, issues: Option<u64>| {
            let truncated =
                failures.len() > MAX_FAILURES || output.chars().count() > OUTPUT_TAIL_CHARS;
            AutoCheckItem {
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
                exit_code,
                issues,
                duration_ms,
                optional: spec.optional,
                truncated,
            }
        };
    let (success, stdout, stderr) = match exit {
        Exit::Finished {
            success,
            stdout,
            stderr,
            ..
        } => (success, stdout, stderr),
        Exit::TimedOut(output) => {
            let summary = match timeout.as_secs() {
                secs if secs >= 60 => format!("超过 {} 分钟未结束，已停止", secs / 60),
                secs => format!("超过 {secs} 秒未结束，已停止"),
            };
            return item("timedOut", summary, Vec::new(), &output, None);
        }
        Exit::Cancelled => {
            return item(
                "cancelled",
                "新一轮开始，已取消".into(),
                Vec::new(),
                "",
                None,
            )
        }
        Exit::SpawnFailed(message) => return item("error", message, Vec::new(), "", None),
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
        let count = Some(errors.len() as u64);
        return if !errors.is_empty() {
            let summary = format!("{} 个错误，{warnings} 个警告", errors.len());
            item("failed", summary, errors, &raw, count)
        } else if !success && result.diagnostics.is_empty() {
            item(
                "error",
                "命令失败，未解析出诊断".into(),
                Vec::new(),
                &raw,
                None,
            )
        } else if warnings > 0 {
            item(
                "passed",
                format!("无错误，{warnings} 个警告"),
                Vec::new(),
                &raw,
                count,
            )
        } else {
            item("passed", "无错误".into(), Vec::new(), &raw, count)
        };
    }
    let result = match spec.profile.as_str() {
        "cargo" => tests_face::parse_cargo_test_output(cwd, &raw),
        "python" => tests_face::parse_pytest_output(cwd, &raw),
        "vitest" => tests_face::parse_vitest_json(cwd, &stdout),
        // A profile with no parser is judged by its exit status.
        _ if success => return item("passed", "通过".into(), Vec::new(), &raw, None),
        _ => {
            return item(
                "failed",
                "命令以非零状态退出".into(),
                Vec::new(),
                &raw,
                None,
            )
        }
    };
    let failures: Vec<String> = result
        .failures
        .iter()
        .map(|f| format!("{}: {}", f.name, f.message))
        .collect();
    if result.failed > 0 || !failures.is_empty() {
        let failed = result.failed.max(failures.len());
        let summary = format!("{} 项中 {failed} 项失败", result.total.max(failed));
        item("failed", summary, failures, &raw, Some(failed as u64))
    } else if !success {
        item(
            "error",
            "测试命令失败（可能没编译通过）".into(),
            Vec::new(),
            &raw,
            None,
        )
    } else {
        item(
            "passed",
            format!("{} 项全部通过", result.total),
            Vec::new(),
            &raw,
            Some(0),
        )
    }
}

/// The overall state of a finished run. B4: `passed` when every required
/// item passed; an optional item's failure is shown but does not fail the
/// run, and a timeout, an error or a check that did not run never passes.
pub fn overall(items: &[AutoCheckItem]) -> &'static str {
    if items.iter().any(|i| i.state == "cancelled") {
        "cancelled"
    } else if items
        .iter()
        .filter(|i| !i.optional)
        .all(|i| i.state == "passed")
    {
        "passed"
    } else {
        "failed"
    }
}

/// B4: the ledger's record of a run: each item's own verdict, and whether
/// the run may rest on less than it seems (cut output, a check that errored
/// or timed out).
pub fn turn_check(
    state: &str,
    items: &[AutoCheckItem],
) -> aeroric_protocol::turn_ledger::TurnCheck {
    let passed = state == "passed";
    aeroric_protocol::turn_ledger::TurnCheck {
        passed,
        summary: if passed {
            String::new()
        } else {
            failure_summary(items)
        },
        items: items
            .iter()
            .map(|i| aeroric_protocol::turn_ledger::TurnCheckItem {
                name: i.label.clone(),
                required: !i.optional,
                verdict: i.state.clone(),
                exit_code: i.exit_code,
                issues: i.issues,
                duration_ms: i.duration_ms,
                truncated: i.truncated,
            })
            .collect(),
        incomplete: items
            .iter()
            .any(|i| i.truncated || matches!(i.state.as_str(), "error" | "timedOut")),
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
        ..
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
    /// B4′: checks a task asks for itself (accepted criteria with a command).
    extra: Mutex<Option<ExtraChecks>>,
    /// R5-1: where each finished run is kept with its turn, so a turn's
    /// grade outlives the session's in-memory log.
    ledger: Mutex<Option<Arc<super::turn_ledger::TurnLedger>>>,
    /// Review 2026-11-22: each task's latest check as published, so a page
    /// opened after it ran still shows it — a commands-only check (B2) has
    /// no turn record to be read back from.
    latest: Mutex<HashMap<String, SessionUpdate>>,
}

/// B4′: a task's own checks, run after the project's (or alone when the
/// project runs none).
pub type ExtraChecks = Arc<dyn Fn(&str) -> Vec<CheckSpec> + Send + Sync>;

/// B2: why a turn's checks run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckTrigger {
    /// The turn changed files; `revision` is the work tree the checks see.
    FileChanges { revision: String },
    /// Nothing to compare (no changes, or not a Git work tree): only the
    /// task's command checks run, and `note` says why.
    CommandsOnly { note: String },
}

/// B2: the notes a commands-only check carries.
pub const NO_FILE_CHANGES: &str = "这一轮没有文件变化，只跑了验收里的命令检查";
pub const NOT_COMPARABLE: &str = "无法比较文件（不是 Git 仓库或读不到快照），只跑了命令检查";

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
            extra: Mutex::new(None),
            ledger: Mutex::new(None),
            latest: Mutex::default(),
        })
    }

    pub fn set_extra(&self, extra: ExtraChecks) {
        *self.extra.lock() = Some(extra);
    }

    pub fn record_into(&self, ledger: Arc<super::turn_ledger::TurnLedger>) {
        *self.ledger.lock() = Some(ledger);
    }

    pub fn set_policy(&self, policy: Arc<dyn AutoCheckPolicy>) {
        *self.policy.lock() = policy;
    }

    fn push(&self, task_id: &str, kind: SessionUpdateKind) {
        let now = now_ms();
        remember(&mut self.failures.lock(), task_id, &kind, now);
        if let SessionUpdateKind::AutoCheck {
            turn_id,
            state,
            items,
            ..
        } = &kind
        {
            if let (Some(ledger), "passed" | "failed") =
                (self.ledger.lock().clone(), state.as_str())
            {
                ledger.set_check(task_id, turn_id, turn_check(state, items));
            }
        }
        if let Some(log) = self.registry.log(task_id) {
            // Held across the push, so whoever reads the log after this
            // update finds it kept as the latest too.
            let mut latest = self.latest.lock();
            let update = log.push(kind, now);
            if matches!(update.kind, SessionUpdateKind::AutoCheck { .. }) {
                latest.insert(task_id.to_string(), update.clone());
            }
            drop(latest);
            self.inner.update(&update);
        }
    }

    /// Review 2026-11-22: the task's latest check, as it was published.
    pub fn latest(&self, task_id: &str) -> Option<SessionUpdate> {
        self.latest.lock().get(task_id).cloned()
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

    /// `turn_id` ended normally; returns whether a run started (the project
    /// may not have opted in, and a commands-only turn needs a command).
    pub fn start(
        self: &Arc<Self>,
        task_id: &str,
        project_id: &str,
        turn_id: &str,
        cwd: &Path,
        trigger: CheckTrigger,
    ) -> bool {
        let selection = self.policy.lock().selection(project_id);
        let extra_checks = self.extra.lock().clone();
        let extra = extra_checks.map(|f| f(task_id)).unwrap_or_default();
        // B2: without file changes the project's own checks (tests,
        // diagnostics) have nothing new to look at; the task's commands run.
        let discover_too = selection.any() && matches!(trigger, CheckTrigger::FileChanges { .. });
        if !discover_too && extra.is_empty() {
            return false;
        }
        let (trigger_word, note, input_revision) = match trigger {
            CheckTrigger::FileChanges { revision } => ("fileChanges", None, Some(revision)),
            CheckTrigger::CommandsOnly { note } => ("commandsOnly", Some(note), None),
        };
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
                        trigger: Some(trigger_word.into()),
                        note: note.clone(),
                        input_revision: input_revision.clone(),
                    },
                );
            };
            let mut specs = if discover_too {
                discover(&cwd, selection)
            } else {
                Vec::new()
            };
            specs.extend(extra);
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
