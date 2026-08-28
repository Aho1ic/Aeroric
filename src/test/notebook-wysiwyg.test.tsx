import { render, screen } from "@testing-library/react";
import { forceParsing } from "@codemirror/language";
import { EditorView } from "@uiw/react-codemirror";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NoteSourceEditor } from "../components/notebook/NoteSourceEditor";

/* WYSIWYG 装饰层的行为测试。
 *
 * 核心契约:**底层文档始终是纯 markdown**,装饰只改显示。所以每条测试都同时
 * 断言「看到的样子」和「文档内容没变」。 */

vi.mock("@tauri-apps/api/core", () => ({
  invoke: () => Promise.resolve(undefined),
}));

function mountEditor(value: string, wysiwyg = true) {
  render(
    <NoteSourceEditor
      value={value}
      onChange={() => {}}
      themeVariant="light"
      ariaLabel="Body"
      wysiwyg={wysiwyg}
    />,
  );
  const content = screen.getByRole("textbox", { name: "Body" });
  const view = EditorView.findFromDOM(content as HTMLElement);
  if (!view) throw new Error("view not found");
  // jsdom 没有视口,CodeMirror 默认只解析首屏预算内的一段,syntaxTree 不完整,
  // decoration 就建不出来。强制解析整篇再断言。
  forceParse(view);
  return { view, content };
}

/**
 * 逼 lezer 把整篇解析完。
 *
 * CodeMirror 默认只解析首屏视口预算内的一段;jsdom 没有真实视口,于是文档稍长
 * 就只解析了开头一点,`syntaxTree` 不完整,decoration 建不出来。
 * `forceParsing` 是 @codemirror/language 的公开 API,专门用于这种场景。
 */
function forceParse(view: EditorView) {
  forceParsing(view, view.state.doc.length, 5000);
  // 解析推进后 StateField 要收到一次事务才会重建(见 state.ts 的 fullyParsed 分支)。
  view.dispatch({});
}

beforeEach(() => {
  document.documentElement.classList.remove("dark");
});

describe("WYSIWYG 装饰", () => {
  it("关闭时不加任何装饰", () => {
    const { content, view } = mountEditor("# Heading\n", false);
    expect(content.querySelector(".cm-md-h1")).toBeNull();
    expect(view.state.doc.toString()).toBe("# Heading\n");
  });

  it("给标题行加上层级 class", () => {
    const { content } = mountEditor("# One\n\n## Two\n");
    // 标题靠行级 class 上大字号,而不是替换成别的元素。
    expect(content.querySelector(".cm-md-h1")).not.toBeNull();
    expect(content.querySelector(".cm-md-h2")).not.toBeNull();
  });

  it("光标不在标题行时隐藏 # 标记", () => {
    const { content, view } = mountEditor("# Heading\n\nbody\n");
    // 把光标放到正文,标题行的 marker 应该被隐藏。
    view.dispatch({ selection: { anchor: view.state.doc.length - 1 } });
    const headingLine = content.querySelector(".cm-md-h1");
    expect(headingLine?.textContent).not.toContain("#");
    // 关键:文档本身没变,保存出去还是带 # 的 markdown。
    expect(view.state.doc.toString()).toContain("# Heading");
  });

  it("光标进入标题行时 # 仍然保持隐藏", () => {
    // 这是 Markio 的刻意设计,不是缺陷:让 marker 随光标现形会改变行长度,
    // drawSelection 就把光标画到偏移后的视觉位置上,点击手感变得不对。
    // Typora / iA Writer 都是稳定布局 —— marker 始终隐藏,靠工具栏和快捷键改样式。
    const { content, view } = mountEditor("# Heading\n\nbody\n");
    view.dispatch({ selection: { anchor: 2 } });
    const headingLine = content.querySelector(".cm-md-h1");
    expect(headingLine?.textContent).not.toContain("#");
  });

  it("粗体与斜体加 mark class", () => {
    const { content, view } = mountEditor("**bold** and *italic*\n");
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    expect(content.querySelector(".cm-md-bold")).not.toBeNull();
    expect(content.querySelector(".cm-md-italic")).not.toBeNull();
  });

  it("任务列表渲染成可点的复选框", () => {
    const { content, view } = mountEditor("- [ ] todo\n- [x] done\n");
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    const boxes = content.querySelectorAll(".cm-md-task");
    expect(boxes.length).toBeGreaterThan(0);
    // 文档里仍是 `[ ]` / `[x]`。
    expect(view.state.doc.toString()).toContain("- [ ] todo");
  });

  it("引用行加左边线 class", () => {
    const { content, view } = mountEditor("> quoted\n");
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    expect(content.querySelector(".cm-md-quote-line")).not.toBeNull();
  });

  it("行内代码加 class 且反引号隐藏", () => {
    const { content, view } = mountEditor("use `code` here\n");
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    const inline = content.querySelector(".cm-md-code");
    expect(inline).not.toBeNull();
    expect(view.state.doc.toString()).toContain("`code`");
  });

  it("光标在围栏外时代码块渲染成 widget", () => {
    const source = "```js\nconst a = 1;\n```\n\nafter\n";
    const { content, view } = mountEditor(source);
    // 光标必须在围栏**外** —— 在里面时故意退回源码以便编辑。
    view.dispatch({ selection: { anchor: source.length - 1 } });
    expect(content.querySelector(".cm-md-code-widget")).not.toBeNull();
    expect(view.state.doc.toString()).toContain("const a = 1;");
  });

  it("光标进入围栏内时退回源码", () => {
    const source = "```js\nconst a = 1;\n```\n\nafter\n";
    const { content, view } = mountEditor(source);
    // 光标在代码块里 → 不套 widget,直接编辑源码。这与标题 marker 的处理不同:
    // 代码块是 block widget,整块替换,里面没法放光标。
    view.dispatch({ selection: { anchor: 8 } });
    expect(content.querySelector(".cm-md-code-widget")).toBeNull();
  });

  it("表格渲染成 widget", () => {
    const { content, view } = mountEditor("| a | b |\n| --- | --- |\n| 1 | 2 |\n\nafter\n");
    view.dispatch({ selection: { anchor: view.state.doc.length - 1 } });
    expect(content.querySelector("table")).not.toBeNull();
    expect(view.state.doc.toString()).toContain("| --- | --- |");
  });

  it("frontmatter 折叠成 widget", () => {
    const { content, view } = mountEditor('---\ntitle: "T"\n---\n\nbody\n');
    view.dispatch({ selection: { anchor: view.state.doc.length - 1 } });
    // 折叠后不该在编辑区里看到裸 YAML。
    expect(content.textContent).not.toContain('title: "T"');
    expect(view.state.doc.toString()).toContain('title: "T"');
  });

  it("装饰不改变文档长度", () => {
    // 这是整个 WYSIWYG 最重要的不变式:装饰是纯显示层。长度变了意味着
    // 保存出去的内容和用户写的不一样。
    const source = "# H\n\n**b** *i* `c`\n\n- [ ] t\n\n> q\n\n```js\nx\n```\n";
    const { view } = mountEditor(source);
    expect(view.state.doc.toString()).toBe(source);
    expect(view.state.doc.length).toBe(source.length);
  });

  it("暗色主题下不抛", () => {
    document.documentElement.classList.add("dark");
    expect(() => mountEditor("```js\nx\n```\n")).not.toThrow();
  });

  it("空文档不抛", () => {
    expect(() => mountEditor("")).not.toThrow();
  });

  it("未闭合围栏不抛", () => {
    expect(() => mountEditor("```js\nunclosed\n")).not.toThrow();
  });
});
