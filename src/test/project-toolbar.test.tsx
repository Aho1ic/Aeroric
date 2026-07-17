import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import userEvent from "@testing-library/user-event";
import React, { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { DiagnosticItem, Project, Task } from "../types";
import { ProjectPage } from "../components/ProjectPage";
import { RightToolbar } from "../components/RightToolbar";
import { ToastProvider } from "../components/Toast";
import { FILE_VIEWER_COMMAND_EVENT } from "../components/file-viewer/editorCommandEvents";

const mockState = vi.hoisted(() => ({
  throwSearchPanel: false,
}));

type RemotePanelMockProps = {
  remote?: {
    connection: { id: string; remotePath?: string };
    projectPath: string;
  };
};

type RemoteConnectionMockProps = {
  remote?: { id: string; remotePath?: string };
  remoteConnection?: { id: string; remotePath?: string };
  remoteProjectPath?: string;
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({}),
}));

vi.mock("../components/NewTaskView", () => ({
  NewTaskView: ({
    onSubmit,
    draftPrompt = "",
  }: {
    onSubmit?: (task: {
      prompt: string;
      agent: "claude";
      permissionMode: "ask";
      images: string[];
      texts: string[];
      immediate: boolean;
      launchMode: "local";
      baseBranch: string;
    }) => void;
    draftPrompt?: string;
  }) => (
    <button
      onClick={() =>
        onSubmit?.({
          prompt: draftPrompt,
          agent: "claude",
          permissionMode: "ask",
          images: [],
          texts: [],
          immediate: true,
          launchMode: "local",
          baseBranch: "",
        })
      }
    >
      Start Terminal
    </button>
  ),
}));

vi.mock("../components/RunningView", () => ({
  RunningView: () => <div>running task</div>,
}));

vi.mock("../components/ProjectRail", () => ({
  ProjectRail: () => <nav>rail</nav>,
}));

vi.mock("../components/ssh/SshWorkspace", () => ({
  SshWorkspace: ({ remoteConnection }: RemoteConnectionMockProps) => (
    <div data-testid="ssh-workspace" data-remote-connection={remoteConnection?.id ?? ""}>
      ssh workspace
    </div>
  ),
}));

vi.mock("../components/FileExplorer", () => ({
  FileExplorer: ({
    onFileSelect,
    remote,
  }: {
    onFileSelect: (path: string, name: string) => void;
  } & RemotePanelMockProps) => (
    <button
      data-testid="file-explorer-panel"
      data-remote-project={remote?.projectPath ?? ""}
      onClick={() => onFileSelect("/srv/app/run.py", "run.py")}
    >
      run.py
    </button>
  ),
}));

vi.mock("../components/FileViewer", () => ({
  FileViewer: ({
    projectPath,
    onRunPythonFile,
    onRunTestTarget,
    onDebugTestTarget,
    diagnostics = [],
  }: {
    projectPath: string;
    onRunPythonFile?: (path: string) => void;
    onRunTestTarget?: (target: { filePath: string; line: number; testName: string | null }) => void;
    onDebugTestTarget?: (target: {
      filePath: string;
      line: number;
      testName: string | null;
    }) => void;
    diagnostics?: DiagnosticItem[];
  }) => (
    <>
      <span data-testid="editor-diagnostic-count">{diagnostics.length}</span>
      <button title="Run current file" onClick={() => onRunPythonFile?.("/srv/app/run.py")}>
        run file
      </button>
      <button
        title="Run test adds numbers"
        onClick={() =>
          onRunTestTarget?.({
            filePath: "/srv/app/src/math.test.ts",
            line: 4,
            testName: "adds numbers",
          })
        }
      >
        run test
      </button>
      <button
        title="Debug test adds numbers"
        onClick={() =>
          onDebugTestTarget?.({
            filePath: `${projectPath}/src/math.test.ts`,
            line: 4,
            testName: "adds numbers",
          })
        }
      >
        debug test
      </button>
    </>
  ),
}));

vi.mock("../components/ssh/SshTerminalPanel", async () => {
  const ReactModule = await import("react");
  return {
    SshTerminalPanel: ReactModule.forwardRef(function MockSshTerminalPanel(
      props: { active: boolean },
      ref,
    ) {
      ReactModule.useImperativeHandle(ref, () => ({ sendCommand: () => {} }));
      return props.active ? <div data-testid="ssh-terminal">ssh terminal</div> : null;
    }),
  };
});

vi.mock("../components/docker/DockerServiceView", () => ({
  DockerServiceView: ({ remote }: RemoteConnectionMockProps) => (
    <div data-testid="docker-view" data-remote-connection={remote?.id ?? ""}>
      docker
    </div>
  ),
}));

