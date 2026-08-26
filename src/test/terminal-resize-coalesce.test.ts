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
});
