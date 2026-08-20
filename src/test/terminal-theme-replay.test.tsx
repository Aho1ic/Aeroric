import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const terminalState = vi.hoisted(() => ({
  deferWrites: false,
  runtimes: [] as Array<{
    theme: string;
    writes: string[];
    pendingWriteCallbacks: Array<() => void>;
    scrollToBottom: ReturnType<typeof vi.fn>;
  }>,
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
    const state = {
      theme,
      writes: [] as string[],
      pendingWriteCallbacks: [] as Array<() => void>,
      scrollToBottom: vi.fn(),
    };
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
      scrollToBottom: state.scrollToBottom,
      scrollToLine: vi.fn(),
      write: vi.fn((data: string, callback?: () => void) => {
        state.writes.push(data);
        if (!callback) return;
        if (terminalState.deferWrites) {
          state.pendingWriteCallbacks.push(callback);
          return;
        }
        callback();
      }),
    };
    return { term, fitAddon: {} };
  },
  fitTerminalAtBottom: () => ({ cols: 80, rows: 24 }),
  createSmartWriter: (term: { write: (data: string, callback?: () => void) => void }) => ({
    write: (data: string, callback?: () => void) => term.write(data, callback),
    writeImmediate: (data: string, callback?: () => void) => term.write(data, callback),
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
    terminalState.deferWrites = false;
    terminalState.runtimes.length = 0;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
  });

  it("keeps restored history hidden until it is positioned at the bottom", async () => {
    terminalState.deferWrites = true;
    const onReady = vi.fn();
    render(
      <TerminalView
        onInput={vi.fn()}
        onResize={vi.fn()}
        onRegisterTerminal={() => 11}
        onReady={onReady}
        terminalFontSize={12}
        monoFontFamily="monospace"
        initialData="large Codex history"
        rawReplayData="large Codex history"
        themeVariant="dark"
      />,
    );

    const container = screen.getByTestId("agent-terminal");
    expect(container).toHaveAttribute("data-terminal-restoring", "true");
    await waitFor(() => expect(terminalState.runtimes[0]?.pendingWriteCallbacks).toHaveLength(1));
    expect(onReady).not.toHaveBeenCalled();

    act(() => terminalState.runtimes[0].pendingWriteCallbacks.shift()?.());

    expect(terminalState.runtimes[0].scrollToBottom).toHaveBeenCalledTimes(1);
    expect(container).toHaveAttribute("data-terminal-restoring", "true");
    await waitFor(() => expect(container).not.toHaveAttribute("data-terminal-restoring"));
    expect(onReady).toHaveBeenCalledWith(11);
  });

  it("does not hide a new terminal that has no history to restore", async () => {
    const onReady = vi.fn();
    render(
      <TerminalView
        onInput={vi.fn()}
        onResize={vi.fn()}
        onRegisterTerminal={() => 13}
        onReady={onReady}
        terminalFontSize={12}
        monoFontFamily="monospace"
        themeVariant="dark"
      />,
    );

    expect(screen.getByTestId("agent-terminal")).not.toHaveAttribute("data-terminal-restoring");
    await waitFor(() => expect(onReady).toHaveBeenCalledWith(13));
  });

  it("keeps a reactivated terminal hidden until its bottom-anchored frame", async () => {
    const onReady = vi.fn();
    const props = {
      onInput: vi.fn(),
      onResize: vi.fn(),
      onRegisterTerminal: () => 17,
      onReady,
      terminalFontSize: 12 as const,
      monoFontFamily: "monospace" as const,
      themeVariant: "dark" as const,
    };
    const view = render(<TerminalView {...props} isActive={false} />);
    await waitFor(() => expect(onReady).toHaveBeenCalledWith(17));

    const frames: FrameRequestCallback[] = [];
    const requestFrame = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback) => {
        frames.push(callback);
        return frames.length;
      });
    const cancelFrame = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

    view.rerender(<TerminalView {...props} isActive />);
    const container = screen.getByTestId("agent-terminal");
    expect(container).toHaveAttribute("data-terminal-activating", "true");

    act(() => frames.shift()?.(0));
    expect(container).toHaveAttribute("data-terminal-activating", "true");

    act(() => frames.shift()?.(16));
    expect(container).not.toHaveAttribute("data-terminal-activating");

    requestFrame.mockRestore();
    cancelFrame.mockRestore();
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
    expect(screen.getByTestId("agent-terminal")).toHaveAttribute("data-terminal-theme", "light");
    act(() => registeredWrite?.("\u001b[48;2;20;20;20mlive\u001b[0m"));

    terminalState.deferWrites = true;
    view.rerender(<TerminalView {...props} themeVariant="dark" />);
    const container = screen.getByTestId("agent-terminal");
    expect(container).toHaveAttribute("data-terminal-theme", "dark");
    await waitFor(() => expect(terminalState.runtimes).toHaveLength(2));
    expect(container).toHaveAttribute("data-terminal-restoring", "true");
    expect(terminalState.runtimes[1].pendingWriteCallbacks).toHaveLength(1);
    act(() => terminalState.runtimes[1].pendingWriteCallbacks.shift()?.());
    expect(terminalState.runtimes[1].scrollToBottom).toHaveBeenCalledTimes(1);
    expect(container).toHaveAttribute("data-terminal-restoring", "true");
    await waitFor(() => expect(container).not.toHaveAttribute("data-terminal-restoring"));
    expect(terminalState.runtimes[1].writes.join("")).toBe(
      "\u001b[40moriginal\u001b[0m\u001b[48;2;20;20;20mlive\u001b[0m",
    );

    terminalState.deferWrites = false;
    view.rerender(<TerminalView {...props} themeVariant="light" />);
    expect(screen.getByTestId("agent-terminal")).toHaveAttribute("data-terminal-theme", "light");
    await waitFor(() => expect(terminalState.runtimes).toHaveLength(3));
    expect(terminalState.runtimes[2].writes.join("")).toBe(
      "\u001b[40moriginal\u001b[0m\u001b[48;2;20;20;20mlive\u001b[0m",
    );
    expect(
      onRegisterTerminal.mock.calls.filter(([write]) => typeof write === "function"),
    ).toHaveLength(1);
  });
});
