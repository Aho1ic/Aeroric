import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const terminalState = vi.hoisted(() => ({
  runtimes: [] as Array<{ theme: string; writes: string[] }>,
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue({}) }));

vi.mock("@xterm/addon-serialize", () => ({
  SerializeAddon: class {
    serialize() {
      return "serialized";
    }
  },
}));

vi.mock("../components/terminalShared", () => ({
  themeFor: (theme: string) => ({ background: theme }),
  initTerminal: (theme: string) => {
    const state = { theme, writes: [] as string[] };
    terminalState.runtimes.push(state);
    const textarea = document.createElement("textarea");
    const term = {
      textarea,
      options: { cursorBlink: true, theme: {} },
      buffer: { active: { viewportY: 0, baseY: 0 } },
      loadAddon: vi.fn(),
      open: vi.fn((container: HTMLElement) => container.appendChild(textarea)),
      focus: vi.fn(),
      resize: vi.fn(),
      dispose: vi.fn(),
      scrollToBottom: vi.fn(),
      scrollToLine: vi.fn(),
      write: vi.fn((data: string, callback?: () => void) => {
        state.writes.push(data);
        callback?.();
      }),
    };
    return { term, fitAddon: {} };
  },
  safeFit: () => ({ cols: 80, rows: 24 }),
  createSmartWriter: (term: { write: (data: string, callback?: () => void) => void }) => ({
    write: (data: string, callback?: () => void) => term.write(data, callback),
    pauseForUserInput: vi.fn(),
  }),
  attachMacWebKitTerminalGuard: () => vi.fn(),
  attachCursorLineHighlight: () => vi.fn(),
  applyTerminalFontSize: () => null,
  applyTerminalFontFamily: () => null,
}));

vi.mock("../components/terminalCopyHelper", () => ({ attachSmartCopy: () => vi.fn() }));
vi.mock("../components/terminalInputFix", () => ({
  applyTerminalTextareaInputAttributes: vi.fn(),
  attachLinuxIMEFix: () => ({ dispose: vi.fn() }),
  attachMacWebKitShiftInputFix: () => vi.fn(),
  attachWindowsIMEPositionFix: () => vi.fn(),
}));

import { TerminalView } from "../components/TerminalView";

describe("TerminalView theme replay", () => {
  beforeEach(() => {
    terminalState.runtimes.length = 0;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
  });

  it("keeps one stable ingress and rebuilds both theme directions from raw ANSI output", async () => {
    let registeredWrite: ((data: string, callback?: () => void) => void) | null = null;
    const onRegisterTerminal = vi.fn((write) => {
      if (write) registeredWrite = write;
      return 7;
    });
    const props = {
      onInput: vi.fn(),
      onResize: vi.fn(),
      onRegisterTerminal,
      onReady: vi.fn(),
      terminalFontSize: 12 as const,
      monoFontFamily: "monospace" as const,
      initialData: "\u001b[40moriginal\u001b[0m",
      rawReplayData: "\u001b[40moriginal\u001b[0m",
    };
    const view = render(<TerminalView {...props} themeVariant="light" />);

    await waitFor(() => expect(props.onReady).toHaveBeenCalledWith(7));
    act(() => registeredWrite?.("\u001b[48;2;20;20;20mlive\u001b[0m"));

    view.rerender(<TerminalView {...props} themeVariant="dark" />);
    await waitFor(() => expect(terminalState.runtimes).toHaveLength(2));
    expect(terminalState.runtimes[1].writes.join("")).toBe(
      "\u001b[40moriginal\u001b[0m\u001b[48;2;20;20;20mlive\u001b[0m",
    );

    view.rerender(<TerminalView {...props} themeVariant="light" />);
    await waitFor(() => expect(terminalState.runtimes).toHaveLength(3));
    expect(terminalState.runtimes[2].writes.join("")).toBe(
      "\u001b[40moriginal\u001b[0m\u001b[48;2;20;20;20mlive\u001b[0m",
    );
    expect(
      onRegisterTerminal.mock.calls.filter(([write]) => typeof write === "function"),
    ).toHaveLength(1);
  });
});
