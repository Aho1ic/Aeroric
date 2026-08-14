import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  invalidateAgentSettingsCache,
  useAgentOptions,
  useAgentSettings,
} from "../hooks/useAgentOptions";
import { APP_SETTINGS_CHANGED_EVENT, type AppSettings } from "../components/app-settings/types";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const settings: AppSettings = {
  claude_path: "",
  claude_gpt55_path: "",
  codex_path: "",
  claude_config_path: "",
  claude_gpt55_config_path: "",
  codex_config_path: "",
  custom_agents: [
    {
      id: "cached-agent",
      label: "Cached Agent",
      path: "/tmp/cached-agent.sh",
      codex_like: false,
      config_lang: "shellscript",
    },
  ],
  agent_label_overrides: {},
  builtin_agent_credentials: {},
  send_shortcut: "enter",
  terminal_shift_enter_newline: false,
};

function Probe({ id }: { id: string }) {
  const options = useAgentOptions();
  const loadedSettings = useAgentSettings();
  return (
    <div data-testid={id}>
      {options.map((option) => option.label).join(",")}
      <span>{loadedSettings ? ":loaded" : ":loading"}</span>
    </div>
  );
}

describe("useAgentOptions settings cache", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invalidateAgentSettingsCache();
  });

  it("deduplicates concurrent loads and reuses the cached Agent list across remounts", async () => {
    let resolveSettings: (value: AppSettings) => void = () => {};
    invokeMock.mockImplementation(
      () =>
        new Promise<AppSettings>((resolve) => {
          resolveSettings = resolve;
        }),
    );

    const first = render(
      <>
        <Probe id="first" />
        <Probe id="second" />
      </>,
    );

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("first")).toHaveTextContent(
      "Claude Code,Codex,DeepSeek Harness:loading",
    );

    await act(async () => {
      resolveSettings(settings);
    });

    await waitFor(() => {
      expect(screen.getByTestId("first")).toHaveTextContent("Cached Agent:loaded");
      expect(screen.getByTestId("second")).toHaveTextContent("Cached Agent:loaded");
    });

    first.unmount();
    invokeMock.mockClear();
    render(<Probe id="remounted" />);

    expect(screen.getByTestId("remounted")).toHaveTextContent("Cached Agent:loaded");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("performs one shared background refresh after settings change events", async () => {
    invokeMock.mockResolvedValue(settings);
    render(
      <>
        <Probe id="first" />
        <Probe id="second" />
      </>,
    );

    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    invokeMock.mockClear();

    window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
  });
});
