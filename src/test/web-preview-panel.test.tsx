import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WebPreviewPanel } from "../components/preview/WebPreviewPanel";
import { I18nProvider } from "../i18n";
import { REMOTE_IDE_COMMAND_TIMEOUT_MS } from "../hooks/useCancellableInvoke";
import type { ListeningPort, RunProcessSnapshot, SshConnection } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

function port(overrides: Partial<ListeningPort>): ListeningPort {
  return {
    port: 5173,
    address: "localhost",
    protocol: "tcp",
    pid: 123,
    processName: "node",
    url: "http://localhost:5173",
    projectContext: "project",
    ...overrides,
  };
}

function renderPanel() {
  render(
    <I18nProvider>
      <WebPreviewPanel projectPath="/tmp/aeroric" width={360} />
    </I18nProvider>,
  );
}

function sshConnection(): SshConnection {
  return {
    id: "conn-2",
    name: "Production",
    host: "prod.example.com",
    port: 22,
    username: "deploy",
    createdAt: 2,
  };
}

function runProcess(overrides: Partial<RunProcessSnapshot>): RunProcessSnapshot {
  return {
    runId: "run-1",
    configId: "dev",
    name: "Dev",
    command: "pnpm dev",
    cwd: "/tmp/aeroric",
    status: "running",
    output: "",
    exitCode: null,
    startedAt: 1,
    finishedAt: null,
    ...overrides,
  };
}

describe("WebPreviewPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(openUrl).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("embeds the most relevant listening port and can switch preview URLs", async () => {
    vi.mocked(invoke).mockResolvedValue([
      port({
        port: 9000,
        pid: 456,
        processName: "service",
        url: "http://localhost:9000",
        projectContext: "project",
      }),
      port({ port: 5173, url: "http://localhost:5173" }),
    ]);

    renderPanel();

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("list_listening_ports", {
        projectPath: "/tmp/aeroric",
      });
    });

    const frame = await screen.findByTitle("Embedded preview");
    expect(frame).toHaveAttribute("src", "http://localhost:5173");

    fireEvent.click(screen.getByRole("button", { name: "Preview" }));

    expect(await screen.findByTitle("Embedded preview")).toHaveAttribute(
      "src",
      "http://localhost:9000",
    );
  });

  it("shows project ports by default and can reveal all listening ports", async () => {
    vi.mocked(invoke).mockResolvedValue([
      port({ port: 5173, url: "http://localhost:5173", projectContext: "project" }),
      port({
        port: 5432,
        pid: 456,
        processName: "postgres",
        url: "http://localhost:5432",
        projectContext: "other",
      }),
    ]);

    renderPanel();

    expect(await screen.findByText("http://localhost:5173")).toBeInTheDocument();
    expect(screen.queryByText("http://localhost:5432")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "All" }));

    expect(await screen.findByText("http://localhost:5432")).toBeInTheDocument();
  });

  it("auto-selects the port advertised by a launched run configuration", async () => {
    const process = runProcess({ output: "Local: http://localhost:5173/" });
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_listening_ports") {
        return Promise.resolve([
          port({ port: 3000, url: "http://localhost:3000", projectContext: "project" }),
          port({ port: 5173, url: "http://localhost:5173", projectContext: "project" }),
        ]);
      }
      if (command === "read_run_process") {
        return Promise.resolve(process);
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <WebPreviewPanel projectPath="/tmp/aeroric" width={360} runProcessTarget={process} />
      </I18nProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTitle("Embedded preview")).toHaveAttribute("src", "http://localhost:5173");
    });
  });

  it("lists remote ports and embeds the SSH preview tunnel URL", async () => {
    const connection = sshConnection();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_list_listening_ports") {
        return Promise.resolve([
          port({ port: 5173, address: "localhost", url: "http://localhost:5173" }),
        ]);
      }
      if (command === "remote_open_preview_tunnel") {
        return Promise.resolve("http://127.0.0.1:61234");
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <WebPreviewPanel
          projectPath="/srv/app"
          width={360}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("remote_list_listening_ports", {
        connection,
        remoteProjectPath: "/srv/app",
      });
    });
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("remote_open_preview_tunnel", {
        connection,
        remoteHost: "127.0.0.1",
        remotePort: 5173,
      });
    });
    expect(await screen.findByTitle("Embedded preview")).toHaveAttribute(
      "src",
      "http://127.0.0.1:61234",
    );
  });

  it("uses the SSH preview tunnel URL when copying or opening remote ports", async () => {
    const connection = sshConnection();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_list_listening_ports") {
        return Promise.resolve([
          port({ port: 8080, address: "0.0.0.0", url: "http://0.0.0.0:8080" }),
        ]);
      }
      if (command === "remote_open_preview_tunnel") {
        return Promise.resolve("http://127.0.0.1:61235");
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <WebPreviewPanel
          projectPath="/srv/app"
          width={360}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    expect(await screen.findByTitle("Embedded preview")).toHaveAttribute(
      "src",
      "http://127.0.0.1:61235",
    );
    vi.mocked(invoke).mockClear();

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("http://127.0.0.1:61235");
    });
    expect(invoke).toHaveBeenCalledWith("remote_open_preview_tunnel", {
      connection,
      remoteHost: "127.0.0.1",
      remotePort: 8080,
    });

    vi.mocked(invoke).mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Open" }));

    await waitFor(() => {
      expect(openUrl).toHaveBeenCalledWith("http://127.0.0.1:61235");
    });
    expect(invoke).toHaveBeenCalledWith("remote_open_preview_tunnel", {
      connection,
      remoteHost: "127.0.0.1",
      remotePort: 8080,
    });
  });

  it("shows a visible timeout when remote port discovery hangs", async () => {
    vi.useFakeTimers();
    const connection = sshConnection();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_list_listening_ports") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <WebPreviewPanel
          projectPath="/srv/app"
          width={360}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(REMOTE_IDE_COMMAND_TIMEOUT_MS);
    });

    expect(screen.getByText(/remote_list_listening_ports.*timed out after 60s/i)).toBeInTheDocument();
  });

  it("shows a visible error when opening the remote preview tunnel fails", async () => {
    const connection = sshConnection();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_list_listening_ports") {
        return Promise.resolve([
          port({ port: 5173, address: "localhost", url: "http://localhost:5173" }),
        ]);
      }
      if (command === "remote_open_preview_tunnel") {
        return Promise.reject(new Error("ssh: connect to host prod.example.com failed"));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <WebPreviewPanel
          projectPath="/srv/app"
          width={360}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    expect(
      await screen.findByText(/ssh: connect to host prod\.example\.com failed/i),
    ).toBeInTheDocument();
    expect(screen.queryByTitle("Embedded preview")).not.toBeInTheDocument();
  });
});
