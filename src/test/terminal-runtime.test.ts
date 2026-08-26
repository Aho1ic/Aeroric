import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createTerminalRuntime } from "../components/terminalRuntime";
import { TERMINAL_RESIZE_SETTLE_MS } from "../components/terminalShared";

const mocks = vi.hoisted(() => {
  const term = {
    cols: 80,
    rows: 24,
    options: { theme: null, fontSize: 12, fontFamily: "monospace" },
    textarea: { disabled: false, focus: vi.fn() },
    open: vi.fn(),
    focus: vi.fn(),
    dispose: vi.fn(),
    writeln: vi.fn(),
  };
  const writer = {
    write: vi.fn(),
    pauseForUserInput: vi.fn(),
  };
  return {
    term,
    writer,
    initTerminal: vi.fn(() => ({ term, fitAddon: {} })),
    applyTerminalTheme: vi.fn(),
    applyTerminalFontSize: vi.fn(() => ({ cols: 100, rows: 30 })),
    applyTerminalFontFamily: vi.fn(() => ({ cols: 110, rows: 32 })),
    safeFit: vi.fn(() => ({ cols: 90, rows: 28 })),
    fitTerminalAtBottom: vi.fn(() => ({ cols: 90, rows: 28 })),
    createSmartWriter: vi.fn(() => writer),
    attachMacWebKitTerminalGuard: vi.fn(() => vi.fn()),
    loadWebglAddon: vi.fn(),
  };
});

// createTerminalFitScheduler 用真实实现:resize 合并是这条路径的关键行为
// (每帧 fit 一次会把 TUI 打成满屏残留),用假的就等于没测。
vi.mock("../components/terminalShared", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../components/terminalShared")>();
  return {
    applyTerminalFontFamily: mocks.applyTerminalFontFamily,
    applyTerminalFontSize: mocks.applyTerminalFontSize,
    applyTerminalTheme: mocks.applyTerminalTheme,
    attachMacWebKitTerminalGuard: mocks.attachMacWebKitTerminalGuard,
    createSmartWriter: mocks.createSmartWriter,
    createTerminalFitScheduler: actual.createTerminalFitScheduler,
    TERMINAL_RESIZE_SETTLE_MS: actual.TERMINAL_RESIZE_SETTLE_MS,
    fitTerminalAtBottom: mocks.fitTerminalAtBottom,
    initTerminal: mocks.initTerminal,
    loadWebglAddon: mocks.loadWebglAddon,
    safeFit: mocks.safeFit,
  };
});

const inputDisposers = vi.hoisted(() => ({
  textarea: vi.fn(),
  linux: vi.fn(),
  mac: vi.fn(),
  windows: vi.fn(),
}));

vi.mock("../components/terminalInputFix", () => ({
  applyTerminalTextareaInputAttributes: inputDisposers.textarea,
  attachLinuxIMEFix: vi.fn(() => ({ dispose: inputDisposers.linux })),
  attachMacWebKitShiftInputFix: vi.fn(() => inputDisposers.mac),
  attachWindowsIMEPositionFix: vi.fn(() => inputDisposers.windows),
}));

const copyDisposer = vi.hoisted(() => vi.fn());
vi.mock("../components/terminalCopyHelper", () => ({
  attachSmartCopy: vi.fn(() => copyDisposer),
}));

class ResizeObserverMock {
  static instances: ResizeObserverMock[] = [];
  constructor(readonly callback: ResizeObserverCallback) {
    ResizeObserverMock.instances.push(this);
  }
  observe = vi.fn();
  disconnect = vi.fn();
}

