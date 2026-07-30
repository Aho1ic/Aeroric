import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppPlatform } from "../platform";
import type { Project } from "../types";
import {
  getCommandPaletteIdeTools,
  getToolbarIdeTools,
  isIdeToolDisabled,
  IDE_TOOL_REGISTRY,
} from "../plugins/ideToolRegistry";
import { projectFeatureAvailability, visibleDockPanel } from "../components/project-page/viewMode";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../hooks/useAgentOptions", () => ({ useAgentOptions: () => [] }));
vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
  UpdateBanner: () => null,
}));
vi.mock("../components/recursive-hero-effect/recursive-hero-effect", () => ({
  createRecursiveHeroEffect: vi.fn(() => ({ destroy: vi.fn(), setReducedMotion: vi.fn() })),
}));
vi.mock("../components/app-settings/WslPanel", () => ({
  WslPanel: () => <div data-testid="wsl-panel" />,
}));

async function importWithPlatform(platform: AppPlatform) {
  vi.resetModules();
  vi.doMock("../platform", async (importOriginal) => {
    const actual = await importOriginal<typeof import("../platform")>();
    return {
      ...actual,
      APP_PLATFORM: platform,
      FONT_PLATFORM: platform === "windows" ? "windows" : platform === "macos" ? "macos" : "linux",
      IS_MAC_WEBKIT: false,
      IS_OTHER_WEBKIT: false,
    };
  });
  const [{ AppSettingsDialog }, { WelcomePage }, { I18nProvider }] = await Promise.all([
    import("../components/AppSettingsDialog"),
    import("../components/WelcomePage"),
    import("../i18n"),
  ]);
  return { AppSettingsDialog, WelcomePage, I18nProvider };
}

type SettingsDialog = Awaited<ReturnType<typeof importWithPlatform>>["AppSettingsDialog"];
type Welcome = Awaited<ReturnType<typeof importWithPlatform>>["WelcomePage"];
type Provider = Awaited<ReturnType<typeof importWithPlatform>>["I18nProvider"];

function renderSettings(AppSettingsDialog: SettingsDialog, I18nProvider: Provider) {
  render(
    React.createElement(
      I18nProvider,
      null,
      React.createElement(AppSettingsDialog, {
        initialNav: "general",
        themeVariant: "light",
        themeMode: "light",
        systemPrefersDark: false,
        onThemeModeChange: vi.fn(),
        terminalFontSize: 13,
        onTerminalFontSizeChange: vi.fn(),
        taskDisplayWindow: 7,
        onTaskDisplayWindowChange: vi.fn(),
        attentionBadge: true,
        onAttentionBadgeChange: vi.fn(),
        sftpLocalDefaultPath: "",
        onSftpLocalDefaultPathChange: vi.fn(),
        uiFontFamily: "system",
        onUiFontFamilyChange: vi.fn(),
        monoFontFamily: "system",
        onMonoFontFamilyChange: vi.fn(),
        onClose: vi.fn(),
      }),
    ),
  );
}

function localProject(): Project {
  return { id: "p1", name: "app", path: "/Users/me/app", lastOpenedAt: 1 };
}

function renderWelcome(WelcomePage: Welcome, I18nProvider: Provider) {
  const projects = [localProject()];
  render(
    React.createElement(
      I18nProvider,
      null,
      React.createElement(WelcomePage, {
        projects,
        allProjects: projects,
        tasks: [],
        onOpen: vi.fn(),
        onProjectClick: vi.fn(),
        onDeleteProject: vi.fn(),
        onRenameProject: vi.fn(),
        onToggleProjectHidden: vi.fn(),
        projectGroups: [],
        onAssignProjectGroup: vi.fn(),
        onCreateProjectGroup: vi.fn(),
        onRenameProjectGroup: vi.fn(),
        onDeleteProjectGroup: vi.fn(),
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
        sftpLocalDefaultPath: "",
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
        onOpenWslProject: vi.fn(),
      }),
    ),
  );
}