vi.mock("../components/GitChanges", () => ({
  GitChanges: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="git-changes-panel" data-remote-project={remote?.projectPath ?? ""}>
      git changes
    </div>
  ),
}));

vi.mock("../components/GitHistory", () => ({
  GitHistory: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="git-history-panel" data-remote-project={remote?.projectPath ?? ""}>
      git history
    </div>
  ),
}));

vi.mock("../components/git-advanced/GitAdvancedPanel", () => ({
  GitAdvancedPanel: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="git-advanced-panel" data-remote-project={remote?.projectPath ?? ""}>
      git advanced
    </div>
  ),
}));

vi.mock("../components/search/SearchPanel", () => ({
  SearchPanel: ({ remote }: RemotePanelMockProps) => {
    if (mockState.throwSearchPanel) {
      throw new Error("Search panel crashed");
    }
    return (
      <div data-testid="search-panel" data-remote-project={remote?.projectPath ?? ""}>
        search
      </div>
    );
  },
}));

vi.mock("../components/problems/ProblemsPanel", () => ({
  ProblemsPanel: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="problems-panel" data-remote-project={remote?.projectPath ?? ""}>
      problems
    </div>
  ),
}));

vi.mock("../components/tests/TestExplorerPanel", () => ({
  TestExplorerPanel: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="tests-panel" data-remote-project={remote?.projectPath ?? ""}>
      tests
    </div>
  ),
}));

vi.mock("../components/debug/DebugPanel", () => ({
  DebugPanel: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="debug-panel" data-remote-project={remote?.projectPath ?? ""}>
      debug
    </div>
  ),
}));

vi.mock("../components/run/RunConfigurationsPanel", () => ({
  RunConfigurationsPanel: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="run-panel" data-remote-project={remote?.projectPath ?? ""}>
      run
    </div>
  ),
}));

vi.mock("../components/preview/WebPreviewPanel", () => ({
  WebPreviewPanel: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="preview-panel" data-remote-project={remote?.projectPath ?? ""}>
      preview
    </div>
  ),
}));

vi.mock("../components/sftp/SftpPanel", () => ({
  SftpPanel: () => <div data-testid="sftp-panel">sftp</div>,
}));

vi.mock("../components/database/DatabaseView", () => ({
  DatabaseView: ({ remoteConnection, remoteProjectPath }: RemoteConnectionMockProps) => (
    <div
      data-testid="database-view"
      data-remote-connection={remoteConnection?.id ?? ""}
      data-remote-project={remoteProjectPath ?? ""}
    >
      database
    </div>
  ),
}));

vi.mock("../components/notebook/NotebookPanel", () => ({
  NotebookPanel: () => <div data-testid="notes-panel">notes</div>,
}));

vi.mock("../components/SettingsDialog", () => ({
  SettingsDialog: ({ remote }: RemotePanelMockProps) => (
    <div data-testid="settings-dialog" data-remote-project={remote?.projectPath ?? ""}>
      settings
    </div>
  ),
}));

vi.mock("../components/ShellTerminalPanel", async () => {
  const ReactModule = await import("react");
  return {
    deriveShellTerminalFontSize: (size: number) => size,
    ShellTerminalPanel: ReactModule.forwardRef(function MockShellTerminalPanel(
      props: { visible: boolean },
      ref,
    ) {
      ReactModule.useImperativeHandle(ref, () => ({ sendCommand: () => {} }));
      return props.visible ? <div data-testid="shell-terminal">terminal</div> : null;
    }),
  };
});

function localProject(): Project {
  return {
    id: "project-1",
    name: "aeroric",
    path: "/tmp/aeroric",
    lastOpenedAt: 1,
  };
}

function sshProject(): Project {
  return {
    id: "project-ssh",
    name: "remote-app",
    path: "ssh://conn-2/srv/app",
    location: {
      kind: "ssh",
      connectionId: "conn-2",
      remotePath: "/srv/app",
    },
    lastOpenedAt: 1,
  };
}

function savedLianyunProject(): Project {
  return {
    id: "1781590903518",
    name: "lianyun",
    path: "ssh://1781590902568/home",
    location: {
      kind: "ssh",
      connectionId: "1781590902568",
      remotePath: "/home",
    },
    lastOpenedAt: 1782717435393,
  };
}

