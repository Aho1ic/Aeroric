import { render, screen } from "@testing-library/react";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Project } from "../types";
import { ProjectPage } from "../components/ProjectPage";
import { SettingsDialog } from "../components/SettingsDialog";
import { WelcomePage } from "../components/WelcomePage";
import { shouldRestartRecursiveHeroLoop } from "../components/recursive-hero-effect/recursive-hero-effect";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({
    agent: {
      default: "claude",
      default_permission_mode: "ask",
      prompt_prefix: "",
    },
    git: {
      commit_prompt: "",
      commit_message_timeout_secs: 15,
    },
  }),
}));

vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
}));

vi.mock("../components/recursive-hero-effect/recursive-hero-effect", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("../components/recursive-hero-effect/recursive-hero-effect")
    >();
  return {
    ...actual,
    createRecursiveHeroEffect: vi.fn(() => ({
      destroy: vi.fn(),
      setReducedMotion: vi.fn(),
    })),
  };
});

vi.mock("../components/NewTaskView", () => ({
  NewTaskView: () => <div>new task</div>,
}));

vi.mock("../components/RunningView", () => ({
  RunningView: () => <div>terminal</div>,
}));

vi.mock("../components/ProjectRail", () => ({
  ProjectRail: () => <nav>rail</nav>,
}));

vi.mock("../components/RightToolbar", () => ({
  renderIdeToolIcon: () => <span />,
  RightToolbar: () => <aside>toolbar</aside>,
}));

function localProject(): Project {
  return {
    id: "project-1",
    name: "Aeroric",
    path: "/tmp/aeroric",
    lastOpenedAt: 1,
  };
}

function renderWithI18n(element: React.ReactElement) {
  return render(<I18nProvider>{element}</I18nProvider>);
}

function projectPageProps(): React.ComponentProps<typeof ProjectPage> {
  return {
    project: localProject(),
    visible: true,
    allProjects: [localProject()],
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
    sshConnections: [],
    onSshConnectionsChange: vi.fn(),
    condaEnvironments: [],
    selectedCondaEnvPath: null,
    onSelectedCondaEnvPathChange: vi.fn(),
  };
}

function welcomePageProps(
  overrides: Partial<React.ComponentProps<typeof WelcomePage>> = {},
): React.ComponentProps<typeof WelcomePage> {
  const project = localProject();
  return {
    projects: [project],
    allProjects: [project],
    tasks: [],
    onOpen: vi.fn(),
    onProjectClick: vi.fn(),
    onDeleteProject: vi.fn(),
    onRenameProject: vi.fn(),
    onToggleProjectHidden: vi.fn(),
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
    skillHubConfig: null,
    onEnterSkillHub: vi.fn(),
    sshConnections: [],
    onSshConnectionsChange: vi.fn(),
    onOpenSshProject: vi.fn(),
    ...overrides,
  };
}

describe("recursive dynamic background", () => {
  it("restarts the recursive animation after the third stage completes", () => {
    expect(
      shouldRestartRecursiveHeroLoop({
        isFinalLayer: true,
        doneAt: 1000,
        currentTime: 1000,
        springVelocity: 0,
      }),
    ).toBe(true);
    expect(
      shouldRestartRecursiveHeroLoop({
        isFinalLayer: true,
        doneAt: 1000,
        currentTime: 3400,
        springVelocity: 0,
      }),
    ).toBe(true);
    expect(
      shouldRestartRecursiveHeroLoop({
        isFinalLayer: true,
        doneAt: 1000,
        currentTime: 900,
        springVelocity: 0,
      }),
    ).toBe(false);
    expect(
      shouldRestartRecursiveHeroLoop({
        isFinalLayer: true,
        doneAt: 1000,
        currentTime: 1000,
        springVelocity: 0.01,
      }),
    ).toBe(false);
    expect(
      shouldRestartRecursiveHeroLoop({
        isFinalLayer: false,
        doneAt: 1000,
        currentTime: 3400,
        springVelocity: 0,
      }),
    ).toBe(false);
  });

  it("renders behind the home page in light mode", () => {
    renderWithI18n(<WelcomePage {...welcomePageProps()} />);

    expect(screen.getByTestId("welcome-recursive-background")).toBeInTheDocument();
    expect(
      screen.getByTestId("welcome-recursive-background").querySelector("canvas"),
    ).toHaveAttribute("data-recursive-hero-background", "true");
  });

  it("does not render behind the home page in dark mode", () => {
    renderWithI18n(
      <WelcomePage
        {...welcomePageProps({
          themeVariant: "dark",
          themeMode: "dark",
        })}
      />,
    );

    expect(screen.queryByTestId("welcome-recursive-background")).not.toBeInTheDocument();
  });

  it("does not render in project pages", () => {
    renderWithI18n(<ProjectPage {...projectPageProps()} />);

    expect(screen.queryByTestId("project-recursive-background")).not.toBeInTheDocument();
  });

  it("does not render after starting a task terminal", () => {
    const props = {
      ...projectPageProps(),
      isNewTask: false,
      selectedTaskId: "task-1",
      tasks: [
        {
          id: "task-1",
          projectId: "project-1",
          prompt: "hello",
          agent: "claude" as const,
          permissionMode: "ask" as const,
          status: "running" as const,
          createdAt: 1,
        },
      ],
    };
    renderWithI18n(<ProjectPage {...props} />);

    expect(screen.queryByTestId("project-recursive-background")).not.toBeInTheDocument();
  });

  it("does not render in settings dialogs", () => {
    renderWithI18n(<SettingsDialog projectPath="/tmp/aeroric" onClose={vi.fn()} />);

    expect(screen.queryByTestId("settings-recursive-background")).not.toBeInTheDocument();
  });
});
