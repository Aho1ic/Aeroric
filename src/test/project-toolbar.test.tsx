import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Project } from "../types";
import { ProjectPage } from "../components/ProjectPage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({}),
}));

vi.mock("../components/NewTaskView", () => ({
  NewTaskView: () => <div>new task</div>,
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

function projectPageProps(): React.ComponentProps<typeof ProjectPage> {
  const project = localProject();
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
    sshConnections: [],
    onSshConnectionsChange: vi.fn(),
    condaEnvironments: [],
    selectedCondaEnvPath: null,
    onSelectedCondaEnvPathChange: vi.fn(),
  };
}

describe("ProjectPage right toolbar", () => {
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
});
