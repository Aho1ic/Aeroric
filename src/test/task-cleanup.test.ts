/* 定时自动物理删除:调度判定与候选筛选。
 *
 * 全部走纯函数 + 显式 `nowMs`。真机等一个周日晚八点不可行,而这条逻辑一旦算错方向就是
 * 不可逆的数据删除 —— 需要能廉价、确定地把每个分支钉住。
 *
 * 时刻一律用本地分量构造(`new Date(y, m, d, h)`),与被测代码同一套语义;写死 epoch
 * 数字会让断言在别的时区变成另一个含义。
 */

import { describe, expect, it } from "vitest";
import { expiredTaskIds, shouldRunCleanup } from "../taskCleanup";
import { normalizeAutoCleanupSettings } from "../components/app-settings/types";
import type { AutoCleanupSettings } from "../components/app-settings/types";
import type { Task, TaskStatus } from "../types";

const DAY_MS = 86_400_000;

function settings(overrides: Partial<AutoCleanupSettings> = {}): AutoCleanupSettings {
  return normalizeAutoCleanupSettings({ enabled: true, ...overrides });
}

function task(overrides: Partial<Task> & { id: string; status: TaskStatus }): Task {
  return {
    projectId: "p1",
    prompt: `Task ${overrides.id}`,
    agent: "claude",
    permissionMode: "ask",
    createdAt: 0,
    ...overrides,
  } as Task;
}

/** 本地时刻,便于按"星期几 + 几点"表达而不用手算 epoch。 */
function localTime(year: number, month: number, day: number, hour: number, minute = 0): number {
  return new Date(year, month - 1, day, hour, minute, 0, 0).getTime();
}

describe("shouldRunCleanup", () => {
  it("关闭时永不执行", () => {
    const disabled = settings({ enabled: false, mode: "interval", last_run_at: 0 });
    expect(shouldRunCleanup(disabled, Date.now())).toBe(false);
  });

  it("从未跑过时不执行,把第一轮让给调用方去记时间", () => {
    // 这条是防"勾上开关的下一秒就删掉半年记录"。last_run_at 缺失只说明这是第一次,
    // 不说明已经欠了一次执行。
    expect(shouldRunCleanup(settings({ mode: "interval" }), Date.now())).toBe(false);
    expect(shouldRunCleanup(settings({ mode: "weekly" }), Date.now())).toBe(false);
  });

  describe("weekly 模式", () => {
    // 2026-09-13 是周日。时间槽设为周日 20:00。
    const weekly = (lastRunAt: number) =>
      settings({ mode: "weekly", weekday: 0, hour: 20, last_run_at: lastRunAt });

    it("越过本周时间槽且上次执行在槽之前时执行", () => {
      const lastRun = localTime(2026, 9, 11, 9);
      expect(shouldRunCleanup(weekly(lastRun), localTime(2026, 9, 13, 20, 1))).toBe(true);
    });

    it("上次执行已在同一个槽之后就不再重复", () => {
      // 半小时一轮的轮询会反复问同一个槽;不比对 last_run_at 就会每半小时删一次。
      const lastRun = localTime(2026, 9, 13, 20, 5);
      expect(shouldRunCleanup(weekly(lastRun), localTime(2026, 9, 13, 23))).toBe(false);
    });

    it("本周槽还没到时不因为星期已过就提前执行", () => {
      // 周日 19:59:今天是周日,但 20:00 还没到。此时最近的应执行时刻是上周日,
      // 而上次执行(本周一)已在其之后,所以不该跑。
      const lastRun = localTime(2026, 9, 7, 10);
      expect(shouldRunCleanup(weekly(lastRun), localTime(2026, 9, 13, 19, 59))).toBe(false);
    });

    it("跨过整整一周没开应用时,重新打开就补跑一次", () => {
      const lastRun = localTime(2026, 9, 6, 21);
      expect(shouldRunCleanup(weekly(lastRun), localTime(2026, 9, 14, 9))).toBe(true);
    });
  });

  describe("interval 模式", () => {
    it("满 7 天执行,差一点不执行", () => {
      const now = localTime(2026, 9, 13, 12);
      const seven = settings({ mode: "interval", interval_days: 7 });
      expect(shouldRunCleanup({ ...seven, last_run_at: now - 7 * DAY_MS }, now)).toBe(true);
      expect(shouldRunCleanup({ ...seven, last_run_at: now - 6 * DAY_MS }, now)).toBe(false);
    });

    it("interval 模式不看 weekday/hour", () => {
      // 两者共用一份配置结构,分支写错时最容易表现为"改了星期,间隔也变了"。
      const now = localTime(2026, 9, 13, 3);
      const base = settings({
        mode: "interval",
        interval_days: 30,
        weekday: 4,
        hour: 20,
        last_run_at: now - 30 * DAY_MS,
      });
      expect(shouldRunCleanup(base, now)).toBe(true);
    });
  });
});