describe("createTerminalRuntime", () => {
  let animationFrames: FrameRequestCallback[];

  beforeEach(() => {
    vi.clearAllMocks();
    // resize 合并靠 setTimeout,必须能手动推进
    vi.useFakeTimers();
    ResizeObserverMock.instances = [];
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    animationFrames = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      animationFrames.push(callback);
      return animationFrames.length;
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shares initialization, appearance updates, resize, visibility, and cleanup", () => {
    const container = document.createElement("div");
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue({
      width: 640,
      height: 480,
    } as DOMRect);
    const onInput = vi.fn();
    const onResize = vi.fn();
    const runtime = createTerminalRuntime({
      container,
      themeVariant: "dark",
      terminalFontSize: 12,
      monoFontFamily: "monospace",
      isActive: () => true,
      onInput,
      onResize,
    });

    expect(mocks.initTerminal).toHaveBeenCalledWith("dark", 5000, 12, "monospace");
    expect(mocks.term.open).toHaveBeenCalledWith(container);
    expect(ResizeObserverMock.instances[0]?.observe).toHaveBeenCalledWith(container);

    // 连续两次上报合并成一次 fit,且要等尺寸稳定之后才落地
    ResizeObserverMock.instances[0]?.callback([], ResizeObserverMock.instances[0] as never);
    ResizeObserverMock.instances[0]?.callback([], ResizeObserverMock.instances[0] as never);
    expect(mocks.fitTerminalAtBottom).not.toHaveBeenCalled();
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(mocks.fitTerminalAtBottom).toHaveBeenCalledOnce();

    const sameSize = { contentRect: { width: 640, height: 480 } } as ResizeObserverEntry;
    const changedSize = { contentRect: { width: 700, height: 480 } } as ResizeObserverEntry;
    ResizeObserverMock.instances[0]?.callback([sameSize], ResizeObserverMock.instances[0] as never);
    ResizeObserverMock.instances[0]?.callback([sameSize], ResizeObserverMock.instances[0] as never);
    ResizeObserverMock.instances[0]?.callback(
      [changedSize],
      ResizeObserverMock.instances[0] as never,
    );
    vi.advanceTimersByTime(TERMINAL_RESIZE_SETTLE_MS);
    expect(mocks.fitTerminalAtBottom).toHaveBeenCalledTimes(2);

    runtime.fit();
    expect(onResize).toHaveBeenCalledWith({ cols: 90, rows: 28 });
    runtime.fit();
    expect(onResize).toHaveBeenCalledTimes(1);
    runtime.updateTheme("light");
    runtime.updateFontSize(14);
    runtime.updateFontSize(14);
    runtime.updateFontFamily("Menlo");
    expect(mocks.applyTerminalTheme).toHaveBeenCalledWith(mocks.term, "light");
    expect(onResize).toHaveBeenNthCalledWith(2, { cols: 100, rows: 30 });
    expect(onResize).toHaveBeenNthCalledWith(3, { cols: 110, rows: 32 });
    expect(onResize).toHaveBeenCalledTimes(3);

    const visibilityEvent = new Event("visibilitychange");
    document.dispatchEvent(visibilityEvent);
    animationFrames.shift()?.(0);
    expect(mocks.term.focus).toHaveBeenCalled();

    runtime.dispose();
    runtime.dispose();
    runtime.updateTheme("dark");
    expect(runtime.updateFontSize(16)).toBeNull();
    expect(runtime.updateFontFamily("monospace")).toBeNull();
    expect(ResizeObserverMock.instances[0]?.disconnect).toHaveBeenCalledOnce();
    expect(inputDisposers.linux).toHaveBeenCalledOnce();
    expect(inputDisposers.mac).toHaveBeenCalledOnce();
    expect(inputDisposers.windows).toHaveBeenCalledOnce();
    expect(copyDisposer).toHaveBeenCalledOnce();
    expect(mocks.term.dispose).toHaveBeenCalledOnce();
    expect(mocks.applyTerminalTheme).toHaveBeenCalledOnce();
    expect(mocks.applyTerminalFontSize).toHaveBeenCalledTimes(2);
    expect(mocks.applyTerminalFontFamily).toHaveBeenCalledOnce();
  });
});
