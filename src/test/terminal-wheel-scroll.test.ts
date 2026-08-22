import { Terminal as XTerm, type Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";
import { attachTerminalWheelScroll, initTerminal } from "../components/terminalShared";

type BufferType = "normal" | "alternate";
type MouseMode = "none" | "x10" | "vt200" | "drag" | "any";

function fakeTerminal(bufferType: BufferType, mouseTrackingMode: MouseMode) {
  const handlers: Array<(event: WheelEvent) => boolean> = [];
  const term = {
    modes: { mouseTrackingMode },
    buffer: { active: { type: bufferType } },
    attachCustomWheelEventHandler: vi.fn((handler: (event: WheelEvent) => boolean) => {
      handlers.push(handler);
    }),
  } as unknown as Terminal;
  attachTerminalWheelScroll(term);
  return { term, handler: handlers[0] };
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

  it("forwards the wheel to apps that asked for mouse reporting", () => {
    for (const mode of ["vt200", "drag", "any"] as const) {
      const { handler } = fakeTerminal("alternate", mode);
      const { event, preventDefault } = wheel(-120);
      expect(handler(event)).toBe(true);
      expect(preventDefault).not.toHaveBeenCalled();
    }
  });

  it("ignores horizontal-only wheel events", () => {
    const { handler } = fakeTerminal("alternate", "none");
    const { event, preventDefault } = wheel(0, { deltaX: -120 });
    expect(handler(event)).toBe(true);
    expect(preventDefault).not.toHaveBeenCalled();
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
});
