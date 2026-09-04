import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  installStallRecorder,
  resetStallRecorder,
  stallReport,
} from "../lib/stallRecorder";

/**
 * 一次 `invoke` 在真实运行时的样子:`ipc-protocol.js` 把它发成一个到
 * `ipc://localhost/<cmd>` 的 POST(macOS / Linux)。记录器就是从这个 URL 取 command 名的,
 * 所以测试也从这一层驱动,而不是去调某个内部函数。
 */
function ipcUrl(command: string): string {
  return `ipc://localhost/${encodeURIComponent(command)}`;
}

/** Windows / Android 上换成 `<scheme>://ipc.localhost/<cmd>`,两种都得认。 */
function ipcUrlWindows(command: string): string {
  return `http://ipc.localhost/${encodeURIComponent(command)}`;
}

/** 发一次 IPC 请求 —— 走 `globalThis.fetch`,也就是记录器包装的那一层。 */
function sendIpc(url: string, init?: RequestInit): Promise<Response> {
  return globalThis.fetch(url, init);
}

/**
 * 装一个 `fetch` 替身,并返回它看到的调用。
 *
 * `advanceMs` 让时钟在「请求期间」推进,用来伪造一次慢命令。
 */
function stubFetch(advanceMs: (url: string) => number, tick: (ms: number) => void) {
  const seen: { url: string; init?: RequestInit }[] = [];
  vi.stubGlobal("fetch", (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : String(input);
    seen.push({ url, init });
    tick(advanceMs(url));
    return Promise.resolve(new Response("null"));
  });
  return seen;
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
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("names the slow command so a stalled click is attributable", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    // 命令执行期间时钟推进 —— 模拟一次 900ms 的后端调用。
    stubFetch(
      (url) => (url.endsWith("slow_command") ? 900 : 5),
      (ms) => {
        now += ms;
      },
    );
    installStallRecorder();

    await sendIpc(ipcUrl("slow_command"));
    await sendIpc(ipcUrl("fast_command"));

    const report = stallReport();
    expect(report.slowInvokes.map((s) => s.label)).toEqual(["slow_command"]);
    expect(report.slowInvokes[0]?.durationMs).toBe(900);
    // 快命令不进 slowInvokes,但仍要计入累计统计。
    expect(report.invokeTotals.map((s) => s.command)).toEqual(["slow_command", "fast_command"]);
  });

  it("still times a command that rejects, and preserves the rejection", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    vi.stubGlobal("fetch", () => {
      now += 500;
      return Promise.reject(new Error("backend blew up"));
    });
    installStallRecorder();

    await expect(sendIpc(ipcUrl("failing_command"))).rejects.toThrow("backend blew up");

    const report = stallReport();
    expect(report.slowInvokes.map((s) => s.label)).toEqual(["failing_command"]);
    expect(report.slowInvokes[0]?.durationMs).toBe(500);
  });

  it("aggregates high-frequency commands that are individually fast", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    stubFetch(
      () => 3,
      (ms) => {
        now += ms;
      },
    );
    installStallRecorder();

    for (let i = 0; i < 40; i += 1) await sendIpc(ipcUrl("chatty_command"));

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
    stubFetch(
      () => 200,
      (ms) => {
        now += ms;
      },
    );
    installStallRecorder();

    for (let i = 0; i < 80; i += 1) await sendIpc(ipcUrl(`slow_${i}`));

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

  it("passes the request through untouched and returns the same response", async () => {
    const seen = stubFetch(
      () => 0,
      () => {},
    );
    installStallRecorder();

    const init = { method: "POST", headers: { "Tauri-Callback": "1" } };
    const response = await sendIpc(ipcUrl("cmd"), init);

    expect(await response.text()).toBe("null");
    expect(seen).toEqual([{ url: ipcUrl("cmd"), init }]);
  });

  it("leaves non-IPC requests unmeasured so ordinary traffic is not attributed to a command", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    stubFetch(
      () => 800,
      (ms) => {
        now += ms;
      },
    );
    installStallRecorder();

    // 资源加载和普通网络请求都不是 IPC,再慢也不该算到某个 command 头上。
    await sendIpc("asset://localhost/assets/index.js");
    await sendIpc("https://example.com/api/models");

    const report = stallReport();
    expect(report.slowInvokes).toEqual([]);
    expect(report.invokeTotals).toEqual([]);
  });

  it("reads the command out of the Windows IPC url shape too", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    stubFetch(
      () => 700,
      (ms) => {
        now += ms;
      },
    );
    installStallRecorder();

    await sendIpc(ipcUrlWindows("load_projects"));

    expect(stallReport().slowInvokes.map((s) => s.label)).toEqual(["load_projects"]);
  });

  it("survives the real tauri internals descriptor instead of blanking the app", () => {
    // 回归测试。tauri 用 `Object.defineProperty` 定义 `invoke` 且只给 `value`
    // (`scripts/core.js:81`),于是 writable / configurable 都是 false。记录器曾经
    // 直接 `internals.invoke = …`:严格模式下当场抛 TypeError,而它跑在 createRoot
    // 之前 —— 整个应用白屏。之前的替身是对象字面量(invoke 可写),抓不到这个。
    const internals = {};
    Object.defineProperty(internals, "invoke", {
      value: (command: string) => Promise.resolve(`${command}-ok`),
    });
    const descriptor = Object.getOwnPropertyDescriptor(internals, "invoke");
    expect(descriptor).toMatchObject({ writable: false, configurable: false });
    vi.stubGlobal("__TAURI_INTERNALS__", internals);

    expect(() => installStallRecorder()).not.toThrow();
    // 而且探针要真的装上了,不是靠静默跳过来「不抛」。
    expect(stallReport().invokeProbeActive).toBe(true);
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
