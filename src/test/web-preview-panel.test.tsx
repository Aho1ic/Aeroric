import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WebPreviewPanel } from "../components/preview/WebPreviewPanel";
import { I18nProvider } from "../i18n";
import type { ListeningPort, RunProcessSnapshot } from "../types";

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
        <WebPreviewPanel
          projectPath="/tmp/aeroric"
          width={360}
          runProcessTarget={process}
        />
      </I18nProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTitle("Embedded preview")).toHaveAttribute(
        "src",
        "http://localhost:5173",
      );
    });
  });
});
