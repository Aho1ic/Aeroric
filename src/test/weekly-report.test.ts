/* 周报的统计区间与任务筛选。
 *
 * 区间算错一天在真机上几乎发现不了 —— 报告"少了周一那天的活"看起来只像那天没干事。
 * 所以边界必须逐条断言,而不是靠肉眼看生成结果。
 *
 * 时刻一律用本地分量构造,与被测代码同一套语义。
 */

import { describe, expect, it } from "vitest";
import {
  localYmd,
  tasksInWindow,
  weekLabel,
  weekWindow,
  weeklyReportFileName,
} from "../weeklyReport";
import { isoWeek } from "../components/notebook/noteTemplates";
import type { Task, TaskStatus } from "../types";

function at(year: number, month: number, day: number, hour = 12, minute = 0): number {
  return new Date(year, month - 1, day, hour, minute, 0, 0).getTime();
}

function task(overrides: Partial<Task> & { id: string; status: TaskStatus }): Task {
  return {
    projectId: "p1",
    prompt: `Task ${overrides.id}`,
    agent: "claude",
    permissionMode: "ask",
    createdAt: at(2026, 9, 9),
    ...overrides,
  } as Task;
}

describe("weekWindow", () => {
  it("周一到周日:周中任一天都得到同一个七天区间", () => {
    // 2026-09-07 是周一,09-13 是周日。
    const monday = weekWindow(at(2026, 9, 7, 0, 1), 1, 0);
    const thursday = weekWindow(at(2026, 9, 10), 1, 0);
    const sunday = weekWindow(at(2026, 9, 13, 23, 30), 1, 0);

    expect(localYmd(monday.from)).toBe("2026-09-07");
    expect(localYmd(monday.to)).toBe("2026-09-13");
    expect(thursday).toEqual(monday);
    expect(sunday).toEqual(monday);
  });

  it("边界含两端且不与相邻周重叠", () => {
    const window = weekWindow(at(2026, 9, 10), 1, 0);
    // from 落在起始日零点,to 落在结束日最后一毫秒 —— 差一毫秒就会漏掉当天最后一条任务。
    expect(new Date(window.from).getHours()).toBe(0);
    expect(new Date(window.from).getMinutes()).toBe(0);
    expect(window.to - window.from).toBe(7 * 86_400_000 - 1);

    const nextWeek = weekWindow(at(2026, 9, 14), 1, 0);
    expect(nextWeek.from).toBe(window.to + 1);
  });

  it("改成周日起始时区间整体前移一天", () => {
    const window = weekWindow(at(2026, 9, 10), 0, 6);
    expect(localYmd(window.from)).toBe("2026-09-06");
    expect(localYmd(window.to)).toBe("2026-09-12");
  });

  it("起止同一天得到单日区间", () => {
    const window = weekWindow(at(2026, 9, 10), 4, 4);
    expect(localYmd(window.from)).toBe("2026-09-10");
    expect(localYmd(window.to)).toBe("2026-09-10");
    expect(window.to - window.from).toBe(86_400_000 - 1);
  });

  it("跨月与跨年不出错", () => {
    // 2026-01-01 是周四,所属周(周一起)从 2025-12-29 开始。
    const window = weekWindow(at(2026, 1, 1), 1, 0);
    expect(localYmd(window.from)).toBe("2025-12-29");
    expect(localYmd(window.to)).toBe("2026-01-04");
  });

  it("UTC+8 的清晨不会把区间整体推早一天", () => {
    // 这是 `toISOString().slice(0, 10)` 会踩的坑:UTC 日期在本地早 8 点前是昨天。
    const earlyMonday = weekWindow(at(2026, 9, 7, 1, 30), 1, 0);
    expect(localYmd(earlyMonday.from)).toBe("2026-09-07");
  });
});

