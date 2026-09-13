/**
 * 周报的统计区间与任务筛选。
 *
 * 纯函数,时间由调用方传入 —— 与 `taskCleanup.ts` 同一条契约:区间算错一天在真机上
 * 极难发现(报告"少了周一那天的任务"看起来只像那天没干活),必须能用普通断言钉住。
 *
 * 日期算术只用本地日历分量。`toISOString().slice(0, 10)` 给的是 UTC 日期,在 UTC+8
 * 早上 8 点之前它报的是昨天,于是"本周从哪天开始"会整体偏一天。
 */

import type { Task } from "./types";
import { isoWeek } from "./components/notebook/noteTemplates";

/** 统计区间,两端都含。`from` 是起始日 00:00:00.000,`to` 是结束日 23:59:59.999。 */
export interface WeekWindow {
  from: number;
  to: number;
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

/**
 * 包含 `nowMs` 的那个自定义周的本地边界。
 *
 * `weekStartDay` / `weekEndDay` 用 0=周日 .. 6=周六。区间长度是
 * `((end - start + 7) % 7) + 1` 天,取值 1..7 —— 起止同一天得单日区间,
 * "周一到周日"(1 → 0)得 7 天。
 *
 * `to` 用「起始日 + 天数」再减 1 毫秒,而不是「结束日 23:59:59.999」:后者在
 * 跨月/跨年时要额外处理,而前者交给 `Date` 的进位。
 */
export function weekWindow(nowMs: number, weekStartDay: number, weekEndDay: number): WeekWindow {
  const spanDays = ((weekEndDay - weekStartDay + 7) % 7) + 1;
  const now = new Date(nowMs);
  const daysSinceStart = (now.getDay() - weekStartDay + 7) % 7;
  const from = new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysSinceStart);
  const next = new Date(from.getFullYear(), from.getMonth(), from.getDate() + spanDays, 0, 0, 0, 0);
  return { from: from.getTime(), to: next.getTime() - 1 };
}

/**
 * 落在区间内的任务。
 *
 * 创建时间**或**结束时间任一落在区间内就算:前者覆盖"本周开工的",后者覆盖"上周开工、
 * 本周才完成的"。只看一个会漏掉跨周的长任务 —— 而那通常恰好是本周最值得写进周报的活。
 *
 * **已归档照样统计。** 归档只表示"不用再在主列表里看见它",不表示这周没干这件事。
 */
export function tasksInWindow(tasks: Task[], window: WeekWindow): Task[] {
  return tasks.filter((task) => {
    const endedAt = task.completedAt ?? task.createdAt;
    const createdIn = task.createdAt >= window.from && task.createdAt <= window.to;
    const endedIn = endedAt >= window.from && endedAt <= window.to;
    return createdIn || endedIn;
  });
}

/**
 * `YYYY-Www`,周号补两位。与既有周报笔记模板同一套 ISO 编号。
 *
 * 取**区间中点**而不是起始日:`isoWeek` 是严格 ISO-8601(周一为周首,写死在算法里),
 * 而区间的周首可配。周日起始时,起始日那天属于上一个 ISO 周,于是标题会比内容早一周
 * ——「周日→周六」的区间有 6/7 天落在下一个 ISO 周,标题却写上一周。中点落在区间里
 * 占天数最多的那个 ISO 周上,7 种周首下都对(区间不足 7 天时中点仍在区间内)。
 */
export function weekLabel(window: WeekWindow): string {
  const { year, week } = isoWeek(new Date(window.from + (window.to - window.from) / 2));
  return `${year}-W${pad2(week)}`;
}

/** 本地 `YYYY-MM-DD`。文件名与报告里的区间行都用它。 */
export function localYmd(ms: number): string {
  const date = new Date(ms);
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

/** 报告文件名。与 `RunningView` 的导出命名同风格,便于在文件管理器里聚在一起。 */
export function weeklyReportFileName(window: WeekWindow): string {
  return `aeroric-weekly-${localYmd(window.from)}_${localYmd(window.to)}.md`;
}