describe("WSL 入口的平台可见性", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
  });

  afterEach(() => {
    vi.doUnmock("../platform");
    vi.resetModules();
  });

  it("Windows 上应用设置里展示 WSL 导航项", async () => {
    const { AppSettingsDialog, I18nProvider } = await importWithPlatform("windows");
    renderSettings(AppSettingsDialog, I18nProvider);

    expect(screen.getByRole("button", { name: "WSL" })).toBeInTheDocument();
  });

  it("macOS 上应用设置里不展示 WSL 导航项", async () => {
    const { AppSettingsDialog, I18nProvider } = await importWithPlatform("macos");
    renderSettings(AppSettingsDialog, I18nProvider);

    expect(screen.queryByRole("button", { name: "WSL" })).not.toBeInTheDocument();
  });

  it("Windows 上首页打开项目菜单包含 WSL 项目入口", async () => {
    const user = userEvent.setup();
    const { WelcomePage, I18nProvider } = await importWithPlatform("windows");
    renderWelcome(WelcomePage, I18nProvider);

    await user.click(screen.getByRole("button", { name: /Open project/ }));

    expect(screen.getByText("Open WSL Project")).toBeInTheDocument();
  });

  it("非 Windows 首页打开项目菜单不包含 WSL 项目入口", async () => {
    const user = userEvent.setup();
    const { WelcomePage, I18nProvider } = await importWithPlatform("other");
    renderWelcome(WelcomePage, I18nProvider);

    await user.click(screen.getByRole("button", { name: /Open project/ }));

    expect(screen.getByText("Open local project")).toBeInTheDocument();
    expect(screen.queryByText("Open WSL Project")).not.toBeInTheDocument();
  });
});

describe("WSL 项目的首版功能开关", () => {
  const wslAvailability = projectFeatureAvailability({
    projectLocation: { kind: "wsl", distribution: "Ubuntu", linuxPath: "/home/dev/app" },
    hasRemoteFileContext: false,
    hasSupportedFileContext: true,
    hasRemoteConnection: false,
  });

  it("WSL 保留文件、Git 与终端, 关闭 LSP 依赖功能", () => {
    expect(wslAvailability).toEqual({
      filesDisabled: false,
      gitChangesDisabled: false,
      gitHistoryDisabled: false,
      gitDisabled: false,
      terminalDisabled: false,
      settingsDisabled: false,
      problemsDisabled: true,
      runDisabled: true,
      testsDisabled: true,
      searchDisabled: true,
      debugDisabled: true,
      previewDisabled: true,
    });
  });

  it("本地项目不受 WSL 限制影响", () => {
    expect(
      projectFeatureAvailability({
        projectLocation: { kind: "local", path: "/Users/me/app" },
        hasRemoteFileContext: false,
        hasSupportedFileContext: false,
        hasRemoteConnection: false,
      }),
    ).toEqual({
      filesDisabled: false,
      gitChangesDisabled: false,
      gitHistoryDisabled: false,
      gitDisabled: false,
      terminalDisabled: false,
      settingsDisabled: false,
      problemsDisabled: false,
      runDisabled: false,
      testsDisabled: false,
      searchDisabled: false,
      debugDisabled: false,
      previewDisabled: false,
    });
  });

  it("SSH 有可用连接时保持全部能力可用", () => {
    expect(
      projectFeatureAvailability({
        projectLocation: { kind: "ssh", connectionId: "ssh-1", remotePath: "/srv/app" },
        hasRemoteFileContext: true,
        hasSupportedFileContext: true,
        hasRemoteConnection: true,
      }),
    ).toMatchObject({
      filesDisabled: false,
      gitDisabled: false,
      problemsDisabled: false,
      terminalDisabled: false,
    });
  });

  it("WSL 下工具栏禁用 LSP 依赖工具, Git Advanced 仍可用", () => {
    const tools = getToolbarIdeTools(wslAvailability);
    const disabledIds = tools.filter((tool) => tool.disabled).map((tool) => tool.id);

    expect(disabledIds.sort()).toEqual(
      ["debug", "preview", "problems", "run", "search", "tests"].sort(),
    );
    expect(tools.find((tool) => tool.id === "git-advanced")?.disabled).toBe(false);
    expect(
      IDE_TOOL_REGISTRY.filter((tool) => isIdeToolDisabled(tool, wslAvailability)).length,
    ).toBe(6);
  });

  it("WSL 下命令面板只暴露可用工具", () => {
    expect(getCommandPaletteIdeTools(wslAvailability).map((tool) => tool.id)).toEqual([
      "git-advanced",
    ]);
  });

  it("WSL 下被禁用的面板不会作为 dock 面板展示", () => {
    const dockFlags = { ...wslAvailability };
    for (const panel of ["problems", "run", "tests", "debug", "preview", "search"] as const) {
      expect(visibleDockPanel(panel, dockFlags)).toBeNull();
    }
    expect(visibleDockPanel("files", dockFlags)).toBe("files");
    expect(visibleDockPanel("git-changes", dockFlags)).toBe("git-changes");
    expect(visibleDockPanel("git-history", dockFlags)).toBe("git-history");
    expect(visibleDockPanel("git-advanced", dockFlags)).toBe("git-advanced");
  });
});