describe("tasksInWindow", () => {
  const window = weekWindow(at(2026, 9, 10), 1, 0);

  it("本周创建的算进来", () => {
    const ids = tasksInWindow(
      [task({ id: "a", status: "done", createdAt: at(2026, 9, 9) })],
      window,
    );
    expect(ids.map((item) => item.id)).toEqual(["a"]);
  });

  it("上周创建、本周才完成的也算进来", () => {
    // 跨周的长任务往往正是本周最值得写进周报的活;只看 createdAt 会把它漏掉。
    const crossWeek = task({
      id: "cross",
      status: "done",
      createdAt: at(2026, 8, 31),
      completedAt: at(2026, 9, 9),
    });
    expect(tasksInWindow([crossWeek], window).map((item) => item.id)).toEqual(["cross"]);
  });

  it("完全在区间之外的不算", () => {
    const before = task({ id: "before", status: "done", createdAt: at(2026, 8, 20) });
    const after = task({ id: "after", status: "done", createdAt: at(2026, 9, 20) });
    expect(tasksInWindow([before, after], window)).toEqual([]);
  });

  it("已归档任务照样统计", () => {
    // 归档只表示"不用在主列表里看见它",不表示这周没干这件事。
    const archived = task({
      id: "archived",
      status: "done",
      createdAt: at(2026, 9, 8),
      archivedAt: at(2026, 9, 9),
    });
    expect(tasksInWindow([archived], window).map((item) => item.id)).toEqual(["archived"]);
  });

  it("没有 completedAt 时按创建时间判", () => {
    const legacy = task({ id: "legacy", status: "done", createdAt: at(2026, 9, 11) });
    expect(tasksInWindow([legacy], window).map((item) => item.id)).toEqual(["legacy"]);
  });

  it("区间两端的任务都不被切掉", () => {
    const first = task({ id: "first", status: "done", createdAt: window.from });
    const last = task({ id: "last", status: "done", createdAt: window.to });
    expect(tasksInWindow([first, last], window).map((item) => item.id)).toEqual(["first", "last"]);
  });
});

describe("weekLabel / weeklyReportFileName", () => {
  it("按 ISO 周编号,周号补两位", () => {
    expect(weekLabel(weekWindow(at(2026, 9, 10), 1, 0))).toBe("2026-W37");
    // 一月初那几天属于上一年的最后一周 —— 补零和跨年归属一起验。
    expect(weekLabel(weekWindow(at(2026, 1, 1), 1, 0))).toBe("2026-W01");
  });

  it("周首改成周日时,标题跟着内容走而不是跟着起始日", () => {
    // 回归:标题原先取区间起始日的 ISO 周。周日起始时那天属于上一个 ISO 周,于是
    // 2026-09-06..09-12 这个区间(6/7 天在 W37)会被标成 W36 —— 标题比内容早一周。
    // 两种周首覆盖的是同一批活,标题必须一致。
    expect(weekLabel(weekWindow(at(2026, 9, 10), 0, 6))).toBe("2026-W37");
    expect(weekLabel(weekWindow(at(2026, 9, 10), 1, 0))).toBe("2026-W37");
  });

  it("七种周首都落在区间内占天数最多的那个 ISO 周", () => {
    // 中点法的不变量。任一周首下,标题周号必须等于区间内天数最多的 ISO 周。
    for (let weekStartDay = 0; weekStartDay < 7; weekStartDay += 1) {
      const weekEndDay = (weekStartDay + 6) % 7;
      const win = weekWindow(at(2026, 9, 10), weekStartDay, weekEndDay);
      const perWeek: Record<string, number> = {};
      for (
        let cursor = new Date(win.from);
        cursor.getTime() <= win.to;
        cursor.setDate(cursor.getDate() + 1)
      ) {
        const { year, week } = isoWeek(new Date(cursor));
        const key = `${year}-W${String(week).padStart(2, "0")}`;
        perWeek[key] = (perWeek[key] ?? 0) + 1;
      }
      const majority = Object.entries(perWeek).sort((a, b) => b[1] - a[1])[0][0];
      expect(weekLabel(win)).toBe(majority);
    }
  });

  it("文件名带上区间两端的本地日期", () => {
    expect(weeklyReportFileName(weekWindow(at(2026, 9, 10), 1, 0))).toBe(
      "aeroric-weekly-2026-09-07_2026-09-13.md",
    );
  });
});
