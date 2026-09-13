/* 后台终端实时刷新。
 *
 * 症状:应用切到后台或最小化后终端不再更新,切回前台一瞬间"哗啦"刷到底部。
 * 根因:agent 输出的落地只由 `requestAnimationFrame` 驱动,而浏览器在窗口不可见时把 rAF
 * 降到近乎停止;PTY 数据却仍经 Tauri Channel 同步到达 JS,于是全部攒在 `pendingOutputs`
 * 里,等 rAF 恢复再按每帧字节预算连续补帧。
 *
 * 这里测的是调度器的选择:隐藏时必须绕开 rAF 走定时器。断言"rAF 没被调用"而不只是
 * "数据最终到了" —— 后者在修复前也成立(只是晚了几十秒),测不出东西。
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Mock } from "vitest";

const { channels } = vi.hoisted(() => ({
  channels: [] as Array<{ onmessage: ((data: string) => void) | null }>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: ((data: string) => void) | null = null;

    constructor() {
      channels.push(this);
    }
  },
  invoke: vi.fn(),
}));

import { useTerminalManager } from "../hooks/useTerminalManager";

/** 把 `document.visibilityState` 钉成给定值,返回还原函数。 */
function stubVisibility(state: DocumentVisibilityState) {
  const spy = vi.spyOn(document, "visibilityState", "get").mockReturnValue(state);
  return () => spy.mockRestore();
}

interface Harness {
  write: Mock;
  raf: Mock;
  frames: FrameRequestCallback[];
  ingest: (data: string) => void;
  restoreData: () => string;
}

function mountTerminal(): Harness {
  const frames: FrameRequestCallback[] = [];
  const raf = vi.fn((callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal("requestAnimationFrame", raf);
  vi.stubGlobal("cancelAnimationFrame", vi.fn());

  const { result } = renderHook(() => useTerminalManager());
  const write = vi.fn();
  act(() => {
    result.current.resetTaskTerminal("task-1");
    const generation = result.current.handleRegisterTerminal("task-1", write);
    result.current.handleTerminalReady("task-1", generation);
    result.current.createOutputChannel("task-1");
  });

  return {
    write,
    raf,
    frames,
    ingest: (data: string) => {
      act(() => {
        channels[0].onmessage?.(data);
      });
    },
    restoreData: () => result.current.getTaskRestoreState("task-1").rawReplayData,
  };
}

describe("后台终端排空调度", () => {
  beforeEach(() => {
    channels.length = 0;
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("窗口隐藏时绕开 rAF,输出仍在 16ms 内落地", () => {
    const restore = stubVisibility("hidden");
    try {
      const term = mountTerminal();

      term.ingest("progress line\r\n");

      // 关键断言:隐藏时一次 rAF 都不该排。排了就等于把这批数据交给一个被浏览器
      // 冻住的时钟,后台期间终端就是死的。
      expect(term.raf).not.toHaveBeenCalled();
      expect(term.write).not.toHaveBeenCalled();

      act(() => {
        vi.advanceTimersByTime(16);
      });

      expect(term.write).toHaveBeenCalledWith("progress line\r\n");
      // 恢复镜像同步前进,否则切回前台重挂终端时会缺这一段。
      expect(term.restoreData()).toBe("progress line\r\n");
    } finally {
      restore();
    }
  });

  it("窗口隐藏期间连续到达的多批数据都实时落地,不攒到切回前台", () => {
    const restore = stubVisibility("hidden");
    try {
      const term = mountTerminal();

      for (let i = 1; i <= 3; i += 1) {
        term.ingest(`chunk ${i}\r\n`);
        act(() => {
          vi.advanceTimersByTime(16);
        });
      }

      expect(term.raf).not.toHaveBeenCalled();
      expect(term.write.mock.calls.map(([data]) => data)).toEqual([
        "chunk 1\r\n",
        "chunk 2\r\n",
        "chunk 3\r\n",
      ]);
    } finally {
      restore();
    }
  });

  it("窗口可见时仍走 rAF,与刷新同步", () => {
    // 对照面:可见时不该退化成定时器,否则写入与重绘不同步会撕裂画面。
    const restore = stubVisibility("visible");
    try {
      const term = mountTerminal();

      term.ingest("visible line\r\n");

      expect(term.raf).toHaveBeenCalledTimes(1);
      expect(term.write).not.toHaveBeenCalled();

      act(() => {
        term.frames[0](0);
      });

      expect(term.write).toHaveBeenCalledWith("visible line\r\n");
    } finally {
      restore();
    }
  });
});
