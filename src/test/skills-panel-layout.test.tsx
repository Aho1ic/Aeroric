import { render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SkillsPanel } from "../components/app-settings/SkillsPanel";
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
});
