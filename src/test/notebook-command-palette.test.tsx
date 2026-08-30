import { fireEvent, render, screen, within } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

import { NoteCommandPalette } from "../components/notebook/NoteCommandPalette";
import type { NoteCommand, PaletteEntry } from "../components/notebook/noteCommands";

const t = (key: string, vars?: Record<string, string | number>) =>
  vars ? `${key}:${Object.values(vars).join(",")}` : key;

function cmd(id: string, label: string, extra: Partial<NoteCommand> = {}): NoteCommand {
  return { id, label, group: "grp", run: () => {}, ...extra };
}

function commandEntry(id: string, label: string, extra: Partial<NoteCommand> = {}): PaletteEntry {
  return { kind: "command", command: cmd(id, label, extra), spans: [] };
}

function noteEntry(noteId: string, title: string, recent = false): PaletteEntry {
  return { kind: "note", noteId, title, recent, spans: [] };
}

function renderPalette(overrides: Partial<Parameters<typeof NoteCommandPalette>[0]> = {}) {
  const props = {
    query: "",
    onQueryChange: vi.fn(),
    entries: [commandEntry("a", "New note"), commandEntry("b", "Trash")],
    selected: 0,
    onSelectedChange: vi.fn(),
    onRun: vi.fn(),
    onClose: vi.fn(),
    inputRef: createRef<HTMLInputElement>(),
    t,
    ...overrides,
  };
  render(<NoteCommandPalette {...props} />);
  return props;
}

function input(): HTMLElement {
  return screen.getByRole("combobox", { name: "notebook.commandPalette" });
}

function options(): HTMLElement[] {
  return within(screen.getByRole("listbox")).getAllByRole("option");
}

