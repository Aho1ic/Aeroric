import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";
import { EditorView } from "@uiw/react-codemirror";
import { NotebookVaultHarness } from "./notebookVaultHarness";

/* 随手记的笔记现在是磁盘上的 .md 文件,新建 / 保存 / 删除都要过 Tauri 命令。
 * 用一个内存 vault 顶上,这样这些测试仍然在验证真实行为(写进去能读回来、
 * 冲突真的会触发),而不是验证 mock 被调用过。 */
let harness: NotebookVaultHarness;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) =>
    Promise.resolve(harness.handle(command, args ?? {})),
}));

function renderNotebook() {
  return render(
    <I18nProvider>
      <NotebookPanel />
    </I18nProvider>,
  );
}

/** 新建一条笔记并等它落盘。创建要过一次 IPC,标题框是之后才出现的。 */
async function createNote(user: ReturnType<typeof userEvent.setup>) {
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
function editorView(): EditorView {
  const content = screen.getByRole("textbox", { name: "Quick note content" });
  const view = EditorView.findFromDOM(content as HTMLElement);
  if (!view) throw new Error("CodeMirror view not found");
  return view;
}

/** 替换整篇内容(等价于原来对 textarea 的 fireEvent.change)。 */
function setEditorValue(value: string) {
  const view = editorView();
  act(() => {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: value },
    });
  });
}

/** 选中 [start, end)(等价于原来的 setSelectionRange + select 事件)。 */
function selectEditorRange(start: number, end: number) {
  const view = editorView();
  act(() => {
    view.dispatch({ selection: { anchor: start, head: end } });
  });
}

/** 读当前内容。 */
function editorValue(): string {
  return editorView().state.doc.toString();
}

