import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DebugPanel } from "../components/debug/DebugPanel";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderAttachPanel() {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "read_debug_configs") {
      return Promise.resolve({ version: 1, configs: [] });
    }
    if (command === "start_debug_config") {
      return Promise.resolve({
        debugId: "debug-attach",
        configId: "attach-node",
        name: "Attach Node",
        program: "127.0.0.1:9229",
        cwd: "/repo",
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
      <DebugPanel projectPath="/repo" width={360} onOpenLocation={vi.fn()} />
    </I18nProvider>,
  );
}

const remoteConnection = {
  id: "ssh-1",
  name: "remote",
  host: "127.0.0.1",
  port: 22,
  username: "dev",
  createdAt: 1,
};

function renderRemoteAttachPanel() {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "remote_read_debug_configs") {
      return Promise.resolve({ version: 1, configs: [] });
    }
    if (command === "remote_start_debug_config") {
      return Promise.resolve({
        debugId: "debug-remote-python",
        configId: "attach-remote-python",
        name: "Attach Remote Python",
        program: "127.0.0.1:5678 via remote",
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
      <DebugPanel
        projectPath="/srv/app"
        width={360}
        onOpenLocation={vi.fn()}
        remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
      />
    </I18nProvider>,
  );
}

describe("DebugPanel attach UI", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts a Node attach session with host and port", async () => {
    const user = userEvent.setup();
    renderAttachPanel();

    await screen.findByText("No debug configurations yet.");
    await user.type(screen.getByLabelText("Name"), "Attach Node");
    await user.click(screen.getByRole("button", { name: "Attach" }));

    expect(screen.queryByLabelText("Program")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Host")).toHaveValue("127.0.0.1");
    expect(screen.getByLabelText("Port")).toHaveValue("9229");

    const controls = within(screen.getByRole("group", { name: "Debug controls" }));
    await user.click(controls.getByRole("button", { name: "Attach" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("start_debug_config", {
        projectPath: "/repo",
        config: expect.objectContaining({
          id: "attach-node",
          name: "Attach Node",
          type: "node",
          request: "attach",
          program: "",
          cwd: ".",
          attachHost: "127.0.0.1",
          attachPort: 9229,
        }),
      });
    });
  });

  it("starts a Python attach session with host and port", async () => {
    const user = userEvent.setup();
    renderAttachPanel();

    await screen.findByText("No debug configurations yet.");
    await user.type(screen.getByLabelText("Name"), "Attach Python");
    await user.click(screen.getByRole("button", { name: "Python" }));
    await user.click(screen.getByRole("button", { name: "Attach" }));

    expect(screen.queryByLabelText("Program")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Python" })).not.toBeDisabled();

    const controls = within(screen.getByRole("group", { name: "Debug controls" }));
    await user.click(controls.getByRole("button", { name: "Attach" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("start_debug_config", {
        projectPath: "/repo",
        config: expect.objectContaining({
          id: "attach-python",
          name: "Attach Python",
          type: "python",
          request: "attach",
          program: "",
          cwd: ".",
          attachHost: "127.0.0.1",
          attachPort: 9229,
        }),
      });
    });
  });

  it("starts a remote Python attach session through the remote debug command", async () => {
    const user = userEvent.setup();
    renderRemoteAttachPanel();

    await screen.findByText("No debug configurations yet.");
    expect(screen.getByRole("button", { name: "Node" })).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Launch" })).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Python" })).not.toBeDisabled();
    expect(
      within(screen.getByRole("group", { name: "Request" })).getByRole("button", {
        name: "Attach",
      }),
    ).not.toBeDisabled();
    expect(screen.getByLabelText("Port")).toHaveValue("5678");

    await user.type(screen.getByLabelText("Name"), "Attach Remote Python");
    const controls = within(screen.getByRole("group", { name: "Debug controls" }));
    await user.click(controls.getByRole("button", { name: "Attach" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_start_debug_config", {
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        projectPath: "/srv/app",
        config: expect.objectContaining({
          id: "attach-remote-python",
          name: "Attach Remote Python",
          type: "python",
          request: "attach",
          program: "",
          cwd: ".",
          attachHost: "127.0.0.1",
          attachPort: 5678,
        }),
      });
    });
  });

  it("starts a remote Node attach session through the remote debug command", async () => {
    const user = userEvent.setup();
    renderRemoteAttachPanel();

    await screen.findByText("No debug configurations yet.");
    await user.click(screen.getByRole("button", { name: "Node" }));
    expect(screen.getByLabelText("Port")).toHaveValue("9229");

    await user.type(screen.getByLabelText("Name"), "Attach Remote Node");
    const controls = within(screen.getByRole("group", { name: "Debug controls" }));
    await user.click(controls.getByRole("button", { name: "Attach" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_start_debug_config", {
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        projectPath: "/srv/app",
        config: expect.objectContaining({
          id: "attach-remote-node",
          name: "Attach Remote Node",
          type: "node",
          request: "attach",
          program: "",
          cwd: ".",
          attachHost: "127.0.0.1",
          attachPort: 9229,
        }),
      });
    });
  });

  it("starts a remote Node launch session through the remote debug command", async () => {
    const user = userEvent.setup();
    renderRemoteAttachPanel();

    await screen.findByText("No debug configurations yet.");
    await user.click(screen.getByRole("button", { name: "Node" }));
    const requestGroup = within(screen.getByRole("group", { name: "Request" }));
    expect(requestGroup.getByRole("button", { name: "Launch" })).not.toBeDisabled();
    await user.click(requestGroup.getByRole("button", { name: "Launch" }));

    await user.type(screen.getByLabelText("Name"), "Remote Vitest");
    await user.type(screen.getByLabelText("Program"), "node_modules/vitest/vitest.mjs");
    const controls = within(screen.getByRole("group", { name: "Debug controls" }));
    await user.click(controls.getByRole("button", { name: "Start" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_start_debug_config", {
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        projectPath: "/srv/app",
        config: expect.objectContaining({
          id: "remote-vitest",
          name: "Remote Vitest",
          type: "node",
          request: "launch",
          program: "node_modules/vitest/vitest.mjs",
          cwd: ".",
        }),
      });
    });
  });

  it("starts a remote Python launch session through the remote debug command", async () => {
    const user = userEvent.setup();
    renderRemoteAttachPanel();

    await screen.findByText("No debug configurations yet.");
    const requestGroup = within(screen.getByRole("group", { name: "Request" }));
    expect(requestGroup.getByRole("button", { name: "Launch" })).not.toBeDisabled();
    await user.click(requestGroup.getByRole("button", { name: "Launch" }));

    await user.type(screen.getByLabelText("Name"), "Remote Python Script");
    await user.type(screen.getByLabelText("Program"), "app/main.py");
    const controls = within(screen.getByRole("group", { name: "Debug controls" }));
    await user.click(controls.getByRole("button", { name: "Start" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_start_debug_config", {
        connection: remoteConnection,
        remoteProjectPath: "/srv/app",
        projectPath: "/srv/app",
        config: expect.objectContaining({
          id: "remote-python-script",
          name: "Remote Python Script",
          type: "python",
          request: "launch",
          program: "app/main.py",
          cwd: ".",
        }),
      });
    });
  });

  it("shows a visible timeout when remote debug config loading hangs", async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_debug_configs") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <DebugPanel
          projectPath="/srv/app"
          width={360}
          onOpenLocation={vi.fn()}
          remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByText(/remote_read_debug_configs.*timed out after 60s/i)).toBeInTheDocument();
  });

  it("shows a visible timeout when remote debug start hangs", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_debug_configs") {
        return Promise.resolve({ version: 1, configs: [] });
      }
      if (command === "remote_start_debug_config") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <DebugPanel
          projectPath="/srv/app"
          width={360}
          onOpenLocation={vi.fn()}
          remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    await screen.findByText("No debug configurations yet.");
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "Attach Remote Python" },
    });

    vi.useFakeTimers();
    const controls = within(screen.getByRole("group", { name: "Debug controls" }));
    fireEvent.click(controls.getByRole("button", { name: "Attach" }));

    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByText(/remote_start_debug_config.*timed out after 60s/i)).toBeInTheDocument();
  });

  it("shows remote debug tool failures without an Error prefix", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_read_debug_configs") {
        return Promise.resolve({ version: 1, configs: [] });
      }
      if (command === "remote_start_debug_config") {
        return Promise.reject(new Error("debugpy: command not found"));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <DebugPanel
          projectPath="/srv/app"
          width={360}
          onOpenLocation={vi.fn()}
          remote={{ connection: remoteConnection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    await screen.findByText("No debug configurations yet.");
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "Attach Remote Python" },
    });
    const controls = within(screen.getByRole("group", { name: "Debug controls" }));
    fireEvent.click(controls.getByRole("button", { name: "Attach" }));

    expect(
      await screen.findByText("Debug session failed: debugpy: command not found"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Error: debugpy/)).not.toBeInTheDocument();
  });
});
