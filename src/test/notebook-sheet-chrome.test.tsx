/* 随手记 sheet 的共用外壳(`noteSheetChrome.ts`)。
 *
 * 为什么单独一个文件:七个 sheet 原先各自复制了同一套 overlay / header / 关闭按钮
 * 样式 + Esc + 挂载聚焦。收进共用模块之后,"改一处会不会把另外六处一起改掉"变成
 * 一个真实风险 —— 这里把**算出来的样式值**和**浮层语义**都钉死。
 *
 * 与 `notebook-panel.test.tsx` 的分工:那边验"面板把某个 sheet 接对了"(开关、
 * 互斥、数据来源),这边验"给定 props,壳画出来对不对"。回收站 / 任务收集箱 /
 * 字段浏览器 / 版本历史这四个此前只有面板级的间接覆盖,壳的语义(role、aria-label、
 * Esc 不外泄、打开即聚焦关闭按钮)没有一处直接钉住 —— 换成共用 hook 之后更需要。
 */

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import {
  noteSheetHeaderStyle,
  noteSheetIconButtonStyle,
  noteSheetOverlayStyle,
  noteSheetSplitOverlayStyle,
} from "../components/notebook/noteSheetChrome";
import { NoteTrashSheet } from "../components/notebook/NoteTrashSheet";
import { NoteTaskInboxSheet } from "../components/notebook/NoteTaskInboxSheet";
import { NoteFieldsSheet } from "../components/notebook/NoteFieldsSheet";
import { NoteHistorySheet } from "../components/notebook/NoteHistorySheet";
import type { TrashItem, NoteSnapshotEntry } from "../components/notebook/notebookApi";
import type { InboxTask } from "../components/notebook/noteTaskInbox";
import type { FieldEntry } from "../components/notebook/noteFields";
import { staticT } from "../i18n";

const t = (key: string, vars?: Record<string, string>) => staticT(key, vars);

describe("共用样式常量", () => {
  it("竖排 overlay 铺满面板而不是整个窗口", () => {
    // `absolute` + `inset:0` 而不是 `fixed`:随手记面板可能只占项目视图的一半,
    // 盖住整个窗口会把用户正在参照的另一半也遮掉。
    expect(noteSheetOverlayStyle.position).toBe("absolute");
    expect(noteSheetOverlayStyle.inset).toBe(0);
    expect(noteSheetOverlayStyle.zIndex).toBe(30);
    expect(noteSheetOverlayStyle.display).toBe("flex");
    expect(noteSheetOverlayStyle.flexDirection).toBe("column");
    expect(noteSheetOverlayStyle.background).toBe("var(--bg-panel)");
  });

  it("主从分栏 overlay 不能有 flexDirection —— 它要横排", () => {
    // 字段浏览器 / 版本历史是「左列表 + 右详情」。给它们加上 column 会把左栏
    // (固定 190px 宽)压成一条横带,右栏挤到下面去。
    expect(noteSheetSplitOverlayStyle.flexDirection).toBeUndefined();
    expect(noteSheetSplitOverlayStyle.display).toBe("flex");
    // 其余几项与竖排版一致。
    expect(noteSheetSplitOverlayStyle.position).toBe("absolute");
    expect(noteSheetSplitOverlayStyle.inset).toBe(0);
    expect(noteSheetSplitOverlayStyle.zIndex).toBe(30);
    expect(noteSheetSplitOverlayStyle.background).toBe("var(--bg-panel)");
  });

  it("header 的 gap 必须显式传,其余各项固定", () => {
    expect(noteSheetHeaderStyle(6).gap).toBe(6);
    expect(noteSheetHeaderStyle(8).gap).toBe(8);
    const style = noteSheetHeaderStyle(6);
    expect(style.minHeight).toBe(32);
    expect(style.alignItems).toBe("center");
    expect(style.padding).toBe("0 8px");
    expect(style.borderBottom).toBe("1px solid var(--border-dim)");
    expect(style.fontSize).toBe(11.5);
  });

  it("每次调用返回新对象 —— 共享一个对象会让某个 sheet 的覆盖泄漏到别处", () => {
    const a = noteSheetHeaderStyle(6);
    const b = noteSheetHeaderStyle(6);
    expect(a).not.toBe(b);
    expect(a).toEqual(b);
  });

  it("图标按钮自带 --text-hint,调用方靠 color 覆盖出别的语义色", () => {
    // 回收站的「彻底删除」是 --danger、「恢复」是 --text-secondary,都靠 spread
    // 之后覆盖 color。这里钉住默认值:默认没有 color 的话那两处覆盖看不出差别,
    // 而关闭按钮会变成继承色。
    expect(noteSheetIconButtonStyle.color).toBe("var(--text-hint)");
    expect(noteSheetIconButtonStyle.padding).toBe(3);
    expect(noteSheetIconButtonStyle.border).toBe("none");
    expect(noteSheetIconButtonStyle.background).toBe("transparent");
    expect(noteSheetIconButtonStyle.cursor).toBe("pointer");
    // 没有 marginLeft —— 靠右是各 sheet 自己按需要加的,挂在壳上会把别的控件推歪。
    expect(noteSheetIconButtonStyle.marginLeft).toBeUndefined();
  });
});

function trashItem(overrides: Partial<TrashItem> = {}): TrashItem {
  return {
    id: "t1",
    name: "笔记.md",
    relativePath: "inbox/笔记.md",
    deletedAtMs: Date.now() - 5 * 60_000,
    size: 128,
    isDir: false,
    ...overrides,
  };
}

