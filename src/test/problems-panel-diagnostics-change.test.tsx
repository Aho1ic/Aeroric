import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { ProblemsPanel } from "../components/problems/ProblemsPanel";
import type { DiagnosticItem } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const diagnostics: DiagnosticItem[] = [
  {
    source: "tsc",
    severity: "error",
    message: "Type mismatch",
    file: "/tmp/aeroric/src/App.tsx",
    line: 2,
    column: 3,
    code: "TS2322",
  },
];

const remoteDiagnostics: DiagnosticItem[] = [
  {
    source: "tsc",
    severity: "error",
    message: "Remote type mismatch",
    file: "/srv/app/src/App.tsx",
    line: 4,
    column: 7,
    code: "TS2322",
  },
];

const connection = {
  id: "ssh-1",
  name: "prod",
  host: "example.com",
  port: 22,
  username: "deploy",
  createdAt: 1,
};

describe("ProblemsPanel diagnostics change", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("reports the latest diagnostics to the parent after a run", async () => {
    const onDiagnosticsChange = vi.fn();
    vi.mocked(invoke).mockResolvedValue({
      profile: "typescript",
      diagnostics,
      rawOutput: "",
    });

    render(
      <I18nProvider>
        <ProblemsPanel
          projectPath="/tmp/aeroric"
          width={320}
          onOpenDiagnostic={vi.fn()}
          onCreateAgentTask={vi.fn()}
          onDiagnosticsChange={onDiagnosticsChange}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => {
      expect(onDiagnosticsChange).toHaveBeenCalledWith(diagnostics);
    });
  });

  it("runs diagnostics through the remote command for SSH projects", async () => {
    vi.mocked(invoke).mockResolvedValue({
      profile: "typescript",
      diagnostics,
      rawOutput: "",
    });

    render(
      <I18nProvider>
        <ProblemsPanel
          projectPath="/srv/app"
          width={320}
          onOpenDiagnostic={vi.fn()}
          onCreateAgentTask={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_run_diagnostics", {
        connection,
        remoteProjectPath: "/srv/app",
        profile: "typescript",
      });
    });
  });

  it("opens remote diagnostics with the remote file path", async () => {
    const onOpenDiagnostic = vi.fn();
    vi.mocked(invoke).mockResolvedValue({
      profile: "typescript",
      diagnostics: remoteDiagnostics,
      rawOutput: "",
    });

    render(
      <I18nProvider>
        <ProblemsPanel
          projectPath="/srv/app"
          width={320}
          onOpenDiagnostic={onOpenDiagnostic}
          onCreateAgentTask={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    fireEvent.click(await screen.findByText(/Remote type mismatch/));

    expect(onOpenDiagnostic).toHaveBeenCalledWith(remoteDiagnostics[0]);
  });

  it("shows remote diagnostic tool failures without hiding the message", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("ruff: command not found"));

    render(
      <I18nProvider>
        <ProblemsPanel
          projectPath="/srv/app"
          width={320}
          onOpenDiagnostic={vi.fn()}
          onCreateAgentTask={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(await screen.findByText("Diagnostics failed: ruff: command not found")).toBeInTheDocument();
    expect(screen.queryByText(/Error: ruff/)).not.toBeInTheDocument();
  });
});
