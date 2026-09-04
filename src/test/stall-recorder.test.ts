import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  installStallRecorder,
  resetStallRecorder,
  stallReport,
} from "../lib/stallRecorder";

interface TauriInternalsShape {
  invoke: (command: string, args?: unknown, options?: unknown) => Promise<unknown>;
}

function internals(): TauriInternalsShape {
  const value = (globalThis as { __TAURI_INTERNALS__?: TauriInternalsShape })
    .__TAURI_INTERNALS__;
  if (!value) throw new Error("__TAURI_INTERNALS__ missing");
  return value;
}

/** `Event.timeStamp` 只有 getter,Object.assign 设不上,得 defineProperty。 */
function eventAt(type: string, timeStamp: number): Event {
  const event = new Event(type);
  Object.defineProperty(event, "timeStamp", { value: timeStamp, configurable: true });
  return event;
}

describe("stall recorder", () => {
  beforeEach(() => {
    resetStallRecorder();
  });

  afterEach(() => {
    delete (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.useRealTimers();
  });

  it("names the slow command so a stalled click is attributable", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
      invoke: (command: string) => {
        // 命令执行期间时钟推进 —— 模拟一次 900ms 的后端调用。
        now += command === "slow_command" ? 900 : 5;
        return Promise.resolve(`${command}-ok`);
      },
    };
    installStallRecorder();

    await internals().invoke("slow_command");
    await internals().invoke("fast_command");

    const report = stallReport();
    expect(report.slowInvokes.map((s) => s.label)).toEqual(["slow_command"]);
    expect(report.slowInvokes[0]?.durationMs).toBe(900);
    // 快命令不进 slowInvokes,但仍要计入累计统计。
    expect(report.invokeTotals.map((s) => s.command)).toEqual(["slow_command", "fast_command"]);
  });

  it("still times a command that rejects, and preserves the rejection", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
      invoke: () => {
        now += 500;
        return Promise.reject(new Error("backend blew up"));
      },
    };
    installStallRecorder();

    await expect(internals().invoke("failing_command")).rejects.toThrow("backend blew up");

    const report = stallReport();
    expect(report.slowInvokes.map((s) => s.label)).toEqual(["failing_command"]);
    expect(report.slowInvokes[0]?.durationMs).toBe(500);
  });

  it("aggregates high-frequency commands that are individually fast", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
      invoke: () => {
        now += 3;
        return Promise.resolve(null);
      },
    };
    installStallRecorder();

    for (let i = 0; i < 40; i += 1) await internals().invoke("chatty_command");

    const report = stallReport();
    // 单次 3ms 从不越过慢阈值,但累计 120ms 必须看得见 —— 这正是"高频小命令磨掉
    // 主线程"那一类,只看 slowInvokes 会漏掉。
    expect(report.slowInvokes).toEqual([]);
    const stat = report.invokeTotals.find((s) => s.command === "chatty_command");
    expect(stat).toMatchObject({ calls: 40, totalMs: 120, maxMs: 3 });
  });

  it("bounds each sample bucket so the recorder cannot become a leak", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
      invoke: () => {
        now += 200;
        return Promise.resolve(null);
      },
    };
    installStallRecorder();

    for (let i = 0; i < 80; i += 1) await internals().invoke(`slow_${i}`);

    const report = stallReport();
    expect(report.slowInvokes).toHaveLength(50);
    // 有界队列保留最近的,丢最老的。
    expect(report.slowInvokes.at(-1)?.label).toBe("slow_79");
    expect(report.slowInvokes[0]?.label).toBe("slow_30");
  });

  it("bounds the input bucket too, so a long laggy session cannot grow it forever", () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const frames: (() => void)[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: () => void) => {
      frames.push(cb);
      return frames.length;
    });
    installStallRecorder();

    // 80 次「排队 900ms」的点击,只应留下最近 50 条。
    for (let i = 0; i < 80; i += 1) {
      now = i * 10_000 + 900;
      window.dispatchEvent(eventAt("click", i * 10_000));
    }
    frames.forEach((f) => f());

    const report = stallReport();
    expect(report.slowInputs).toHaveLength(50);
    expect(report.slowInputs[0]?.atMs).toBe(30 * 10_000 + 900);
    expect(report.slowInputs.at(-1)?.atMs).toBe(79 * 10_000 + 900);
  });

  it("passes through arguments and the resolved value unchanged", async () => {
    const seen: unknown[] = [];
    (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
      invoke: (command: string, args?: unknown, options?: unknown) => {
        seen.push({ command, args, options });
        return Promise.resolve({ ok: true });
      },
    };
    installStallRecorder();

    const result = await internals().invoke("cmd", { a: 1 }, { headers: {} });

    expect(result).toEqual({ ok: true });
    expect(seen).toEqual([{ command: "cmd", args: { a: 1 }, options: { headers: {} } }]);
  });

  it("records a click that did not reach JS promptly — the shape long tasks cannot see", () => {
    // 这是「点了没反应」的典型形状:JS 之前就被拖住了,所以 longTasks / slowInvokes
    // 都是空的,只有输入探针能抓到。
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const frames: (() => void)[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: () => void) => {
      frames.push(cb);
      return frames.length;
    });
    installStallRecorder();

    now = 1_400;
    // timeStamp 是事件生成时刻;处理器在 1400ms 才跑到 → 排队 900ms。
    window.dispatchEvent(eventAt("click", 500));
    now = 1_410;
    frames.forEach((f) => f());

    const report = stallReport();
    expect(report.longTasks).toEqual([]);
    expect(report.slowInvokes).toEqual([]);
    expect(report.slowInputs).toHaveLength(1);
    expect(report.slowInputs[0]?.label).toBe("click");
    expect(report.slowInputs[0]?.toHandlerMs).toBe(900);
    expect(report.slowInputs[0]?.toFrameMs).toBe(10);
  });

  it("separates a slow paint from a slow queue so the two get different diagnoses", () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const frames: (() => void)[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: () => void) => {
      frames.push(cb);
      return frames.length;
    });
    installStallRecorder();

    // JS 及时收到(排队 5ms),但下一帧 800ms 后才来 → 画不出来,不是没收到。
    now = 105;
    window.dispatchEvent(eventAt("pointerdown", 100));
    now = 905;
    frames.forEach((f) => f());

    const report = stallReport();
    expect(report.slowInputs).toHaveLength(1);
    expect(report.slowInputs[0]?.toHandlerMs).toBe(5);
    expect(report.slowInputs[0]?.toFrameMs).toBe(800);
  });

  it("keeps prompt input out of the report entirely", () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const frames: (() => void)[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: () => void) => {
      frames.push(cb);
      return frames.length;
    });
    installStallRecorder();

    now = 20;
    window.dispatchEvent(eventAt("click", 12));
    now = 28;
    frames.forEach((f) => f());

    expect(stallReport().slowInputs).toEqual([]);
  });

  it("discards an incomparable timeStamp instead of reporting a bogus delay", () => {
    // 个别引擎上 event.timeStamp 曾是 epoch 毫秒,减出来是天文数字 —— 宁可不记。
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const frames: (() => void)[] = [];
    vi.stubGlobal("requestAnimationFrame", (cb: () => void) => {
      frames.push(cb);
      return frames.length;
    });
    installStallRecorder();

    now = 1_700_000_000_000;
    window.dispatchEvent(eventAt("click", 0));
    frames.forEach((f) => f());

    expect(stallReport().slowInputs).toEqual([]);
  });
});
