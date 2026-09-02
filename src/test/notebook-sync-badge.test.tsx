/* 状态栏上那一段同步状态。
 *
 * 三件事:
 *   1. **主标签只显示最该让人知道的那一件。** 平铺全部状态会让"有 3 处冲突等你决定"被
 *      "已是最新"挤掉 —— 那正是最需要看见的一句。
 *   2. **倒计时要自己往下走。** `nextRunInMs` 是快照,不本地推的话数字会卡住,看着像调度死了。
 *   3. **没配云盘时整段不出现。** 显示"已关闭"会让用户去找一个不存在的开关。
 *
 * 文案用 `key|var=value` 的形式,断言钉的是 key 和变量值 —— 改文案不该让这些测试变红,而
 * key 拼错或者数字算错必须变红。
 */

import { describe, expect, it, vi, afterEach, beforeEach } from "vitest";
import { act, render, screen } from "@testing-library/react";

import { NoteSyncBadge } from "../components/notebook/NoteSyncBadge";
import type { SyncRemoteStatus, SyncReport } from "../components/notebook/noteSync";

const t = (key: string, vars?: Record<string, string>) =>
  vars
    ? `${key}|${Object.entries(vars)
        .map(([name, value]) => `${name}=${value}`)
        .join(",")}`
    : key;

/** 真实的 epoch 毫秒。从 0 或 1000 起算会让"没读到"的哨兵值和真实值撞上。 */
const NOW = Date.parse("2026-09-01T12:00:00.000Z");

const status = (over: Partial<SyncRemoteStatus> = {}): SyncRemoteStatus => ({
  remoteId: "r1",
  autoSync: true,
  failures: 0,
  dirty: false,
  lastAttemptMs: NOW - 60_000,
  nextRunInMs: 30_000,
  ...over,
});

const conflictReport = (count: number): SyncReport => ({
  plan: {
    actions: Array.from({ length: count }, (_, index) => ({
      path: `c${index}.md`,
      reason: "both_modified",
      action: {
        kind: "conflict" as const,
        resolution: null,
        localHash: "1",
        remoteHash: "2",
      },
    })),
    summary: { upload: 0, download: 0, deleteRemote: 0, deleteLocal: 0, conflict: count },
  },
  outcomes: Array.from({ length: count }, (_, index) => ({
    path: `c${index}.md`,
    reason: "both_modified",
    status: { kind: "pending" as const, detail: "awaiting_user" },
  })),
  tombstonesWritten: 0,
  seq: null,
});

const failedReport = (): SyncReport => ({
  plan: {
    actions: [{ path: "a.md", reason: "upload", action: { kind: "upload" } }],
    summary: { upload: 1, download: 0, deleteRemote: 0, deleteLocal: 0, conflict: 0 },
  },
  outcomes: [{ path: "a.md", reason: "upload", status: { kind: "failed", error: "boom" } }],
  tombstonesWritten: 0,
  seq: null,
});

const renderBadge = (props: Partial<React.ComponentProps<typeof NoteSyncBadge>> = {}) =>
  render(
    <NoteSyncBadge
      status={status()}
      statusAt={NOW}
      report={null}
      stale={false}
      running={false}
      onSyncNow={vi.fn()}
      onOpenConflicts={vi.fn()}
      t={t}
      {...props}
    />,
  );

afterEach(() => {
  vi.useRealTimers();
});

describe("主标签", () => {
  it("没有状态时整段不渲染", () => {
    renderBadge({ status: null });
    expect(screen.queryByTestId("note-sync-badge")).toBeNull();
  });

  it("正在跑时压过其他一切", () => {
    renderBadge({ running: true, report: conflictReport(3), status: status({ dirty: true }) });
    expect(screen.getByText("notebook.sync.syncing")).toBeInTheDocument();
  });

  it("有失败时压过冲突", () => {
    renderBadge({ report: failedReport(), status: status({ dirty: true }) });
    expect(screen.getByText("notebook.sync.failed|count=1")).toBeInTheDocument();
  });

  it("有冲突时压过「有改动待同步」", () => {
    renderBadge({ report: conflictReport(2), status: status({ dirty: true }) });
    expect(screen.getByText("notebook.sync.awaitingUser|count=2")).toBeInTheDocument();
  });

  it("只是有改动没同步", () => {
    renderBadge({ status: status({ dirty: true }) });
    expect(screen.getByText("notebook.sync.dirty")).toBeInTheDocument();
  });

  it("自动同步关着", () => {
    renderBadge({ status: status({ autoSync: false, nextRunInMs: null }) });
    expect(screen.getByText("notebook.sync.off")).toBeInTheDocument();
  });

  it("干净且开着 = 已是最新", () => {
    renderBadge();
    expect(screen.getByText("notebook.sync.idle")).toBeInTheDocument();
  });
});

