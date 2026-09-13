/**
 * 定时自动物理删除对话记录的调度判定与候选筛选。
 *
 * 两条契约,别破:
 *
 * 1. **纯函数,不读时钟。** 现在几点一律由调用方传 `nowMs`。调度规则要能用普通断言
 *    钉住,而不是靠 `sleep` 去撞一个真实的周日晚八点。
 * 2. **日期算术用本地日历分量,不用 `toISOString()`。** `toISOString().slice(0, 10)`
 *    给的是 UTC 日期:在 UTC+8,早上 8 点之前它报的是昨天,于是"本周日 20:00"这个
 *    时刻会算错一整天。构造具体时刻一律走 `new Date(y, m, d, h, ...)`(本地),
 *    只有"加减整天"才走 `Date.UTC` 毫秒算术(那里没有夏令时)。
 */

import type { Task } from "./types";
import { isTerminalTaskStatus } from "./types";
import type { AutoCleanupSettings } from "./components/app-settings/types";

const DAY_MS = 86_400_000;

/**
 * `nowMs` 之前(含当刻)最近的那个 `weekday` 的 `hour:00` 本地时刻。
 *
 * 本周该时刻还没到就退回上周同一时刻 —— 调度问的是"最近一个应执行的时间点过了没",
 * 而不是"下一个是什么时候"。
 */
function latestWeeklySlot(nowMs: number, weekday: number, hour: number): number {
  const now = new Date(nowMs);
  const daysSinceSlot = (now.getDay() - weekday + 7) % 7;
  const slot = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() - daysSinceSlot,
    hour,
    0,
    0,
    0,
  );
  if (slot.getTime() > nowMs) return slot.getTime() - 7 * DAY_MS;
  return slot.getTime();
}

/**
 * 是否到点该跑一次清理。
 *
 * `lastRunAt` 缺失时返回 `false`:刚打开开关的用户不该在下一秒就丢掉半年的记录。
 * 调用方负责先把 `lastRunAt` 落成当前时间,下一个周期才真的删。
 */
export function shouldRunCleanup(settings: AutoCleanupSettings, nowMs: number): boolean {
  if (!settings.enabled) return false;
  const lastRunAt = settings.last_run_at;
  if (lastRunAt == null) return false;

  if (settings.mode === "weekly") {
    const slot = latestWeeklySlot(nowMs, settings.weekday, settings.hour);
    return nowMs >= slot && lastRunAt < slot;
  }
  return nowMs - lastRunAt >= settings.interval_days * DAY_MS;
}

/**
 * 该被物理删除的 taskId。
 *
 * 门槛是**正向**的 `isTerminalTaskStatus`(`done`/`failed`/`cancelled`),不是"排除活动状态"。
 * 差别不是风格:`interrupted` 既不活动也不是 `todo`,用排除法写会把一条还能续跑的任务
 * 判成已结束并删掉。`todo` 同理 —— 它承载的是用户写下的待办,没开工谈不上超期。
 *
 * `starred` 也跳过:`deleteTasks` 本来就会过滤收藏项,这里提前跳过,免得 toast 报出一个
 * 比实际删除数更大的数字。
 *
 * **已归档不豁免。** 归档管"从主列表移走",保留期管"回收磁盘",两件事。
 * 若要改成"归档等于永久保留",在这里加一条 `if (task.archivedAt) continue;`。
 *
 * **未处理的 worktree 不删。** `deleteTasks` 对非活动任务一律 `git worktree remove --force`
 * 再 `git branch -D` —— `--force` 丢未提交改动,`-D` 强删未合并分支。用户点删除按钮时
 * 那是明确的丢弃;定时清理若走同一条路,会在用户还没决定合不合(`pending_merge` 分组
 * 就是这个状态)时把代码静默丢掉。`worktreeDiscarded` 为真表示用户已经合过或丢过,
 * 工作树不在了,对话记录可以回收。
 */
export function expiredTaskIds(
  tasks: Task[],
  settings: AutoCleanupSettings,
  nowMs: number,
): string[] {
  const cutoff = nowMs - settings.retain_days * DAY_MS;
  const expired: string[] = [];
  for (const task of tasks) {
    if (!isTerminalTaskStatus(task.status)) continue;
    if (task.starred) continue;
    if (task.worktreePath && !task.worktreeDiscarded) continue;
    // completedAt 是本次新增的字段,历史任务没有 —— 回退到创建时间,
    // 否则老任务永远超不了期,磁盘照样长。偏差在真实数据上中位 0.08 天、
    // 最大几天,保留期默认 30 天,不会把刚完成的任务提前清掉。
    const endedAt = task.completedAt ?? task.createdAt;
    if (endedAt <= cutoff) expired.push(task.id);
  }
  return expired;
}
