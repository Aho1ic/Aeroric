import { act, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SkillsPanel } from "../components/app-settings/SkillsPanel";
import { SKILL_LOAD_TIMEOUT_MS, SkillHubView } from "../components/skill-hub/SkillHubView";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  confirm: vi.fn(),
}));

describe("Skills settings layout", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_skill_hub_config") {
        return Promise.resolve({
          hubPath: "/Users/macbook/.aeroric/skills",
          hubProjectId: "skill-hub",
        });
      }
      if (command === "load_projects" || command === "list_skills") {
        return Promise.resolve([]);
      }
      if (command === "list_skill_installations") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps the path section compact and connects it to the embedded tabs", async () => {
    const { container } = render(
      <I18nProvider>
        <SkillsPanel />
      </I18nProvider>,
    );

    expect(await screen.findByRole("tablist", { name: "Skills" })).toBeInTheDocument();
    const root = container.firstElementChild;
    const pathSection = root?.children[0];
    const content = root?.children[1];
    const embeddedHub = content?.firstElementChild;

    expect((pathSection as HTMLElement).style.flex).toBe("0 0 auto");
    expect((pathSection as HTMLElement).style.borderBottom).toBe("1px solid var(--border-dim)");
    expect((content as HTMLElement).style.padding).toBe("12px 20px 18px");
    expect((content as HTMLElement).style.overflow).toBe("hidden");
    expect((embeddedHub as HTMLElement).style.padding).toBe("0px");
  });

  it("ends loading with a recoverable error when the skill scan never settles", async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_skills") return new Promise(() => undefined);
      if (command === "list_skill_installations") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <SkillHubView
          config={{ hubPath: "/skills", hubProjectId: "skill-hub" }}
          allProjects={[]}
          onOpenAppSettings={vi.fn()}
        />
      </I18nProvider>,
    );

    expect(screen.getByText("Loading skills...")).toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SKILL_LOAD_TIMEOUT_MS);
    });

    expect(screen.queryByText("Loading skills...")).not.toBeInTheDocument();
    expect(screen.getByText(/list_skills timed out/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "list_skills"),
    ).toHaveLength(2);
  });

  it("keeps loaded skills visible when installation health checks time out", async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_skills") {
        return Promise.resolve([
          {
            name: "test-skill",
            displayName: "Test Skill",
            description: "Loaded before installation health",
            path: "/skills/test-skill",
          },
        ]);
      }
      if (command === "list_skill_installations") return new Promise(() => undefined);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <SkillHubView
          config={{ hubPath: "/skills", hubProjectId: "skill-hub" }}
          allProjects={[]}
          onOpenAppSettings={vi.fn()}
        />
      </I18nProvider>,
    );

    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByText("Test Skill")).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(SKILL_LOAD_TIMEOUT_MS);
    });
    expect(screen.getByText(/list_skill_installations timed out/)).toBeInTheDocument();
    expect(screen.getByText("Test Skill")).toBeInTheDocument();
  });
});
