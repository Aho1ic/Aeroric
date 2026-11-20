//! C8 (2026-11-20): the tools one launch of a session has.
//!
//! Aeroric's own come from the context server, whose families are offered
//! on the same conditions `aeroric-cli mcp-context` asks the host about
//! (memory on for the project, Wiki pages the agent may read, a reference
//! library, an expert model). The harness's own are a known list, marked as
//! such: Aeroric neither gives nor controls them.

use std::path::PathBuf;

use aeroric_protocol::tools::{
    EffectiveTool, ToolGroup, ToolLaunch, ToolOwner, ToolScope, AERORIC_TOOLS,
};
use parking_lot::Mutex;

use crate::storage::launch_log;

/// Whether this launch got the context server, and if not why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Injection {
    Injected,
    /// A session on another machine: the server would have to reach this
    /// Mac's host (batch D).
    Remote,
    /// The harness has no way to be given an MCP server per start yet.
    Unsupported,
    /// The host has not set the server up (a test host, a broken bundle).
    NoServer,
}

impl Injection {
    pub fn reason(self, harness: &str) -> Option<String> {
        match self {
            Injection::Injected => None,
            Injection::Remote => Some("远端会话还没有接入 Aeroric 工具".into()),
            Injection::Unsupported => Some(format!(
                "{} 还不能在启动时接入 Aeroric 工具",
                harness_name(harness)
            )),
            Injection::NoServer => Some("宿主没有准备 Aeroric 工具服务器".into()),
        }
    }
}

/// Which of the context server's conditional families this launch's
/// project qualifies for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offered {
    pub memory: bool,
    pub wiki: bool,
    pub references: bool,
    pub expert: bool,
}

fn harness_name(harness: &str) -> &str {
    match harness {
        "claude" => "Claude",
        "codex" => "Codex",
        "omp" => "OMP",
        "dsh" => "DSH",
        other => other,
    }
}

/// The harness's own tools Aeroric knows of. Not exhaustive and not
/// controlled by Aeroric: the permission mode decides which may run.
pub fn harness_tools(harness: &str) -> &'static [(&'static str, ToolScope)] {
    match harness {
        "claude" => &[
            ("Read", ToolScope::ReadOnly),
            ("Glob", ToolScope::ReadOnly),
            ("Grep", ToolScope::ReadOnly),
            ("Edit", ToolScope::Writes),
            ("Write", ToolScope::Writes),
            ("Bash", ToolScope::Writes),
            ("Task", ToolScope::Writes),
            ("TodoWrite", ToolScope::ReadOnly),
            ("WebFetch", ToolScope::SendsOut),
            ("WebSearch", ToolScope::SendsOut),
        ],
        "codex" => &[
            ("shell", ToolScope::Writes),
            ("apply_patch", ToolScope::Writes),
            ("update_plan", ToolScope::ReadOnly),
            ("view_image", ToolScope::ReadOnly),
            ("web_search", ToolScope::SendsOut),
        ],
        _ => &[],
    }
}

fn group_basis(group: ToolGroup, offered: Offered) -> (bool, &'static str) {
    match group {
        ToolGroup::Base => (true, "Aeroric 工具，所有接入的会话都有"),
        ToolGroup::Memory if offered.memory => (true, "这个项目开启了记忆"),
        ToolGroup::Memory => (false, "这个项目没有开启记忆"),
        ToolGroup::Wiki if offered.wiki => (true, "项目有 Wiki，且允许 Agent 读"),
        ToolGroup::Wiki => (false, "项目还没有 Wiki 页面，或没有允许 Agent 读"),
        ToolGroup::References if offered.references => (true, "你登记了参考代码库"),
        ToolGroup::References => (false, "还没有登记参考代码库"),
        ToolGroup::Expert if offered.expert => (true, "设置了专家模型"),
        ToolGroup::Expert => (false, "还没有设置专家模型"),
        ToolGroup::Harness => (true, ""),
    }
}

/// The tools of one launch of `harness`.
pub fn catalog(harness: &str, injection: Injection, offered: Offered, at_ms: u64) -> ToolLaunch {
    let reason = injection.reason(harness);
    let mut tools: Vec<EffectiveTool> = AERORIC_TOOLS
        .iter()
        .map(|(name, group, scope)| {
            let (ready, basis) = match &reason {
                Some(why) => (false, why.clone()),
                None => {
                    let (ready, basis) = group_basis(*group, offered);
                    (ready, basis.to_string())
                }
            };
            EffectiveTool {
                name: name.to_string(),
                owner: ToolOwner::Aeroric,
                group: *group,
                scope: *scope,
                ready,
                basis,
            }
        })
        .collect();
    let own = harness_tools(harness);
    tools.extend(own.iter().map(|(name, scope)| EffectiveTool {
        name: name.to_string(),
        owner: ToolOwner::Harness,
        group: ToolGroup::Harness,
        scope: *scope,
        ready: true,
        basis: format!("{} 自带；是否能用由权限模式决定", harness_name(harness)),
    }));
    ToolLaunch {
        at_ms,
        harness: harness.to_string(),
        injected: injection == Injection::Injected,
        reason,
        tools,
    }
}

/// Each task's launches and their tools, under `<data>/tool-catalogs/`.
#[derive(Default)]
pub struct ToolLog {
    data_dir: Mutex<Option<PathBuf>>,
}

const FOLDER: &str = "tool-catalogs";

impl ToolLog {
    pub fn set_dir(&self, data_dir: PathBuf) {
        *self.data_dir.lock() = Some(data_dir);
    }

    pub fn note(&self, task_id: &str, launch: ToolLaunch) {
        let Some(data_dir) = self.data_dir.lock().clone() else {
            return;
        };
        let path = launch_log::file(&data_dir, FOLDER, task_id);
        if let Err(error) = launch_log::push(&path, launch) {
            eprintln!("[aeroric] tools of {task_id} not kept: {error}");
        }
    }

    pub fn launches(&self, task_id: &str) -> Vec<ToolLaunch> {
        let Some(data_dir) = self.data_dir.lock().clone() else {
            return Vec::new();
        };
        launch_log::read(&launch_log::file(&data_dir, FOLDER, task_id))
    }

    pub fn latest(&self, task_id: &str) -> Option<ToolLaunch> {
        self.launches(task_id).pop()
    }
}

#[cfg(test)]
#[path = "tool_catalog_tests.rs"]
mod tests;
