import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { NoteStatusBar, vaultRelativePath } from "../components/notebook/NoteStatusBar";
import type { NoteSaveState } from "../components/notebook/useNoteAutosave";
import { staticT } from "../i18n";

const VAULT = "/home/u/notes";

const renderBar = (saveState: NoteSaveState, notePath = `${VAULT}/daily/today.md`) =>
  render(<NoteStatusBar notePath={notePath} vault={VAULT} saveState={saveState} t={staticT} />);

describe("vaultRelativePath", () => {
  it("剥掉 vault 前缀", () => {
    expect(vaultRelativePath(`${VAULT}/daily/today.md`, VAULT)).toBe("daily/today.md");
  });

  it("vault 结尾多余的斜杠不影响", () => {
    expect(vaultRelativePath(`${VAULT}/a.md`, `${VAULT}/`)).toBe("a.md");
  });

  it("Windows 路径归一化后再比", () => {
    expect(vaultRelativePath("C:\\notes\\sub\\a.md", "C:\\notes")).toBe("sub/a.md");
  });

  it("不在 vault 里就退回文件名", () => {
    // 前缀比对必须带上那个 `/`,否则 `<vault>-backup` 会被当成 vault 内的目录,
    // 显示出 `-backup/a.md` 这种不存在的相对路径。
    expect(vaultRelativePath(`${VAULT}-backup/a.md`, VAULT)).toBe("a.md");
    expect(vaultRelativePath("/tmp/elsewhere/a.md", VAULT)).toBe("a.md");
  });

  it("还不知道 vault 时退回文件名", () => {
    expect(vaultRelativePath(`${VAULT}/daily/today.md`, null)).toBe("today.md");
  });

  it("vault 根目录下的笔记就是文件名", () => {
    expect(vaultRelativePath(`${VAULT}/a.md`, VAULT)).toBe("a.md");
  });
});

describe("NoteStatusBar", () => {
  const cases: [NoteSaveState, string][] = [
    ["pending", "notebook.saveStatePending"],
    ["saving", "notebook.saveStateSaving"],
    ["saved", "notebook.saveStateSaved"],
    ["error", "notebook.saveStateError"],
  ];

  for (const [state, key] of cases) {
    it(`${state} 报对应文案`, () => {
      renderBar(state);
      expect(screen.getByRole("status").textContent).toContain(staticT(key));
    });
  }

  it("保存状态挂在 role=status 上", () => {
    // 自动保存是静默的,状态变化要能被屏幕阅读器播报,不能只靠看。
    renderBar("saving");
    expect(screen.getByRole("status")).toBeTruthy();
  });

  it("显示 vault 相对路径,完整路径放 title", () => {
    renderBar("saved");
    const path = screen.getByTitle(`${VAULT}/daily/today.md`);
    expect(path.textContent).toContain("daily/today.md");
  });

  it("四种状态各有各的颜色", () => {
    const colors = new Set<string>();
    for (const [state] of cases) {
      const view = renderBar(state);
      colors.add(screen.getByRole("status").style.color);
      view.unmount();
    }
    expect(colors.size).toBe(4);
  });
});
