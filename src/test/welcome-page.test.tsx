import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import type { Project, SshConnection } from "../types";
import { WelcomePage, projectMetaLabel } from "../components/WelcomePage";
import { layout } from "../styles";

vi.mock("../components/NotificationBell", () => ({
  NotificationBell: () => null,
}));

vi.mock("../components/recursive-hero-effect/recursive-hero-effect", () => ({
  createRecursiveHeroEffect: vi.fn(() => ({
    destroy: vi.fn(),
    setReducedMotion: vi.fn(),
  })),
}));

function remoteProject(): Project {
  return {
    id: "p1",
    name: "Prod",
    path: "ssh://conn-1/data",
    location: { kind: "ssh", connectionId: "conn-1", remotePath: "/data" },
    lastOpenedAt: 1,
  };
}

function sshConnection(): SshConnection {
  return {
    id: "conn-1",
    name: "Prod SSH",
    host: "192.168.10.95",
    port: 22,
    username: "root",
    remotePath: "/data",
    createdAt: 1,
  };
}

function renderWelcome(overrides: Partial<React.ComponentProps<typeof WelcomePage>> = {}) {
  const projects = overrides.projects ?? [remoteProject()];
  const props: React.ComponentProps<typeof WelcomePage> = {
    projects,
    allProjects: overrides.allProjects ?? projects,
    tasks: overrides.tasks ?? [],
    onOpen: overrides.onOpen ?? vi.fn(),
    onProjectClick: overrides.onProjectClick ?? vi.fn(),
    onDeleteProject: overrides.onDeleteProject ?? vi.fn(),
    onRenameProject: overrides.onRenameProject ?? vi.fn(),
    onToggleProjectHidden: overrides.onToggleProjectHidden ?? vi.fn(),
    projectGroups: overrides.projectGroups ?? [],
    onAssignProjectGroup: overrides.onAssignProjectGroup ?? vi.fn(),
    onCreateProjectGroup: overrides.onCreateProjectGroup ?? vi.fn(),
    onRenameProjectGroup: overrides.onRenameProjectGroup ?? vi.fn(),
    onDeleteProjectGroup: overrides.onDeleteProjectGroup ?? vi.fn(),
    themeVariant: overrides.themeVariant ?? "light",
    themeMode: overrides.themeMode ?? "light",
    systemPrefersDark: overrides.systemPrefersDark ?? false,
    onThemeModeChange: overrides.onThemeModeChange ?? vi.fn(),
    onToggleTheme: overrides.onToggleTheme ?? vi.fn(),
    terminalFontSize: overrides.terminalFontSize ?? 11,
    onTerminalFontSizeChange: overrides.onTerminalFontSizeChange ?? vi.fn(),
    taskDisplayWindow: overrides.taskDisplayWindow ?? 3,
    onTaskDisplayWindowChange: overrides.onTaskDisplayWindowChange ?? vi.fn(),
    attentionBadge: overrides.attentionBadge ?? true,
    onAttentionBadgeChange: overrides.onAttentionBadgeChange ?? vi.fn(),
    sftpLocalDefaultPath: overrides.sftpLocalDefaultPath ?? "/Users/macbook/Downloads/同步空间",
    onSftpLocalDefaultPathChange: overrides.onSftpLocalDefaultPathChange ?? vi.fn(),
    uiFontFamily: overrides.uiFontFamily ?? "sans-serif",
    onUiFontFamilyChange: overrides.onUiFontFamilyChange ?? vi.fn(),
    monoFontFamily: overrides.monoFontFamily ?? "monospace",
    onMonoFontFamilyChange: overrides.onMonoFontFamilyChange ?? vi.fn(),
    skillHubConfig: overrides.skillHubConfig ?? null,
    onEnterSkillHub: overrides.onEnterSkillHub ?? vi.fn(),
    sshConnections: overrides.sshConnections ?? [sshConnection()],
    onSshConnectionsChange: overrides.onSshConnectionsChange ?? vi.fn(),
    onOpenSshProject: overrides.onOpenSshProject ?? vi.fn(),
  };

  return render(React.createElement(I18nProvider, null, React.createElement(WelcomePage, props)));
}

