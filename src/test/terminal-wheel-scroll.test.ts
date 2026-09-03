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
  // 正常缓冲区走本地滚动:记下每次 scrollLines 的行数,断言行程 1:1。
  const scrolled: number[] = [];
  const term = {
    element,
    rows,
    modes: { mouseTrackingMode },
    buffer: { active: { type: bufferType } },
    scrollLines: vi.fn((lines: number) => {
      scrolled.push(lines);
    }),
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
    scrolled,
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
 * 超出突发额度的长尾按帧补发,所以断言总行程前要把排期的帧跑完。
 *
 * 循环推进直到再没有新上报,用"上一轮的条数"判断是否已经排空,而不是猜需要几帧。
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

  it("scrolls the normal buffer itself so travel stays 1:1", () => {
    // 交回 xterm 就会吃到 consumeWheelEvent 对 |deltaY| < 50 的 0.3 倍"触控板阻尼",
    // 那正好反着我们要的行程一致 —— 轻滑变成"滑了半屏只动两行"。
    const { handler, scrolled } = fakeTerminal("normal", "none", { cellHeight: 16 });
    const { event, preventDefault } = wheel(-120);
    expect(handler(event)).toBe(false);
    expect(preventDefault).toHaveBeenCalledOnce();
    // 120 / 16 = 7 行,向上。
    expect(scrolled).toEqual([-7]);
  });

  it("keeps sub-line trackpad travel instead of dropping it on the normal buffer", () => {
    const { handler, scrolled } = fakeTerminal("normal", "none", { cellHeight: 16 });
    // 一次 4px 不足一行:不能当 0 丢掉(丢了就是"滑了没反应"),攒到第四次凑满一行。
    for (let index = 0; index < 3; index += 1) {
      expect(handler(wheel(4).event)).toBe(false);
    }
    expect(scrolled).toEqual([]);
    expect(handler(wheel(4).event)).toBe(false);
    expect(scrolled).toEqual([1]);
  });

  it("hands an unmeasurable normal-buffer wheel back to xterm instead of eating it", () => {
    // 量不到行高时 wheelLinesForEvent 返回 0,和"余量没攒满一行"撞成同一个值。
    // 分不开就会把事件吞掉,症状是滚轮彻底没反应 —— 打了折的滚动也比不动好。
    const { handler, scrolled } = fakeTerminal("normal", "none", { cellHeight: 0 });
    const { event, preventDefault } = wheel(-120);
    expect(handler(event)).toBe(true);
    expect(preventDefault).not.toHaveBeenCalled();
    expect(scrolled).toEqual([]);

    // LINE / PAGE 不需要行高,照旧自己滚。
    const page = fakeTerminal("normal", "none", { cellHeight: 0, rows: 20 });
    expect(page.handler(wheel(1, { deltaMode: WheelEvent.DOM_DELTA_PAGE }).event)).toBe(false);
    expect(page.scrolled).toEqual([20]);
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

  it("sends a normal notch synchronously so it lands on the same tick as the gesture", async () => {
    // 这是"跟手"的核心断言。之前一条都不当场发、全推到后续帧,一个普通档位要四帧四次
    // 重绘才走完,叠起来就是用户报的"较大延迟"。把配额换回 4 条/帧跑这条必须红。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      rows: 24,
      cellHeight: 16,
    });
    handler(wheel(160).event); // 160 / 16 = 10 行,一屏(24)以内
    // 同步返回时就已经全部发出,不需要等任何一帧。
    expect(replayed).toHaveLength(10);

    // 后续帧不会再补发 —— 队列是空的,总行程仍是 1:1。
    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(10);
  });

  it("queues only the part of a fling that exceeds one screen", async () => {
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      rows: 10,
      cellHeight: 16,
    });
    // 400 / 16 = 25 行,远超一屏(10)。突发发满一屏,余下 15 行进队列。
    handler(wheel(400).event);
    expect(replayed).toHaveLength(10);

    await flushWheelFrames(replayed);
    expect(replayed).toHaveLength(25);
  });

  it("abandons the queued backlog when the wheel reverses mid-drain", async () => {
    // 分帧后余量会跨帧存活,于是多了一种反向:上一段还没发完,用户已经往回滚了。
    // 此时补完旧方向只会让画面先往回跳一段再跟手,所以直接丢掉。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      rows: 10,
      cellHeight: 16,
    });
    // 480 / 16 = 30 行:突发发掉一屏(10),余下 20 行留在队列里。
    handler(wheel(480).event);
    const beforeReversal = replayed.length;
    expect(beforeReversal).toBe(10);

    handler(wheel(-32).event); // 反向 2 行
    await flushWheelFrames(replayed);
    // 只补了反向的 2 条,原方向排队的 20 行被丢掉,画面不会先往回跳一段再跟手。
    expect(replayed).toHaveLength(beforeReversal + 2);
    expect(replayed.slice(beforeReversal).every((event) => event.deltaY === -1)).toBe(true);
  });

  it("stops pending frames once disposed so a torn-down terminal is never touched", async () => {
    const { handler, replayed, dispose } = fakeTerminal("alternate", "vt200", {
      rows: 10,
      cellHeight: 16,
    });
    // 要让队列里真的有东西才验得到:一屏以内会当场发完,不留待发帧。
    handler(wheel(480).event); // 30 行 → 突发 10,排队 20
    const burst = replayed.length;
    expect(burst).toBe(10);
    dispose();
    await flushWheelFrames(replayed);
    // 排队的 20 行一条都没再发出去。
    expect(replayed).toHaveLength(burst);
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

  it("waits for the agent's repaint before draining the queued tail", async () => {
    // 长尾仍然闭环:不管 agent 画完没有就按帧灌,我们会跑在它前面,画面变成
    // "憋一下、跳一段"。突发那一屏不受这条约束 —— 它是同步发的。
    const { handler, replayed, repaint } = fakeTerminal("alternate", "vt200", {
      rows: 16,
      cellHeight: 16,
      repaintSignal: true,
    });
    // 800/16 = 50 行,但单个事件被 MAX_WHEEL_LINES_PER_EVENT_SCREENS 截到 3 屏 = 48 行。
    handler(wheel(800).event); // 48 行 → 突发 16,排队 32
    const burst = replayed.length;
    expect(burst).toBe(16);

    // 突发之后就在等重绘:后续帧一条都不补,而不是继续加深管道深度。
    await nextFrame();
    await nextFrame();
    expect(replayed).toHaveLength(burst);

    // agent 画完了 → 放行下一批。
    repaint();
    await nextFrame();
    expect(replayed.length).toBeGreaterThan(burst);
  });

  it("advances without a repaint once the grace period lapses", async () => {
    // agent 滚到顶/底时一个字节都不回吐,只等信号会把队列锁死 —— 症状是"滚到顶再往回
    // 滚要卡一下"。超时兜底必须能自己推进。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      rows: 16,
      cellHeight: 16,
      repaintSignal: true,
    });
    handler(wheel(800).event); // 48 行 → 突发 16,排队 32
    const burst = replayed.length;
    expect(burst).toBe(16);

    // 一次 repaint 都不给,只等宽限期过去。
    await new Promise<void>((resolve) => {
      globalThis.setTimeout(resolve, WHEEL_REPAINT_GRACE_MS + 40);
    });
    expect(replayed.length).toBeGreaterThan(burst);
  });

  it("drains a fling in few large batches rather than dribbling it out", async () => {
    // 旧行为是 ceil(剩余/3) 夹进 1..4,一次甩动要十几帧、十几次 agent 重绘才走完。
    // 一次 write 带多条上报,agent 是一次 read、一次重绘 —— 拆细既加延迟又加重绘。
    const { handler, replayed } = fakeTerminal("alternate", "vt200", {
      rows: 16,
      cellHeight: 16,
    });
    handler(wheel(800).event); // 48 行 → 突发 16,排队 32

    const batches: number[] = [];
    let previous = replayed.length;
    for (let frame = 0; frame < 32 && replayed.length < 48; frame += 1) {
      await nextFrame();
      batches.push(replayed.length - previous);
      previous = replayed.length;
    }

    expect(replayed).toHaveLength(48);
    // 32 行的长尾在 16 行/帧下两帧收完。旧的 1..4 配额要 ≥8 帧,过不了这条。
    expect(batches.filter((batch) => batch > 0).length).toBeLessThanOrEqual(2);
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

  // 不插值。补间让位移看着连续,但画面必然滞后手指一个插值周期,100ms 时用户直接
  // 报"滑动有较大延迟"。行程已经是 1:1,立刻到位才跟手。
  it("does not interpolate local viewport scrolling, so it stays glued to the wheel", () => {
    const { term } = initTerminal("dark", 5000, 12, "monospace");
    expect(term.options.smoothScrollDuration).toBe(TERMINAL_SMOOTH_SCROLL_MS);
    expect(TERMINAL_SMOOTH_SCROLL_MS).toBe(0);
  });
});
