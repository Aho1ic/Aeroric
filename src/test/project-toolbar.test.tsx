import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Project } from "../types";
import { ProjectPage } from "../components/ProjectPage";
import { RightToolbar } from "../components/RightToolbar";

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
  SshWorkspace: () => <div data-testid="ssh-workspace">ssh workspace</div>,
}));

vi.mock("../components/FileExplorer", () => ({
  FileExplorer: ({ onFileSelect }: { onFileSelect: (path: string, name: string) => void }) => (
    <button onClick={() => onFileSelect("/srv/app/run.py", "run.py")}>run.py</button>
  ),
}));

vi.mock("../components/FileViewer", () => ({
  FileViewer: ({ onRunPythonFile }: { onRunPythonFile?: (path: string) => void }) => (
    <button title="Run current file" onClick={() => onRunPythonFile?.("/srv/app/run.py")}>
      run file
    </button>
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
  DockerServiceView: () => <div data-testid="docker-view">docker</div>,
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
    ],
    onSshConnectionsChange: vi.fn(),
    condaEnvironments: [],
    selectedCondaEnvPath: null,
    onSelectedCondaEnvPathChange: vi.fn(),
  };
}

describe("ProjectPage right toolbar", () => {
  it("uses shadcn-like selected icon button styling for the active toolbar item", async () => {
    const user = userEvent.setup();

    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
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

    for (const title of ["Problems", "Test Explorer", "Debug", "Run Configurations", "Web Preview"]) {
      expect(screen.queryByTitle(title)).not.toBeInTheDocument();
    }
  });

  it("renders run and debug tools in the project top-right toolbar", () => {
    render(
      <I18nProvider>
        <ProjectPage {...projectPageProps()} />
      </I18nProvider>,
    );

    const toolbar = screen.getByRole("toolbar", { name: "Run and debug tools" });
    for (const title of ["Problems", "Test Explorer", "Debug", "Run Configurations", "Web Preview"]) {
      expect(within(toolbar).getByTitle(title)).toHaveStyle({
        width: "30px",
        height: "30px",
      });
    }
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

    expect(screen.getByTitle("Docker")).toHaveStyle({
      width: "36px",
      height: "36px",
      opacity: "0.5",
      cursor: "not-allowed",
    });
    expect(screen.getByTitle("Settings")).toHaveStyle({
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
