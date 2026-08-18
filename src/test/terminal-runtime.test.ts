import { beforeEach, describe, expect, it, vi } from "vitest";
import { createTerminalRuntime } from "../components/terminalRuntime";

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
    createSmartWriter: vi.fn(() => writer),
    attachMacWebKitTerminalGuard: vi.fn(() => vi.fn()),
    loadWebglAddon: vi.fn(),
  };
});

vi.mock("../components/terminalShared", () => ({
  applyTerminalFontFamily: mocks.applyTerminalFontFamily,
  applyTerminalFontSize: mocks.applyTerminalFontSize,
  applyTerminalTheme: mocks.applyTerminalTheme,
  attachMacWebKitTerminalGuard: mocks.attachMacWebKitTerminalGuard,
  createSmartWriter: mocks.createSmartWriter,
  initTerminal: mocks.initTerminal,
  loadWebglAddon: mocks.loadWebglAddon,
  safeFit: mocks.safeFit,
}));

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
  beforeEach(() => {
    vi.clearAllMocks();
    ResizeObserverMock.instances = [];
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      callback(0);
      return 1;
    });
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