function inboxTask(overrides: Partial<InboxTask> = {}): InboxTask {
  return {
    path: "/vault/a.md",
    title: "a",
    line: 3,
    checked: false,
    text: "写测试",
    tags: [],
    // 未摘标记的原文。类型上是必填 —— 悬浮提示和「复制任务文本」读的是它。
    raw: "- [ ] 写测试",
    ...overrides,
  };
}

function fieldEntry(overrides: Partial<FieldEntry> = {}): FieldEntry {
  return {
    key: "status",
    label: "status",
    notes: 2,
    values: [{ value: "doing", notes: [{ path: "/vault/a.md", title: "a" }] }],
    emptyNotes: [],
    ...overrides,
  };
}

function snapshotEntry(overrides: Partial<NoteSnapshotEntry> = {}): NoteSnapshotEntry {
  return {
    id: "s1",
    filePath: "/vault/.notebook/snapshots/s1.md",
    relativePath: "a.md",
    createdAtMs: Date.now() - 60_000,
    size: 64,
    ...overrides,
  };
}

/**
 * 四个 sheet 的渲染器。每个都回 `onClose` 的 spy —— 壳的三件事(Esc 关、点关闭
 * 按钮关、打开即聚焦)对它们是同一套断言。
 */
const SHEETS = [
  {
    name: "回收站",
    closeLabel: "notebook.trashClose",
    ariaLabel: () => staticT("notebook.trashTitle"),
    split: false,
    render: (onClose: () => void) =>
      render(
        <NoteTrashSheet
          items={[trashItem()]}
          loading={false}
          busyId={null}
          purgingAll={false}
          error={null}
          onRestore={vi.fn()}
          onPurge={vi.fn()}
          onPurgeAll={vi.fn()}
          onClose={onClose}
          t={t}
        />,
      ),
  },
  {
    name: "任务收集箱",
    closeLabel: "notebook.taskInboxClose",
    ariaLabel: () => staticT("notebook.taskInboxTitle"),
    split: false,
    render: (onClose: () => void) =>
      render(
        <NoteTaskInboxSheet
          tasks={[inboxTask()]}
          loading={false}
          error={null}
          onJump={vi.fn()}
          onRefresh={vi.fn()}
          onClose={onClose}
          onContextMenu={vi.fn()}
          t={t}
        />,
      ),
  },
  {
    name: "字段浏览器",
    closeLabel: "notebook.fieldsClose",
    ariaLabel: () => staticT("notebook.fieldsTitle"),
    split: true,
    render: (onClose: () => void) =>
      render(
        <NoteFieldsSheet
          entries={[fieldEntry()]}
          loading={false}
          error={null}
          onOpenNote={vi.fn()}
          onClose={onClose}
          t={t}
        />,
      ),
  },
  {
    name: "版本历史",
    closeLabel: "notebook.historyClose",
    ariaLabel: () => staticT("notebook.historyTitle", { name: "a" }),
    split: true,
    render: (onClose: () => void) =>
      render(
        <NoteHistorySheet
          noteTitle="a"
          entries={[snapshotEntry()]}
          selectedId={null}
          snapshotContent={null}
          currentContent="# a\n"
          loading={false}
          snapshotLoading={false}
          restoring={false}
          error={null}
          onSelect={vi.fn()}
          onRestore={vi.fn()}
          onClose={onClose}
          t={t}
        />,
      ),
  },
] as const;

describe.each(SHEETS)("$name 的壳", (sheet) => {
  it("是个 modal dialog,名字取自该 sheet 的标题文案", () => {
    sheet.render(vi.fn());
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAttribute("aria-label", sheet.ariaLabel());
  });

  it("打开就把焦点放到关闭按钮上", () => {
    sheet.render(vi.fn());
    // 不挪焦点的话焦点还在编辑器上:Esc 会被编辑器的按键处理先吃掉(事件在编辑器
    // 那棵子树里冒泡,不经过 overlay),Tab 也会从被遮住的元素开始走。
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: staticT(sheet.closeLabel) }),
    );
  });

  it("Esc 关掉自己,并且不冒到 window", () => {
    const onClose = vi.fn();
    const outer = vi.fn();
    // 挂在 window 上,对应宿主那个"Esc 关整个视图"的监听。挂在 RTL 的 container
    // 上没有意义:React 的事件系统就 root 在那个元素上,`stopPropagation` 停不掉
    // 同一个元素上的另一个监听(那要 `stopImmediatePropagation`)。
    window.addEventListener("keydown", outer);
    try {
      sheet.render(onClose);
      fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
      expect(onClose).toHaveBeenCalledTimes(1);
      // stopPropagation 生效:少了它,一次按键会连带把随手记面板一起关掉。
      expect(outer).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", outer);
    }
  });

  it("Esc 之外的键不关", () => {
    const onClose = vi.fn();
    sheet.render(onClose);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Enter" });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "a" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("点关闭按钮关", () => {
    const onClose = vi.fn();
    sheet.render(onClose);
    fireEvent.click(screen.getByRole("button", { name: staticT(sheet.closeLabel) }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("overlay 的排布方向和它的布局对得上", () => {
    sheet.render(vi.fn());
    const dialog = screen.getByRole("dialog");
    // 主从分栏(左列表 + 右详情)必须横排;其余竖排。走 style 属性而不是
    // getComputedStyle:这两个 overlay 的差别就是"有没有写 flexDirection"。
    expect(dialog.style.flexDirection).toBe(sheet.split ? "" : "column");
    expect(dialog.style.position).toBe("absolute");
    expect(dialog.style.zIndex).toBe("30");
  });
});
