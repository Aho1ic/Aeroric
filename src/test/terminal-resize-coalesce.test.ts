import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  TERMINAL_RESIZE_SETTLE_MS,
  createTerminalFitScheduler,
} from "../components/terminalShared";

/**
 * 拖窗口期间每帧 fit 一次会把 TUI 打成满屏残留:xterm resize 时 reflow scrollback,
 * 一条全宽 `─` 变窄后占 2 个屏幕行,于是那一帧的屏幕高度变了;而 Ink 擦除上一帧用的是
 * 它自己记的逻辑行数,擦不干净,帧顶部留成永久残留。每帧 resize 一次就每帧叠一条。
 * 这些用例锁住"拖动全程只 fit 一次"。
 */
describe("terminal resize coalescing", () => {
  let container: HTMLElement;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement("div");
    document.body.appendChild(container);
  });

  afterEach(() => {
    vi.useRealTimers();
    container.remove();
  });

  const entryFor = (width: number, height: number) =>
    [{ contentRect: { width, height } }] as unknown as ResizeObserverEntry[];

  it("collapses a drag into a single fit", () => {
    const fit = vi.fn();
    const scheduler = createTerminalFitScheduler(container, fit, () => true);

    // 模拟 60fps 拖动:每 16ms 报一个新尺寸
    for (let i = 0; i < 30; i += 1) {
      scheduler.schedule(entryFor(800 - i * 5, 600));
      vi.advanceTimersByTime(16);
    }
    expect(fit).not.toHaveBeenCalled();

    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit).toHaveBeenCalledTimes(1);
  });

  it("keeps the container hidden for the whole drag, then reveals", () => {
    const fit = vi.fn(() => container.removeAttribute("data-terminal-resizing"));
    const scheduler = createTerminalFitScheduler(container, fit, () => true);

    scheduler.schedule(entryFor(800, 600));
    expect(container.getAttribute("data-terminal-resizing")).toBe("true");

    vi.advanceTimersByTime(16);
    scheduler.schedule(entryFor(780, 600));
    vi.advanceTimersByTime(16);
    // 中途仍然是隐藏的,合并掉的中间帧不会暴露给用户
    expect(container.getAttribute("data-terminal-resizing")).toBe("true");
    expect(fit).not.toHaveBeenCalled();

    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit).toHaveBeenCalledTimes(1);
    expect(container.hasAttribute("data-terminal-resizing")).toBe(false);
  });

  it("ignores repeated reports of an unchanged size", () => {
    const fit = vi.fn();
    const scheduler = createTerminalFitScheduler(container, fit, () => true);

    scheduler.schedule(entryFor(800, 600));
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit).toHaveBeenCalledTimes(1);

    scheduler.schedule(entryFor(800, 600));
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit).toHaveBeenCalledTimes(1);
  });

  it("does not fit an inactive terminal, and unhides it", () => {
    const fit = vi.fn();
    let active = true;
    const scheduler = createTerminalFitScheduler(container, fit, () => active);

    scheduler.schedule(entryFor(800, 600));
    active = false;
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);

    expect(fit).not.toHaveBeenCalled();
    expect(container.hasAttribute("data-terminal-resizing")).toBe(false);
  });

  it("drops a pending fit on dispose", () => {
    const fit = vi.fn();
    const scheduler = createTerminalFitScheduler(container, fit, () => true);

    scheduler.schedule(entryFor(800, 600));
    scheduler.dispose();
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS * 2);

    expect(fit).not.toHaveBeenCalled();
    expect(container.hasAttribute("data-terminal-resizing")).toBe(false);
  });

  it("settles faster than a fit-per-frame drag would emit", () => {
    // 回归护栏:settle 窗口必须明显大于一帧,否则合并不掉拖动
    expect(TERMINAL_RESIZE_SETTLE_MS).toBeGreaterThan(16);
  });

  /**
   * 关掉左右分屏后终端要回到全宽。
   *
   * 这条盯的是"尺寸没变就跳过"那道去重与非激活早退的先后顺序:去重的记录若在隐藏
   * 期间照记,那么"隐藏 → 尺寸变过 → 重新可见且回到隐藏前的尺寸"这条路上,
   * 调度器会判定无事发生并跳过 fit,而 xterm 可能已被激活时的 fit 改成了别的列数,
   * 于是排版停在被挤压的状态。清空记录是唯一能让重新可见后必定重新 fit 的做法。
   */
  it("refits after a hidden resize returns to the pre-hidden size", () => {
    const fit = vi.fn();
    let active = true;
    const scheduler = createTerminalFitScheduler(container, fit, () => active);

    // 全宽,正常 fit 一次
    scheduler.schedule(entryFor(1600, 600));
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit).toHaveBeenCalledTimes(1);

    // 面板隐藏(display:none → contentRect 归零),期间的报告不该被记账
    active = false;
    scheduler.schedule(entryFor(0, 0));
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit).toHaveBeenCalledTimes(1);

    // 重新可见,尺寸与隐藏前相同 —— 仍然必须重新 fit
    active = true;
    scheduler.schedule(entryFor(1600, 600));
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit).toHaveBeenCalledTimes(2);
  });

  /**
   * 分屏的完整往返:全宽 → 半宽 → 全宽,三段都要各自 fit 一次。
   */
  it("fits on both directions of a split toggle", () => {
    const fit = vi.fn();
    const scheduler = createTerminalFitScheduler(container, fit, () => true);

    for (const width of [1600, 800, 1600]) {
      scheduler.schedule(entryFor(width, 600));
      vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    }
    expect(fit).toHaveBeenCalledTimes(3);
  });

  /**
   * 0 尺寸不是稳定状态:它只说明容器此刻在 display:none 子树里,safeFit 到点也会
   * 因为 rect 为 0 而放弃。记下它等于把上面那个漏洞换个入口再造一遍 —— 哪怕
   * isActive 一直是 true(面板本身没切,是祖先节点被隐藏的情形)。
   */
  it("does not remember a zero size even while active", () => {
    const fit = vi.fn();
    const scheduler = createTerminalFitScheduler(container, fit, () => true);

    scheduler.schedule(entryFor(1600, 600));
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit).toHaveBeenCalledTimes(1);

    scheduler.schedule(entryFor(0, 0));
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    const afterZero = fit.mock.calls.length;

    scheduler.schedule(entryFor(1600, 600));
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(fit.mock.calls.length).toBeGreaterThan(afterZero);
  });
});
