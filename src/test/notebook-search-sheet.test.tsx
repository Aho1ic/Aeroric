import { fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

import { NoteSearchSheet } from "../components/notebook/NoteSearchSheet";
import type { NoteSearchHit } from "../components/notebook/noteGlobalSearch";

/* 文案直接回 key:断言就不跟着翻译改,而 `{hits}` 这类插值在真实 `t` 里才展开,
   所以带参数的那几条只验 key 出现,不验数字 —— 数字在面板测试里验。 */
const t = (key: string, vars?: Record<string, string | number>) =>
  vars ? `${key}:${Object.values(vars).join(",")}` : key;

function hit(over: Partial<NoteSearchHit> = {}): NoteSearchHit {
  return {
    path: "/vault/Notes.md",
    name: "Notes.md",
    line: 3,
    column: 1,
    lineText: "cat sat",
    matchText: "cat",
    ...over,
  };
}

function renderSheet(over: Partial<Parameters<typeof NoteSearchSheet>[0]> = {}) {
  const props = {
    query: "cat",
    onQueryChange: vi.fn(),
    flags: { caseSensitive: false, wholeWord: false, regex: false },
    onFlagsChange: vi.fn(),
    hits: [] as readonly NoteSearchHit[],
    loading: false,
    error: null as string | null,
    capped: false,
    searched: false,
    onSubmit: vi.fn(),
    onOpen: vi.fn(),
    onClose: vi.fn(),
    inputRef: createRef<HTMLInputElement>(),
    t,
    ...over,
  };
  render(<NoteSearchSheet {...props} />);
  return props;
}

function status(): string {
  return document.querySelector('[aria-live="polite"]')?.textContent ?? "";
}

describe("NoteSearchSheet", () => {
  it("是个有可及名字的模态对话框", () => {
    renderSheet();
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAccessibleName("notebook.globalSearch");
  });

  it("回车提交,别的键不提交", () => {
    const props = renderSheet();
    const input = screen.getByRole("textbox", { name: "notebook.globalSearch" });
    fireEvent.keyDown(input, { key: "a" });
    expect(props.onSubmit).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onSubmit).toHaveBeenCalledTimes(1);
  });

  it("Escape 关掉面板,别的键不关", () => {
    const props = renderSheet();
    const dialog = screen.getByRole("dialog");
    /* 「别的键不关」这一半必须验:keyDown 挂在整个对话框上,漏了判 key 的话在
       输入框里敲任何一个字都会把面板关掉 —— 那等于搜不了。 */
    fireEvent.keyDown(dialog, { key: "a" });
    fireEvent.keyDown(dialog, { key: "Enter" });
    expect(props.onClose).not.toHaveBeenCalled();

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it("三个开关各带 aria-pressed,点一下只翻自己那一位", () => {
    const props = renderSheet();
    const whole = screen.getByRole("button", { name: "notebook.findWholeWord" });
    expect(whole).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(whole);
    expect(props.onFlagsChange).toHaveBeenCalledWith({
      caseSensitive: false,
      wholeWord: true,
      regex: false,
    });
  });

  it("已经开着的开关点一下会关掉", () => {
    /* 只验「关 → 开」的话,写成"点了就设 true"也能过 —— 而那样的开关一旦打开就再也
       关不掉,用户只能重开面板。 */
    const props = renderSheet({ flags: { caseSensitive: false, wholeWord: true, regex: false } });
    fireEvent.click(screen.getByRole("button", { name: "notebook.findWholeWord" }));
    expect(props.onFlagsChange).toHaveBeenCalledWith({
      caseSensitive: false,
      wholeWord: false,
      regex: false,
    });
  });

  it("开着的开关 aria-pressed 是 true", () => {
    renderSheet({ flags: { caseSensitive: true, wholeWord: false, regex: true } });
    expect(screen.getByRole("button", { name: "notebook.findCaseSensitive" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "notebook.findRegex" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "notebook.findWholeWord" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("没搜过时给提示,而不是「没有结果」", () => {
    renderSheet({ searched: false });
    expect(status()).toContain("notebook.globalSearchHint");
    expect(status()).not.toContain("notebook.globalSearchEmpty");
  });

  it("搜过且为空才说「没有结果」", () => {
    renderSheet({ searched: true });
    expect(status()).toContain("notebook.globalSearchEmpty");
  });

  it("搜索中显示进行态,盖过「没有结果」", () => {
    renderSheet({ loading: true, searched: true });
    expect(status()).toContain("notebook.globalSearchRunning");
    expect(status()).not.toContain("notebook.globalSearchEmpty");
  });

  it("有错时报错,并盖过命中摘要", () => {
    renderSheet({ error: "regex parse error", searched: true, hits: [hit()] });
    expect(status()).toContain("regex parse error");
    expect(status()).not.toContain("notebook.globalSearchSummary");
  });

  it("摘要给出命中数和文件数", () => {
    renderSheet({
      searched: true,
      hits: [hit(), hit({ line: 9 }), hit({ path: "/vault/Other.md", name: "Other.md" })],
    });
    // 3 处命中分布在 2 篇里 —— 前两条同一个 path。
    expect(status()).toContain("notebook.globalSearchSummary:3,2");
  });

  it("触顶时摘要后面追加截断提示", () => {
    renderSheet({ searched: true, hits: [hit()], capped: true });
    expect(status()).toContain("notebook.globalSearchCapped");
  });

  it("不触顶就不提截断", () => {
    renderSheet({ searched: true, hits: [hit()], capped: false });
    expect(status()).not.toContain("notebook.globalSearchCapped");
  });

  it("按文件分组,组名是文件名并带上该组命中数", () => {
    renderSheet({
      searched: true,
      hits: [hit(), hit({ line: 9 }), hit({ path: "/vault/Other.md", name: "Other.md" })],
    });
    const groups = screen.getAllByRole("region");
    expect(groups.map((group) => group.getAttribute("aria-label"))).toEqual([
      "Notes.md",
      "Other.md",
    ]);
    expect(groups[0]!.textContent).toContain("2");
  });

  it("命中行按字节列高亮,中文在前也不串位", () => {
    /* `标题 ` 是 3 个汉字 + 空格 = 10 字节,所以 rg 给的列是 11。按 JS 下标切会切在
       「题」上,高亮框整体左移 —— 这条就是为了守住那次换算。 */
    renderSheet({
      searched: true,
      hits: [hit({ lineText: "标题 abc def", matchText: "abc", column: 11 })],
    });
    const mark = document.querySelector("mark");
    expect(mark?.textContent).toBe("abc");
  });

  it("点一条命中把那条 hit 交回去", () => {
    const target = hit({ line: 7 });
    const props = renderSheet({ searched: true, hits: [target] });
    fireEvent.click(screen.getByRole("button", { name: /Notes\.md/ }));
    expect(props.onOpen).toHaveBeenCalledWith(target);
  });

  it("命中的可及名字带上文件名和行号", () => {
    renderSheet({ searched: true, hits: [hit({ line: 42, lineText: "  cat sat  " })] });
    // 行文本 trim 过:读屏念一串前导空格没有意义。
    expect(
      screen.getByRole("button", { name: "notebook.globalSearchHit:Notes.md,42,cat sat" }),
    ).toBeInTheDocument();
  });

  it("关闭按钮走 onClose", () => {
    const props = renderSheet();
    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it("输入框改动上报新值", () => {
    const props = renderSheet();
    fireEvent.change(screen.getByRole("textbox", { name: "notebook.globalSearch" }), {
      target: { value: "dog" },
    });
    expect(props.onQueryChange).toHaveBeenCalledWith("dog");
  });
});
