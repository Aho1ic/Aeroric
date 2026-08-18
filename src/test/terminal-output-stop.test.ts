import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { channels } = vi.hoisted(() => ({
  channels: [] as Array<{ onmessage: ((data: string) => void) | null }>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: ((data: string) => void) | null = null;

    constructor() {
      channels.push(this);
    }
  },
  invoke: vi.fn(),
}));

import { useTerminalManager } from "../hooks/useTerminalManager";

describe("terminal output lifecycle", () => {
  beforeEach(() => {
    channels.length = 0;
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("drops queued and late chunks after a task output stream is stopped", () => {
    let frame: FrameRequestCallback | undefined;
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn((callback: FrameRequestCallback) => {
        frame = callback;
        return 1;
      }),
    );
    const { result } = renderHook(() => useTerminalManager());
    const write = vi.fn();

    act(() => {
      result.current.resetTaskTerminal("task-1");
      const generation = result.current.handleRegisterTerminal("task-1", write);
      result.current.handleTerminalReady("task-1", generation);
      result.current.createOutputChannel("task-1");
      channels[0].onmessage?.("queued");
      result.current.stopTaskOutput("task-1");
      channels[0].onmessage?.("late");
      frame?.(0);
    });

    expect(write).not.toHaveBeenCalled();
    expect(result.current.getTaskRestoreState("task-1").rawReplayData).toBe("");
  });
});