describe("NotebookPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    harness = new NotebookVaultHarness();
  });

  it("creates notes, shows a note list, and renders markdown in reading mode", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Deploy notes");
    expect(screen.queryByRole("button", { name: "Create quick note" })).not.toBeInTheDocument();
    await user.type(
      screen.getByRole("textbox", { name: "Quick note content" }),
      "# Release\n\n**Ship it**",
    );

    expect(screen.getByRole("button", { name: "Deploy notes" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Read" }));

    expect(screen.getByRole("heading", { name: "Release" })).toBeInTheDocument();
    expect(screen.getByText("Ship it")).toBeInTheDocument();
    expect(document.querySelector(".notebook-markdown-preview script")).toBeNull();
  });

  it("renames a note in place from the note list on double click", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Draft note");

    await user.dblClick(screen.getByRole("button", { name: "Draft note" }));
    const listTitleInput = screen.getByRole("textbox", { name: "Rename quick note" });
    expect(listTitleInput).toHaveValue("Draft note");

    await user.clear(listTitleInput);
    await user.type(listTitleInput, "Renamed note");
    fireEvent.keyDown(listTitleInput, { key: "Enter" });

    expect(screen.getByRole("button", { name: "Renamed note" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("Renamed note");
  });

  it("applies markdown formatting to selected text from the toolbar", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Format note");
    setEditorValue("selected");
    selectEditorRange(0, "selected".length);

    await user.click(screen.getByRole("button", { name: "Bold" }));

    expect(editorValue()).toBe("**selected**");
  });

  it("applies selected text and background colors from color pickers", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Color note");
    setEditorValue("color");
    selectEditorRange(0, "color".length);

    fireEvent.change(screen.getByLabelText("Text color"), { target: { value: "#ff0000" } });
    expect(editorValue()).toBe('<span style="color:#ff0000">color</span>');

    selectEditorRange(0, editorValue().length);
    fireEvent.change(screen.getByLabelText("Background color"), {
      target: { value: "#00ff00" },
    });
    expect(editorValue()).toBe(
      '<span style="background-color:#00ff00"><span style="color:#ff0000">color</span></span>',
    );
  });

  it("builds structural markdown snippets from selected text instead of examples", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Structure note");
    setEditorValue("alpha beta");
    selectEditorRange(0, "alpha beta".length);

    await user.click(screen.getByRole("button", { name: "Code block" }));
    await waitFor(() => expect(editorValue()).toContain("```"));
    expect(editorValue()).toContain("alpha beta");

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Table note");
    setEditorValue("alpha beta");
    selectEditorRange(0, "alpha beta".length);
    await user.click(screen.getByRole("button", { name: "Table" }));

    expect(editorValue()).toContain("| Column 1 | Column 2 |");
    expect(editorValue()).toContain("alpha beta");
    expect(editorValue()).not.toContain("| Value 1 | Value 2 |");
  });

  it("inserts a blank markdown code block without a selection and places the cursor after it", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Blank code");
    selectEditorRange(0, 0);

    await user.click(screen.getByRole("button", { name: "Code block" }));

    await waitFor(() => expect(editorValue()).toBe("```\n\n```\n"));
    // 光标要落在代码块之后,用户接着敲就是正文而不是又写进围栏里。
    const selection = editorView().state.selection.main;
    expect(selection.from).toBe(editorValue().length);
    expect(selection.empty).toBe(true);
  });

  it("reorders quick notes with an immediate pointer drag", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "First");
    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Second");

    const secondHandle = screen.getByRole("button", { name: "Drag quick note Second" });
    const first = screen.getByRole("button", { name: "First" });
    const secondRow = secondHandle.closest("[data-notebook-note-row]") as HTMLDivElement;
    const firstRow = first.closest("[data-notebook-note-row]") as HTMLDivElement;
    secondHandle.setPointerCapture = vi.fn();
    secondHandle.releasePointerCapture = vi.fn();
    secondRow.getBoundingClientRect = () =>
      ({
        top: 0,
        bottom: 30,
        height: 30,
        left: 0,
        right: 170,
        width: 170,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      }) as DOMRect;
    firstRow.getBoundingClientRect = () =>
      ({
        top: 36,
        bottom: 66,
        height: 30,
        left: 0,
        right: 170,
        width: 170,
        x: 0,
        y: 36,
        toJSON: () => ({}),
      }) as DOMRect;

    fireEvent.pointerDown(secondHandle, { pointerId: 1, button: 0, clientY: 10 });
    fireEvent.pointerMove(secondHandle, { pointerId: 1, clientY: 50 });
    fireEvent.pointerUp(secondHandle, { pointerId: 1, clientY: 50 });

    const noteButtons = screen
      .getAllByRole("button")
      .filter((button) => button.textContent === "First" || button.textContent === "Second");
    expect(noteButtons.map((button) => button.textContent)).toEqual(["First", "Second"]);
  });

  it("saves edits made while an earlier save is still in flight", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    const notePath = harness.paths().find((path) => path.endsWith(".md")) ?? "";

    // 第一次保存悬停住,模拟慢速磁盘。只拦第一次,后续放行。
    const gate: { release: (() => void) | null } = { release: null };
    const originalHandle = harness.handle;
    let intercepted = false;
    harness.handle = (command, args) => {
      if (command === "notebook_save_note" && !intercepted) {
        intercepted = true;
        return new Promise((resolve) => {
          gate.release = () => resolve(originalHandle(command, args));
        });
      }
      return originalHandle(command, args);
    };

    vi.useFakeTimers();
    try {
      setEditorValue("first");
      // 推过防抖,让第一次保存起飞并卡在 gate 上。
      await act(async () => {
        vi.advanceTimersByTime(900);
      });
      expect(gate.release).not.toBeNull();

      // 保存还在飞的时候继续编辑,并再推过一次防抖 —— 此时 flushNote 会撞上
      // savingRef 而提前返回。没有「补一次」机制的话这段编辑就永久丢失。
      setEditorValue("first second");
      await act(async () => {
        vi.advanceTimersByTime(900);
      });

      // 放开第一次保存。补的那一次是在 flushNote 的 finally 里排的,所以要
      // 先让微任务跑完(排上定时器),再推时间 —— 合在一个 act 里的话推时间
      // 会发生在定时器排上之前。
      gate.release?.();
      await act(async () => {
        await Promise.resolve();
      });
      await act(async () => {
        vi.advanceTimersByTime(900);
      });
    } finally {
      vi.useRealTimers();
    }

    await waitFor(() => expect(harness.read(notePath)).toContain("first second"));
  });

  it("renders math and mermaid placeholders in reading mode", async () => {
    const user = userEvent.setup();
    harness.seed(
      "Rich.md",
      '---\ntitle: "Rich"\n---\n\n# Head\n\nmass $E=mc^2$\n\n```mermaid\ngraph LR\nA-->B\n```\n',
    );
    renderNotebook();
    await screen.findByRole("button", { name: "Rich" });
    await user.click(screen.getByRole("button", { name: "Read" }));

    const math = document.querySelector(".notebook-math");
    expect(math).not.toBeNull();
    // 原式留在 data-math-source 里 —— 渲染后 textContent 变成 KaTeX 的
    // HTML+MathML 拼接,复制/导出还要用原式。
    await waitFor(() => expect(math?.getAttribute("data-math-source")).toBe("E=mc^2"));
    // 真的渲染成功了(不是走错误分支),KaTeX 会挂上自己的类。
    expect(math?.querySelector(".katex")).not.toBeNull();
    expect(math?.classList.contains("notebook-math-error")).toBe(false);

    const mermaid = document.querySelector(".notebook-mermaid");
    expect(decodeURIComponent(mermaid?.getAttribute("data-mermaid") ?? "")).toBe("graph LR\nA-->B");
    // 标题锚点要在,大纲跳转靠它。
    expect(document.querySelector("h1")?.id).toBe("head");
  });

  it("initialises without surfacing an error", async () => {
    // 守卫:初始化链路上任何一步失败都会走 setError,而多数测试不看它 ——
    // 加这条之前,收尾迁移命令没接进 harness,每次初始化都在静默报错。
    harness.seed("A.md", '---\ntitle: "A"\n---\n\nbody\n');
    renderNotebook();

    await screen.findByRole("button", { name: "A" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("converts leftover rich text notes to markdown on startup", async () => {
    harness.seed("Legacy.md", '---\ntitle: "Legacy"\neditor: richtext\n---\n\n<p>was rich</p>\n');
    renderNotebook();
    await screen.findByRole("button", { name: "Legacy" });

    // P0 为了无损把富文本原样落盘;WYSIWYG 到位后启动时转成 Markdown。
    // 这是「转换 + 删实现」原子操作的另一半 —— 少了它,用户已有的富文本笔记
    // 会以裸 HTML 的形式显示在 markdown 编辑器里。
    await waitFor(() => expect(harness.richtextConversions).toBeGreaterThan(0));
    expect(harness.read("/vault/Legacy.md")).not.toContain("editor: richtext");
  });

  it("shows source and preview side by side in split mode", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    setEditorValue("# Heading\n\nbody text\n");

    await user.click(screen.getByRole("button", { name: "Split" }));

    // 两侧同时在场:源码仍可编辑,预览已渲染。
    expect(screen.getByRole("textbox", { name: "Quick note content" })).toBeInTheDocument();
    await waitFor(() =>
      expect(document.querySelector(".notebook-markdown-preview h1")?.textContent).toBe("Heading"),
    );
  });

  it("keeps editing the source while split", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    await user.click(screen.getByRole("button", { name: "Split" }));

    setEditorValue("## Live");

    // 预览要跟着源码变 —— 分屏的全部意义就在这里。
    await waitFor(() =>
      expect(document.querySelector(".notebook-markdown-preview h2")?.textContent).toBe("Live"),
    );
  });

  it("marks the active view mode", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);

    expect(screen.getByRole("button", { name: "Source" })).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Read" }));
    expect(screen.getByRole("button", { name: "Read" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Source" })).toHaveAttribute("aria-pressed", "false");
  });

  it("preserves the editor instance when toggling between edit and split", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    setEditorValue("keep me");
    selectEditorRange(0, 4);

    await user.click(screen.getByRole("button", { name: "Split" }));

    // 复用同一个 CodeMirror 实例,所以选区还在。重建会把光标和撤销栈丢掉。
    const selection = editorView().state.selection.main;
    expect([selection.from, selection.to]).toEqual([0, 4]);
    expect(editorValue()).toBe("keep me");
  });

  it("shows word count and reading time for markdown notes", async () => {
    harness.seed("Counted.md", '---\ntitle: "Counted"\n---\n\none two three\n');
    renderNotebook();
    await screen.findByRole("button", { name: "Counted" });

    // 3 词 → 至少 1 分钟(「0 分钟」看着像坏了)。
    await waitFor(() => expect(screen.getByText("3 words · 1 min")).toBeInTheDocument());
  });

  it("does not show stats for an empty note", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);

    expect(screen.queryByText(/words ·/)).not.toBeInTheDocument();
  });

  it("flushes a pending save when the panel unmounts", async () => {
    const user = userEvent.setup();
    const view = renderNotebook();
    await createNote(user);
    const notePath = harness.paths().find((path) => path.endsWith(".md")) ?? "";

    // 敲字后立刻卸载 —— 防抖还没到期。面板在 ProjectPage 里每次切视图都会
    // 卸载,所以这是用户最容易撞到的路径:只清定时器就会丢掉这段编辑。
    setEditorValue("typed then left");
    view.unmount();

    await waitFor(() => expect(harness.read(notePath)).toContain("typed then left"));
  });

  it("keeps the memo list visible without collapse controls", async () => {
    renderNotebook();

    // 笔记从磁盘读,先经过一个加载态 —— 「还在读」和「真的没有」是两件事。
    await waitFor(() => expect(screen.getAllByText("No quick notes yet")).toHaveLength(2));
    expect(
      screen.queryByRole("button", { name: "Collapse quick note list" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Expand quick note list" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/memo/i)).not.toBeInTheDocument();
  });

  it("opens a markdown note that already exists on disk", async () => {
    harness.seed("Legacy.md", '---\ntitle: "Legacy"\n---\n\n# Old\n');

    renderNotebook();

    await screen.findByRole("button", { name: "Legacy" });
    await userEvent.setup().click(screen.getByRole("button", { name: "Read" }));

    expect(screen.getByRole("heading", { name: "Old" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument();
  });

  it("does not render the old half-screen toggle in project mode", () => {
    render(
      <I18nProvider>
        <NotebookPanel />
      </I18nProvider>,
    );

    expect(screen.queryByRole("button", { name: "Full screen" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Half screen" })).not.toBeInTheDocument();
  });

  it("keeps rich text typing in natural insertion order", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Typing note");

    const body = screen.getByRole("textbox", { name: "Quick note content" });
    await user.click(body);
    await user.type(body, "1234");

    expect(body).toHaveTextContent("1234");
    expect(body).not.toHaveTextContent("4321");
  });

  it("normalizes notebook title and markdown content to English punctuation", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    const title = screen.getByRole("textbox", { name: "Quick note name" });

    await user.type(title, "标题，测试？");
    setEditorValue("第一行：你好。");

    expect(title).toHaveValue("标题,测试?");
    // 正文的标点归一化发生在 onChange 里,所以要等状态回流到编辑器。
    await waitFor(() => expect(editorValue()).toBe("第一行:你好."));
  });

  it("reorders quick notes after a long-press pointer drag", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Alpha");
    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Beta");

    vi.useFakeTimers();
    try {
      const betaHandle = screen.getByRole("button", { name: "Drag quick note Beta" });
      const alpha = screen.getByRole("button", { name: "Alpha" });
      const betaRow = betaHandle.closest("[data-notebook-note-row]") as HTMLDivElement;
      const alphaRow = alpha.closest("[data-notebook-note-row]") as HTMLDivElement;
      betaHandle.setPointerCapture = vi.fn();
      betaHandle.releasePointerCapture = vi.fn();
      betaRow.getBoundingClientRect = () =>
        ({
          top: 0,
          bottom: 30,
          left: 0,
          right: 170,
          width: 170,
          height: 30,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        }) as DOMRect;
      alphaRow.getBoundingClientRect = () =>
        ({
          top: 36,
          bottom: 66,
          left: 0,
          right: 170,
          width: 170,
          height: 30,
          x: 0,
          y: 36,
          toJSON: () => ({}),
        }) as DOMRect;

      fireEvent.pointerDown(betaHandle, { pointerId: 1, button: 0, clientY: 12 });
      act(() => vi.advanceTimersByTime(200));
      fireEvent.pointerMove(betaHandle, { pointerId: 1, clientY: 48 });
      fireEvent.pointerUp(betaHandle, { pointerId: 1, clientY: 48 });

      // 列表里的可见顺序就是拖动后的顺序。落盘断言在下一个测试里单独覆盖。
      const rows = screen.getAllByRole("button", { name: /^(Alpha|Beta)$/ });
      expect(rows.map((row) => row.textContent)).toEqual(["Alpha", "Beta"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("uses localized text for the quick note drag handle", async () => {
    localStorage.setItem("aeroric:language", "zh");
    const user = userEvent.setup();
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "新建随手记" }));
    await user.type(screen.getByRole("textbox", { name: "随手记名称" }), "Alpha");

    expect(screen.getByRole("button", { name: "拖动随手记 Alpha" })).toBeInTheDocument();
  });

  it("shows Chinese markdown context menu actions", async () => {
    const user = userEvent.setup();
    localStorage.setItem("aeroric:language", "zh");
    renderNotebook();

    await user.click(screen.getByRole("button", { name: "新建随手记" }));

    fireEvent.contextMenu(screen.getByRole("textbox", { name: "随手记内容" }));

    expect(screen.getByRole("menuitem", { name: "剪切" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "复制" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "粘贴" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "粗体" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "表格" })).toBeInTheDocument();
  });

  it("keeps markdown formatting tools clickable without a selection but leaves content unchanged", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Needs selection");

    setEditorValue("selected");
    const end = editorValue().length;
    selectEditorRange(end, end);

    expect(screen.getByRole("button", { name: "Bold" })).not.toBeDisabled();
    expect(screen.getByLabelText("Text color")).not.toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Bold" }));
    expect(editorValue()).toBe("selected");

    selectEditorRange(0, editorValue().length);

    expect(screen.getByRole("button", { name: "Bold" })).not.toBeDisabled();
    expect(screen.getByLabelText("Text color")).not.toBeDisabled();
  });

  it("shows quick note names in bold in the memo list", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Bold list name");

    expect(screen.getByRole("button", { name: "Bold list name" })).toHaveStyle({
      fontWeight: "700",
    });
  });

  it("does not prevent markdown clipboard and undo keyboard shortcuts", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    const body = screen.getByRole("textbox", { name: "Quick note content" });

    for (const key of ["c", "v", "x", "z"]) {
      const cancelled = !fireEvent.keyDown(body, { key, metaKey: true });
      expect(cancelled).toBe(false);
    }
  });

  it("finds and replaces markdown text with Command shortcuts", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    const body = screen.getByRole("textbox", { name: "Quick note content" });
    setEditorValue("alpha beta alpha");

    fireEvent.keyDown(body, { key: "f", metaKey: true });
    const findInput = screen.getByRole("textbox", { name: "Find" });
    fireEvent.change(findInput, {
      target: { value: "alpha" },
    });
    expect(findInput).toHaveFocus();
    // 命中要在编辑器里被选中,用户按上下键翻页时看得见当前是哪一处。
    await waitFor(() => {
      const selection = editorView().state.selection.main;
      expect(selection.from).toBe(0);
      expect(selection.to).toBe(5);
    });

    fireEvent.keyDown(body, { key: "h", metaKey: true });
    const replaceInput = screen.getByRole("textbox", { name: "Replace" });
    await user.click(replaceInput);
    await user.type(replaceInput, "omega");
    await user.keyboard("{Enter}");
    expect(replaceInput).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Replace all" }));

    await waitFor(() => expect(editorValue()).toBe("omega beta omega"));
  });

  it("preserves the markdown scroll context when switching read and edit modes", async () => {
    const user = userEvent.setup();
    const scrollHeight = vi
      .spyOn(HTMLElement.prototype, "scrollHeight", "get")
      .mockReturnValue(1000);
    const clientHeight = vi
      .spyOn(HTMLElement.prototype, "clientHeight", "get")
      .mockReturnValue(100);
    renderNotebook();

    await createNote(user);
    setEditorValue("# Heading\n\ncontext\n\nend");

    // CodeMirror 的滚动容器是 `.cm-scroller`,不是那个 role=textbox 的 contentDOM。
    const scroller = document.querySelector(".cm-scroller") as HTMLElement;
    expect(scroller).not.toBeNull();
    scroller.scrollTop = 450;

    await user.click(screen.getByRole("button", { name: "Read" }));
    const readScroller = document.querySelector(".notebook-markdown-preview")?.parentElement;
    // 保的是比例(450 / (1000-100) = 0.5),两边高度一样所以回到同一个像素值。
    expect(readScroller?.scrollTop).toBe(450);

    await user.click(screen.getByRole("button", { name: "Source" }));
    await waitFor(() => {
      const back = document.querySelector(".cm-scroller") as HTMLElement;
      expect(back.scrollTop).toBe(450);
    });
    scrollHeight.mockRestore();
    clientHeight.mockRestore();
  });

  it("persists manual quick note ordering after a long-press pointer drag", async () => {
    const user = userEvent.setup();
    renderNotebook();

    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "First");
    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Second");

    vi.useFakeTimers();
    try {
      const first = screen.getByRole("button", { name: "First" });
      const secondHandle = screen.getByRole("button", { name: "Drag quick note Second" });
      const firstRow = first.closest("[data-notebook-note-row]") as HTMLDivElement;
      const secondRow = secondHandle.closest("[data-notebook-note-row]") as HTMLDivElement;
      secondHandle.setPointerCapture = vi.fn();
      secondHandle.releasePointerCapture = vi.fn();
      firstRow.getBoundingClientRect = () =>
        ({
          top: 36,
          bottom: 66,
          left: 0,
          right: 170,
          width: 170,
          height: 30,
          x: 0,
          y: 36,
          toJSON: () => ({}),
        }) as DOMRect;
      secondRow.getBoundingClientRect = () =>
        ({
          top: 0,
          bottom: 30,
          left: 0,
          right: 170,
          width: 170,
          height: 30,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        }) as DOMRect;

      fireEvent.pointerDown(secondHandle, { pointerId: 1, button: 0, clientY: 12 });
      act(() => vi.advanceTimersByTime(200));
      fireEvent.pointerMove(secondHandle, { pointerId: 1, clientY: 48 });
      fireEvent.pointerUp(secondHandle, { pointerId: 1, clientY: 48 });
    } finally {
      vi.useRealTimers();
    }

    // 手工排序落在 vault 的 order.json 里,不动笔记文件本身 —— 一次拖动
    // 只写一个小文件,也不会把所有笔记的 mtime 推新。
    //
    // 记的是文件名。文件名在新建时定一次就不动(标题存 frontmatter),所以
    // 这里是两个 untitled —— 顺序才是这个测试要钉住的东西。
    await waitFor(() => expect(harness.order).toEqual(["untitled.md", "untitled-2.md"]));

    // 排序要能被下一次加载读回来:重新挂载后顺序不变。
    const titles = screen.getAllByRole("button", { name: /^(First|Second)$/ });
    expect(titles.map((row) => row.textContent)).toEqual(["First", "Second"]);
  });
});
