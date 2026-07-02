import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "../components/SettingsDialog";
import { I18nProvider } from "../i18n";
import { REMOTE_IDE_COMMAND_TIMEOUT_MS } from "../hooks/useCancellableInvoke";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderSettingsDialog(
  onClose = vi.fn(),
  remote?: React.ComponentProps<typeof SettingsDialog>["remote"],
) {
  render(
    <I18nProvider>
      <SettingsDialog
        projectPath={remote ? "/srv/app" : "/tmp/aeroric"}
        onClose={onClose}
        remote={remote}
      />
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
      file_browser_sort: { field: "modified", direction: "desc" },
      sftp_sort: { field: "modified", direction: "desc" },
    },
  };
}

describe("SettingsDialog", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
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
          file_browser_sort: { field: "modified", direction: "desc" },
          sftp_sort: { field: "modified", direction: "desc" },
        },
      },
    });
  });

  it("saves default file browser and SFTP sort preferences", async () => {
    const onClose = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") return Promise.resolve(projectConfig(false));
      if (command === "write_project_config") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderSettingsDialog(onClose);

    await screen.findByRole("checkbox", { name: /Format on save/ });
    await userEvent.selectOptions(screen.getByLabelText("File browser default sort"), "name:asc");
    await userEvent.selectOptions(screen.getByLabelText("SFTP default sort"), "modified:asc");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("write_project_config", {
      projectPath: "/tmp/aeroric",
      config: expect.objectContaining({
        editor: {
          format_on_save: false,
          file_browser_sort: { field: "name", direction: "asc" },
          sftp_sort: { field: "modified", direction: "asc" },
        },
      }),
    });
  });

  it("uses remote project config commands for SSH project settings", async () => {
    const onClose = vi.fn();
    const connection = {
      id: "conn-2",
      name: "Production",
      host: "prod.example.com",
      port: 22,
      username: "deploy",
      createdAt: 2,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_project_config") return Promise.resolve(projectConfig(false));
      if (command === "remote_write_project_config") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderSettingsDialog(onClose, { connection, projectPath: "/srv/app" });

    const formatOnSave = await screen.findByRole("checkbox", { name: /Format on save/ });
    await userEvent.click(formatOnSave);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_read_project_config", {
      connection,
      remoteProjectPath: "/srv/app",
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_write_project_config", {
      connection,
      remoteProjectPath: "/srv/app",
      config: expect.objectContaining({
        editor: expect.objectContaining({ format_on_save: true }),
      }),
    });
  });

  it("shows a visible timeout when remote project settings loading hangs", async () => {
    vi.useFakeTimers();
    const connection = {
      id: "conn-2",
      name: "Production",
      host: "prod.example.com",
      port: 22,
      username: "deploy",
      createdAt: 2,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_project_config") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderSettingsDialog(vi.fn(), { connection, projectPath: "/srv/app" });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(REMOTE_IDE_COMMAND_TIMEOUT_MS);
    });

    expect(
      screen.getByText(/remote_read_project_config.*timed out after 60s/i),
    ).toBeInTheDocument();
  });

  it("shows a visible timeout when remote project settings saving hangs", async () => {
    const connection = {
      id: "conn-2",
      name: "Production",
      host: "prod.example.com",
      port: 22,
      username: "deploy",
      createdAt: 2,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_project_config") return Promise.resolve(projectConfig(false));
      if (command === "remote_write_project_config") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderSettingsDialog(vi.fn(), { connection, projectPath: "/srv/app" });

    await screen.findByRole("checkbox", { name: /Format on save/ });
    vi.useFakeTimers();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(REMOTE_IDE_COMMAND_TIMEOUT_MS);
    });

    expect(
      screen.getByText(/remote_write_project_config.*timed out after 60s/i),
    ).toBeInTheDocument();
  });

  it("shows remote project settings save failures without an Error prefix", async () => {
    const connection = {
      id: "conn-2",
      name: "Production",
      host: "prod.example.com",
      port: 22,
      username: "deploy",
      createdAt: 2,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_project_config") return Promise.resolve(projectConfig(false));
      if (command === "remote_write_project_config") {
        return Promise.reject(new Error("Permission denied writing /srv/app/.aeroric/config.toml"));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderSettingsDialog(vi.fn(), { connection, projectPath: "/srv/app" });

    await screen.findByRole("checkbox", { name: /Format on save/ });
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(
      await screen.findByText("Permission denied writing /srv/app/.aeroric/config.toml"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Error: Permission denied/)).not.toBeInTheDocument();
  });
});