describe("WelcomePage project cards", () => {
  it("shows SSH projects as username and IP instead of the persisted ssh URL", () => {
    expect(projectMetaLabel(remoteProject(), [sshConnection()])).toBe("root,192.168.10.95");

    renderWelcome();

    expect(screen.getByText("root,192.168.10.95")).toBeInTheDocument();
    expect(screen.queryByText("ssh://conn-1/data")).not.toBeInTheDocument();
  });

  it("opens the SSH configuration page from the home sidebar", async () => {
    const user = userEvent.setup();

    renderWelcome();

    await user.click(screen.getByRole("button", { name: "SSH" }));

    expect(screen.getAllByText("Open SSH project").length).toBeGreaterThan(0);
  });

  it("opens project group management and assigns a project to a group", async () => {
    localStorage.setItem("aeroric:language", "en");
    const user = userEvent.setup();
    const onAssignProjectGroup = vi.fn();

    renderWelcome({
      projectGroups: ["Work"],
      onAssignProjectGroup,
    });

    await user.click(screen.getByRole("button", { name: "Manage project groups" }));

    expect(screen.getByRole("dialog", { name: "Manage project groups" })).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("Assign Prod to a group"), "Work");
    expect(onAssignProjectGroup).toHaveBeenCalledWith("p1", "Work");
  });

  it("uses the Docker logo icon in the home sidebar", () => {
    renderWelcome();

    expect(screen.getByRole("button", { name: "Docker" })).not.toHaveTextContent("🐳");
    expect(screen.getByTestId("docker-logo-icon")).toBeInTheDocument();
  });

  it("keeps the recursive animation mounted when switching home sections", async () => {
    const user = userEvent.setup();

    renderWelcome({ themeVariant: "light" });

    expect(screen.getByTestId("welcome-recursive-background")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Skills" }));

    expect(screen.getByTestId("welcome-recursive-background")).toBeInTheDocument();
  });

  it("keeps the home project surface translucent so the recursive animation remains visible", () => {
    expect(layout.welcomePane.background).toContain("transparent");
    expect(layout.searchRow.background).toContain("transparent");
  });

  it("renames a project from the home page edit button", async () => {
    const user = userEvent.setup();
    const onRenameProject = vi.fn();
    const onProjectClick = vi.fn();

    renderWelcome({ onRenameProject, onProjectClick });

    await user.click(screen.getByTitle("Rename project"));
    const input = screen.getByRole("textbox", { name: "Rename project" });
    expect(input).toHaveValue("Prod");

    await user.clear(input);
    await user.type(input, "Prod API{Enter}");

    expect(onRenameProject).toHaveBeenCalledWith("p1", "Prod API");
    expect(onProjectClick).not.toHaveBeenCalled();
  });

  it("does not capitalize project names during home page rename", async () => {
    const user = userEvent.setup();
    const onRenameProject = vi.fn();

    renderWelcome({ onRenameProject });

    await user.click(screen.getByTitle("Rename project"));
    const input = screen.getByRole("textbox", { name: "Rename project" });
    await user.clear(input);
    await user.type(input, "aeroric api{Enter}");

    expect(onRenameProject).toHaveBeenCalledWith("p1", "aeroric api");
  });

  it("keeps project clicks disabled while a home page rename is active", async () => {
    const user = userEvent.setup();
    const onProjectClick = vi.fn();

    renderWelcome({ onProjectClick });

    await user.click(screen.getByTitle("Rename project"));
    const input = screen.getByRole("textbox", { name: "Rename project" });
    const row = screen.getByText("root,192.168.10.95").closest('[role="button"]');

    expect(input).toHaveAttribute("autocapitalize", "off");
    expect(input).toHaveAttribute("spellcheck", "false");
    expect(row).not.toBeNull();

    await user.click(screen.getByText("root,192.168.10.95"));
    expect(onProjectClick).not.toHaveBeenCalled();

    await user.click(row as HTMLElement);
    expect(onProjectClick).toHaveBeenCalledWith(expect.objectContaining({ id: "p1" }));
  });

  it("uses a frosted transparent edit field and keeps the project card transparent on hover", async () => {
    const user = userEvent.setup();

    renderWelcome();

    const row = screen.getByText("root,192.168.10.95").closest('[role="button"]');
    expect(row).not.toBeNull();

    await user.hover(row as HTMLElement);

    expect(row).toHaveStyle({ background: "transparent" });
    expect(row).not.toHaveStyle({ borderColor: "transparent" });

    await user.click(screen.getByTitle("Rename project"));
    const input = screen.getByRole("textbox", { name: "Rename project" });

    expect(input).toHaveStyle({
      background: "color-mix(in srgb, var(--bg-panel) 46%, transparent)",
      backdropFilter: "blur(14px) saturate(1.12)",
    });
    expect((input as HTMLInputElement).style.border).toBe("1px solid transparent");
    expect(input).toHaveStyle({ textTransform: "none" });
  });
});
