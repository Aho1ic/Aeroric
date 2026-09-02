/**
 * 使用频次 store 的「实时」行为。
 *
 * 这里断言的是排序依据**什么时候**更新 —— 用户明确要求不能等到点开配置选择界面才算。
 * 三条更新路径各有一个用例:记账后立刻可见、跨窗口广播、跨过本地零点让 7 天窗口滑动。
 */

import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AGENT_USAGE_CHANGED_EVENT,
  recordAgentConfigUsage,
  resetAgentUsageCacheForTests,
  useAgentUsageStats,
} from "../hooks/useAgentUsage";
import type { AgentUsageStats } from "../lib/agentUsageRanking";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

function snapshot(agents: Record<string, AgentUsageStats>) {
  return { agents, windowDays: 7, computedAt: 1_700_000_000_000 };
}

function stats(recentCount: number, totalCount: number, lastUsedAt = 0): AgentUsageStats {
  return { recentCount, totalCount, lastUsedAt };
}

/** 把当前 stats 摊到 DOM 上,断言读到的是哪一份快照。 */
function StatsProbe() {
  const usage = useAgentUsageStats();
  return <div data-testid="probe">{JSON.stringify(usage)}</div>;
}

describe("agent usage store", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    resetAgentUsageCacheForTests();
  });

  afterEach(() => {
    vi.useRealTimers();
    resetAgentUsageCacheForTests();
  });

  it("loads the snapshot on mount", async () => {
    invokeMock.mockResolvedValue(snapshot({ claude: stats(3, 5, 9_000) }));

    const { getByTestId } = render(<StatsProbe />);

    await waitFor(() => {
      expect(JSON.parse(getByTestId("probe").textContent ?? "{}")).toEqual({
        claude: stats(3, 5, 9_000),
      });
    });
    expect(invokeMock).toHaveBeenCalledWith("load_agent_usage_snapshot");
  });

  /**
   * 记账后的快照必须立刻可见,且**不能**再去读一遍 —— 记账方手里已经是权威结果,
   * 多一次往返只会让排序在两次渲染之间抖一下。
   */
  it("applies the snapshot returned by a recorded use without re-reading", async () => {
    invokeMock.mockResolvedValueOnce(snapshot({}));
    const { getByTestId } = render(<StatsProbe />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("load_agent_usage_snapshot"));

    invokeMock.mockResolvedValueOnce(snapshot({ codex: stats(1, 1, 4_000) }));
    await act(async () => {
      await recordAgentConfigUsage("codex");
    });

    expect(invokeMock).toHaveBeenCalledWith("record_agent_config_usage", { agent: "codex" });
    expect(JSON.parse(getByTestId("probe").textContent ?? "{}")).toEqual({
      codex: stats(1, 1, 4_000),
    });
    expect(
      invokeMock.mock.calls.filter(([name]) => name === "load_agent_usage_snapshot"),
    ).toHaveLength(1);
  });

  it("applies the snapshot carried on a broadcast without re-reading", async () => {
    invokeMock.mockResolvedValueOnce(snapshot({}));
    const { getByTestId } = render(<StatsProbe />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    await act(async () => {
      window.dispatchEvent(
        new CustomEvent(AGENT_USAGE_CHANGED_EVENT, {
          detail: { snapshot: snapshot({ dsh: stats(2, 2, 7_000) }) },
        }),
      );
    });

    expect(JSON.parse(getByTestId("probe").textContent ?? "{}")).toEqual({
      dsh: stats(2, 2, 7_000),
    });
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("re-reads when a broadcast carries no snapshot", async () => {
    invokeMock.mockResolvedValueOnce(snapshot({}));
    const { getByTestId } = render(<StatsProbe />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    invokeMock.mockResolvedValueOnce(snapshot({ dsh: stats(2, 2, 7_000) }));
    await act(async () => {
      window.dispatchEvent(new Event(AGENT_USAGE_CHANGED_EVENT));
    });

    await waitFor(() => {
      expect(JSON.parse(getByTestId("probe").textContent ?? "{}")).toEqual({
        dsh: stats(2, 2, 7_000),
      });
    });
  });

  /**
   * 7 天窗口的边界是日历日。这条用例必须真把时钟推过零点 —— 只 flush 微任务的话
   * 定时器不会醒,而「没重取」和「重取了但快照没变」在 DOM 上是一样的。
   */
  it("re-reads after the local day rolls over so the 7-day window slides", async () => {
    vi.useFakeTimers({ now: new Date(2026, 8, 2, 23, 59, 30) });
    invokeMock.mockResolvedValue(snapshot({ claude: stats(1, 1, 1_000) }));

    render(<StatsProbe />);
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    invokeMock.mockResolvedValue(snapshot({ claude: stats(0, 1, 1_000) }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(31_000);
    });

    expect(
      invokeMock.mock.calls.filter(([name]) => name === "load_agent_usage_snapshot").length,
    ).toBeGreaterThan(1);
  });

  it("keeps the previous stats when reading the ledger fails", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    invokeMock.mockResolvedValueOnce(snapshot({ claude: stats(2, 2, 3_000) }));
    const { getByTestId } = render(<StatsProbe />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    invokeMock.mockRejectedValueOnce(new Error("ledger unreadable"));
    await act(async () => {
      window.dispatchEvent(new Event(AGENT_USAGE_CHANGED_EVENT));
    });

    expect(JSON.parse(getByTestId("probe").textContent ?? "{}")).toEqual({
      claude: stats(2, 2, 3_000),
    });
    consoleError.mockRestore();
  });

  it("does not record a blank agent id", async () => {
    await recordAgentConfigUsage("   ");

    expect(invokeMock).not.toHaveBeenCalledWith("record_agent_config_usage", expect.anything());
  });
});