describe("expiredTaskIds", () => {
  const now = localTime(2026, 9, 13, 12);
  const retain30 = settings({ retain_days: 30 });
  const longAgo = now - 100 * DAY_MS;

  it("只收已结束且超过保留期的任务", () => {
    const ids = expiredTaskIds(
      [
        task({ id: "done-old", status: "done", completedAt: longAgo }),
        task({ id: "failed-old", status: "failed", completedAt: longAgo }),
        task({ id: "cancelled-old", status: "cancelled", completedAt: longAgo }),
        task({ id: "done-fresh", status: "done", completedAt: now - DAY_MS }),
      ],
      retain30,
      now,
    );
    expect(ids).toEqual(["done-old", "failed-old", "cancelled-old"]);
  });

  it("活动中的任务再老也不动", () => {
    // 一条 running 任务的 createdAt 可以是半年前(长跑作业);按创建时间删它等于
    // 在用户眼前把正在工作的会话抹掉。
    for (const status of ["pending", "running", "input_required", "detached"] as TaskStatus[]) {
      const ids = expiredTaskIds([task({ id: status, status, createdAt: longAgo })], retain30, now);
      expect(ids, `${status} must survive`).toEqual([]);
    }
  });

  it("todo 与 interrupted 不算已结束", () => {
    // todo 承载的是用户写下的待办;interrupted 还能续跑。两者都不是终态。
    const ids = expiredTaskIds(
      [
        task({ id: "todo", status: "todo", createdAt: longAgo }),
        task({ id: "interrupted", status: "interrupted", createdAt: longAgo }),
      ],
      retain30,
      now,
    );
    expect(ids).toEqual([]);
  });

  it("收藏的任务被跳过", () => {
    // deleteTasks 本来就会过滤收藏项;这里提前跳过,免得 toast 报一个比实际删除数更大的数。
    const ids = expiredTaskIds(
      [task({ id: "starred", status: "done", completedAt: longAgo, starred: true })],
      retain30,
      now,
    );
    expect(ids).toEqual([]);
  });

  it("已归档不豁免", () => {
    // 归档管"从主列表移走",保留期管"回收磁盘"。混淆两者会让磁盘无声长下去。
    const ids = expiredTaskIds(
      [task({ id: "archived", status: "done", completedAt: longAgo, archivedAt: longAgo })],
      retain30,
      now,
    );
    expect(ids).toEqual(["archived"]);
  });

  it("没有 completedAt 的历史任务按创建时间判", () => {
    // completedAt 是本次新增字段。若不回退到 createdAt,升级前积攒的任务永远超不了期。
    const ids = expiredTaskIds(
      [
        task({ id: "legacy-old", status: "done", createdAt: longAgo }),
        task({ id: "legacy-fresh", status: "done", createdAt: now - DAY_MS }),
      ],
      retain30,
      now,
    );
    expect(ids).toEqual(["legacy-old"]);
  });

  it("保留期按结束时间而不是创建时间算", () => {
    // 这条是加 completedAt 的全部理由:40 天前建、昨天才完成的任务不能被当成超期。
    const ids = expiredTaskIds(
      [task({ id: "long-running", status: "done", createdAt: longAgo, completedAt: now - DAY_MS })],
      retain30,
      now,
    );
    expect(ids).toEqual([]);
  });

  it("还没处理的 worktree 任务不删,已丢弃的才删", () => {
    // 这条防的是静默丢代码:deleteTasks 对非活动任务一律 `git worktree remove --force`
    // 再 `git branch -D`,前者丢未提交改动、后者强删未合并分支。用户点删除是明确的
    // 丢弃动作,定时清理不是 —— 它不该在用户还没决定合不合的时候替他决定。
    const ids = expiredTaskIds(
      [
        task({
          id: "pending-merge",
          status: "done",
          completedAt: longAgo,
          worktreePath: "/repo/.aeroric-worktrees/pending-merge",
          worktreeBranch: "aeroric/abc123",
        }),
        task({
          id: "already-discarded",
          status: "done",
          completedAt: longAgo,
          worktreePath: "/repo/.aeroric-worktrees/already-discarded",
          worktreeBranch: "aeroric/def456",
          worktreeDiscarded: true,
        }),
        task({ id: "no-worktree", status: "done", completedAt: longAgo }),
      ],
      retain30,
      now,
    );
    expect(ids).toEqual(["already-discarded", "no-worktree"]);
  });
});

describe("normalizeAutoCleanupSettings", () => {
  it("补齐缺失字段并给出与 Rust 一致的默认值", () => {
    expect(normalizeAutoCleanupSettings(undefined)).toEqual({
      enabled: false,
      mode: "weekly",
      weekday: 0,
      hour: 20,
      interval_days: 7,
      retain_days: 30,
    });
  });

  it("越界值夹紧而不是抛错", () => {
    // 手改过 settings.json 的用户不该被一个手抖的数字挡在设置面板外。
    const normalized = normalizeAutoCleanupSettings({
      mode: "monthly" as AutoCleanupSettings["mode"],
      weekday: 99,
      hour: -3,
      interval_days: 0,
      retain_days: 99999,
    });
    expect(normalized.mode).toBe("weekly");
    expect(normalized.weekday).toBe(6);
    expect(normalized.hour).toBe(0);
    expect(normalized.interval_days).toBe(1);
    expect(normalized.retain_days).toBe(3650);
  });
});
