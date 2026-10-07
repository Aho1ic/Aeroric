//! 自动清理的**执行**半边（规则半边在 [`crate::tasks::cleanup`]）。
//!
//! 规则（"到点了吗""哪些任务过期了"）由 P2-04 落到 `tasks/cleanup.rs`，但一直没有
//! 生产调用方——真正跑它的是 React 的 `useAppMaintenance`（30 分钟一 tick，G06 的
//! 第二条证据）。这里补上宿主与规则之间的那一步：读项目表、逐项目取任务、按策略
//! 算过期、经 [`TaskService`] 删除。删除只有一条写路径（INV-03），所以本模块自身
//! 不碰任务文件。
//!
//! 三条不删的东西写在类型上而不是注释里：
//!
//! - **非终端状态不删。** [`expired_task_ids`] 只对 `done`/`failed`/`cancelled` 出票；
//!   `interrupted` 与 `todo` 都不在内——前者还能续跑，后者是用户写下的待办。
//! - **未处理的 worktree 不删。** 删除会跟着 `git worktree remove --force` 与
//!   `git branch -D`，定时器没有资格替用户丢代码。
//! - **无归属目录不删。** 项目表里找不到的项目根本不进循环；一条项目失败只记进
//!   [`CleanupRun::errors`]，不吞掉其余项目。

use crate::tasks::cleanup::{
    expired_task_ids, should_run_cleanup, CleanupSchedule, LocalCalendar, ShouldRun,
};
use crate::tasks::service::TaskService;

/// 旧的 React tick 间隔（`useAppMaintenance.ts:92`，30 分钟）。宿主沿用同一节奏，
/// 换的是持有者而不是频率。
pub const CLEANUP_TICK_MS: u64 = 30 * 60 * 1000;

/// 一次清理调用的结果。
///
/// `Skipped` 带着 [`ShouldRun`] 原样返回，宿主据此记日志而不必重算；`Ran` 里
/// `expired` 是策略选出的候选、`removed` 是真正落盘的删除、`errors` 是逐项目失败。
/// 候选与实删分开记：`TaskService::delete` 会再过滤一遍被 star 的与运行中的，
/// 两个数不一致是有信息的，不应该合并。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupRun {
    Skipped(ShouldRun),
    Ran {
        expired: Vec<String>,
        removed: Vec<String>,
        errors: Vec<String>,
    },
}

impl CleanupRun {
    pub fn ran(&self) -> bool {
        matches!(self, CleanupRun::Ran { .. })
    }

    pub fn deleted_count(&self) -> usize {
        match self {
            CleanupRun::Skipped(_) => 0,
            CleanupRun::Ran { removed, .. } => removed.len(),
        }
    }
}

/// 到点就跑一轮清理：项目表 → 逐项目任务 → 过期候选 → `TaskService::delete`。
///
/// `now_ms` 由调用方给（规则不读钟）；`last_run_at` 的落盘也由调用方负责——本函数
/// 不改设置，只回答"删了什么"。这样崩溃夹在删除与记账之间时，下一轮会重算候选
/// （删除是幂等的：不存在的 id 不再出现在候选里），不会因为时间戳先走一步而漏删。
pub fn run_cleanup_once(
    service: &TaskService,
    schedule: &CleanupSchedule,
    calendar: &dyn LocalCalendar,
    now_ms: i64,
) -> CleanupRun {
    let due = should_run_cleanup(calendar, schedule, now_ms);
    if !due.is_due() {
        return CleanupRun::Skipped(due);
    }

    let mut expired = Vec::new();
    let mut removed = Vec::new();
    let mut errors = Vec::new();

    let projects = match crate::storage::load_projects(service.context()) {
        Ok(projects) => projects,
        Err(error) => {
            return CleanupRun::Ran {
                expired,
                removed,
                errors: vec![format!("projects: {error}")],
            }
        }
    };

    for project in projects {
        let Some(project_id) = project.id().map(str::to_string) else {
            // 没有 id 的记录没有归属，跳过而不是猜。
            continue;
        };
        let snapshot = match service.load(&project_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                errors.push(format!("{project_id}: {error}"));
                continue;
            }
        };
        let candidates = expired_task_ids(&snapshot.records, schedule, now_ms);
        if candidates.is_empty() {
            continue;
        }
        match service.delete(&project_id, &candidates) {
            Ok(deleted) => {
                // 候选与实删都记账：被 star 的、运行中的由 TaskService 再判一次。
                expired.extend(candidates);
                removed.extend(deleted.removed);
            }
            Err(error) => {
                errors.push(format!("{project_id}: {error}"));
            }
        }
    }

    CleanupRun::Ran {
        expired,
        removed,
        errors,
    }
}