function projectPageProps(
  project: Project = localProject(),
): React.ComponentProps<typeof ProjectPage> {
  return {
    project,
    visible: true,
    allProjects: [project],
    otherProjects: [],
    tasks: [],
    getTaskRestoreState: () => ({}),
    taskRunCounts: {},
    selectedTaskId: null,
    isNewTask: true,
    onNewTask: vi.fn(),
    onSelectTask: vi.fn(),
    onDeleteTask: vi.fn(),
    onDeleteTasks: vi.fn(),
    onDeleteAllTasks: vi.fn(),
    onToggleTaskStar: vi.fn(),
    onRenameTask: vi.fn(),
    onGenerateTaskName: vi.fn(),
    onSubmitTask: vi.fn(),
    onRunTodoTask: vi.fn(),
    onUpdateTodo: vi.fn(),
    onCancelTask: vi.fn(),
    onResumeTask: vi.fn(),
    onMergeWorktree: vi.fn(),
    onDiscardWorktree: vi.fn(),
    onReconnectTask: vi.fn(),
    onMarkTaskDone: vi.fn(),
    onInput: vi.fn(),
    onResize: vi.fn(),
    onRegisterTerminal: vi.fn(),
    onTerminalReady: vi.fn(),
    onSnapshot: vi.fn(),
    onBack: vi.fn(),
    onSwitchProject: vi.fn(),
    onReorderProjects: vi.fn(),
    onOpen: vi.fn(),
    themeVariant: "light",
    themeMode: "light",
    systemPrefersDark: false,
    onThemeModeChange: vi.fn(),
    onToggleTheme: vi.fn(),
    terminalFontSize: 11,
    onTerminalFontSizeChange: vi.fn(),
    taskDisplayWindow: 3,
    onTaskDisplayWindowChange: vi.fn(),
    attentionBadge: true,
    onAttentionBadgeChange: vi.fn(),
    sftpLocalDefaultPath: "/Users/macbook/Downloads/同步空间",
    onSftpLocalDefaultPathChange: vi.fn(),
    uiFontFamily: "sans-serif",
    onUiFontFamilyChange: vi.fn(),
    monoFontFamily: "monospace",
    onMonoFontFamilyChange: vi.fn(),
    sshConnections: [
      {
        id: "conn-1",
        name: "Staging",
        host: "staging.example.com",
        port: 22,
        username: "deploy",
        createdAt: 1,
      },
      {
        id: "conn-2",
        name: "Production",
        host: "prod.example.com",
        port: 22,
        username: "deploy",
        remotePath: "/srv/app",
        createdAt: 2,
      },
      {
        id: "1781590902568",
        name: "lianyun",
        host: "192.168.0.182",
        port: 22,
        username: "root",
        remotePath: "/home",
        createdAt: 1781590902568,
        lastConnectedAt: 1782717435401,
      },
    ],
    onSshConnectionsChange: vi.fn(),
    condaEnvironments: [],
    selectedCondaEnvPath: null,
    onSelectedCondaEnvPathChange: vi.fn(),
  };
}

function runningTask(projectId: string): Task {
  return {
    id: `${projectId}-task-1`,
    projectId,
    prompt: "Run diagnostics",
    agent: "claude",
    permissionMode: "ask",
    status: "running",
    createdAt: 1,
  };
}

function projectPagePropsWithWorkspace(project: Project = localProject()) {
  const props = projectPageProps(project);
  const task = runningTask(project.id);
  return {
    ...props,
    tasks: [task],
    selectedTaskId: task.id,
    isNewTask: false,
  };
}

