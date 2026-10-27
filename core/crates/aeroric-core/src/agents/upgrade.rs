//! Agent upgrades and new-version reminders (2026-10-27): the installed and latest
//! version of each of the four harnesses, the user's reminder choices, and one
//! streamed upgrade (or install) job per harness.
//!
//! The latest version comes from the public npm registry (every harness publishes
//! there). Upgrade commands come from `settings::plan_agent_upgrade`, which picks
//! the channel of the active installation; omp runs with the managed home
//! (`PI_CODING_AGENT_DIR`) and never sees the user's `~/.omp`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::settings::AgentUpgradeStep;
use crate::storage::atomic::{write_atomic, FileMode};

/// How long a registry answer stays fresh.
pub const LATEST_TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// Output lines a job keeps.
const LOG_LINES: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Harness {
    Claude,
    Codex,
    Dsh,
    Omp,
}

impl Harness {
    pub const ALL: [Harness; 4] = [Harness::Claude, Harness::Codex, Harness::Dsh, Harness::Omp];

    pub fn id(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Dsh => "dsh",
            Harness::Omp => "omp",
        }
    }

    pub fn from_id(id: &str) -> Option<Harness> {
        Harness::ALL.into_iter().find(|harness| harness.id() == id)
    }

    pub fn title(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Codex => "Codex",
            Harness::Dsh => "DSH",
            Harness::Omp => "omp",
        }
    }

    pub fn npm_package(self) -> &'static str {
        match self {
            Harness::Claude => "@anthropic-ai/claude-code",
            Harness::Codex => "@openai/codex",
            Harness::Dsh => "@deepseek-ai/dsh",
            Harness::Omp => "@oh-my-pi/pi-coding-agent",
        }
    }

    /// The oldest version native sessions run on (ADR-043 §8; omp phase two).
    pub fn minimum(self) -> Option<&'static str> {
        match self {
            Harness::Claude => Some("2.1.0"),
            Harness::Codex => Some("0.162.0"),
            Harness::Omp => Some("18.12.0"),
            Harness::Dsh => None,
        }
    }
}

