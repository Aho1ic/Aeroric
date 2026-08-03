import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: ((data: string) => void) | null = null;
  },
  invoke: vi.fn(),
}));

import { useTerminalManager } from "../hooks/useTerminalManager";

describe("remote terminal resize synchronization", () => {
  it("forwards valid remote sizes to the mounted terminal without invoking PTY resize", () => {
    const { result } = renderHook(() => useTerminalManager());
    const write = vi.fn();
    const resize = vi.fn();

    act(() => {
      result.current.handleRegisterTerminal("task-1", write, resize);
      result.current.handleRemoteResize("task-1", 46, 32);
    });

    expect(resize).toHaveBeenCalledOnce();
    expect(resize).toHaveBeenCalledWith(46, 32);
  });

  it("ignores invalid sizes and stops forwarding after terminal cleanup", () => {
    const { result } = renderHook(() => useTerminalManager());
    const resize = vi.fn();

    act(() => {
      result.current.handleRegisterTerminal("task-1", vi.fn(), resize);
      result.current.handleRemoteResize("task-1", 1, 32);
      result.current.handleRemoteResize("task-1", 46, 0);
      result.current.handleRegisterTerminal("task-1", null);
      result.current.handleRemoteResize("task-1", 46, 32);
    });

    expect(resize).not.toHaveBeenCalled();
  });
});