describe("NoteCommandPalette", () => {
  it("是个模态对话框", () => {
    renderPalette();
    const dialog = screen.getByRole("dialog", { name: "notebook.commandPalette" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
  });

  it("输入框是 combobox,列表是 listbox", () => {
    /* 焦点要一直留在输入框上(否则打字就断了),所以「当前选中哪一条」只能靠
       aria-activedescendant 告诉读屏软件 —— 那要求候选是 option 而不是可聚焦控件。 */
    renderPalette();
    expect(input()).toHaveAttribute("aria-controls", screen.getByRole("listbox").id);
    expect(options()).toHaveLength(2);
  });

  it("aria-activedescendant 指向选中项", () => {
    renderPalette({ selected: 1 });
    expect(input().getAttribute("aria-activedescendant")).toBe(options()[1]?.id);
  });

  it("选中项标 aria-selected,别的不标", () => {
    renderPalette({ selected: 1 });
    expect(options()[0]).toHaveAttribute("aria-selected", "false");
    expect(options()[1]).toHaveAttribute("aria-selected", "true");
  });

  it("没有选中项时不给 aria-activedescendant", () => {
    // 指向一个不存在的 id 会让读屏软件念不出东西,而不是念「没有选中」。
    renderPalette({ entries: [], selected: -1 });
    expect(input()).not.toHaveAttribute("aria-activedescendant");
  });

  it("打字回调出去", () => {
    const props = renderPalette();
    fireEvent.change(input(), { target: { value: "gr" } });
    expect(props.onQueryChange).toHaveBeenCalledWith("gr");
  });

  it("下键往下选", () => {
    const props = renderPalette({ selected: 0 });
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(props.onSelectedChange).toHaveBeenCalledWith(1);
  });

  it("上键往上选", () => {
    const props = renderPalette({ selected: 1 });
    fireEvent.keyDown(input(), { key: "ArrowUp" });
    expect(props.onSelectedChange).toHaveBeenCalledWith(0);
  });

  it("到底不再往下", () => {
    const props = renderPalette({ selected: 1 });
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(props.onSelectedChange).toHaveBeenCalledWith(1);
  });

  it("回车执行选中的那一条", () => {
    const props = renderPalette({ selected: 1 });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(props.onRun).toHaveBeenCalledWith(props.entries[1]);
  });

  it("列表为空时回车什么都不做", () => {
    const props = renderPalette({ entries: [], selected: -1 });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(props.onRun).not.toHaveBeenCalled();
  });

  it("IME 组字时的回车不执行", () => {
    /* 组字中的回车是「确认候选词」。不挡的话中文输入法下打第一个字就把面板执行掉了
       —— 而用户以为自己还在打字。 */
    const props = renderPalette({ selected: 0 });
    fireEvent.keyDown(input(), { key: "Enter", isComposing: true });
    expect(props.onRun).not.toHaveBeenCalled();
  });

  it("Escape 关掉", () => {
    const props = renderPalette();
    fireEvent.keyDown(input(), { key: "Escape" });
    expect(props.onClose).toHaveBeenCalled();
  });

  it("别的键不关也不执行", () => {
    // 只按过 Escape 的话,「这里判的是 Escape」和「这里什么键都关」分不出来。
    const props = renderPalette();
    fireEvent.keyDown(input(), { key: "a" });
    fireEvent.keyDown(input(), { key: "Tab" });
    expect(props.onClose).not.toHaveBeenCalled();
    expect(props.onRun).not.toHaveBeenCalled();
  });

  it("鼠标按下就执行", () => {
    /* 用 mousedown 而不是 click:click 之前会先 mousedown,那一下会把焦点从输入框挪走
       并让面板关掉,于是 click 落到已经不存在的节点上 —— 表现是点了没反应。 */
    const props = renderPalette();
    fireEvent.mouseDown(options()[1] as HTMLElement);
    expect(props.onRun).toHaveBeenCalledWith(props.entries[1]);
  });

  it("鼠标划过改变选中项", () => {
    const props = renderPalette();
    fireEvent.mouseEnter(options()[1] as HTMLElement);
    expect(props.onSelectedChange).toHaveBeenCalledWith(1);
  });

  it("灰着的命令标 aria-disabled", () => {
    renderPalette({ entries: [commandEntry("a", "Delete", { disabled: true })] });
    expect(options()[0]).toHaveAttribute("aria-disabled", "true");
  });

  it("能用的命令不标 aria-disabled", () => {
    renderPalette({ entries: [commandEntry("a", "Delete")] });
    expect(options()[0]).not.toHaveAttribute("aria-disabled");
  });

  it("命令显示分组名与快捷键提示", () => {
    renderPalette({ entries: [commandEntry("a", "Find", { hint: "⌘F" })] });
    expect(options()[0]?.textContent).toContain("grp");
    expect(options()[0]?.textContent).toContain("⌘F");
  });

  it("笔记显示「笔记」而不是分组名", () => {
    renderPalette({ entries: [noteEntry("/v/A.md", "A")] });
    expect(options()[0]?.textContent).toContain("notebook.commandPaletteNote");
  });

  it("空列表给一句话,而不是空白", () => {
    // 空白面板看起来像坏了,而这一句同时回答了「它还活着」和「没找到」。
    renderPalette({ entries: [], selected: -1 });
    expect(screen.getByText("notebook.commandPaletteEmpty")).toBeInTheDocument();
  });

  it("按命中区间画高亮", () => {
    renderPalette({
      entries: [{ kind: "command", command: cmd("a", "Link graph"), spans: [{ from: 5, to: 10 }] }],
    });
    expect(screen.getByText("graph").tagName).toBe("MARK");
  });

  it("多段命中画多个 mark", () => {
    renderPalette({
      entries: [
        {
          kind: "command",
          command: cmd("a", "New note"),
          spans: [
            { from: 0, to: 1 },
            { from: 4, to: 5 },
          ],
        },
      ],
    });
    const marks = options()[0]?.querySelectorAll("mark") ?? [];
    expect([...marks].map((m) => m.textContent)).toEqual(["N", "n"]);
  });

  it("没有命中区间时不画 mark", () => {
    renderPalette({ entries: [commandEntry("a", "New note")] });
    expect(options()[0]?.querySelector("mark")).toBeNull();
  });

  it("高亮按码点切,不按码元", () => {
    /* 区间是模型层用 `[...text]` 数出来的下标,而 `slice` 按码元 —— emoji 或生僻字
       的标题上两者不等,直接 slice 会把代理对切成两半,渲染出半个乱码字符。 */
    renderPalette({
      entries: [
        {
          kind: "note",
          noteId: "/v/a.md",
          title: "🎯目标",
          recent: false,
          spans: [{ from: 1, to: 3 }],
        },
      ],
    });
    expect(screen.getByText("目标").tagName).toBe("MARK");
    expect(options()[0]?.textContent).toContain("🎯");
  });

  it("最近打开过的笔记显示时钟图标", () => {
    /* 选中项自己会渲染一个回车图标,所以不能只数 svg —— 那样「非 recent 的笔记」
       也会有一个,这条断言就永远成立。用 selected:-1 把回车图标摘掉。 */
    renderPalette({ entries: [noteEntry("/v/A.md", "A", true)], selected: -1 });
    expect(options()[0]?.querySelector("svg")).not.toBeNull();
  });

  it("没打开过的笔记不显示时钟图标", () => {
    renderPalette({ entries: [noteEntry("/v/A.md", "A", false)], selected: -1 });
    expect(options()[0]?.querySelector("svg")).toBeNull();
  });
});
