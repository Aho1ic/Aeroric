import { Terminal as XTerm, type Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";
import {
  TERMINAL_SMOOTH_SCROLL_MS,
  WHEEL_REPAINT_GRACE_MS,
  attachTerminalWheelScroll,
  initTerminal,
} from "../components/terminalShared";

type BufferType = "normal" | "alternate";
type MouseMode = "none" | "x10" | "vt200" | "drag" | "any";

interface FakeTerminalOptions {
  /** 有 element 才走放大路径；不给就是"还没 open()",应当交回 xterm。 */
  attached?: boolean;
  rows?: number;
  cellHeight?: number;
  /**
   * 是否提供 `onWriteParsed`。给了就走闭环(发一批等一次重绘),不给退回纯 rAF 节奏。
   *
   * 默认不给:大部分用例只关心"总行程"和方向,不该被闭环的等待复杂化。
   */
  repaintSignal?: boolean;
}

function fakeTerminal(
  bufferType: BufferType,
  mouseTrackingMode: MouseMode,
  options: FakeTerminalOptions = {},
) {
  const { attached = true, rows = 24, cellHeight = 16, repaintSignal = false } = options;
  const handlers: Array<(event: WheelEvent) => boolean> = [];
  // 收集 onWriteParsed 的订阅者,测试用 repaint() 手动触发"agent 画完了"。
  const repaintListeners: Array<() => void> = [];
  let repaintDisposed = false;
  // xterm 的 wheel listener 挂在根 element 上,合成事件派发到这里。
  const replayed: WheelEvent[] = [];
  let element: HTMLElement | undefined;
  if (attached) {
    element = document.createElement("div");
    const screen = document.createElement("div");
    screen.className = "xterm-screen";
    screen.getBoundingClientRect = () => ({ height: cellHeight * rows }) as DOMRect;
    element.appendChild(screen);
    element.addEventListener("wheel", (event) => {
      replayed.push(event as WheelEvent);
    });
  }
  const term = {
    element,
    rows,
    modes: { mouseTrackingMode },
    buffer: { active: { type: bufferType } },
    attachCustomWheelEventHandler: vi.fn((handler: (event: WheelEvent) => boolean) => {
      handlers.push(handler);
    }),
    ...(repaintSignal
      ? {
          onWriteParsed: (listener: () => void) => {
            repaintListeners.push(listener);
            return {
              dispose: () => {
                repaintDisposed = true;
              },
            };
          },
        }
      : {}),
  } as unknown as Terminal;
  const wheelScroll = attachTerminalWheelScroll(term);
  return {
    term,
    handler: handlers[0],
    replayed,
    element,
    dispose: wheelScroll.dispose,
    isReplayingWheel: wheelScroll.isReplayingWheel,
    /** 模拟 agent 那批重绘到位。 */
    repaint: () => {
      for (const listener of repaintListeners) listener();
    },
    wasRepaintDisposed: () => repaintDisposed,
  };
}

/**
 * 上报按帧分摊,所以断言前要把排期的帧跑完。
 *
 * 每帧最多 4 条(`MAX_WHEEL_REPORTS_PER_FRAME`),因此这里循环推进直到再没有新上报,
 * 用"上一轮的条数"判断是否已经排空,而不是猜需要几帧。
 */
async function flushWheelFrames(replayed: WheelEvent[], maxFrames = 64): Promise<void> {
  let previous = -1;
  for (let frame = 0; frame < maxFrames && replayed.length !== previous; frame += 1) {
    previous = replayed.length;
    await nextFrame();
  }
}

function nextFrame(): Promise<void> {
  return new Promise<void>((resolve) => {
    requestAnimationFrame(() => resolve());
  });
}

function wheel(deltaY: number, init: WheelEventInit = {}) {
  const event = new WheelEvent("wheel", { deltaY, cancelable: true, ...init });
  const preventDefault = vi.spyOn(event, "preventDefault");
  return { event, preventDefault };
}

describe("attachTerminalWheelScroll", () => {
  it("swallows the wheel in the alt screen so xterm never synthesizes arrow keys", () => {
    const { handler } = fakeTerminal("alternate", "none");
    const up = wheel(-120);
    expect(handler(up.event)).toBe(false);
    expect(up.preventDefault).toHaveBeenCalledOnce();

    const down = wheel(120);
    expect(handler(down.event)).toBe(false);
    expect(down.preventDefault).toHaveBeenCalledOnce();
  });

  it("still swallows it under modifiers, which hit the same xterm fallback", () => {
    const { handler } = fakeTerminal("alternate", "none");
    for (const init of [
      { ctrlKey: true },
      { metaKey: true },
      { shiftKey: true },
      { altKey: true },
    ]) {
      const { event, preventDefault } = wheel(-120, init);
      expect(handler(event)).toBe(false);
      expect(preventDefault).toHaveBeenCalledOnce();
    }
  });

  it("treats x10 as no app wheel reporting", () => {
    const { handler } = fakeTerminal("alternate", "x10");
    const { event, preventDefault } = wheel(-120);
    expect(handler(event)).toBe(false);
    expect(preventDefault).toHaveBeenCalledOnce();
  });

  it("leaves the normal buffer to xterm's own scrollback scrolling", () => {
    const { handler } = fakeTerminal("normal", "none");
    const { event, preventDefault } = wheel(-120);
    expect(handler(event)).toBe(true);
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it("reports one wheel event per line of travel to apps that asked for it", async () => {
    for (const mode of ["vt200", "drag", "any"] as const) {
      // 120px / 16px 行高 = 7 行。xterm 自己只会发 1 条,agent 那边就只滚 1 行。
      const { handler, replayed } = fakeTerminal("alternate", mode, { cellHeight: 16 });
      const { event, preventDefault } = wheel(120);
      expect(handler(event)).toBe(false);
      expect(preventDefault).toHaveBeenCalledOnce();
      await flushWheelFrames(replayed);
      // 行程 1:1 —— 分帧只改变发送节奏,不改变总行数。
      expect(replayed).toHaveLength(7);
      // 每条都是 LINE 模式的 ±1:走 LINE 分支就绕开了 cell 换算和触控板阻尼,
      // 一个事件恰好一行上报。
      for (const replay of replayed) {
        expect(replay.deltaMode).toBe(WheelEvent.DOM_DELTA_LINE);
        expect(replay.deltaY).toBe(1);
      }
    }
  });

  it("keeps the wheel direction when scrolling up", async () => {
    const { handler, replayed } = fakeTerminal("alternate", "vt200", { cellHeight: 16 });
    expect(handler(wheel(-48).event)).toBe(false);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(3);
    expect(replayed.every((replay) => replay.deltaY === -1)).toBe(true);
  });

  it("carries the report coordinates so the mouse report is not dropped", async () => {
    // getMouseReportCoords 用 clientX/clientY 算格子;缺了会拿不到 pos,整条上报被丢掉。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", { cellHeight: 16 });
    handler(wheel(32, { clientX: 42, clientY: 99 }).event);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(2);
    for (const replay of replayed) {
      expect(replay.clientX).toBe(42);
      expect(replay.clientY).toBe(99);
    }
  });

  it("does not re-amplify its own replayed events", async () => {
    const { handler, element, replayed } = fakeTerminal("alternate", "vt200", {
      cellHeight: 16,
    });
    // 合成事件重新进 handler 时必须原样放过,否则每行又放大一轮直到炸栈。
    element!.addEventListener("wheel", (event) => {
      handler(event as WheelEvent);
    });
    expect(handler(wheel(32).event)).toBe(false);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(2);
  });

  it("accumulates trackpad deltas below one line instead of dropping them", async () => {
    const { handler, replayed } = fakeTerminal("alternate", "vt200", { cellHeight: 16 });
    // 每次 6px,不足一行。吃掉事件但攒着余量,第三次跨过 16px 才发一行。
    expect(handler(wheel(6).event)).toBe(false);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(0);
    expect(handler(wheel(6).event)).toBe(false);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(0);
    expect(handler(wheel(6).event)).toBe(false);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(1);
  });

  it("drops the carry when the wheel reverses so the first tick back is not eaten", async () => {
    const { handler, replayed } = fakeTerminal("alternate", "vt200", { cellHeight: 16 });
    handler(wheel(12).event);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(0);
    // 反向:残留的 +0.75 行不该抵掉这一下。
    handler(wheel(-16).event);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(1);
    expect(replayed[0].deltaY).toBe(-1);
  });

  it("caps one event at three screens so an inertial fling cannot flood the pty", async () => {
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      rows: 10,
      cellHeight: 16,
    });
    // 100000px ÷ 16 = 6250 行,远超 10 行 × 3 屏。
    handler(wheel(100000).event);
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(30);
  });

  it("paces reports across frames instead of flooding the pty in one tick", async () => {
    // 这是"不流畅"的直接成因:原来一次手势把全部行数同步灌进 pty,agent 只能逐条重绘、
    // 逐屏回吐,前端收到一长串已经过时的中间态。分帧后每帧只发少量,画面每帧都在动。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", { cellHeight: 16 });
    handler(wheel(160).event); // 160 / 16 = 10 行
    // 同步返回后一条都还没发出去 —— 全部推到了后续帧。
    expect(replayed).toHaveLength(0);

    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    const afterFirstFrame = replayed.length;
    expect(afterFirstFrame).toBeGreaterThan(0);
    expect(afterFirstFrame).toBeLessThanOrEqual(4);

    // 剩下的在后续帧补齐,总行程仍是 1:1。
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(10);
  });

  it("abandons the queued backlog when the wheel reverses mid-drain", async () => {
    // 分帧后余量会跨帧存活,于是多了一种反向:上一段还没发完,用户已经往回滚了。
    // 此时补完旧方向只会让画面先往回跳一段再跟手,所以直接丢掉。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      rows: 40,
      cellHeight: 16,
    });
    handler(wheel(480).event); // 30 行,一帧发不完
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    const beforeReversal = replayed.length;
    expect(beforeReversal).toBeLessThan(30);

    handler(wheel(-32).event); // 反向 2 行
    await flushWheelFrames(replayed);
    // 只补了反向的 2 条,原方向剩下的 20+ 行没有继续发。
    expect(replayed).toHaveLength(beforeReversal + 2);
    expect(replayed.slice(beforeReversal).every((event) => event.deltaY === -1)).toBe(true);
  });

  it("stops pending frames once disposed so a torn-down terminal is never touched", async () => {
    const { handler, replayed, dispose } = fakeTerminal("alternate", "vt200", {
      cellHeight: 16,
    });
    handler(wheel(160).event);
    dispose();
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(0);
  });

  it("honours line and page wheel modes without needing a measured row height", async () => {
    const line = fakeTerminal("alternate", "vt200", { rows: 20 });
    line.handler(wheel(4, { deltaMode: WheelEvent.DOM_DELTA_LINE }).event);
    await flushWheelFrames(line.replayed);
    expect(line.replayed).toHaveLength(4);

    const page = fakeTerminal("alternate", "vt200", { rows: 20 });
    page.handler(wheel(1, { deltaMode: WheelEvent.DOM_DELTA_PAGE }).event);
    await flushWheelFrames(page.replayed);
    expect(page.replayed).toHaveLength(20);
  });

  it("falls back to xterm before open() so the wheel is never dead", () => {
    const { handler } = fakeTerminal("alternate", "vt200", { attached: false });
    const { event, preventDefault } = wheel(120);
    expect(handler(event)).toBe(true);
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it("ignores horizontal-only wheel events", () => {
    const { handler } = fakeTerminal("alternate", "none");
    const { event, preventDefault } = wheel(0, { deltaX: -120 });
    expect(handler(event)).toBe(true);
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it("waits for the agent's repaint before sending the next batch", async () => {
    // 卡顿的主因是开环:不管 agent 画完没有都按帧灌上报,于是我们跑在 agent 前面,
    // 画面变成"憋一下、跳一段"。闭环后一次重绘对应一批上报。
    const { handler, replayed, repaint } = fakeTerminal("alternate", "vt200", {
      cellHeight: 16,
      repaintSignal: true,
    });
    handler(wheel(160).event); // 10 行

    await nextFrame();
    const firstBatch = replayed.length;
    expect(firstBatch).toBeGreaterThan(0);
    expect(firstBatch).toBeLessThan(10);

    // 没有重绘信号:后续帧一条都不发,而不是继续加深管道深度。
    await nextFrame();
    await nextFrame();
    expect(replayed).toHaveLength(firstBatch);

    // agent 画完了 → 放行下一批。
    repaint();
    await nextFrame();
    expect(replayed.length).toBeGreaterThan(firstBatch);
  });

  it("advances without a repaint once the grace period lapses", async () => {
    // agent 滚到顶/底时一个字节都不回吐,只等信号会把队列锁死 —— 症状是"滚到顶再往回
    // 滚要卡一下"。超时兜底必须能自己推进。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      cellHeight: 16,
      repaintSignal: true,
    });
    handler(wheel(160).event);
    await nextFrame();
    const firstBatch = replayed.length;

    await new Promise<void>((resolve) => {
      globalThis.setTimeout(resolve, WHEEL_REPAINT_GRACE_MS + 40);
    });
    expect(replayed.length).toBeGreaterThan(firstBatch);
  });

  it("eases out instead of stepping at a fixed rate", async () => {
    // 固定 4 行/帧是匀速直线,台阶感就是"卡"。按剩余量取商后尾部自然收窄到 1 行/帧。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      rows: 40,
      cellHeight: 16,
    });
    handler(wheel(480).event); // 30 行

    const batches: number[] = [];
    let previous = 0;
    for (let frame = 0; frame < 32 && replayed.length < 30; frame += 1) {
      await nextFrame();
      batches.push(replayed.length - previous);
      previous = replayed.length;
    }

    expect(replayed).toHaveLength(30);
    // 单调不增:任何一帧都不该比上一帧更快,否则就不是减速。
    for (let index = 1; index < batches.length; index += 1) {
      expect(batches[index]).toBeLessThanOrEqual(batches[index - 1]);
    }
    // 关键断言:必须是一段**渐进**的减速尾巴,不是"匀速跑完 + 最后一帧凑数"。
    // 固定 4 行/帧的旧行为收敛成 [4,…,4,2],末三帧首项就是 4,过不了这条。
    expect(batches.length).toBeGreaterThan(Math.ceil(30 / 4));
    for (const batch of batches.slice(-3)) {
      expect(batch).toBeLessThanOrEqual(2);
    }
    expect(batches[0]).toBe(4);
  });

  it("flags its own replayed events so they do not count as user input", async () => {
    // 上报若走 pauseForUserInput,滚动期间每帧好几条 = 反复把 agent 的重绘往后推。
    const { handler, element, replayed, isReplayingWheel } = fakeTerminal("alternate", "vt200", {
      cellHeight: 16,
    });
    const flagDuringReplay: boolean[] = [];
    element!.addEventListener("wheel", () => {
      flagDuringReplay.push(isReplayingWheel());
    });

    expect(isReplayingWheel()).toBe(false);
    handler(wheel(32).event);
    await flushWheelFrames(replayed);

    expect(flagDuringReplay).toHaveLength(2);
    expect(flagDuringReplay.every(Boolean)).toBe(true);
    // 派发结束后必须落回 false,否则真正的键入也会被误判成滚轮。
    expect(isReplayingWheel()).toBe(false);
  });

  it("unsubscribes the repaint signal on dispose", () => {
    const { dispose, wasRepaintDisposed } = fakeTerminal("alternate", "vt200", {
      repaintSignal: true,
    });
    expect(wasRepaintDisposed()).toBe(false);
    dispose();
    expect(wasRepaintDisposed()).toBe(true);
  });
});

describe("initTerminal", () => {
  // 兜底只属于 agent 终端（TerminalView 自己装）。shell / SSH / WSL 面板共用 initTerminal，
  // 那边的 less、man、git log、vim 正是靠 xterm 的 alternate scroll 滚动的。
  it("leaves the wheel to xterm so shell panels keep alternate scroll", () => {
    const attach = vi.spyOn(XTerm.prototype, "attachCustomWheelEventHandler");
    try {
      initTerminal("dark", 5000, 12, "monospace");
      expect(attach).not.toHaveBeenCalled();
    } finally {
      attach.mockRestore();
    }
  });

  // 本地 scrollback 滚动（shell 面板、agent 终端不在 alt screen 时）是唯一能做到
  // 真正连续位移的路径:开了鼠标上报的 alt screen 由 agent 重绘,这个选项管不到。
  it("smooth-scrolls the local viewport so shell panels feel like a browser", () => {
    const { term } = initTerminal("dark", 5000, 12, "monospace");
    expect(term.options.smoothScrollDuration).toBe(TERMINAL_SMOOTH_SCROLL_MS);
    expect(TERMINAL_SMOOTH_SCROLL_MS).toBeGreaterThan(0);
  });
});
