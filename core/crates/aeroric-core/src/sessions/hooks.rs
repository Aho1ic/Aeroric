//! Hook 事件路径布局与 `HookEventReader`（P3-04 第六片，Action 3）。
//!
//! 第 12 项定 ①：`src-tauri/src/hooks.rs` 的事件路径布局四件（`hooks_dir`／
//! `hook_script_path`／`events_root`／`events_dir_for`）随本片进 core 并成为
//! **唯一真相**；宿主留同名薄包装注入自己的用户目录（同第二片的家目录关系），
//! 该文件其余部分（settings 注入、安装/卸载、readiness）仍归 P8-01。
//!
//! `HookEventReader` 是 Action 3 要的「不带宿主应用句柄的 hook watcher 读侧」：
//! 每任务 `events.jsonl` 的增量读取、offset 表与消失清理都在这里，宿主循环只
//! 剩下 notify 唤醒与 `dispatch`。既有语义**逐字保留**（含三个不漂亮的边界，
//! 改动即回归）：
//! - **partial JSONL**：无 `\n` 的残行不消费，offset 只推进到最后一个完整行末；
//! - **截断/轮转**：`size <= offset` 时原样返回 offset（不回头重放，也不失步）；
//! - **消失清理**：目录扫描没再出现的文件才丢 offset；`read_dir` 与 `open` 之间
//!   文件消失时 offset 保持旧值（今天的 `or_insert(0)` 语义）。
//!
//! 重复 SessionStart 去重与 claimed path 排他属于登记侧：排他锁在 core
//! TaskRegistry（`runtime::registry.rs`，手写约束 4 的路径粒度例外），宿主
//! dispatch 的 already/last_status 判定留宿主——那是宿主应用句柄的地盘。

use crate::paths::AppPaths;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 共享事件根下 `events.jsonl` 的一行。字段名就是 hook 脚本写盘的键，勿改。
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct HookEvent {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: String,
    #[serde(default)]
    pub tool_name: String,
    /// 新版由启动器显式传入,旧版事件没有此字段,回退到 agent == "codex"。
    #[serde(default)]
    pub codex_like: Option<bool>,
}

/// hook 脚本安装目录：`<home>/.aeroric/hooks`。
pub fn hooks_dir(home: &Path) -> PathBuf {
    home.join(".aeroric").join("hooks")
}

/// 共享 mjs 脚本落点：`<home>/.aeroric/hooks/aeroric-hook.mjs`。
pub fn hook_script_path(home: &Path) -> PathBuf {
    hooks_dir(home).join("aeroric-hook.mjs")
}

/// 全部任务事件的根目录：`<home>/.aeroric/events`。
pub fn events_root(home: &Path) -> PathBuf {
    home.join(".aeroric").join("events")
}

/// 单任务事件目录 `<events_root>/<task_id>`；id 的规则复用
/// [`AppPaths::validate_id`]（唯一字符类判定），错误文案沿用宿主扁平日志形
/// `Invalid {label} id`（pty 在网契约）。
pub fn events_dir_for(home: &Path, task_id: &str) -> Result<PathBuf, String> {
    AppPaths::validate_id(task_id, "task").map_err(|_| "Invalid task id".to_string())?;
    Ok(events_root(home).join(task_id))
}

/// 从本次读到的字节里解析完整行的事件,返回 (事件列表, 应推进到的 offset)。
///
/// 每行单独做 `String::from_utf8_lossy`:非法 UTF-8 只影响所在行(该行 JSON 大概率
/// 解析失败被跳过),绝不会让 offset 停滞 —— 只要行以 \n 结尾,无论解析成败都推进
/// 到该行末尾;残行(无 \n)留待下次循环,保持增量读取语义不变。
pub fn parse_events_and_offset(buf: &[u8], offset: u64) -> (Vec<HookEvent>, u64) {
    let mut events = Vec::new();
    let mut last_complete_end = 0usize;
    for (idx, byte) in buf.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let line = String::from_utf8_lossy(&buf[last_complete_end..idx]);
        last_complete_end = idx + 1;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<HookEvent>(&line) {
            events.push(ev);
        }
    }
    (events, offset + last_complete_end as u64)
}

/// 从 `offset` 起读一个 `events.jsonl` 的增量。
///
/// 按字节读,不要求整段是合法 UTF-8:hook 写入的 events.jsonl 混进非法字节时,
/// read_to_string 会在中间失败、offset 永不前进,watcher 每轮原地重读整段。
/// `size <= offset`（未增长、截断或轮转后的更小文件）原样返回旧 offset。
pub fn read_hook_events(path: &Path, offset: u64) -> Option<(Vec<HookEvent>, u64)> {
    let mut file = fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size <= offset {
        return Some((Vec::new(), offset));
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let (events, new_offset) = parse_events_and_offset(&buf, offset);
    Some((events, new_offset))
}

/// 不带宿主应用句柄的 hook 事件读取器：持有 events 根与每文件 offset 表，
/// 每轮 `scan()` 扫一遍 `<root>/<task>/events.jsonl`。调用方（宿主循环）
/// 负责 notify 唤醒与 dispatch；本类型只保证「每个完整行恰好交付一次」。
#[derive(Debug)]
pub struct HookEventReader {
    events_root: PathBuf,
    offsets: HashMap<PathBuf, u64>,
}

impl HookEventReader {
    pub fn new(events_root: PathBuf) -> Self {
        Self {
            events_root,
            offsets: HashMap::new(),
        }
    }

    /// 一轮增量扫描。返回顺序与旧宿主循环一致：按 `read_dir` 序、文件内行序。
    /// 读失败的保持旧 offset（文件可能已在 `read_dir` 与 `open` 之间消失）；
    /// 本轮没再出现的文件才从 offset 表清除。
    pub fn scan(&mut self) -> Vec<HookEvent> {
        let Ok(entries) = fs::read_dir(&self.events_root) else {
            return Vec::new();
        };
        let mut seen: Vec<PathBuf> = Vec::new();
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let file = dir.join("events.jsonl");
            if !file.is_file() {
                continue;
            }
            seen.push(file.clone());
            let offset = *self.offsets.entry(file.clone()).or_insert(0);
            if let Some((events, new_offset)) = read_hook_events(&file, offset) {
                self.offsets.insert(file, new_offset);
                out.extend(events);
            }
        }
        self.offsets
            .retain(|path, _| seen.iter().any(|p| p == path));
        out
    }
}