/// Orders dotted versions numerically; a pre-release sorts before its release.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    fn split(version: &str) -> (Vec<u64>, Option<&str>) {
        let version = version.trim().trim_start_matches('v');
        let (core, pre) = match version.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (version, None),
        };
        let numbers = core
            .split('.')
            .map(|part| {
                part.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect();
        (numbers, pre)
    }
    let (left, left_pre) = split(a);
    let (right, right_pre) = split(b);
    for index in 0..left.len().max(right.len()) {
        let order = left
            .get(index)
            .unwrap_or(&0)
            .cmp(right.get(index).unwrap_or(&0));
        if order != Ordering::Equal {
            return order;
        }
    }
    match (left_pre, right_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(l), Some(r)) => l.cmp(r),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpgradeStatus {
    UpToDate,
    UpdateAvailable,
    /// Below the native-session minimum: sessions cannot run until upgraded.
    Required,
    NotInstalled,
    /// Installed, latest version not known yet.
    Unknown,
}

pub fn status(
    installed: Option<&str>,
    latest: Option<&str>,
    minimum: Option<&str>,
) -> UpgradeStatus {
    let Some(installed) = installed else {
        return UpgradeStatus::NotInstalled;
    };
    if minimum.is_some_and(|minimum| compare_versions(installed, minimum) == Ordering::Less) {
        return UpgradeStatus::Required;
    }
    match latest {
        None => UpgradeStatus::Unknown,
        Some(latest) if compare_versions(installed, latest) == Ordering::Less => {
            UpgradeStatus::UpdateAvailable
        }
        Some(_) => UpgradeStatus::UpToDate,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderPref {
    #[serde(default = "yes")]
    pub remind: bool,
    /// Quiet until a version newer than this one appears.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_version: Option<String>,
}

fn yes() -> bool {
    true
}

impl Default for ReminderPref {
    fn default() -> Self {
        ReminderPref {
            remind: true,
            skip_version: None,
        }
    }
}

/// Whether the session page shows the capsule. A required upgrade always shows.
pub fn should_remind(status: UpgradeStatus, pref: &ReminderPref, latest: Option<&str>) -> bool {
    match status {
        UpgradeStatus::Required => true,
        UpgradeStatus::UpdateAvailable => {
            pref.remind
                && match (&pref.skip_version, latest) {
                    (Some(skipped), Some(latest)) => {
                        compare_versions(latest, skipped) == Ordering::Greater
                    }
                    _ => true,
                }
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReminderMode {
    On,
    SkipVersion,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub state: JobState,
    pub installing: bool,
    pub log: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessView {
    pub agent: &'static str,
    pub title: &'static str,
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub minimum: Option<&'static str>,
    pub channel: String,
    pub status: UpgradeStatus,
    pub remind: bool,
    pub skip_version: Option<String>,
    pub should_remind: bool,
    pub job: Option<JobView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeSnapshot {
    pub checked_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_error: Option<String>,
    pub agents: Vec<HarnessView>,
}

/// The outside world, injectable for tests.
pub struct UpgradeDeps {
    pub latest:
        Arc<dyn Fn(&'static str) -> BoxFuture<'static, Result<String, String>> + Send + Sync>,
    pub installed: Arc<dyn Fn(Harness) -> Option<String> + Send + Sync>,
    pub channel: Arc<dyn Fn(Harness) -> String + Send + Sync>,
    /// `Ok(None)` = not installed → install instead.
    pub plan: Arc<dyn Fn(Harness) -> Result<Option<Vec<AgentUpgradeStep>>, String> + Send + Sync>,
    pub install: Arc<dyn Fn(Harness) -> Result<Vec<AgentUpgradeStep>, String> + Send + Sync>,
    pub env: Arc<dyn Fn(Harness) -> Vec<(String, String)> + Send + Sync>,
    pub forget_versions: Arc<dyn Fn() + Send + Sync>,
}

impl UpgradeDeps {
    /// npm registry + the active installations on this machine.
    pub fn system() -> UpgradeDeps {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        UpgradeDeps {
            latest: Arc::new(move |package| {
                let client = client.clone();
                Box::pin(async move { registry_latest(&client, package).await })
            }),
            installed: Arc::new(|harness| crate::settings::installed_agent_version(harness.id())),
            channel: Arc::new(|harness| crate::settings::agent_install_channel(harness.id())),
            plan: Arc::new(|harness| crate::settings::plan_agent_upgrade(harness.id())),
            install: Arc::new(npm_install_steps),
            env: Arc::new(system_env),
            forget_versions: Arc::new(crate::settings::forget_agent_versions),
        }
    }
}

async fn registry_latest(client: &reqwest::Client, package: &str) -> Result<String, String> {
    let url = format!("https://registry.npmjs.org/{package}/latest");
    let response = client
        .get(&url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("npm registry answered {}", response.status()));
    }
    let body: serde_json::Value = response.json().await.map_err(|error| error.to_string())?;
    body.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "npm registry answer has no version".to_string())
}

fn npm_install_steps(harness: Harness) -> Result<Vec<AgentUpgradeStep>, String> {
    let npm = crate::settings::npm_program();
    if npm.trim().is_empty() {
        return Err("找不到 npm，无法安装。请先安装 Node.js（含 npm）。".to_string());
    }
    Ok(vec![AgentUpgradeStep {
        channel: "npm".to_string(),
        program: npm,
        args: vec![
            "install".to_string(),
            "-g".to_string(),
            format!("{}@latest", harness.npm_package()),
        ],
    }])
}

fn system_env(harness: Harness) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = crate::settings::login_shell_env()
        .iter()
        .filter(|(key, _)| {
            !(harness == Harness::Omp && matches!(key.as_str(), "PI_PROFILE" | "OMP_PROFILE"))
        })
        .cloned()
        .collect();
    if harness == Harness::Omp {
        if let Some(home) = crate::settings::managed_omp_home() {
            env.retain(|(key, _)| key != "PI_CODING_AGENT_DIR");
            env.push((
                "PI_CODING_AGENT_DIR".to_string(),
                home.to_string_lossy().into_owned(),
            ));
        }
    }
    env
}

struct Job {
    state: JobState,
    installing: bool,
    log: VecDeque<String>,
    message: Option<String>,
    started_at: u64,
    finished_at: Option<u64>,
}

#[derive(Default)]
struct Latest {
    versions: HashMap<Harness, String>,
    checked_at: Option<u64>,
    error: Option<String>,
}

/// The process-wide upgrade state behind `agents.*`.
pub struct UpgradeCenter {
    prefs_path: PathBuf,
    deps: UpgradeDeps,
    latest: Mutex<Latest>,
    installed: Mutex<HashMap<Harness, Option<String>>>,
    jobs: Arc<Mutex<HashMap<Harness, Job>>>,
    check: tokio::sync::Mutex<()>,
}

impl UpgradeCenter {
    pub fn new(data_dir: &Path, deps: UpgradeDeps) -> Arc<UpgradeCenter> {
        Arc::new(UpgradeCenter {
            prefs_path: data_dir.join("agents").join("upgrade-prefs.json"),
            deps,
            latest: Mutex::new(Latest::default()),
            installed: Mutex::new(HashMap::new()),
            jobs: Arc::new(Mutex::new(HashMap::new())),
            check: tokio::sync::Mutex::new(()),
        })
    }

    fn prefs(&self) -> BTreeMap<String, ReminderPref> {
        std::fs::read(&self.prefs_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn set_reminder(&self, harness: Harness, mode: ReminderMode) -> Result<(), String> {
        let mut prefs = self.prefs();
        let latest = self.latest.lock().versions.get(&harness).cloned();
        let pref = prefs.entry(harness.id().to_string()).or_default();
        match mode {
            ReminderMode::On => {
                pref.remind = true;
                pref.skip_version = None;
            }
            ReminderMode::SkipVersion => {
                pref.skip_version = Some(latest.ok_or("还不知道最新版本，无法跳过")?);
            }
            ReminderMode::Off => pref.remind = false,
        }
        let bytes = serde_json::to_vec_pretty(&prefs).map_err(|error| error.to_string())?;
        if let Some(parent) = self.prefs_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        write_atomic(&self.prefs_path, &bytes, FileMode::Private).map_err(|error| error.to_string())
    }

    /// Ask the registry again when forced or stale. Concurrent callers share one check.
    pub async fn check(&self, force: bool) {
        let _guard = self.check.lock().await;
        let fresh = self
            .latest
            .lock()
            .checked_at
            .is_some_and(|at| now_ms().saturating_sub(at) < LATEST_TTL.as_millis() as u64);
        if fresh && !force {
            return;
        }
        let mut errors = Vec::new();
        let mut found = HashMap::new();
        for harness in Harness::ALL {
            match (self.deps.latest)(harness.npm_package()).await {
                Ok(version) => {
                    found.insert(harness, version);
                }
                Err(error) => errors.push(format!("{}: {error}", harness.title())),
            }
        }
        let mut latest = self.latest.lock();
        latest.versions.extend(found);
        latest.checked_at = Some(now_ms());
        latest.error = (!errors.is_empty()).then(|| errors.join("；"));
        drop(latest);
        if force {
            self.installed.lock().clear();
        }
    }

    fn installed_version(&self, harness: Harness) -> Option<String> {
        if let Some(known) = self.installed.lock().get(&harness) {
            return known.clone();
        }
        let version = (self.deps.installed)(harness);
        self.installed.lock().insert(harness, version.clone());
        version
    }

    /// Blocking: may run `--version` for harnesses not probed yet.
    pub fn snapshot(&self) -> UpgradeSnapshot {
        let prefs = self.prefs();
        let (latest, checked_at, check_error) = {
            let latest = self.latest.lock();
            (
                latest.versions.clone(),
                latest.checked_at,
                latest.error.clone(),
            )
        };
        let agents = Harness::ALL
            .into_iter()
            .map(|harness| {
                let installed = self.installed_version(harness);
                let latest = latest.get(&harness).cloned();
                let pref = prefs.get(harness.id()).cloned().unwrap_or_default();
                let status = status(installed.as_deref(), latest.as_deref(), harness.minimum());
                let job = self.jobs.lock().get(&harness).map(|job| JobView {
                    state: job.state,
                    installing: job.installing,
                    log: job.log.iter().cloned().collect(),
                    message: job.message.clone(),
                    started_at: job.started_at,
                    finished_at: job.finished_at,
                });
                HarnessView {
                    agent: harness.id(),
                    title: harness.title(),
                    channel: if installed.is_some() {
                        (self.deps.channel)(harness)
                    } else {
                        String::new()
                    },
                    should_remind: should_remind(status, &pref, latest.as_deref()),
                    installed,
                    latest,
                    minimum: harness.minimum(),
                    status,
                    remind: pref.remind,
                    skip_version: pref.skip_version,
                    job,
                }
            })
            .collect();
        UpgradeSnapshot {
            checked_at,
            check_error,
            agents,
        }
    }

    /// Start upgrading (or installing) `harness` in the background. Blocking while planning.
    pub fn start(self: &Arc<Self>, harness: Harness) -> Result<(), String> {
        if self
            .jobs
            .lock()
            .get(&harness)
            .is_some_and(|job| job.state == JobState::Running)
        {
            return Ok(());
        }
        let previous = self.installed_version(harness);
        let (steps, installing) = match (self.deps.plan)(harness)? {
            Some(steps) => (steps, false),
            None => ((self.deps.install)(harness)?, true),
        };
        self.jobs.lock().insert(
            harness,
            Job {
                state: JobState::Running,
                installing,
                log: VecDeque::new(),
                message: None,
                started_at: now_ms(),
                finished_at: None,
            },
        );
        let center = Arc::clone(self);
        let env = (self.deps.env)(harness);
        std::thread::spawn(move || {
            let outcome = run_steps(&steps, &env, &center.jobs, harness);
            (center.deps.forget_versions)();
            center.installed.lock().remove(&harness);
            let current = center.installed_version(harness);
            let (state, message) = match outcome {
                Err(error) => (JobState::Failed, error),
                Ok(()) => match (&previous, &current) {
                    (_, None) => (JobState::Failed, "命令已完成，但仍检测不到版本".to_string()),
                    (Some(old), Some(new)) if compare_versions(new, old) != Ordering::Greater => {
                        // Already newest is fine when the registry agrees.
                        let latest = center.latest.lock().versions.get(&harness).cloned();
                        if latest
                            .is_some_and(|latest| compare_versions(new, &latest) != Ordering::Less)
                        {
                            (JobState::Succeeded, format!("已是最新 {new}"))
                        } else {
                            (JobState::Failed, format!("命令已完成，但版本仍是 {new}"))
                        }
                    }
                    (_, Some(new)) => (JobState::Succeeded, format!("已升级到 {new}")),
                },
            };
            if let Some(job) = center.jobs.lock().get_mut(&harness) {
                job.state = state;
                job.message = Some(message);
                job.finished_at = Some(now_ms());
            }
        });
        Ok(())
    }
}

fn push_line(jobs: &Mutex<HashMap<Harness, Job>>, harness: Harness, line: String) {
    let line = line.trim_end().to_string();
    if line.is_empty() {
        return;
    }
    if let Some(job) = jobs.lock().get_mut(&harness) {
        if job.log.len() == LOG_LINES {
            job.log.pop_front();
        }
        job.log.push_back(line);
    }
}

fn run_steps(
    steps: &[AgentUpgradeStep],
    env: &[(String, String)],
    jobs: &Arc<Mutex<HashMap<Harness, Job>>>,
    harness: Harness,
) -> Result<(), String> {
    for step in steps {
        push_line(
            jobs,
            harness,
            format!("$ {} {}", step.program, step.args.join(" ")),
        );
        let mut command = Command::new(&step.program);
        if harness == Harness::Omp {
            command.env_remove("PI_PROFILE").env_remove("OMP_PROFILE");
        }
        crate::runtime::subprocess::configure_background_command(&mut command);
        command
            .args(&step.args)
            .envs(env.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("无法运行 {}：{error}", step.program))?;
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|out| Box::new(out) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|err| Box::new(err) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|stream| {
            let jobs = Arc::clone(jobs);
            std::thread::spawn(move || {
                for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
                    // Progress bars redraw with `\r`; keep the last frame.
                    let text = String::from_utf8_lossy(&line);
                    let last = text
                        .rsplit('\r')
                        .find(|part| !part.trim().is_empty())
                        .unwrap_or("");
                    push_line(&jobs, harness, last.to_string());
                }
            })
        })
        .collect();
        let status = child.wait().map_err(|error| error.to_string())?;
        for reader in readers {
            let _ = reader.join();
        }
        if !status.success() {
            let tail = jobs
                .lock()
                .get(&harness)
                .and_then(|job| job.log.back().cloned())
                .unwrap_or_default();
            return Err(if tail.is_empty() {
                format!("升级命令退出码 {status}")
            } else {
                tail
            });
        }
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;
