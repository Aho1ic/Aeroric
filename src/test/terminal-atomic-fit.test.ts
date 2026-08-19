import { afterEach, describe, expect, it, vi } from "vitest";
import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";
import { fitTerminalAtBottom } from "../components/terminalShared";

afterEach(() => vi.restoreAllMocks());

describe("fitTerminalAtBottom", () => {
  it("keeps reflow hidden until the bottom-anchored frame is ready", () => {
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
    const container = document.createElement("div");
    vi.spyOn(container, "getBoundingClientRect").mockReturnValue({
      width: 640,
      height: 480,
    } as DOMRect);
    const fitAddon = {
      proposeDimensions: vi.fn(() => ({ cols: 90, rows: 28 })),
      fit: vi.fn(),
    } as unknown as FitAddon;
    const term = {
      cols: 90,
      rows: 28,
      scrollToBottom: vi.fn(),
      refresh: vi.fn(),
    } as unknown as Terminal;

    expect(fitTerminalAtBottom(fitAddon, term, container)).toEqual({ cols: 90, rows: 28 });
    expect(container).toHaveAttribute("data-terminal-resizing", "true");
    expect(fitAddon.fit).toHaveBeenCalledOnce();
    expect(term.scrollToBottom).toHaveBeenCalledOnce();
    expect(term.refresh).toHaveBeenCalledWith(0, 27);

    frames[0]?.(0);
    expect(container).not.toHaveAttribute("data-terminal-resizing");
  });
});
