/* `notebook-panel*.test.tsx` 共用的渲染与编辑器辅助。
 *
 * 为什么只放这些:`vi.mock` 按文件提升,从被 import 的模块里调用对当前测试文件
 * **无效**;而 mock 工厂闭包读的 `harness` 也必须每文件一份。所以两个 `vi.mock`、
 * `harness` 声明和 `beforeEach` 留在各测试文件里,这里只收不依赖它们的纯函数。
 *
 * 拆成多个文件是为了并行度:vitest 的并行粒度是文件级,原来 7374 行 379 个测试
 * 挤在一个文件里只能占一个核,跑 14 分 45 秒。 */
import { act, render, screen } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";
import { EditorView } from "@uiw/react-codemirror";
import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";

export function renderNotebook() {
  return render(
    <I18nProvider>
      <NotebookPanel />
    </I18nProvider>,
  );
}

/** 新建一条笔记并等它落盘。创建要过一次 IPC,标题框是之后才出现的。 */
export async function createNote(user: ReturnType<typeof userEvent.setup>) {
  // 加号直接建 Markdown 笔记。富文本下线后不再有格式选择菜单 —— 只剩一种格式时
  // 菜单只是多一次点击。
  await user.click(screen.getByRole("button", { name: "New quick note" }));
  await screen.findByRole("textbox", { name: "Quick note name" });
}

/* Markdown 编辑器现在是 CodeMirror,不是 textarea。下面三个辅助把测试里
 * 「设内容 / 选一段 / 读内容」这三件事换成 CodeMirror 的事务操作。
 *
 * 走 EditorView 而不是 fireEvent:CodeMirror 的文档状态在 EditorState 里,
 * 对 contentDOM 派发 input 事件不会更新它。 */

/** 从 aria-label 找到编辑器视图。 */
export function editorView(): EditorView {
  const content = screen.getByRole("textbox", { name: "Quick note content" });
  const view = EditorView.findFromDOM(content as HTMLElement);
  if (!view) throw new Error("CodeMirror view not found");
  return view;
}

/** 替换整篇内容(等价于原来对 textarea 的 fireEvent.change)。 */
export function setEditorValue(value: string) {
  const view = editorView();
  act(() => {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: value },
    });
  });
}

/** 选中 [start, end)(等价于原来的 setSelectionRange + select 事件)。 */
export function selectEditorRange(start: number, end: number) {
  const view = editorView();
  act(() => {
    view.dispatch({ selection: { anchor: start, head: end } });
  });
}

/** 读当前内容。 */
export function editorValue(): string {
  return editorView().state.doc.toString();
}
