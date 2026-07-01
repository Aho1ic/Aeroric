import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RunConfigurationsPanel } from "../components/run/RunConfigurationsPanel";
import { I18nProvider } from "../i18n";
import { REMOTE_IDE_COMMAND_TIMEOUT_MS } from "../hooks/useCancellableInvoke";
import type { RunConfig, SshConnection } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const remoteConnection: SshConnection = {
  id: "ssh-1",
  name: "remote",
  host: "127.0.0.1",
  port: 22,
  username: "dev",
  createdAt: 1,
};

function renderRemotePanel(configs: RunConfig[] = []) {
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "remote_read_run_configs") {
      return Promise.resolve({ version: 1, configs });
    }
    if (command === "remote_write_run_configs") {
      return Promise.resolve((args as { document: unknown }).document);
    }
    if (command === "remote_start_run_config") {
      return Promise.resolve({
        runId: "run-remote",
        configId: "dev",
        name: "Dev Server",
        command: "pnpm dev",
        cwd: "/srv/app",
        status: "running",
        output: "",
        startedAt: 1,
      });
    }
    if (command === "remote_start_debug_config") {
      return Promise.resolve({
        debugId: "debug-remote",
        configId: "debug",
        name: "Debug App",
        program: "/srv/app/src/index.js",
        cwd: "/srv/app",
        status: "running",
        output: "",
        callStack: [],
        scopes: [],
        startedAt: 1,
      });
    }
    return Promise.resolve(undefined);
  });

  render(
    <I18nProvider>
      <RunConfigurationsPanel
        projectPath="/srv/app"
        width={360}
        remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
      />
    </I18nProvider>,
  );
}

describe("RunConfigurationsPanel remote mode", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("reads and writes remote run configs", async () => {
    const user = userEvent.setup();
    renderRemotePanel();

    await screen.findByText("No run configurations yet.");
    await user.type(screen.getByLabelText("Name"), "Dev Server");
    await user.type(screen.getByLabelText("Command"), "pnpm dev");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_read_run_configs", {
        projectPath: "/srv/app",
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
      });
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_write_run_configs", {
        projectPath: "/srv/app",
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        document: {
          version: 1,
          configs: [
            {
              id: "dev-server",
              name: "Dev Server",
              type: "shell",
              command: "pnpm dev",
              cwd: ".",
              env: {},
            },
          ],
        },
      });
    });
  });

  it("starts shell run configs through the remote run command", async () => {
    const user = userEvent.setup();
    const config: RunConfig = {
      id: "dev",
      name: "Dev Server",
      type: "shell",
      command: "pnpm dev",
      cwd: ".",
      env: { PORT: "5173" },
    };
    renderRemotePanel([config]);

    await screen.findByText("Dev Server");
    await user.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_start_run_config", {
        projectPath: "/srv/app",
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        config,
      });
    });
  });

  it("starts debug run configs through the remote debug command", async () => {
    const user = userEvent.setup();
    renderRemotePanel([
      {
        id: "debug",
        name: "Debug App",
        type: "debug",
        debugType: "node",
        program: "src/index.js",
        cwd: ".",
        args: ["--watch"],
        env: { NODE_ENV: "test" },
        breakpoints: [{ file: "src/index.js", line: 12, column: 1 }],
      },
    ]);

    await screen.findByText("Debug App");
    await user.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_start_debug_config", {
        projectPath: "/srv/app",
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        config: {
          id: "debug",
          name: "Debug App",
          type: "node",
          program: "src/index.js",
          cwd: ".",
          args: ["--watch"],
          env: { NODE_ENV: "test" },
          breakpoints: [{ file: "src/index.js", line: 12, column: 1 }],
        },
      });
    });
  });

  it("shows a visible timeout when remote run config loading hangs", async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_run_configs") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <RunConfigurationsPanel
          projectPath="/srv/app"
          width={360}
          remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(REMOTE_IDE_COMMAND_TIMEOUT_MS);
    });

    expect(screen.getByText(/remote_read_run_configs.*timed out after 60s/i)).toBeInTheDocument();
  });

  it("shows a visible timeout when remote shell run start hangs", async () => {
    const config: RunConfig = {
      id: "dev",
      name: "Dev Server",
      type: "shell",
      command: "pnpm dev",
      cwd: ".",
      env: {},
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_run_configs") {
        return Promise.resolve({ version: 1, configs: [config] });
      }
      if (command === "remote_start_run_config") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <RunConfigurationsPanel
          projectPath="/srv/app"
          width={360}
          remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    await screen.findByText("Dev Server");

    vi.useFakeTimers();
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    act(() => {
      vi.advanceTimersByTime(180_000);
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByText(/remote_start_run_config.*timed out after 180s/i)).toBeInTheDocument();
  });

  it("shows remote debug run tool failures without an Error prefix", async () => {
    const config: RunConfig = {
      id: "debug",
      name: "Debug App",
      type: "debug",
      debugType: "node",
      program: "src/index.js",
      cwd: ".",
      args: [],
      env: {},
      breakpoints: [],
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_run_configs") {
        return Promise.resolve({ version: 1, configs: [config] });
      }
      if (command === "remote_start_debug_config") {
        return Promise.reject(new Error("node: command not found"));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <RunConfigurationsPanel
          projectPath="/srv/app"
          width={360}
          remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    await screen.findByText("Debug App");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(
      await screen.findByText("Run configuration failed: node: command not found"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Error: node/)).not.toBeInTheDocument();
  });
});
