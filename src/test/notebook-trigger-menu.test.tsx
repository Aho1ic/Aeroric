import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";

import {
  NoteTriggerMenu,
  completionRow,
  slashRow,
  type TriggerRow,
} from "../components/notebook/NoteTriggerMenu";
import type { CompletionItem } from "../components/notebook/noteCompletions";
import type { SlashItem } from "../components/notebook/noteSlashItems";
import type { TriggerKind } from "../components/notebook/noteTriggers";

const t = (key: string) => key;

function row(over: Partial<TriggerRow> = {}): TriggerRow {
  return { id: "r1", glyph: "#", label: "row one", spans: [], ...over };
}

function renderMenu(over: Partial<React.ComponentProps<typeof NoteTriggerMenu>> = {}) {
  const props = {
    kind: "tag" as TriggerKind,
    query: "",
    rows: [row(), row({ id: "r2", label: "row two" })],
    selected: 0,
    onSelectedChange: vi.fn(),
    onPick: vi.fn(),
    onDismiss: vi.fn(),
    anchor: { x: 40, y: 80 },
    t,
    ...over,
  };
  const utils = render(<NoteTriggerMenu {...props} />);
  return { ...utils, props };
}

const listbox = () => screen.getByRole("listbox");
const options = () => within(listbox()).getAllByRole("option");

describe("NoteTriggerMenu", () => {
  it("画成 listbox + option,而不是一堆按钮", () => {
    /* 焦点必须留在编辑器上(否则打字就断了),所以「选中哪一条」只能靠
       aria-activedescendant 表达 —— 那要求候选是 option 而不是可聚焦控件。 */
    renderMenu();
    expect(options()).toHaveLength(2);
    expect(within(listbox()).queryAllByRole("button")).toHaveLength(0);
  });

  it("选中项由 aria-activedescendant 指出,并且 aria-selected 只有它是 true", () => {
    renderMenu({ selected: 1 });
    const active = listbox().getAttribute("aria-activedescendant");
    expect(active).toBeTruthy();
    expect(options()[1]!.id).toBe(active);
    expect(options()[0]!).toHaveAttribute("aria-selected", "false");
    expect(options()[1]!).toHaveAttribute("aria-selected", "true");
  });

  it("没有选中项时不给 aria-activedescendant", () => {
    // 空列表时 selected 是 -1;指一个不存在的 id 会让读屏软件报"空白"。
    renderMenu({ rows: [], selected: -1 });
    expect(listbox()).not.toHaveAttribute("aria-activedescendant");
  });

  it("mouseDown 提交,并挡掉默认行为", () => {
    /* 必须是 mouseDown 而不是 click:click 之前的 mousedown 会把焦点从编辑器抢走,
       而那会先把菜单关掉 —— click 于是落到一个已经不存在的节点上。 */
    const { props } = renderMenu();
    const event = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    fireEvent(options()[1]!, event);
    expect(props.onPick).toHaveBeenCalledWith(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it("鼠标移上去改选中", () => {
    const { props } = renderMenu();
    fireEvent.mouseEnter(options()[1]!);
    expect(props.onSelectedChange).toHaveBeenCalledWith(1);
  });

  it("点菜单外面收起", () => {
    const { props } = renderMenu();
    fireEvent.pointerDown(document.body);
    expect(props.onDismiss).toHaveBeenCalled();
  });

  it("点菜单里面不收起", () => {
    const { props } = renderMenu();
    fireEvent.pointerDown(options()[0]!);
    expect(props.onDismiss).not.toHaveBeenCalled();
  });

  it("卸载后不再监听 pointerdown", () => {
    const { props, unmount } = renderMenu();
    unmount();
    fireEvent.pointerDown(document.body);
    expect(props.onDismiss).not.toHaveBeenCalled();
  });

  it("空列表显示占位文案,slash 和补全各一份", () => {
    const { unmount } = renderMenu({ rows: [], selected: -1 });
    expect(screen.getByText("notebook.completionEmpty")).toBeInTheDocument();
    unmount();
    renderMenu({ rows: [], selected: -1, kind: "slash" });
    expect(screen.getByText("notebook.slashEmpty")).toBeInTheDocument();
  });

  it("标题和徽标按触发种类走", () => {
    renderMenu({ kind: "wiki" });
    expect(listbox()).toHaveAccessibleName("notebook.completionWiki");
    expect(screen.getByText("[[")).toBeInTheDocument();
  });

  it("查询非空时回显在标题上", () => {
    renderMenu({ query: "abc" });
    expect(screen.getByText("· abc")).toBeInTheDocument();
  });

  it("按命中区间画高亮", () => {
    renderMenu({ rows: [row({ label: "Alpha", spans: [{ from: 0, to: 2 }] })] });
    const mark = within(options()[0]!).getByText("Al");
    expect(mark.tagName).toBe("MARK");
  });

  it("高亮按码点切,不切开代理对", () => {
    /* 区间是模型层用 `[...text]` 数出来的;按码元 slice 会把 `🚀` 的代理对切成两半,
       渲染出两个替换字符。所以既要断言高亮落在 `ab` 上,也要断言前缀原样还在。 */
    renderMenu({ rows: [row({ label: "🚀ab", spans: [{ from: 1, to: 3 }] })] });
    const mark = within(options()[0]!).getByText("ab");
    expect(mark.tagName).toBe("MARK");
    expect(options()[0]!.textContent).toBe("#🚀ab");
  });

  it("没有 detail 时不画第二行", () => {
    /* 按结构断言而不是按文案:空的第二行 textContent 也是空串,只看文字两种写法
       分不出来 —— 而在真实排版下它多占一行的高度,行高就跳了。 */
    const withDetail = renderMenu({ rows: [row({ detail: "d" })] });
    const lines = (node: HTMLElement) => node.querySelectorAll(":scope > span:last-of-type > span");
    expect(lines(options()[0]!)).toHaveLength(2);
    withDetail.unmount();

    renderMenu({ rows: [row()] });
    expect(lines(options()[0]!)).toHaveLength(1);
  });

  it("detail 为空串等同于没有", () => {
    renderMenu({ rows: [row({ detail: "" })] });
    expect(options()[0]!.querySelectorAll(":scope > span:last-of-type > span")).toHaveLength(1);
  });
});

describe("行折叠", () => {
  it("slashRow 把 i18n key 翻成文案", () => {
    const item: SlashItem = {
      id: "h1",
      glyph: "H1",
      labelKey: "k.label",
      hintKey: "k.hint",
      text: "# ",
    };
    expect(slashRow(item, t)).toEqual({
      id: "h1",
      glyph: "H1",
      label: "k.label",
      detail: "k.hint",
      spans: [],
    });
  });

  it("completionRow 没有 glyph 时给空串,而不是 undefined", () => {
    const item: CompletionItem = { id: "x", label: "L", insert: "L", spans: [] };
    expect(completionRow(item).glyph).toBe("");
  });
});
