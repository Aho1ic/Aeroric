import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { APP_SETTINGS_CHANGED_EVENT } from "../components/app-settings/types";
import {
  AGENT_LATEST_REFRESH_INTERVAL_MS,
  AGENT_STATUS_REFRESH_INTERVAL_MS,
  AgentVersionsProvider,
} from "../hooks/useAgentVersions";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

function flushPromises() {
  return Promise.resolve().then(() => Promise.resolve());
}

describe("AgentVersionsProvider", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_agent_tool_status") {
        return Promise.resolve([
          { agent: "claude", version: "1.0.0" },
          { agent: "codex", version: "1.0.0" },
          { agent: "dsh", version: "0.1.0" },
        ]);
      }
      if (command === "get_agent_latest_versions") {
        return Promise.resolve([
          { agent: "claude", version: "1.1.0" },
          { agent: "codex", version: "1.1.0" },
          { agent: "dsh", version: "0.2.0" },
        ]);
      }
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts current and latest version detection before the updates panel is mounted", async () => {
    render(
      <AgentVersionsProvider>
        <div>Application shell</div>
      </AgentVersionsProvider>,
    );

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("get_agent_tool_status");
      expect(invokeMock).toHaveBeenCalledWith("get_agent_latest_versions");
    });
  });

  it("polls current versions frequently and latest versions on the background cadence", async () => {
    vi.useFakeTimers();
    render(
      <AgentVersionsProvider>
        <div>Application shell</div>
      </AgentVersionsProvider>,
    );
    await act(flushPromises);
    invokeMock.mockClear();

    await act(async () => {
      vi.advanceTimersByTime(AGENT_STATUS_REFRESH_INTERVAL_MS);
      await flushPromises();
    });
    expect(
      invokeMock.mock.calls.filter(([command]) => command === "get_agent_tool_status"),
    ).toHaveLength(1);
    expect(
      invokeMock.mock.calls.filter(([command]) => command === "get_agent_latest_versions"),
    ).toHaveLength(0);

    invokeMock.mockClear();
    await act(async () => {
      vi.advanceTimersByTime(AGENT_LATEST_REFRESH_INTERVAL_MS - AGENT_STATUS_REFRESH_INTERVAL_MS);
      await flushPromises();
    });
    expect(
      invokeMock.mock.calls.filter(([command]) => command === "get_agent_latest_versions"),
    ).toHaveLength(1);
  });

  it("refreshes installed versions when the app regains focus or settings change", async () => {
    render(
      <AgentVersionsProvider>
        <div>Application shell</div>
      </AgentVersionsProvider>,
    );
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_agent_tool_status"));
    invokeMock.mockClear();

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await flushPromises();
    });
    expect(invokeMock).toHaveBeenCalledWith("get_agent_tool_status");
    expect(invokeMock).not.toHaveBeenCalledWith("get_agent_latest_versions");

    invokeMock.mockClear();
    await act(async () => {
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      await flushPromises();
    });
    expect(invokeMock).toHaveBeenCalledWith("get_agent_tool_status");
  });
});
