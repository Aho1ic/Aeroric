/* 导出窗的画法与交互。
 *
 * 和 `notebook-export-run.test.ts` 的分工:那边验"跑完说什么",这边验"给定一组状态,
 * 画出来对不对" —— 哪些按钮禁用、忙的时候能不能重复点、取消按钮什么时候在场、Esc
 * 会不会连着关掉外面那层。
 */

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { NoteExportSheet } from "../components/notebook/NoteExportSheet";
import { staticT } from "../i18n";

type Overrides = Partial<Parameters<typeof NoteExportSheet>[0]>;

function renderSheet(overrides: Overrides = {}) {
  const onRun = vi.fn();
  const onCancelSite = vi.fn();
  const onClose = vi.fn();
  render(
    <NoteExportSheet
      hasNote
      busy={null}
      progress={null}
      notice={null}
      error={null}
      onRun={onRun}
      onCancelSite={onCancelSite}
      onClose={onClose}
      t={staticT}
      {...overrides}
    />,
  );
  return { onRun, onCancelSite, onClose };
}

function button(key: string): HTMLButtonElement {
  const label = screen.getByText(staticT(key));
  const found = label.closest("button");
  if (!found) throw new Error(`${key} 不在按钮里`);
  return found as HTMLButtonElement;
}

describe("NoteExportSheet 的结构", () => {
  it("是个带名字的 dialog", () => {
    renderSheet();
    expect(screen.getByRole("dialog", { name: staticT("notebook.exportTitle") })).toBeTruthy();
  });

  it("六项动作都在场", () => {
    renderSheet();
    for (const key of [
      "notebook.exportAsPdf",
      "notebook.exportAsHtml",
      "notebook.exportAsMarkdown",
      "notebook.exportCopyHtml",
      "notebook.exportCopyMarkdown",
      "notebook.exportSite",
    ]) {
      expect(button(key)).toBeTruthy();
    }
  });

  it("每项都带一句说明 —— 「导出 PDF」到底会发生什么不该靠猜", () => {
    renderSheet();
    expect(screen.getByText(staticT("notebook.exportAsPdfHint"))).toBeTruthy();
    expect(screen.getByText(staticT("notebook.exportSiteHint"))).toBeTruthy();
  });

  it("开窗时焦点落在第一项上", () => {
    renderSheet();
    expect(document.activeElement).toBe(button("notebook.exportAsPdf"));
  });

  it("标成菜单类容器,不被面板的全局点击关掉", () => {
    renderSheet();
    const dialog = screen.getByRole("dialog");
    expect(dialog.hasAttribute("data-notebook-context-menu")).toBe(true);
  });
});

describe("NoteExportSheet 的动作", () => {
  it("点一项就发起对应动作", () => {
    const { onRun } = renderSheet();
    fireEvent.click(button("notebook.exportAsHtml"));
    expect(onRun).toHaveBeenCalledWith("html");
  });

  it("整库导出发的是 site", () => {
    const { onRun } = renderSheet();
    fireEvent.click(button("notebook.exportSite"));
    expect(onRun).toHaveBeenCalledWith("site");
  });

  it("没有笔记时单篇那五项禁用", () => {
    renderSheet({ hasNote: false });
    for (const key of [
      "notebook.exportAsPdf",
      "notebook.exportAsHtml",
      "notebook.exportAsMarkdown",
      "notebook.exportCopyHtml",
      "notebook.exportCopyMarkdown",
    ]) {
      expect(button(key).disabled, `${key} 应禁用`).toBe(true);
    }
  });

  it("没有笔记时整库导出仍然可用 —— 它和当前笔记无关", () => {
    renderSheet({ hasNote: false });
    expect(button("notebook.exportSite").disabled).toBe(false);
  });

  it("禁用的按钮点不动", () => {
    const { onRun } = renderSheet({ hasNote: false });
    fireEvent.click(button("notebook.exportAsPdf"));
    expect(onRun).not.toHaveBeenCalled();
  });

  it("跑着的时候全部禁用,防止并发两条", () => {
    renderSheet({ busy: "html" });
    expect(button("notebook.exportAsHtml").disabled).toBe(true);
    expect(button("notebook.exportSite").disabled).toBe(true);
  });

  it("忙的那一项标上 aria-busy,其余不标", () => {
    renderSheet({ busy: "html" });
    expect(button("notebook.exportAsHtml").getAttribute("aria-busy")).toBe("true");
    expect(button("notebook.exportAsPdf").getAttribute("aria-busy")).toBe("false");
  });
});

describe("NoteExportSheet 的进度", () => {
  it("没在跑时不画取消按钮", () => {
    renderSheet();
    expect(screen.queryByText(staticT("notebook.exportSiteCancel"))).toBeNull();
  });

  it("跑整库导出时画进度和取消", () => {
    renderSheet({
      busy: "site",
      progress: { done: 3, total: 10, current: "某篇" },
    });
    const status = screen.getByRole("status");
    // 三个数字都要在:只报 3/10 不说在处理哪篇,用户不知道是不是卡住了。
    expect(status.textContent).toContain("某篇");
    expect(status.textContent).toContain("3");
    expect(status.textContent).toContain("10");
  });

  it("点取消调 onCancelSite,不调 onClose —— 取消导出不等于关窗", () => {
    const { onCancelSite, onClose } = renderSheet({
      busy: "site",
      progress: { done: 1, total: 5, current: "x" },
    });
    fireEvent.click(screen.getByText(staticT("notebook.exportSiteCancel")));
    expect(onCancelSite).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
  });
});

describe("NoteExportSheet 的反馈", () => {
  it("成功文案用 status,不打断屏幕阅读器", () => {
    renderSheet({ notice: "已导出到 /out/a.html" });
    expect(screen.getByRole("status").textContent).toContain("/out/a.html");
  });

  it("错误用 alert", () => {
    renderSheet({ error: "导出失败:磁盘满了" });
    expect(screen.getByRole("alert").textContent).toContain("磁盘满了");
  });

  it("两行的成功文案不会被折成一行", () => {
    renderSheet({ notice: "已导出到 /out/a.html\n2 张图片未内联" });
    const status = screen.getByRole("status");
    // 换行靠 CSS 保留:HTML 默认会把 \n 折成空格,那行提醒会贴在路径后面。
    expect(status.style.whiteSpace).toBe("pre-wrap");
    expect(status.textContent).toContain("2 张图片未内联");
  });

  it("没有反馈时既不画 status 也不画 alert", () => {
    renderSheet();
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("NoteExportSheet 的关窗", () => {
  it("点关闭", () => {
    const { onClose } = renderSheet();
    fireEvent.click(screen.getByText(staticT("common.close")));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Esc 关窗,并且不往上冒 —— 一次按键不该连着关掉外面那层", () => {
    const { onClose } = renderSheet();
    const outer = vi.fn();
    document.addEventListener("keydown", outer);
    try {
      fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(outer).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", outer);
    }
  });

  it("IME 组字中的 Esc 是取消候选词,不关窗", () => {
    const { onClose } = renderSheet();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape", isComposing: true });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("导出跑着的时候 Esc 也能关 —— 锁住窗会让人以为应用卡了", () => {
    const { onClose } = renderSheet({
      busy: "site",
      progress: { done: 1, total: 9, current: "x" },
    });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("别的键不关窗", () => {
    const { onClose } = renderSheet();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "a" });
    expect(onClose).not.toHaveBeenCalled();
  });
});
