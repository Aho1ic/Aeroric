import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: ((data: string) => void) | null = null;
  },
  invoke: vi.fn(),
}));

import { useTerminalManager } from "../hooks/useTerminalManager";

/**
 * 后端错误（agent 的多行 stderr、Rust 的多行 Err）只带 \n。xterm 不把裸 \n
 * 当作回到行首，直接写进去会排成阶梯状，每行比上一行缩进更深，长堆栈基本没法读。
 */
describe("error output written to the terminal", () => {
  function writeError(message: string): string {
    const { result } = renderHook(() => useTerminalManager());
    const write = vi.fn();
    act(() => {
      result.current.resetTaskTerminal("task-1");
      const generation = result.current.handleRegisterTerminal("task-1", write);
      result.current.handleTerminalReady("task-1", generation);
      result.current.writeErrorToTerminal("task-1", message);
    });
    return write.mock.calls.map(([chunk]) => String(chunk)).join("");
  }

  it("gives every bare newline a carriage return so multi-line errors do not stair-step", () => {
    const written = writeError(
      "\r\nError: plugin tree failed to load\n  at boot\n  at runProfile\n",
    );

    // 每个 \n 前面都必须有 \r，否则就是阶梯状那个 bug。
    for (const [index, char] of [...written].entries()) {
      if (char === "\n") {
        expect(written[index - 1]).toBe("\r");
      }
    }
    expect(written).toContain("Error: plugin tree failed to load");
    expect(written).toContain("at runProfile");
  });

  it("does not double the carriage return on text that already uses CRLF", () => {
    const written = writeError("\r\nError: already normalized\r\n");

    expect(written).not.toContain("\r\r");
    expect(written).toBe("\r\nError: already normalized\r\n");
  });

  it("keeps the normalized text in the replay buffer, not just on screen", () => {
    const { result } = renderHook(() => useTerminalManager());
    act(() => {
      result.current.resetTaskTerminal("task-2");
      result.current.writeErrorToTerminal("task-2", "Error: line one\nline two\n");
    });

    // 缓冲区是重挂载后回放的来源。若只修屏幕不修缓冲，切走再切回来又是阶梯状。
    const replayed = result.current.getTaskRestoreState("task-2").rawReplayData;
    expect(replayed).toContain("line one\r\nline two");
    expect(replayed).not.toContain("one\nline");
  });
});
