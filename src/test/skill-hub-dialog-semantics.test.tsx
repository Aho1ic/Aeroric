import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SkillConflictDialog } from "../components/skill-hub/SkillConflictDialog";
import { SkillInstallDialog } from "../components/skill-hub/SkillInstallDialog";
import { SkillManageDialog } from "../components/skill-hub/SkillManageDialog";
import { I18nProvider } from "../i18n";
import type { Skill } from "../types";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const skill: Skill = {
  name: "code-review",
  displayName: "Code Review",
  description: "Review a diff",
  path: "/tmp/skills/code-review",
};

/**
 * 这三个对话框原先是裸 div,没有 role="dialog"/aria-modal,读屏软件不会把它们
 * 当模态处理(不宣告、不限制在对话框内浏览)。项目其余 27 个对话框都带这两个
 * 属性,这里守住的是"不再退回裸 div"。
 */
describe("skill hub dialog semantics", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    invokeMock.mockReset();
    invokeMock.mockResolvedValue([]);
  });

  it("exposes the install dialog as a labelled modal", () => {
    render(
      <I18nProvider>
        <SkillInstallDialog
          skill={skill}
          allProjects={[]}
          existingInstallations={[]}
          onClose={vi.fn()}
          onInstalled={vi.fn()}
        />
      </I18nProvider>,
    );

    const dialog = screen.getByRole("dialog", { name: 'Install "Code Review"' });
    expect(dialog).toHaveAttribute("aria-modal", "true");
  });

  it("exposes the manage dialog as a labelled modal", () => {
    render(
      <I18nProvider>
        <SkillManageDialog skill={skill} allProjects={[]} onClose={vi.fn()} onChanged={vi.fn()} />
      </I18nProvider>,
    );

    const dialog = screen.getByRole("dialog", { name: "Code Review" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
  });

  it("exposes the conflict dialog as a labelled modal", () => {
    render(
      <I18nProvider>
        <SkillConflictDialog
          conflict={{ existingKind: "symlink", linkPath: "/tmp/link" }}
          onChoose={vi.fn()}
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );

    const dialog = screen.getByRole("dialog", { name: "Existing file at target" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
  });
});
