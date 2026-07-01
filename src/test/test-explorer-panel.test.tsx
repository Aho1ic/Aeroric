import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TestExplorerPanel } from "../components/tests/TestExplorerPanel";
import { I18nProvider } from "../i18n";
import { REMOTE_IDE_COMMAND_TIMEOUT_MS } from "../hooks/useCancellableInvoke";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("TestExplorerPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("passes a scoped Vitest target when file and test name are provided", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "discover_tests") {
        return Promise.resolve({
          profiles: [{ id: "vitest", label: "Vitest", command: "pnpm exec vitest run" }],
        });
      }
      if (command === "run_tests") {
        return Promise.resolve({
          profile: "vitest",
          status: "passed",
          total: 1,
          passed: 1,
          failed: 0,
          tests: [],
          failures: [],
          coverage: null,
          rawOutput: "",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/tmp/aeroric"
          width={320}
          onOpenFailure={vi.fn()}
          onCreateAgentTask={vi.fn()}
        />
      </I18nProvider>,
    );

    fireEvent.change(await screen.findByPlaceholderText("Test file"), {
      target: { value: "/tmp/aeroric/src/test/math.test.ts" },
    });
    fireEvent.change(screen.getByPlaceholderText("Test name"), {
      target: { value: "adds numbers" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("run_tests", {
        projectPath: "/tmp/aeroric",
        profile: "vitest",
        target: {
          filePath: "/tmp/aeroric/src/test/math.test.ts",
          testName: "adds numbers",
        },
        coverage: false,
      });
    });
  });

  it("shows coverage summary from the latest test run", async () => {
    const onTestRunResult = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "discover_tests") {
        return Promise.resolve({
          profiles: [{ id: "vitest", label: "Vitest", command: "pnpm exec vitest run" }],
        });
      }
      if (command === "run_tests") {
        return Promise.resolve({
          profile: "vitest",
          status: "passed",
          total: 2,
          passed: 2,
          failed: 0,
          tests: [],
          failures: [],
          coverage: {
            lines: { covered: 17, total: 20, percent: 85 },
            functions: { covered: 6, total: 8, percent: 75 },
            branches: { covered: 3, total: 6, percent: 50 },
          },
          rawOutput: "",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/tmp/aeroric"
          width={320}
          onOpenFailure={vi.fn()}
          onCreateAgentTask={vi.fn()}
          onTestRunResult={onTestRunResult}
        />
      </I18nProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Run" }));

    await screen.findByText("Lines 85.0%");
    expect(screen.getAllByText("Coverage")).toHaveLength(2);
    expect(screen.getByText("Lines 85.0%")).toBeInTheDocument();
    expect(screen.getByText("Functions 75.0%")).toBeInTheDocument();
    expect(screen.getByText("Branches 50.0%")).toBeInTheDocument();
    expect(onTestRunResult).toHaveBeenCalledWith(
      expect.objectContaining({
        coverage: expect.objectContaining({
          lines: { covered: 17, total: 20, percent: 85 },
        }),
      }),
    );
  });

  it("requests a coverage run from the coverage button", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "discover_tests") {
        return Promise.resolve({
          profiles: [{ id: "vitest", label: "Vitest", command: "pnpm exec vitest run" }],
        });
      }
      if (command === "run_tests") {
        return Promise.resolve({
          profile: "vitest",
          status: "passed",
          total: 1,
          passed: 1,
          failed: 0,
          tests: [],
          failures: [],
          coverage: null,
          rawOutput: "",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/tmp/aeroric"
          width={320}
          onOpenFailure={vi.fn()}
          onCreateAgentTask={vi.fn()}
        />
      </I18nProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Coverage" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("run_tests", {
        projectPath: "/tmp/aeroric",
        profile: "vitest",
        target: null,
        coverage: true,
      });
    });
  });

  it("runs an external editor gutter test request", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "discover_tests") {
        return Promise.resolve({
          profiles: [{ id: "vitest", label: "Vitest", command: "pnpm exec vitest run" }],
        });
      }
      if (command === "run_tests") {
        return Promise.resolve({
          profile: "vitest",
          status: "passed",
          total: 1,
          passed: 1,
          failed: 0,
          tests: [],
          failures: [],
          coverage: null,
          rawOutput: "",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/tmp/aeroric"
          width={320}
          onOpenFailure={vi.fn()}
          onCreateAgentTask={vi.fn()}
          runRequest={{
            id: 1,
            profile: "vitest",
            target: {
              filePath: "/tmp/aeroric/src/math.test.ts",
              testName: "adds numbers",
            },
          }}
        />
      </I18nProvider>,
    );

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("run_tests", {
        projectPath: "/tmp/aeroric",
        profile: "vitest",
        target: {
          filePath: "/tmp/aeroric/src/math.test.ts",
          testName: "adds numbers",
        },
        coverage: false,
      });
    });
    expect(screen.getByPlaceholderText("Test file")).toHaveValue("/tmp/aeroric/src/math.test.ts");
    expect(screen.getByPlaceholderText("Test name")).toHaveValue("adds numbers");
  });

  it("discovers and runs tests through remote commands for SSH projects", async () => {
    const connection = {
      id: "ssh-1",
      name: "prod",
      host: "example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_discover_tests") {
        return Promise.resolve({
          profiles: [{ id: "python", label: "Pytest", command: "remote pytest" }],
        });
      }
      if (command === "remote_run_tests") {
        return Promise.resolve({
          profile: "python",
          status: "passed",
          total: 1,
          passed: 1,
          failed: 0,
          tests: [],
          failures: [],
          coverage: null,
          rawOutput: "",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/srv/app"
          width={320}
          onOpenFailure={vi.fn()}
          onCreateAgentTask={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.change(await screen.findByPlaceholderText("Test file"), {
      target: { value: "/srv/app/tests/test_math.py" },
    });
    fireEvent.change(screen.getByPlaceholderText("Test name"), {
      target: { value: "test_adds_numbers" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("remote_run_tests", {
        connection,
        remoteProjectPath: "/srv/app",
        profile: "python",
        target: {
          filePath: "/srv/app/tests/test_math.py",
          testName: "test_adds_numbers",
        },
        coverage: false,
      });
    });
  });

  it("opens remote test failures with the remote file path", async () => {
    const connection = {
      id: "ssh-1",
      name: "prod",
      host: "example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    const remoteFailure = {
      profile: "python",
      name: "test_adds_numbers",
      file: "/srv/app/tests/test_math.py",
      line: 12,
      column: 5,
      message: "assert 1 == 2",
    };
    const onOpenFailure = vi.fn();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_discover_tests") {
        return Promise.resolve({
          profiles: [{ id: "python", label: "Pytest", command: "remote pytest" }],
        });
      }
      if (command === "remote_run_tests") {
        return Promise.resolve({
          profile: "python",
          status: "failed",
          total: 1,
          passed: 0,
          failed: 1,
          tests: [],
          failures: [remoteFailure],
          coverage: null,
          rawOutput: "assert 1 == 2",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/srv/app"
          width={320}
          onOpenFailure={onOpenFailure}
          onCreateAgentTask={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Run" }));
    fireEvent.click(await screen.findByText(/test_adds_numbers/));

    expect(onOpenFailure).toHaveBeenCalledWith(remoteFailure);
  });

  it("shows a visible timeout when remote test discovery hangs", async () => {
    vi.useFakeTimers();
    const connection = {
      id: "ssh-1",
      name: "prod",
      host: "example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_discover_tests") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/srv/app"
          width={320}
          onOpenFailure={vi.fn()}
          onCreateAgentTask={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(REMOTE_IDE_COMMAND_TIMEOUT_MS);
    });

    expect(screen.getByText(/remote_discover_tests.*timed out after 60s/i)).toBeInTheDocument();
  });

  it("shows a visible timeout when a remote test run hangs", async () => {
    vi.useFakeTimers();
    const connection = {
      id: "ssh-1",
      name: "prod",
      host: "example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_discover_tests") {
        return Promise.resolve({
          profiles: [{ id: "python", label: "Pytest", command: "remote pytest" }],
        });
      }
      if (command === "remote_run_tests") return new Promise(() => {});
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/srv/app"
          width={320}
          onOpenFailure={vi.fn()}
          onCreateAgentTask={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    act(() => {
      vi.advanceTimersByTime(600_000);
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByText(/remote_run_tests.*timed out after 600s/i)).toBeInTheDocument();
  });

  it("shows remote test tool failures without hiding the message", async () => {
    const connection = {
      id: "ssh-1",
      name: "prod",
      host: "example.com",
      port: 22,
      username: "deploy",
      createdAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "remote_discover_tests") {
        return Promise.resolve({
          profiles: [{ id: "python", label: "Pytest", command: "remote pytest" }],
        });
      }
      if (command === "remote_run_tests") {
        return Promise.reject(new Error("pytest: command not found"));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <I18nProvider>
        <TestExplorerPanel
          projectPath="/srv/app"
          width={320}
          onOpenFailure={vi.fn()}
          onCreateAgentTask={vi.fn()}
          remote={{ connection, projectPath: "/srv/app" }}
        />
      </I18nProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Run" }));

    expect(await screen.findByText("Tests failed: pytest: command not found")).toBeInTheDocument();
    expect(screen.queryByText(/Error: pytest/)).not.toBeInTheDocument();
  });
});
