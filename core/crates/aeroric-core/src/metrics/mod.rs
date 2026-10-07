//! 用量度量族（Task P8-04）：会话指标、用量统计与索引、Agent 配置用量、订阅用量快照。
//!
//! 这一族与其它模块的形状差别在于**它读用户家目录里的外部日志**（`.codex*`/
//! `.claude*`/`.dsh`/`.omp` 的会话 jsonl），而它自己写的只有数据根下的
//! `usage-statistics.sqlite3` 与 `agent-usage.json`。两个根都必须可注入：
//!
//! - **G21**：宿主测试过去直接写用户的真实统计库。落点由 [`MetricsPaths`] 显式给出，
//!   测试指向临时目录即可，进程内没有共享的"当前数据根"可被并发测试互相污染。
//! - **G03**：索引 watcher 过去是 `thread::spawn` 裸线程，进程退出也不会停。
//!   现在由 [`index::UsageIndexWatcher`] 持停止位与 `JoinHandle`，drop 即停。
//! - **G06**：定时器归 core。索引扫描节奏在 [`index`]（事件驱动＋30s 兜底＋30s 重解析
//!   节流），清理调度节奏在 [`crate::maintenance::cleanup`]，都不再由 React 的
//!   `setInterval` 决定。
//!
//! 出口是注入的回调（[`index::IndexEventSink`]），不是宿主的事件句柄：core 不认识窗口，
//! 宿主把回调转成旧桌面通道＋v1 topic 双发。

pub mod agent_usage;
pub mod analytics;
pub mod index;
pub mod usage;

use std::path::{Path, PathBuf};

use crate::context::CoreContext;

pub use agent_usage::{
    load_agent_usage_snapshot_at, record_agent_config_usage_at, AgentUsageSnapshot, AgentUsageStats,
};
pub use analytics::{
    collect_jsonl_files, is_codex_session, is_dsh_session, is_omp_session, is_zstd_usage_log,
    parse_session_metrics_cached, parse_session_metrics_from_path, read_session_metrics_at,
    read_usage_statistics_async, read_usage_statistics_at, usage_roots_for_home, SessionMetrics,
    UsageAgent, UsageRequest, UsageStatistics, UsageStatisticsBreakdown, UsageStatisticsDay,
    UsageStatisticsTotals,
};
pub use index::{
    latest_updated_at_at, load_requests_at, refresh_index_at, refresh_now_at, start as start_index,
    IndexEventSink, UsageIndexWatcher, INDEX_EVENT, INDEX_TOPIC,
};
pub use usage::{
    read_usage_snapshot, ClaudeUsageData, CodexUsageData, UsageSnapshot, UsageSource, UsageWindow,
};

/// 这一族要知道的两个根。`home` 只读（外部会话日志），`data_dir` 是本进程的写落点。
///
/// 两个根分开是 P2-02 就定下的规矩：`--data-dir` 移动 Aeroric 自己拥有的东西，
/// 但绝不重定向它只读的外部 transcripts。测试要同时覆盖两件事时给两个临时目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricsPaths {
    pub home: PathBuf,
    pub data_dir: PathBuf,
}

impl MetricsPaths {
    pub fn new(home: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            data_dir: data_dir.into(),
        }
    }

    /// 从注入的上下文取：外部家目录＋已解析的数据根。
    pub fn from_context(context: &CoreContext) -> Self {
        Self {
            home: context.paths.user_home().to_path_buf(),
            data_dir: context.paths.data_dir().to_path_buf(),
        }
    }

    /// `usage-statistics.sqlite3` 的落点。
    pub fn index_database(&self) -> PathBuf {
        self.data_dir.join("usage-statistics.sqlite3")
    }

    /// `agent-usage.json` 的落点。
    pub fn agent_usage_file(&self) -> PathBuf {
        self.data_dir.join("agent-usage.json")
    }

    /// 会话日志的扫描根（外部家目录下的四族＋受管 agent-homes）。
    pub fn usage_roots(&self) -> Vec<PathBuf> {
        usage_roots_for_home(&self.home)
    }

    /// 外部家目录之外，被管理的 agent-homes 也要扫——它们在数据根下。
    pub fn managed_agent_homes(&self) -> PathBuf {
        self.data_dir.join("agent-homes")
    }
}

impl MetricsPaths {
    /// 测试与宿主都用同一个构造口径：家目录给 `home`，数据库落 `data_dir`。
    pub fn for_home(home: &Path) -> Self {
        Self::new(home, home.join(".aeroric"))
    }
}