describe("ProjectPage right toolbar", () => {
  beforeEach(() => {
    mockState.throwSearchPanel = false;
    window.localStorage.clear();
  });

  it("uses shadcn-like selected icon button styling for the active toolbar item", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPagePropsWithWorkspace()} />
      </I18nProvider>,
    );

    const sshButton = screen.getByTitle("SSH");
    await user.click(sshButton);

    expect(sshButton).toHaveStyle({
      width: "36px",
      height: "36px",
      background: "var(--accent-subtle)",
      color: "var(--accent-strong)",
    });
    expect(sshButton.style.border).toBe("1px solid var(--accent-soft)");
    expect(sshButton).toHaveAttribute("aria-pressed", "true");
  });

  it("renders every right toolbar button with stable shadcn icon sizing", () => {
    render(
      <I18nProvider>
        <RightToolbar
          activePanel={null}
          onToggle={vi.fn()}
          terminalActive={false}
          onToggleTerminal={vi.fn()}
          onOpenSettings={vi.fn()}
        />
      </I18nProvider>,
    );

    for (const title of [
      "File Explorer",
      "Git Changes",
      "Git History",
      "Git Advanced",
      "SSH",
      "SFTP",
      "Database",
      "Quick Notes",
      "Docker",
      "Terminal",
      "Search",
      "Settings",
    ]) {
      expect(screen.getByTitle(title)).toHaveStyle({
        width: "36px",
        height: "36px",
      });
    }

    for (const title of [
      "Problems",
      "Test Explorer",
      "Debug",
      "Run Configurations",
      "Web Preview",
    ]) {
      expect(screen.queryByTitle(title)).not.toBeInTheDocument();
    }
  });

  it("renders run and debug tools only when a project workspace context is visible", async () => {
    const user = userEvent.setup();
    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    expect(screen.queryByRole("toolbar", { name: "Run and debug tools" })).not.toBeInTheDocument();

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(await screen.findByText("run.py"));

    const toolbar = screen.getByRole("toolbar", { name: "Run and debug tools" });
    for (const title of [
      "Problems",
      "Test Explorer",
      "Debug",
      "Run Configurations",
      "Web Preview",
    ]) {
      expect(within(toolbar).getByTitle(title)).toHaveStyle({
        width: "30px",
        height: "30px",
      });
    }
  });

  it("hides top-right IDE tools while the running terminal workspace is visible", () => {
    render(
      <I18nProvider>
        <ProjectPage {...projectPagePropsWithWorkspace()} />
      </I18nProvider>,
    );

    expect(screen.queryByRole("toolbar", { name: "Run and debug tools" })).not.toBeInTheDocument();
    expect(screen.getByText("running task")).toBeInTheDocument();
  });

  it("opens every right toolbar target when clicked", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPagePropsWithWorkspace()} />
      </I18nProvider>,
    );

    const expectations = [
      ["File Explorer", "run.py"],
      ["Git Changes", "git-changes-panel"],
      ["Git History", "git-history-panel"],
      ["Git Advanced", "git-advanced-panel"],
      ["SSH", "ssh-workspace"],
      ["SFTP", "sftp-panel"],
      ["Database", "database-view"],
      ["Quick Notes", "notes-panel"],
      ["Docker", "docker-view"],
      ["Terminal", "shell-terminal"],
      ["Search", "search-panel"],
      ["Settings", "settings-dialog"],
    ] as const;

    for (const [title, visibleTarget] of expectations) {
      await user.click(screen.getByTitle(title));
      if (visibleTarget === "run.py") {
        expect(await screen.findByText("run.py")).toBeInTheDocument();
      } else {
        expect(await screen.findByTestId(visibleTarget)).toBeInTheDocument();
      }
    }
  });

  it("dispatches editor LSP actions from the command palette", async () => {
    const user = userEvent.setup();
    const commands: string[] = [];
    const onEditorCommand = (event: Event) => {
      const command = (event as CustomEvent<{ command?: string }>).detail?.command;
      if (command) commands.push(command);
    };
    window.addEventListener(FILE_VIEWER_COMMAND_EVENT, onEditorCommand);

    try {
      render(
        <I18nProvider>
          <ProjectPage {...projectPageProps()} />
        </I18nProvider>,
      );

      for (const [title, command] of [
        ["Find References", "findReferences"],
        ["Rename Symbol", "renameSymbol"],
        ["Quick Fix", "quickFix"],
      ] as const) {
        fireEvent.keyDown(window, { key: "P", ctrlKey: true, shiftKey: true });
        await user.click(await screen.findByRole("button", { name: title }));
        expect(commands).toContain(command);
      }
    } finally {
      window.removeEventListener(FILE_VIEWER_COMMAND_EVENT, onEditorCommand);
    }
  });

  it("announces toolbar actions through a shared project feedback status", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPagePropsWithWorkspace()} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("Git Changes"));
    const openedFeedback = await screen.findByTestId("project-action-feedback");
    expect(openedFeedback).toHaveTextContent("Opened Git Changes");
    expect(openedFeedback).toHaveAttribute("data-action-kind", "open");
    expect(openedFeedback).toHaveAttribute("data-action-target", "git-changes");
    expect(openedFeedback).toHaveAttribute("data-action-status", "completed");

    await user.click(screen.getByTitle("Git Changes"));
    const closedFeedback = await screen.findByTestId("project-action-feedback");
    expect(closedFeedback).toHaveTextContent("Closed Git Changes");
    expect(closedFeedback).toHaveAttribute("data-action-kind", "close");
    expect(closedFeedback).toHaveAttribute("data-action-target", "git-changes");

    await user.click(screen.getByTitle("Search"));
    const problemsFeedback = await screen.findByTestId("project-action-feedback");
    expect(problemsFeedback).toHaveTextContent("Opened Search");
    expect(problemsFeedback).toHaveAttribute("data-action-kind", "open");
    expect(problemsFeedback).toHaveAttribute("data-action-target", "search");

    expect(screen.queryByTestId("project-action-log-summary")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-action-log-details")).not.toBeInTheDocument();
    const persisted = JSON.parse(
      window.localStorage.getItem("aeroric:project-action-log:project-1") ?? "[]",
    );
    expect(persisted).toHaveLength(3);
    expect(persisted[0]).toMatchObject({
      action: "open",
      target: "search",
      status: "completed",
    });
  });

  it("does not render persisted project action log statistics", () => {
    window.localStorage.setItem(
      "aeroric:project-action-log:project-1",
      JSON.stringify([
        {
          id: 11,
          action: "open",
          target: "preview",
          startedAt: 1000,
          status: "failed",
          message: "Preview failed",
          finishedAt: 1044,
          durationMs: 44,
          error: "port unavailable",
        },
        {
          id: 10,
          action: "open",
          target: "search",
          startedAt: 900,
          status: "completed",
          message: "Opened Search",
          finishedAt: 920,
          durationMs: 20,
        },
      ]),
    );

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    expect(screen.queryByTestId("project-action-log-summary")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-action-log-details")).not.toBeInTheDocument();
  });

  it("aggregates panel render failures into action feedback and toast", async () => {
    const user = userEvent.setup();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mockState.throwSearchPanel = true;

    try {
      render(
        <I18nProvider>
          <ToastProvider>
            <ProjectPage {...projectPageProps()} />
          </ToastProvider>
        </I18nProvider>,
      );

      await user.click(screen.getByTitle("Search"));
      const feedback = await screen.findByTestId("project-action-feedback");
      expect(feedback).toHaveTextContent("Search failed");
      expect(feedback).toHaveAttribute("data-action-kind", "open");
      expect(feedback).toHaveAttribute("data-action-target", "search");
      expect(feedback).toHaveAttribute("data-action-status", "failed");
      expect(feedback).toHaveAttribute("title", expect.stringContaining("Search panel crashed"));
      expect(await screen.findByText("Search failed: Search panel crashed")).toBeInTheDocument();
    } finally {
      consoleError.mockRestore();
      mockState.throwSearchPanel = false;
    }
  });

  it("opens each project top-right IDE panel when clicked", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(await screen.findByText("run.py"));

    const toolbar = screen.getByRole("toolbar", { name: "Run and debug tools" });
    const expectations = [
      ["Problems", "problems-panel"],
      ["Test Explorer", "tests-panel"],
      ["Debug", "debug-panel"],
      ["Run Configurations", "run-panel"],
      ["Web Preview", "preview-panel"],
    ] as const;

    for (const [title, panelTestId] of expectations) {
      await user.click(within(toolbar).getByTitle(title));
      expect(await screen.findByTestId(panelTestId)).toBeInTheDocument();
    }
  });

  it("merges local LSP diagnostics events into editor diagnostics", async () => {
    const user = userEvent.setup();
    let diagnosticsHandler:
      | ((event: {
          payload: {
            projectPath: string;
            filePath: string;
            diagnostics: DiagnosticItem[];
          };
        }) => void)
      | null = null;
    vi.mocked(listen).mockImplementationOnce((event, handler) => {
      if (event === "lsp://diagnostics") {
        diagnosticsHandler = handler as typeof diagnosticsHandler;
      }
      return Promise.resolve(() => {});
    });

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(screen.getByText("run.py"));
    expect(await screen.findByTestId("editor-diagnostic-count")).toHaveTextContent("0");

    await act(async () => {
      diagnosticsHandler?.({
        payload: {
          projectPath: "/tmp/aeroric",
          filePath: "/tmp/aeroric/src/App.ts",
          diagnostics: [
            {
              source: "lsp:typescript",
              severity: "error",
              message: "Type mismatch",
              file: "/tmp/aeroric/src/App.ts",
              line: 1,
              column: 7,
              code: "2322",
            },
          ],
        },
      });
    });

    expect(await screen.findByTestId("editor-diagnostic-count")).toHaveTextContent("1");
  });

  it("merges remote LSP diagnostics events into SSH project editor diagnostics", async () => {
    const user = userEvent.setup();
    let diagnosticsHandler:
      | ((event: {
          payload: {
            projectPath: string;
            filePath: string;
            diagnostics: DiagnosticItem[];
          };
        }) => void)
      | null = null;
    vi.mocked(listen).mockImplementationOnce((event, handler) => {
      if (event === "lsp://diagnostics") {
        diagnosticsHandler = handler as typeof diagnosticsHandler;
      }
      return Promise.resolve(() => {});
    });

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps(sshProject())} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(screen.getByText("run.py"));
    expect(await screen.findByTestId("editor-diagnostic-count")).toHaveTextContent("0");

    await act(async () => {
      diagnosticsHandler?.({
        payload: {
          projectPath: "/srv/app",
          filePath: "/srv/app/run.py",
          diagnostics: [
            {
              source: "lsp:typescript",
              severity: "warning",
              message: "Unused value",
              file: "/srv/app/run.py",
              line: 2,
              column: 5,
              code: "6133",
            },
          ],
        },
      });
    });

    expect(await screen.findByTestId("editor-diagnostic-count")).toHaveTextContent("1");
  });

  it("enables remote-capable SSH project IDE buttons", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps(sshProject())} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(await screen.findByText("run.py"));

    const toolbar = screen.getByRole("toolbar", { name: "Run and debug tools" });
    expect(screen.getByTitle("Run Configurations")).toBeEnabled();
    expect(screen.getByTitle("Terminal")).toBeEnabled();
    expect(screen.getByTitle("Web Preview")).toBeEnabled();
    expect(screen.getByTitle("Debug")).toBeEnabled();
    expect(screen.getByTitle("Git Changes")).toBeEnabled();
    expect(screen.getByTitle("Git History")).toBeEnabled();
    expect(screen.getByTitle("Git Advanced")).toBeEnabled();
    expect(screen.getByTitle("Problems")).toBeEnabled();
    expect(screen.getByTitle("Test Explorer")).toBeEnabled();
    expect(screen.getByTitle("Search")).toBeEnabled();
    expect(screen.getByTitle("Settings")).toBeEnabled();

    await user.click(within(toolbar).getByTitle("Run Configurations"));
    expect(await screen.findByTestId("run-panel")).toBeInTheDocument();
  });

  it("opens SSH project IDE targets with the remote project context", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps(sshProject())} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(await screen.findByText("run.py"));

    let toolbar = screen.getByRole("toolbar", { name: "Run and debug tools" });
    const remoteIdeTargets = [
      ["Problems", "problems-panel"],
      ["Test Explorer", "tests-panel"],
      ["Debug", "debug-panel"],
      ["Run Configurations", "run-panel"],
      ["Web Preview", "preview-panel"],
    ] as const;

    for (const [title, testId] of remoteIdeTargets) {
      await user.click(within(toolbar).getByTitle(title));
      expect(await screen.findByTestId(testId)).toHaveAttribute("data-remote-project", "/srv/app");
      toolbar = screen.getByRole("toolbar", { name: "Run and debug tools" });
    }

    const remoteDockTargets = [
      ["File Explorer", "file-explorer-panel"],
      ["Git Changes", "git-changes-panel"],
      ["Git History", "git-history-panel"],
      ["Git Advanced", "git-advanced-panel"],
      ["Search", "search-panel"],
      ["Settings", "settings-dialog"],
    ] as const;

    for (const [title, testId] of remoteDockTargets) {
      await user.click(screen.getByTitle(title));
      expect(await screen.findByTestId(testId)).toHaveAttribute("data-remote-project", "/srv/app");
    }

    await user.click(screen.getByTitle("SSH"));
    expect(await screen.findByTestId("ssh-workspace")).toHaveAttribute(
      "data-remote-connection",
      "conn-2",
    );
    expect(screen.queryByRole("toolbar", { name: "Run and debug tools" })).not.toBeInTheDocument();

    await user.click(screen.getByTitle("Docker"));
    expect(await screen.findByTestId("docker-view")).toHaveAttribute(
      "data-remote-connection",
      "conn-2",
    );
    expect(screen.queryByRole("toolbar", { name: "Run and debug tools" })).not.toBeInTheDocument();

    await user.click(screen.getByTitle("Database"));
    expect(await screen.findByTestId("database-view")).toHaveAttribute(
      "data-remote-connection",
      "conn-2",
    );
    expect(screen.getByTestId("database-view")).toHaveAttribute("data-remote-project", "/srv/app");
    expect(screen.queryByRole("toolbar", { name: "Run and debug tools" })).not.toBeInTheDocument();
  });

  it("keeps saved SSH projects with numeric-string ids enabled across IDE toolbars", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps(savedLianyunProject())} />
      </I18nProvider>,
    );

    expect(screen.queryByTestId("ssh-connection-missing")).not.toBeInTheDocument();
    for (const title of [
      "Git Changes",
      "Git History",
      "Git Advanced",
      "Search",
      "Settings",
      "Terminal",
    ]) {
      expect(screen.getByTitle(title)).toBeEnabled();
    }

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(await screen.findByText("run.py"));

    const toolbar = screen.getByRole("toolbar", { name: "Run and debug tools" });
    for (const title of [
      "Problems",
      "Test Explorer",
      "Debug",
      "Run Configurations",
      "Web Preview",
    ]) {
      expect(within(toolbar).getByTitle(title)).toBeEnabled();
    }

    await user.click(screen.getByTitle("Git Changes"));
    expect(await screen.findByTestId("git-changes-panel")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(screen.getByTitle("Git History"));
    expect(await screen.findByTestId("git-history-panel")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(screen.getByTitle("Git Advanced"));
    expect(await screen.findByTestId("git-advanced-panel")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(screen.getByTitle("Search"));
    expect(await screen.findByTestId("search-panel")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(screen.getByTitle("Settings"));
    expect(await screen.findByTestId("settings-dialog")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(within(toolbar).getByTitle("Problems"));
    expect(await screen.findByTestId("problems-panel")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(within(toolbar).getByTitle("Test Explorer"));
    expect(await screen.findByTestId("tests-panel")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(within(toolbar).getByTitle("Debug"));
    expect(await screen.findByTestId("debug-panel")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(within(toolbar).getByTitle("Run Configurations"));
    expect(await screen.findByTestId("run-panel")).toHaveAttribute("data-remote-project", "/home");

    await user.click(within(toolbar).getByTitle("Web Preview"));
    expect(await screen.findByTestId("preview-panel")).toHaveAttribute(
      "data-remote-project",
      "/home",
    );

    await user.click(screen.getByTitle("SSH"));
    expect(await screen.findByTestId("ssh-workspace")).toHaveAttribute(
      "data-remote-connection",
      "1781590902568",
    );

    await user.click(screen.getByTitle("Docker"));
    expect(await screen.findByTestId("docker-view")).toHaveAttribute(
      "data-remote-connection",
      "1781590902568",
    );

    await user.click(screen.getByTitle("Database"));
    expect(await screen.findByTestId("database-view")).toHaveAttribute(
      "data-remote-connection",
      "1781590902568",
    );
    expect(screen.getByTestId("database-view")).toHaveAttribute("data-remote-project", "/home");

    await user.click(screen.getByTitle("Terminal"));
    expect(await screen.findByTestId("ssh-terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("shell-terminal")).not.toBeInTheDocument();
  });

  it("shows a reconnect state when an SSH project connection is missing", async () => {
    const user = userEvent.setup();
    const props = projectPageProps(sshProject());
    props.sshConnections = props.sshConnections.filter((connection) => connection.id !== "conn-2");

    render(
      <I18nProvider>
        <ProjectPage {...props} />
      </I18nProvider>,
    );

    expect(screen.getByTestId("ssh-connection-missing")).toHaveTextContent(
      "SSH connection unavailable",
    );
    expect(screen.getByTestId("ssh-connection-missing")).toHaveTextContent("/srv/app");
    expect(screen.getByTitle("File Explorer requires an active SSH connection")).toBeDisabled();
    expect(screen.queryByRole("toolbar", { name: "Run and debug tools" })).not.toBeInTheDocument();
    expect(
      screen.queryByTitle("Run Configurations require an active SSH connection"),
    ).not.toBeInTheDocument();
    expect(screen.getByTitle("Terminal requires an active SSH connection")).toBeDisabled();
    expect(screen.getByTitle("Settings require an active SSH connection")).toBeDisabled();
    expect(screen.getByTitle("Docker requires an active SSH connection")).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Reconnect" }));

    expect(await screen.findByTestId("ssh-workspace")).toBeInTheDocument();
  });

  it("uses a drawn Docker toolbar icon instead of an emoji glyph", () => {
    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    expect(screen.getByTitle("Docker")).not.toHaveTextContent("🐳");
    expect(screen.getByTestId("docker-logo-icon")).toBeInTheDocument();
  });

  it("keeps the Docker toolbar icon monochrome until Docker is selected", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    const dockerButton = screen.getByTitle("Docker");
    const dockerIcon = screen.getByTestId("docker-logo-icon");

    expect(dockerButton).toHaveStyle({
      background: "transparent",
      color: "var(--text-muted)",
    });
    expect(dockerButton.style.border).toBe("1px solid transparent");
    expect(dockerIcon.querySelectorAll('[fill="#2496ED"]')).toHaveLength(0);

    await user.click(dockerButton);

    expect(dockerButton).toHaveStyle({
      background: "var(--accent-subtle)",
      color: "var(--accent-strong)",
    });
    expect(dockerButton.style.border).toBe("1px solid var(--accent-soft)");
    expect(await screen.findByTestId("docker-view")).toBeInTheDocument();
  });

  it("keeps disabled toolbar buttons in shadcn disabled state", () => {
    render(
      <I18nProvider>
        <RightToolbar
          activePanel={null}
          onToggle={vi.fn()}
          terminalActive={false}
          onToggleTerminal={vi.fn()}
          onOpenSettings={vi.fn()}
          dockerDisabled
          settingsDisabled
        />
      </I18nProvider>,
    );

    expect(screen.getByTitle("Docker requires an active SSH connection")).toHaveStyle({
      width: "36px",
      height: "36px",
      opacity: "0.5",
      cursor: "not-allowed",
    });
    expect(screen.getByTitle("Settings require an active SSH connection")).toHaveStyle({
      width: "36px",
      height: "36px",
      opacity: "0.5",
      cursor: "not-allowed",
    });
  });

  it("hides the SSH workspace when Terminal is opened next", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("SSH"));
    expect(screen.getByTestId("ssh-workspace")).toBeInTheDocument();

    await user.click(screen.getByTitle("Terminal"));
    expect(screen.queryByTestId("ssh-workspace")).not.toBeInTheDocument();
    expect(screen.getByTestId("shell-terminal")).toBeInTheDocument();
  });

  it("shows only the latest right-toolbar workspace", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("SSH"));
    expect(screen.getByTestId("ssh-workspace")).toBeInTheDocument();

    await user.click(screen.getByTitle("Docker"));
    expect(screen.queryByTestId("ssh-workspace")).not.toBeInTheDocument();
    expect(screen.getByTestId("docker-view")).toBeInTheDocument();

    await user.click(screen.getByTitle("Terminal"));
    expect(screen.queryByTestId("docker-view")).not.toBeInTheDocument();
    expect(screen.getByTestId("shell-terminal")).toBeInTheDocument();

    await user.click(screen.getByTitle("SSH"));
    expect(screen.queryByTestId("shell-terminal")).not.toBeInTheDocument();
    expect(screen.getByTestId("ssh-workspace")).toBeInTheDocument();
  });

  it("runs a remote file in the SSH terminal without opening the SSH workspace", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps(sshProject())} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(screen.getByText("run.py"));
    await user.click(await screen.findByTitle("Run current file"));

    expect(screen.getByTestId("ssh-terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("ssh-workspace")).not.toBeInTheDocument();
  });

  it("opens the SSH project terminal from the right toolbar Terminal button", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps(sshProject())} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(screen.getByText("run.py"));
    expect(await screen.findByTitle("Run current file")).toBeInTheDocument();
    expect(screen.queryByTestId("ssh-terminal")).not.toBeInTheDocument();

    await user.click(screen.getByTitle("Terminal"));

    expect(await screen.findByTestId("ssh-terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("shell-terminal")).not.toBeInTheDocument();
  });

  it("opens remote Test Explorer from an editor test gutter request", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps(sshProject())} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(screen.getByText("run.py"));
    await user.click(await screen.findByTitle("Run test adds numbers"));

    expect(await screen.findByTestId("tests-panel")).toBeInTheDocument();
    expect(screen.queryByTestId("ssh-workspace")).not.toBeInTheDocument();
  });

  it("starts a Vitest debug session from an editor test gutter request", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(screen.getByText("run.py"));
    await user.click(await screen.findByTitle("Debug test adds numbers"));

    expect(invoke).toHaveBeenCalledWith("start_debug_config", {
      projectPath: "/tmp/aeroric",
      config: expect.objectContaining({
        id: "debug-vitest-src-math-test-ts-adds-numbers",
        name: "Debug Vitest: adds numbers",
        type: "node",
        program: "node_modules/vitest/vitest.mjs",
        cwd: ".",
        args: ["run", "src/math.test.ts", "-t", "adds numbers", "--runInBand"],
      }),
    });
  });

  it("starts a remote Vitest debug session from an editor test gutter request", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps(sshProject())} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("File Explorer"));
    await user.click(screen.getByText("run.py"));
    await user.click(await screen.findByTitle("Debug test adds numbers"));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("remote_start_debug_config", {
        connection: expect.objectContaining({
          id: "conn-2",
          host: "prod.example.com",
          remotePath: "/srv/app",
        }),
        remoteProjectPath: "/srv/app",
        projectPath: "/srv/app",
        config: expect.objectContaining({
          id: "debug-vitest-src-math-test-ts-adds-numbers",
          name: "Debug Vitest: adds numbers",
          type: "node",
          program: "node_modules/vitest/vitest.mjs",
          cwd: ".",
          args: ["run", "src/math.test.ts", "-t", "adds numbers", "--runInBand"],
        }),
      });
    });
    expect(await screen.findByTestId("debug-panel")).toBeInTheDocument();
  });

  it("creates an agent terminal task from the composer even when the prompt is blank", async () => {
    const user = userEvent.setup();
    const onSubmitTask = vi.fn();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} onSubmitTask={onSubmitTask} />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Start Terminal" }));

    expect(screen.queryByTestId("shell-terminal")).not.toBeInTheDocument();
    expect(onSubmitTask).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "",
        agent: "claude",
        immediate: true,
      }),
    );
  });

  it("keeps the right toolbar terminal as a regular shell terminal", async () => {
    const user = userEvent.setup();
    const onSubmitTask = vi.fn();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} onSubmitTask={onSubmitTask} />
      </I18nProvider>,
    );

    await user.click(screen.getByTitle("Terminal"));

    expect(screen.getByTestId("shell-terminal")).toBeInTheDocument();
    expect(onSubmitTask).not.toHaveBeenCalled();
  });
});
