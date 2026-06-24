import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "../components/SettingsDialog";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderSettingsDialog(onClose = vi.fn()) {
  render(
    <I18nProvider>
      <SettingsDialog projectPath="/tmp/aeroric" onClose={onClose} />
    </I18nProvider>,
  );
}

function projectConfig(formatOnSave: boolean) {
  return {
    agent: {
      default: "claude",
      default_permission_mode: "ask",
      prompt_prefix: "prefix",
    },
    git: {
      commit_prompt: "commit",
      commit_message_timeout_secs: 15,
    },
    editor: {
      format_on_save: formatOnSave,
    },
  };
}

describe("SettingsDialog", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("saves the format-on-save editor setting without dropping project config", async () => {
    const onClose = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") return Promise.resolve(projectConfig(false));
      if (command === "write_project_config") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderSettingsDialog(onClose);

    const formatOnSave = await screen.findByRole("checkbox", { name: /Format on save/ });
    expect(formatOnSave).not.toBeChecked();

    await userEvent.click(formatOnSave);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    const writeCall = vi
      .mocked(invoke)
      .mock.calls.find(([command]) => command === "write_project_config");
    expect(writeCall?.[1]).toEqual({
      projectPath: "/tmp/aeroric",
      config: {
        agent: {
          default: "claude",
          default_permission_mode: "ask",
          prompt_prefix: "prefix",
        },
        git: {
          commit_prompt: "commit",
          commit_message_timeout_secs: 15,
        },
        editor: {
          format_on_save: true,
        },
      },
    });
  });
});