describe("倒计时", () => {
  /* 整块都要接管时钟。秒数是「`statusAt` 到现在」算出来的,拿真实时钟去比一个写死的
     `statusAt`,结果取决于跑测试的那一刻,而不取决于被测代码。 */
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });

  it("按快照算出秒数", () => {
    renderBadge({ status: status({ nextRunInMs: 30_000 }) });
    expect(screen.getByText("notebook.sync.nextRun|seconds=30")).toBeInTheDocument();
  });

  it("自己往下走", () => {
    renderBadge({ status: status({ nextRunInMs: 30_000 }), statusAt: NOW });
    expect(screen.getByText("notebook.sync.nextRun|seconds=30")).toBeInTheDocument();

    /* 关键的一条:状态本身没有重查过(`statusAt` 没动),数字却必须往下走。不推的话它会
       卡在 30 秒直到下一次轮询,看着像调度已经死了。 */
    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(screen.getByText("notebook.sync.nextRun|seconds=25")).toBeInTheDocument();
  });

  it("走过头也不显示负数", () => {
    renderBadge({ status: status({ nextRunInMs: 2_000 }), statusAt: NOW });
    act(() => {
      vi.advanceTimersByTime(9_000);
    });
    expect(screen.getByText("notebook.sync.nextRun|seconds=0")).toBeInTheDocument();
  });

  it("时钟往回跳时倒计时不会被撑大", () => {
    /* `statusAt` 落在未来 —— 系统时钟被往回调了(NTP 校正、用户改时间)。不夹住的话
       "已经过去的时间"是负的,减出来的秒数比真正要等的还长:要等 30 秒,显示 90 秒。 */
    renderBadge({ status: status({ nextRunInMs: 30_000 }), statusAt: NOW + 60_000 });
    expect(screen.getByText("notebook.sync.nextRun|seconds=30")).toBeInTheDocument();
  });

  it("退避中说的是重试,还带上失败次数", () => {
    renderBadge({ status: status({ failures: 3, nextRunInMs: 8_000, dirty: true }) });
    /* 「下一轮」和「重试」对用户的含义不同:一个是正常节奏,一个是出过错。 */
    expect(screen.getByText("notebook.sync.retrying|seconds=8,failures=3")).toBeInTheDocument();
  });

  it("自动同步关着时没有倒计时", () => {
    renderBadge({ status: status({ autoSync: false, nextRunInMs: null }) });
    expect(screen.queryByText(/notebook\.sync\.nextRun/)).toBeNull();
    expect(screen.queryByText(/notebook\.sync\.retrying/)).toBeNull();
  });

  it("此刻就该跑是 0 秒,不是「关着」", () => {
    // 后端把「现在就跑」映射成 `Some(0)`,只有关着才给 `None`(见 `daemon::status_for`)。
    renderBadge({ status: status({ nextRunInMs: 0 }) });
    expect(screen.getByText("notebook.sync.nextRun|seconds=0")).toBeInTheDocument();
    expect(screen.queryByText("notebook.sync.off")).toBeNull();
  });
});

describe("按钮", () => {
  it("立即同步", async () => {
    const onSyncNow = vi.fn();
    renderBadge({ onSyncNow });
    screen.getByRole("button", { name: "notebook.sync.syncNow" }).click();
    expect(onSyncNow).toHaveBeenCalledTimes(1);
  });

  it("正在跑时禁掉立即同步", () => {
    renderBadge({ running: true });
    expect(screen.getByRole("button", { name: "notebook.sync.syncNow" })).toBeDisabled();
  });

  it("没冲突也没过期时不给冲突入口", () => {
    renderBadge();
    expect(screen.queryByRole("button", { name: "notebook.sync.conflicts" })).toBeNull();
  });

  it("有冲突时给入口并显示条数", () => {
    const onOpenConflicts = vi.fn();
    renderBadge({ report: conflictReport(4), onOpenConflicts });
    const button = screen.getByRole("button", { name: "notebook.sync.conflicts" });
    expect(button).toHaveTextContent("4");
    button.click();
    expect(onOpenConflicts).toHaveBeenCalledTimes(1);
  });

  it("只是报告过期时也给入口,但不显示条数", () => {
    renderBadge({ stale: true });
    const button = screen.getByRole("button", { name: "notebook.sync.conflicts" });
    expect(button).toBeInTheDocument();
    expect(button).not.toHaveTextContent("0");
  });
});
