//! 会话发现层：Claude／Codex 族的根目录与文件收集（P3-04 第二片）。
//!
//! 逐字迁移自宿主 `src-tauri/src/session.rs` 的 `encode_claude_project_path`、
//! `append_custom_agent_session_roots`、`collect_session_files_from_roots`、
//! `collect_session_files`、`collect_jsonl_files`、`codex_sessions_roots`、
//! `claude_sessions_dirs_for_project`、`claude_sessions_dir_for_agent_in`。
//! 唯一形状变化：**`home` 与「自定义 agent 的 home 目录名」由调用方注入**——core 不碰
//! 全局 home（`core-discovers-no-real-home`），agent→档案名的映射规则留在宿主的
//! `app_settings`（唯一真相），搬进来就会有两份会各自漂移的判定（补记一百一十九
//! 「更正只落到第三份副本」的同族坑）。
//!
//! 四条发现层规则照抄宿主原文（corpus §二）：
//! - Claude：`<agent-home>/projects/<编码 cwd>/`，递归收**任意** `*.jsonl`；
//! - Codex：`<project>/.codex/sessions` ＋ `<home>/.codex/sessions` ＋
//!   全部 `agent-homes/*` 的 `sessions`，递归但**只收 `rollout-*.jsonl`**；
//! - 家族由任务记录决定，**不从目录后缀推断**（21 个不以 `_codex` 结尾的档案里
//!   放着 221 份 `rollout-*.jsonl`，按后缀"简化"会让它们静默消失——corpus §二·结论 2）；
//! - 档案根／`.tmp`／`cache` 不在任何根里，放宽 glob 正是 `G13`／INV-06 的失效形状。

use std::fs;
use std::path::{Path, PathBuf};

/// Claude 的 cwd→目录名编码：非 `[A-Za-z0-9-]` 一律换 `-`。
pub fn encode_claude_project_path(project_path: &str) -> String {
    project_path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// 把每个受管 agent home（`<home>/.aeroric/agent-homes/*`）的族内会话根追加进 `roots`。
pub fn append_custom_agent_session_roots(
    roots: &mut Vec<PathBuf>,
    home: &Path,
    project_path: &str,
    is_codex: bool,
) {
    let agent_homes = home.join(".aeroric").join("agent-homes");
    let Ok(entries) = fs::read_dir(agent_homes) else {
        return;
    };
    let encoded_project = encode_claude_project_path(project_path);
    for entry in entries.flatten() {
        let agent_home = entry.path();
        if !agent_home.is_dir() {
            continue;
        }
        let root = if is_codex {
            agent_home.join("sessions")
        } else {
            agent_home.join("projects").join(&encoded_project)
        };
        if !roots.iter().any(|existing| existing == &root) {
            roots.push(root);
        }
    }
}

pub fn collect_session_files_from_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        collect_session_files(root, &mut files);
    }
    files
}

/// Codex 侧收集：递归，但只收 `rollout-*.jsonl`。
pub fn collect_session_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_session_files(&path, out);
            continue;
        }

        let is_rollout_jsonl = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
            .unwrap_or(false);

        if is_rollout_jsonl {
            out.push(path);
        }
    }
}

/// Claude 侧收集：递归收任意 `*.jsonl`——子代理嵌套转录
/// （`<uuid>/subagents/worker-*.jsonl`，corpus §一·硬约束 ②）今天同样被收，
/// 「算不算一个会话」的归属裁决留给 `grill-me [sessions]`，本片不改行为。
pub fn collect_jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

/// Codex 族的发现根：项目内 `.codex/sessions` ＋ 家目录 `~/.codex/sessions`
/// ＋ 全部受管档案的 `sessions`。`home` 由宿主注入其平台家目录，core 不自行发现家目录。
pub fn codex_sessions_roots(home: Option<&Path>, project_path: &str) -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(project_path).join(".codex").join("sessions")];
    if let Some(home) = home {
        let home_root = home.join(".codex").join("sessions");
        if !roots.iter().any(|root| root == &home_root) {
            roots.push(home_root);
        }
        append_custom_agent_session_roots(&mut roots, home, project_path, true);
    }
    roots
}

/// Claude 族的发现根：`~/.claude/projects/<encoded>` ＋ 全部受管档案的
/// `projects/<encoded>`。`home` 由宿主注入。
pub fn claude_sessions_dirs_for_project(home: Option<&Path>, project_path: &str) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let mut roots = vec![home
        .join(".claude")
        .join("projects")
        .join(encode_claude_project_path(project_path))];
    append_custom_agent_session_roots(&mut roots, home, project_path, false);
    roots
}

/// 某个具体 Agent 写 transcript 的目录。
///
/// 内建 claude/claude_gpt55 用 `~/.claude/projects/<encoded>`;自定义 claude-like Agent 的
/// 启动脚本会 `export CLAUDE_CONFIG_DIR="$HOME/.aeroric/agent-homes/{id}"`(见
/// `app_settings::agent_scripts`),因此写到 `<agent-home>/projects/<encoded>`。
///
/// 早期实现统一取 `claude_sessions_dirs_for_project()` 的第一项(永远是 `~/.claude`),
/// 导致自定义 Agent 的预置 session 路径指向一个永不存在的文件:它会被立刻广播并持久化,
/// 之后读会话就报 `Cannot resolve session path`。
///
/// `custom_agent_home` ＝宿主 agent 名录判定的档案目录名，由调用方注入；
/// core 不复述名录语义（唯一真相关系见模块头）。
pub fn claude_sessions_dir_for_agent_in(
    home: &Path,
    custom_agent_home: Option<&str>,
    project_path: &str,
) -> Option<PathBuf> {
    let encoded_project = encode_claude_project_path(project_path);
    let config_root = match custom_agent_home {
        Some(name) => home.join(".aeroric").join("agent-homes").join(name),
        None => home.join(".claude"),
    };
    Some(config_root.join("projects").join(encoded_project))
}
