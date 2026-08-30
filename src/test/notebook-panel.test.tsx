import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";
import { EditorView } from "@uiw/react-codemirror";
import { NotebookVaultHarness } from "./notebookVaultHarness";
import { registerAppDialogHandler, resetAppDialogHandlerForTests } from "../lib/appDialog";
import { triggerResize } from "./resizeObserverStub";

/* 随手记的笔记现在是磁盘上的 .md 文件,新建 / 保存 / 删除都要过 Tauri 命令。
 * 用一个内存 vault 顶上,这样这些测试仍然在验证真实行为(写进去能读回来、
 * 冲突真的会触发),而不是验证 mock 被调用过。 */
let harness: NotebookVaultHarness;

/* 注意 `async` 不是可以省的:harness 的失败分支是同步 `throw`,而真实 `invoke`
 * 只会以 rejection 的形式报错。写成 `Promise.resolve(harness.handle(...))` 的话
 * 抛错会在 promise 生成之前同步逃出调用点,凡是"发出去不等结果、用 .catch 收错"
 * 的写法都会变成未捕获异常 —— 测到的是 mock 的怪癖,不是产品行为。 */
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: Record<string, unknown>) =>
    harness.handle(command, args ?? {}),
}));

/* 剪贴板:面板写走 `navigator.clipboard.writeText`、读走 Tauri 的 clipboard 插件
 * (与 Aeroric 别处一致)。
 *
 * 写侧**不自己 stub** —— `userEvent.setup()` 会装一个能往返读写的实现,而它在每个
 * 测试里调用,时机在 `beforeEach` 之后,会覆盖掉我们自己装的那个(踩过)。
 * 直接用它,顺带比手写 stub 更接近真实浏览器行为。
 *
 * 读侧要 mock:Tauri 插件在测试环境里不存在。让它转读 `navigator.clipboard`,
 * 于是「复制 → 粘贴」在测试里是真的经过剪贴板走了一圈。 */
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: () => navigator.clipboard.readText(),
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

  it("collapses the note list in the compact tier and reopens it from the toggle", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note name" }), "Compact");

    // jsdom 一律量到 0,所以宽度得自己喂。400px 是面板在项目视图里的常见宽度。
    const panel = screen.getByRole("region", { name: "Quick Notes" });
    panel.getBoundingClientRect = () => ({ width: 400, height: 600 }) as DOMRect;
    act(() => triggerResize());

    // 紧凑档:列宽压到 0,整宽让给正文。列表**没有被卸载** —— 卸载会丢掉它的
    // 滚动位置,而且开关一次就要重建整列。
    expect(panel.style.gridTemplateColumns).toBe("0px minmax(0, 1fr)");
    expect(screen.getByRole("button", { name: "Compact" })).toBeInTheDocument();

    // 开关把它拉回来。
    await user.click(screen.getByRole("button", { name: "Show note list" }));
    expect(panel.style.gridTemplateColumns).toBe("170px minmax(0, 1fr)");

    // 正文一路没丢。
    expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("Compact");
  });

  it("widens the note list in the wide tier and hides the list toggle", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);

    const panel = screen.getByRole("region", { name: "Quick Notes" });
    panel.getBoundingClientRect = () => ({ width: 1200, height: 600 }) as DOMRect;
    act(() => triggerResize());

    expect(panel.style.gridTemplateColumns).toBe("220px minmax(0, 1fr)");
    // 宽档列表一直在,开关只会是噪音。
    expect(screen.queryByRole("button", { name: "Show note list" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Hide note list" })).toBeNull();
  });

  it("reports the save lifecycle in the status bar", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    const status = () => screen.getByRole("status").textContent ?? "";

    // 刚建出来的笔记和磁盘一致。
    expect(status()).toContain("Saved");

    // 敲字之后到防抖到期之前是「未保存」—— 这段窗口正是状态栏存在的理由:
    // 自动保存是静默的,用户切走之前没有别的办法确认那几个字落盘了没有。
    setEditorValue("typed but not yet flushed");
    await waitFor(() => expect(status()).toContain("Unsaved"));

    // 防抖到期 → 落盘 → 回到「已保存」。
    await waitFor(() => expect(status()).toContain("Saved"), { timeout: 3000 });
  });

  it("reports a failed save in the status bar", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);

    const originalHandle = harness.handle;
    harness.handle = (command, args) => {
      if (command === "notebook_save_note") return Promise.reject(new Error("disk on fire"));
      return originalHandle(command, args);
    };

    setEditorValue("this write will fail");
    // 写失败必须看得见。静默失败 + 「已保存」是最坏的组合:用户会放心地切走。
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Save failed"), {
      timeout: 3000,
    });
  });

  it("reports saved after the user keeps the disk version of a conflict", async () => {
    const notePath = harness.seed("Shared.md", '---\ntitle: "Shared"\n---\n\nmine\n');
    renderNotebook();
    await screen.findByRole("button", { name: "Shared" });

    const unregister = registerAppDialogHandler(async () => false);
    try {
      harness.externalWrite(notePath, '---\ntitle: "Shared"\n---\n\ntheirs\n');
      setEditorValue("ours");
      // 选「保留磁盘版本」后编辑器换成磁盘那一版,两边一致了 —— 报「保存失败」
      // 会让用户以为编辑丢了、去做多余的补救。
      await waitFor(() => expect(editorValue()).toContain("theirs"), { timeout: 3000 });
      await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Saved"));
    } finally {
      unregister();
      resetAppDialogHandlerForTests();
    }
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
    // 等的是「渲染完了」而不是 data-math-source:后者在 `await getKatex()` **之前**
    // 就写上了(见 noteVisuals 的 renderMathBlock),等它会早一拍放行,满负载跑
    // 整个套件时 KaTeX 的动态 import 还没回来,下面那条 `.katex` 就会扑空。
    await waitFor(() => expect(math?.querySelector(".katex")).not.toBeNull());
    // 原式留在 data-math-source 里 —— 渲染后 textContent 变成 KaTeX 的
    // HTML+MathML 拼接,复制/导出还要用原式。
    expect(math?.getAttribute("data-math-source")).toBe("E=mc^2");
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

  /** 在编辑器上开右键菜单并点某一项。 */
  async function runContextAction(user: ReturnType<typeof userEvent.setup>, name: string) {
    fireEvent.contextMenu(screen.getByRole("textbox", { name: "Quick note content" }));
    await user.click(await screen.findByRole("menuitem", { name }));
  }

  it("copies the selection to the clipboard", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    setEditorValue("alpha beta");
    selectEditorRange(0, 5);

    await runContextAction(user, "Copy");

    // 断言剪贴板真的收到了内容 —— 不是断言某个 API 被调用过。
    // 旧实现用 document.execCommand,它作用于 DOM 选区,改不动 CodeMirror 的
    // EditorState,所以这条断言在旧实现下必然失败。
    await waitFor(async () => expect(await navigator.clipboard.readText()).toBe("alpha"));
    // 复制不该改文档。
    expect(editorValue()).toBe("alpha beta");
  });

  it("cuts the selection: clipboard gets it, document loses it", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    setEditorValue("alpha beta");
    selectEditorRange(0, 6);

    await runContextAction(user, "Cut");

    await waitFor(async () => expect(await navigator.clipboard.readText()).toBe("alpha "));
    await waitFor(() => expect(editorValue()).toBe("beta"));
  });

  it("pastes clipboard content over the selection", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    setEditorValue("keep REPLACE end");
    await navigator.clipboard.writeText("pasted");
    selectEditorRange(5, 12);

    await runContextAction(user, "Paste");

    await waitFor(() => expect(editorValue()).toBe("keep pasted end"));
  });

  it("does not cut when the clipboard write fails", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    setEditorValue("precious");
    selectEditorRange(0, 8);

    // 写剪贴板失败(WebView 权限被拒是真实场景)。
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: () => Promise.reject(new Error("denied")) },
    });

    await runContextAction(user, "Cut");

    // 关键:复制失败就不能删 —— 否则内容既没进剪贴板也没留在文档里。
    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
    expect(editorValue()).toBe("precious");
  });

  it("ignores copy with an empty selection", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    setEditorValue("text");
    selectEditorRange(2, 2);
    await navigator.clipboard.writeText("untouched");

    await runContextAction(user, "Copy");

    // 空选区不该把剪贴板清空 —— 用户可能正拿着别处复制的东西。
    expect(await navigator.clipboard.readText()).toBe("untouched");
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

  it("does not resurrect a deleted note from a pending autosave", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    const notePath = harness.paths().find((path) => path.endsWith(".md")) ?? "";
    expect(notePath).not.toBe("");

    vi.useFakeTimers();
    try {
      // 敲字后立刻删除 —— 防抖还没到期。不取消那个定时器的话它会在 800ms 后
      // 醒来,把刚进回收站的文件重新写出来,用户会看到"删掉的笔记又回来了"。
      setEditorValue("about to be deleted");
      fireEvent.click(screen.getByRole("button", { name: "Delete" }));
      await act(async () => {
        await Promise.resolve();
      });
      await act(async () => {
        vi.advanceTimersByTime(2000);
      });
    } finally {
      vi.useRealTimers();
    }

    expect(harness.read(notePath)).toBeUndefined();
  });

  it("does not carry a deleted note's save state onto a new note at the same path", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    const notePath = harness.paths().find((path) => path.endsWith(".md")) ?? "";

    // 敲字(状态变成「未保存」)后立刻删掉,防抖还没到期。
    setEditorValue("about to be deleted");
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Unsaved"));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(harness.read(notePath)).toBeUndefined());

    // 文件名会被回收利用:新笔记落在刚腾出来的同一个路径上。保存状态按路径存,
    // 不随笔记消失一起清掉的话,新笔记一出生就顶着上一条的「未保存」。
    await createNote(user);
    expect(harness.paths()).toContain(notePath);
    expect(screen.getByRole("status").textContent).toContain("Saved");
  });

  it("keeps the version on disk when the user declines to overwrite a conflict", async () => {
    // 磁盘上先有一条笔记,面板打开它。
    const notePath = harness.seed("Shared.md", '---\ntitle: "Shared"\n---\n\nmine\n');
    renderNotebook();
    await screen.findByRole("button", { name: "Shared" });
    await screen.findByRole("textbox", { name: "Quick note content" });

    // 用户选「保留磁盘版本」。没注册 host 时 confirm 直接返回 false,也是这条
    // 分支 —— 但那是巧合,显式注册才算真的钉住了用户的选择。
    const requests: string[] = [];
    const unregister = registerAppDialogHandler(async (request) => {
      requests.push(request.kind);
      return false;
    });

    try {
      // 外部编辑器改了同一个文件 —— 面板手里的指纹过期了,下一次保存会撞冲突。
      harness.externalWrite(notePath, '---\ntitle: "Shared"\n---\n\ntheirs\n');

      setEditorValue("ours");
      await waitFor(() => expect(requests).toEqual(["confirm"]));

      // 磁盘保持外部那一版,不被覆盖。
      await waitFor(() => expect(harness.read(notePath)).toContain("theirs"));
      expect(harness.read(notePath)).not.toContain("ours");
      // 编辑器换成磁盘的内容 —— 否则用户接着敲字,下一次保存又会撞同一个冲突。
      await waitFor(() => expect(editorValue()).toContain("theirs"));
    } finally {
      unregister();
      resetAppDialogHandlerForTests();
    }
  });

  describe("笔记 tab 条", () => {
    /** 播两条笔记并把它们都打开(点一下就会开出 tab)。 */
    async function openTwoNotes() {
      harness.seed("First.md", '---\ntitle: "First"\n---\n\none\n');
      harness.seed("Second.md", '---\ntitle: "Second"\n---\n\ntwo\n');
      renderNotebook();
      await screen.findByRole("button", { name: "First" });
      await screen.findByRole("button", { name: "Second" });
      // 初始选中的那条已经有 tab 了,再点开另一条就是两个。
      fireEvent.click(screen.getByRole("button", { name: "First" }));
      await screen.findByDisplayValue("First");
      fireEvent.click(screen.getByRole("button", { name: "Second" }));
      await screen.findByDisplayValue("Second");
    }

    it("只开一条时不占地方", async () => {
      const user = userEvent.setup();
      renderNotebook();
      await createNote(user);

      // tab 条在只开一条时没有信息量,而随手记大多数时候就是开着一条。
      expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    });

    it("开第二条才出现,并且高亮当前那条", async () => {
      await openTwoNotes();

      const strip = screen.getByRole("tablist", { name: "Open quick notes" });
      expect(strip).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: "First" })).toHaveAttribute("aria-selected", "false");
      expect(screen.getByRole("tab", { name: "Second" })).toHaveAttribute("aria-selected", "true");
    });

    it("点 tab 切换笔记", async () => {
      await openTwoNotes();

      fireEvent.click(screen.getByRole("tab", { name: "First" }));

      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("First"),
      );
      expect(screen.getByRole("tab", { name: "First" })).toHaveAttribute("aria-selected", "true");
    });

    it("tab 条消失时不重建编辑器", async () => {
      /* 关掉非当前的那条 tab:tab 条从两个掉到一个因而整条消失,当前笔记却没变 ——
         它的 CodeMirror 必须还是原来那个实例。

         (开 tab 观察不到这件事:打开另一条笔记本来就会换实例,key 带着笔记 id。)

         注:把 tab 条改成 `{tabs.length > 1 && <NoteTabStrip/>}` 并不会让这条测试
         变红,实测过。cond 为假时那个位置仍然占着一个 child slot,兄弟不挪位。
         这条测试钉的是「关 tab 不动当前编辑器」这个行为本身,不是某一种写法。 */
      await openTwoNotes();
      // 切回 First 并留下一个选区,然后关掉 Second 的 tab。
      fireEvent.click(screen.getByRole("tab", { name: "First" }));
      await screen.findByDisplayValue("First");
      selectEditorRange(1, 3);

      fireEvent.click(screen.getByRole("button", { name: "Close Second" }));
      await waitFor(() => expect(screen.queryByRole("tablist")).not.toBeInTheDocument());

      // 当前还是 First,所以它的编辑器必须是原来那个实例,选区还在。
      expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("First");
      const selection = editorView().state.selection.main;
      expect([selection.from, selection.to]).toEqual([1, 3]);
    });

    it("关 tab 不删笔记", async () => {
      await openTwoNotes();

      fireEvent.click(screen.getByRole("button", { name: "Close Second" }));

      await waitFor(() => expect(screen.queryByRole("tablist")).not.toBeInTheDocument());
      // 笔记还在磁盘上,也还在列表里 —— 关 tab 只是收起来,点一下就回来。
      expect(harness.read("/vault/Second.md")).toContain("two");
      expect(screen.getByRole("button", { name: "Second" })).toBeInTheDocument();
    });

    it("关掉当前 tab 时落到左边那个", async () => {
      /* 必须开三条并关**正中间**那条。两条的时候 index-1 和 index+1 都指向剩下的
         同一个 tab,左右取反看不出区别;三条但关的是最后一条也一样(index+1 是
         undefined,会回落到左边)—— 那样的断言是绿的但什么都没钉住。

         而且 tab 的顺序是**打开顺序**,不是列表顺序:列表按修改时间倒序,所以初始
         选中的是最后播的那条。下面先把顺序断言出来,免得这条测试哪天又悄悄变回
         「关最后一个」。 */
      harness.seed("First.md", '---\ntitle: "First"\n---\n\none\n');
      harness.seed("Second.md", '---\ntitle: "Second"\n---\n\ntwo\n');
      harness.seed("Third.md", '---\ntitle: "Third"\n---\n\nthree\n');
      renderNotebook();
      await screen.findByRole("button", { name: "First" });
      for (const name of ["First", "Second"]) {
        fireEvent.click(screen.getByRole("button", { name }));
        await screen.findByDisplayValue(name);
      }

      // 初始选中 Third(最新),然后依次开 First、Second。
      expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
        "Third",
        "First",
        "Second",
      ]);

      // 切到正中间的 First 再关掉它:左边是 Third,右边是 Second,两侧都有。
      fireEvent.click(screen.getByRole("tab", { name: "First" }));
      await screen.findByDisplayValue("First");
      fireEvent.click(screen.getByRole("button", { name: "Close First" }));

      // 落到左边的 Third,不是右边的 Second —— 和大多数编辑器一致,关掉一串 tab
      // 时手不用动。
      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("Third"),
      );
    });

    it("删掉笔记会把它的 tab 一起摘掉", async () => {
      await openTwoNotes();
      expect(screen.getByRole("tab", { name: "Second" })).toBeInTheDocument();

      // 死 tab 是这里最容易漏的 bug:笔记没了但 tab 还在,点它什么都不会发生。
      fireEvent.click(screen.getByRole("button", { name: "Delete" }));

      await waitFor(() => expect(screen.queryByRole("tab", { name: "Second" })).toBeNull());
    });

    it("关 tab 时把挂起的改动落盘", async () => {
      await openTwoNotes();
      const editor = screen.getByRole("textbox", { name: "Quick note content" });
      expect(editor).toBeInTheDocument();

      vi.useFakeTimers();
      try {
        // 敲字后立刻关 tab —— 防抖还没到期。不落盘的话这段编辑就没了。
        setEditorValue("typed then closed");
        await act(async () => {
          vi.advanceTimersByTime(100);
        });
        fireEvent.click(screen.getByRole("button", { name: "Close Second" }));
        await act(async () => {
          await Promise.resolve();
        });
      } finally {
        vi.useRealTimers();
      }

      await waitFor(() => expect(harness.read("/vault/Second.md")).toContain("typed then closed"));
    });

    it("保存失败过的 tab,关闭要确认;取消就留着", async () => {
      await openTwoNotes();

      const requests: string[] = [];
      const unregister = registerAppDialogHandler(async (request) => {
        requests.push(request.kind);
        return false;
      });

      try {
        // 让这一次保存真的失败,把 tab 顶到「保存失败」态。
        harness.failNextSave = true;
        setEditorValue("never lands on disk");
        await waitFor(() =>
          expect(screen.getByRole("status").textContent).toContain("Save failed"),
        );

        // 失败态的 tab 会改可访问名字,读屏才能听见状态栏播报不到的那一条。
        expect(screen.getByRole("tab", { name: "Second (save failed)" })).toBeInTheDocument();

        fireEvent.click(screen.getByRole("button", { name: "Close Second" }));

        // 自动保存的应用不该拿「未保存」去问用户 —— 但保存真的失败过是另一回事,
        // 关掉就等于丢掉那段编辑,所以这一档要确认。用户选了取消,tab 得留着。
        await waitFor(() => expect(requests).toEqual(["confirm"]));
        expect(screen.getByRole("tab", { name: "Second (save failed)" })).toBeInTheDocument();
      } finally {
        unregister();
        resetAppDialogHandlerForTests();
      }
    });
  });

  describe("⌘S", () => {
    it("不等防抖,立刻落盘", async () => {
      const user = userEvent.setup();
      renderNotebook();
      await createNote(user);
      const notePath = harness.paths().find((path) => path.endsWith(".md")) ?? "";

      vi.useFakeTimers();
      try {
        setEditorValue("typed but not yet debounced");
        // 防抖是 800ms,这里只走 100ms —— 自动保存还没到期。
        await act(async () => {
          vi.advanceTimersByTime(100);
        });
        expect(harness.read(notePath)).not.toContain("typed but not yet debounced");

        fireEvent.keyDown(screen.getByRole("region", { name: "Quick Notes" }), {
          key: "s",
          metaKey: true,
        });
        await act(async () => {
          await Promise.resolve();
        });
      } finally {
        vi.useRealTimers();
      }

      await waitFor(() => expect(harness.read(notePath)).toContain("typed but not yet debounced"));
    });

    it("没有改动时不写盘", async () => {
      const user = userEvent.setup();
      renderNotebook();
      await createNote(user);
      // 新建后自身会落一次盘,等它安静下来再数。
      await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Saved"));
      const before = harness.saveCalls;

      fireEvent.keyDown(screen.getByRole("region", { name: "Quick Notes" }), {
        key: "s",
        metaKey: true,
      });
      await act(async () => {
        await Promise.resolve();
      });

      // 空写不是无害的:推高 mtime,还会在别人改过磁盘时弹出一个用户没有理由
      // 看到的冲突框 —— 他刚才什么都没改。
      expect(harness.saveCalls).toBe(before);
    });

    it("不冒泡到面板外面", async () => {
      const user = userEvent.setup();
      renderNotebook();
      await createNote(user);

      // 面板外面还有 window 级的键盘监听(ProjectPage 的命令面板就是一个)。
      // 不拦住的话一次 ⌘S 会同时触发面板内和面板外两件事。
      const escaped: string[] = [];
      const spy = (event: KeyboardEvent) => escaped.push(event.key);
      window.addEventListener("keydown", spy);
      try {
        fireEvent.keyDown(screen.getByRole("region", { name: "Quick Notes" }), {
          key: "s",
          metaKey: true,
        });
        expect(escaped).toEqual([]);

        // 对照:没接的键照常放过去,不然用户会以为快捷键坏了。⌘K 现在就是这一类
        // —— 随手记还没有插入链接那种功能给它接。
        fireEvent.keyDown(screen.getByRole("region", { name: "Quick Notes" }), {
          key: "k",
          metaKey: true,
        });
        expect(escaped).toEqual(["k"]);
      } finally {
        window.removeEventListener("keydown", spy);
      }
    });
  });

  describe("版本历史", () => {
    /** 打开当前笔记的历史面板,等它把列表拉回来。 */
    async function openHistory() {
      fireEvent.click(screen.getByRole("button", { name: "Version history" }));
      return screen.findByRole("dialog", { name: /Version history/ });
    }

    function diffText(): string {
      return screen.getByTestId("note-history-diff").textContent ?? "";
    }

    /* 快照列表项按 `aria-pressed` 找,不按文案:harness 的时钟是假的(从 1000 起),
     * 相对时间会落到 `toLocaleString()` 那一支,文案跟着测试机的 locale 变。
     * 顺序和后端 `list` 一致 —— 新的在前。 */
    function historyEntries(dialog: HTMLElement): HTMLButtonElement[] {
      return Array.from(dialog.querySelectorAll<HTMLButtonElement>("button[aria-pressed]"));
    }

    it("打开就选中最新那条并显示它和当前内容的行级 diff", async () => {
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nalpha\nbravo\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nalpha\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nalpha\nbravo-old\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });
      await screen.findByRole("textbox", { name: "Quick note content" });

      await openHistory();

      // 最新那条自动选中(列表里 seedSnapshot 后进的排最前)—— 历史面板里
      // "最近改了什么"是最常见的问题,让用户多点一次没有意义。
      await waitFor(() => expect(diffText()).toContain("-bravo-old"));
      expect(diffText()).toContain("+bravo");
      // 只差一行:frontmatter 和 `alpha` 都没动。按行号逐行比会把整篇报成改动。
      expect(screen.getByText("2 lines differ from the current note")).toBeInTheDocument();
    });

    it("标题没改时 frontmatter 不进 diff", async () => {
      // 快照存的是**整个文件**,当前内容只比 `body` 的话 frontmatter 那三行会
      // 全部报成删除 —— 用户每次打开历史都看到一堆假改动。
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nsame line\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nsame line\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });

      await openHistory();

      await screen.findByText("0 lines differ from the current note");
      expect(diffText()).not.toContain("-title:");
    });

    it("慢的响应不盖掉快的", async () => {
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nnow\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nolder body\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nnewer body\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });

      harness.holdSnapshotReads();
      const dialog = await openHistory();
      await waitFor(() => expect(historyEntries(dialog)).toHaveLength(2));
      // 自动选中的那条(最新)先发起,停住。
      await waitFor(() => expect(harness.heldSnapshotReadCount()).toBe(1));

      // 用户在它飞行途中点了旧的那条。
      fireEvent.click(historyEntries(dialog)[1]);
      await waitFor(() => expect(harness.heldSnapshotReadCount()).toBe(2));

      // 先放行**先发起**的那个(最新那条)。它已经不是当前选中的了。
      harness.releaseSnapshotRead(0);
      await act(async () => {
        await Promise.resolve();
      });
      expect(screen.queryByTestId("note-history-diff")).toBeNull();

      // 再放行用户真正选的那条。
      harness.releaseSnapshotRead(1);
      await waitFor(() => expect(diffText()).toContain("-older body"));
      // 这才是判据:被丢掉的那个响应没有留在界面上,高亮的条目和 diff 对得上。
      expect(diffText()).not.toContain("newer body");
      expect(historyEntries(dialog)[1]).toHaveAttribute("aria-pressed", "true");
      expect(historyEntries(dialog)[0]).toHaveAttribute("aria-pressed", "false");
    });

    it("回滚先等在飞的保存落完,再动磁盘", async () => {
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\ndisk\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nrolled back\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });
      await screen.findByRole("textbox", { name: "Quick note content" });

      await openHistory();
      await waitFor(() => expect(diffText()).toContain("-rolled back"));

      // 关掉面板去敲字,让防抖挂着(800ms 还没到),再开回来点回滚。
      fireEvent.click(screen.getByRole("button", { name: "Close version history" }));
      setEditorValue("typed but not yet saved");
      await openHistory();
      await waitFor(() => expect(diffText()).toContain("+typed but not yet saved"));

      fireEvent.click(screen.getByRole("button", { name: "Restore" }));

      // 磁盘落到快照那一版。不等挂起的保存的话,那次写入会在回滚**之后**落地,
      // 内容是回滚前的正文 —— 用户会看到自己的恢复"没生效"。
      await waitFor(() => expect(harness.read(notePath)).toContain("rolled back"));
      expect(harness.read(notePath)).not.toContain("typed but not yet saved");
      // 编辑器也换成回滚后的内容。
      await waitFor(() => expect(editorValue()).toContain("rolled back"));
      // 那段编辑进了兜底快照(回滚前的磁盘版)—— 它落了盘才有这个效果,
      // 「撤销这次回滚」能把它拿回来。
      expect(harness.snapshotContents(notePath)[0]).toContain("typed but not yet saved");
    });

    it("回滚后接着打字,不会被延迟的外部更新覆盖", async () => {
      /* `@uiw/react-codemirror` 对外部 value 变化有一道「打字闩」:本地刚改过文档的
       * 200ms 内,外部更新存进 pendingUpdate 等闩到期。那个闭包捕获了当时的 value,
       * 于是"回滚 → 立刻接着打字"会在闩到期时把用户刚打的字换成回滚后的内容。
       * 面板靠回滚时重建编辑器(`editorEpoch`)绕开它。 */
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\ndisk\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nrolled back\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });
      await screen.findByRole("textbox", { name: "Quick note content" });

      // 先本地改一次,把闩点起来。
      setEditorValue("just typed");
      await openHistory();
      await waitFor(() => expect(diffText()).toContain("-rolled back"));

      fireEvent.click(screen.getByRole("button", { name: "Restore" }));
      // 编辑器立刻换成回滚后的内容,不用等闩。
      await waitFor(() => expect(editorValue()).toContain("rolled back"));

      // 闩的窗口内接着打字。
      setEditorValue("typed right after the rollback");

      // 等到闩肯定过期(200 tick × 1ms 的 interval,jsdom 里会更慢),确认那段字
      // 还在 —— 被覆盖的话这里会变回 "rolled back"。
      await new Promise((resolve) => setTimeout(resolve, 1200));
      expect(editorValue()).toBe("typed right after the rollback");
      await waitFor(() => expect(harness.read(notePath)).toContain("typed right after the"));
    });

    it("回滚把标题一起带回去", async () => {
      // 快照里 title 是旧的,后端原样写回整个文件。内存留着新标题的话下一次保存
      // 会把它写回去,回滚只成功一半。
      const notePath = harness.seed("Notes.md", '---\ntitle: "New name"\n---\n\nbody\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Old name"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "New name" });
      await screen.findByRole("textbox", { name: "Quick note content" });

      await openHistory();
      await waitFor(() => expect(diffText()).toContain('-title: "Old name"'));
      fireEvent.click(screen.getByRole("button", { name: "Restore" }));

      await screen.findByRole("button", { name: "Old name" });
      expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("Old name");

      // 关键:回滚后再保存一次,frontmatter 不能出现第二份,标题也不能弹回去。
      setEditorValue("body edited");
      await waitFor(() => expect(harness.read(notePath)).toContain("body edited"));
      const saved = harness.read(notePath) ?? "";
      expect(saved).toContain('title: "Old name"');
      expect(saved).not.toContain("New name");
      expect(saved.match(/^---$/gm)).toHaveLength(2);
    });

    it("面板针对的笔记被删掉就关掉,不悄悄换成另一条", async () => {
      const doomed = harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\ndelete me\n');
      harness.seed("Other.md", '---\ntitle: "Other"\n---\n\nother body\n');
      harness.seedSnapshot(doomed, '---\ntitle: "Doomed"\n---\n\nold body\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Doomed" });

      fireEvent.click(screen.getByRole("button", { name: "Doomed" }));
      await screen.findByDisplayValue("Doomed");
      await openHistory();
      await waitFor(() => expect(diffText()).toContain("-old body"));

      // 从列表右键删掉它。回落到 activeNote 的话面板会留着,显示另一条笔记的
      // diff,而「回滚」按钮打在那条上 —— 用户以为自己在恢复 Doomed。
      const row = screen
        .getByRole("button", { name: "Doomed" })
        .closest("[data-notebook-note-row]");
      fireEvent.contextMenu(row as Element);
      fireEvent.click(screen.getByRole("menuitem", { name: "Move to Trash" }));

      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: /Version history/ })).toBeNull(),
      );
    });

    it("目标笔记被回收的路径上出现新笔记时,历史不跟着复活", async () => {
      // 文件名会被回收利用(见「不把删掉那条的保存状态带给同路径的新笔记」)。
      // 删除时只让面板不渲染、状态留着的话,同路径的新笔记一出生就会把上一条的
      // 快照列表连同「回滚」按钮一起接过去。
      const user = userEvent.setup();
      renderNotebook();
      await createNote(user);
      const notePath = harness.paths().find((path) => path.endsWith(".md")) ?? "";
      harness.seedSnapshot(notePath, '---\ntitle: ""\n---\n\nold body\n');

      await openHistory();
      await waitFor(() => expect(diffText()).toContain("-old body"));

      fireEvent.click(screen.getByRole("button", { name: "Delete" }));
      await waitFor(() => expect(harness.read(notePath)).toBeUndefined());

      // 新笔记落在刚腾出来的同一个路径上。
      await createNote(user);
      expect(harness.paths()).toContain(notePath);
      expect(screen.queryByRole("dialog", { name: /Version history/ })).toBeNull();
    });

    it("Esc 关面板,不往外传", async () => {
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nbody\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nold\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });

      const escaped: string[] = [];
      const spy = (event: KeyboardEvent) => escaped.push(event.key);
      window.addEventListener("keydown", spy);
      try {
        const dialog = await openHistory();
        fireEvent.keyDown(dialog, { key: "Escape" });

        expect(screen.queryByRole("dialog", { name: /Version history/ })).toBeNull();
        // 面板外面还有 window 级的 Esc 监听(会去关整个视图)。漏出去的话
        // 一次 Esc 同时关掉历史和视图。
        expect(escaped).toEqual([]);
      } finally {
        window.removeEventListener("keydown", spy);
      }
    });

    it("没有历史时说明,而不是显示空白 diff", async () => {
      harness.seed("Fresh.md", '---\ntitle: "Fresh"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Fresh" });

      await openHistory();

      await screen.findByText("No earlier versions yet");
      expect(screen.queryByTestId("note-history-diff")).toBeNull();
      expect(screen.getByRole("button", { name: "Restore" })).toBeDisabled();
    });

    it("列表拉不回来时把错误显示出来", async () => {
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });

      harness.failNextSnapshotCall = true;
      await openHistory();

      // 静默空列表会被读成"这条笔记没有历史",而它其实有。
      await screen.findByText(/history is unavailable/);
      expect(screen.queryByText("No earlier versions yet")).toBeNull();
    });

    it("回滚失败时留在面板上并报错", async () => {
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\ndisk\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nold\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });
      await screen.findByRole("textbox", { name: "Quick note content" });

      await openHistory();
      await waitFor(() => expect(diffText()).toContain("-old"));

      harness.failNextSnapshotCall = true;
      fireEvent.click(screen.getByRole("button", { name: "Restore" }));

      await screen.findByText(/rollback failed/);
      // 面板不关:关掉的话用户以为回滚成功了,而磁盘没动。
      expect(screen.getByRole("dialog", { name: /Version history/ })).toBeInTheDocument();
      expect(harness.read(notePath)).toContain("disk");
      expect(screen.getByRole("button", { name: "Restore" })).toBeEnabled();
    });

    it("面板铺在两列外面,贴着随手记面板而不是整个窗口", async () => {
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nbody\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nold\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });

      const dialog = await openHistory();

      // 放进正文那一列的话会被列宽裁掉。
      const panel = screen.getByRole("region", { name: "Quick Notes" });
      expect(dialog.parentElement).toBe(panel);
      // `absolute; inset:0` 需要一个定位祖先。缺了它会一路找到视口,把用户正在
      // 参照的另一半视图也遮掉。
      expect(panel.style.position).toBe("relative");
      expect(dialog.style.position).toBe("absolute");
    });
  });

  describe("回收站", () => {
    /** 打开回收站,等它把列表拉回来。 */
    async function openTrash() {
      fireEvent.click(screen.getByRole("button", { name: "Trash" }));
      return screen.findByRole("dialog", { name: "Trash" });
    }

    /** 回收站里的条目行。按 testid 找 —— 时间文案跟着假时钟走,不能当锚点。 */
    function trashRows(): HTMLElement[] {
      return screen.getAllByTestId("note-trash-row");
    }

    /** 删掉当前那条笔记(标题栏的删除按钮)。 */
    async function deleteActiveNote() {
      fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    }

    /** 注册一个把所有确认都答"是"的对话框 host,返回收到的请求列表。 */
    function acceptDialogs(): { requests: string[]; unregister: () => void } {
      const requests: string[] = [];
      const unregister = registerAppDialogHandler(async (request) => {
        requests.push(request.kind);
        return true;
      });
      return { requests, unregister };
    }

    it("删掉的笔记进回收站,恢复后回到列表并且正文还在", async () => {
      harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\nprecious\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Doomed" });
      await screen.findByRole("textbox", { name: "Quick note content" });

      await deleteActiveNote();
      await waitFor(() => expect(harness.trashedNames()).toEqual(["Doomed.md"]));

      await openTrash();
      expect(await screen.findByText("Doomed.md")).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Restore Doomed.md" }));

      // 回到列表 —— 恢复不只是把文件搬回磁盘,面板也要重新认得它,否则用户还得
      // 重开一次面板才看得到自己刚恢复的笔记。
      await screen.findByRole("button", { name: "Doomed" });
      expect(harness.read("/vault/Doomed.md")).toContain("precious");
      expect(harness.trashedNames()).toEqual([]);
    });

    it("恢复一条不会丢掉别处未落盘的编辑", async () => {
      harness.seed("Keeper.md", '---\ntitle: "Keeper"\n---\n\nold body\n');
      harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\nbye\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Keeper" });
      await screen.findByRole("textbox", { name: "Quick note content" });

      // 先删掉 Doomed(它此刻不是当前笔记,走列表右键菜单)。
      const row = screen
        .getByRole("button", { name: "Doomed" })
        .closest("[data-notebook-note-row]");
      if (!row) throw new Error("no row for Doomed");
      fireEvent.contextMenu(row);
      fireEvent.click(screen.getByRole("menuitem", { name: "Move to Trash" }));
      await waitFor(() => expect(harness.trashedNames()).toEqual(["Doomed.md"]));

      // 在 Keeper 里敲字但不等防抖落盘。
      setEditorValue("unsaved edit");

      await openTrash();
      fireEvent.click(await screen.findByRole("button", { name: "Restore Doomed.md" }));
      await waitFor(() => expect(harness.trashedNames()).toEqual([]));
      fireEvent.click(screen.getByRole("button", { name: "Close trash" }));

      // 恢复只把那一条加回列表,不重扫整个 vault。重扫会用磁盘上的旧正文覆盖掉
      // 内存里还没落盘的编辑 —— 用户刚打的字就没了。
      await waitFor(() => expect(editorValue()).toBe("unsaved edit"));
    });

    it("彻底删除要确认,取消就什么都不做", async () => {
      harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\nbye\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Doomed" });
      await deleteActiveNote();
      await waitFor(() => expect(harness.trashedNames()).toEqual(["Doomed.md"]));
      await openTrash();
      await screen.findByText("Doomed.md");

      const requests: string[] = [];
      const unregister = registerAppDialogHandler(async (request) => {
        requests.push(request.kind);
        return false;
      });
      try {
        fireEvent.click(screen.getByRole("button", { name: "Delete Doomed.md permanently" }));
        await waitFor(() => expect(requests).toEqual(["confirm"]));

        // 取消后那条还在:彻底删除会连历史一起清掉,是这个面板里唯一不可逆的
        // 操作,点错一次就没了。
        expect(harness.trashedNames()).toEqual(["Doomed.md"]);
        expect(screen.getByText("Doomed.md")).toBeInTheDocument();
      } finally {
        unregister();
        resetAppDialogHandlerForTests();
      }
    });

    it("确认后彻底删除,那条从回收站消失", async () => {
      harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\nbye\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Doomed" });
      await deleteActiveNote();
      await waitFor(() => expect(harness.trashedNames()).toEqual(["Doomed.md"]));
      await openTrash();
      await screen.findByText("Doomed.md");

      const { unregister } = acceptDialogs();
      try {
        fireEvent.click(screen.getByRole("button", { name: "Delete Doomed.md permanently" }));

        await waitFor(() => expect(harness.trashedNames()).toEqual([]));
        expect(await screen.findByText("Nothing in the trash")).toBeInTheDocument();
      } finally {
        unregister();
        resetAppDialogHandlerForTests();
      }
    });

    it("清空回收站要确认,确认后一条不剩", async () => {
      harness.seed("One.md", '---\ntitle: "One"\n---\n\na\n');
      harness.seed("Two.md", '---\ntitle: "Two"\n---\n\nb\n');
      renderNotebook();
      await screen.findByRole("button", { name: "One" });
      for (const name of ["One", "Two"]) {
        fireEvent.click(screen.getByRole("button", { name }));
        await deleteActiveNote();
      }
      await waitFor(() => expect(harness.trashedNames()).toHaveLength(2));
      await openTrash();
      await waitFor(() => expect(trashRows()).toHaveLength(2));

      const { requests, unregister } = acceptDialogs();
      try {
        fireEvent.click(screen.getByRole("button", { name: "Empty trash" }));
        await waitFor(() => expect(requests).toEqual(["confirm"]));

        await waitFor(() => expect(harness.trashedNames()).toEqual([]));
        expect(await screen.findByText("Nothing in the trash")).toBeInTheDocument();
      } finally {
        unregister();
        resetAppDialogHandlerForTests();
      }
    });

    it("清空回收站取消后一条都不清", async () => {
      harness.seed("One.md", '---\ntitle: "One"\n---\n\na\n');
      harness.seed("Two.md", '---\ntitle: "Two"\n---\n\nb\n');
      renderNotebook();
      await screen.findByRole("button", { name: "One" });
      for (const name of ["One", "Two"]) {
        fireEvent.click(screen.getByRole("button", { name }));
        await deleteActiveNote();
      }
      await waitFor(() => expect(harness.trashedNames()).toHaveLength(2));
      await openTrash();
      await waitFor(() => expect(trashRows()).toHaveLength(2));

      const requests: string[] = [];
      const unregister = registerAppDialogHandler(async (request) => {
        requests.push(request.kind);
        return false;
      });
      try {
        fireEvent.click(screen.getByRole("button", { name: "Empty trash" }));
        await waitFor(() => expect(requests).toEqual(["confirm"]));

        // 清空是整个面板里破坏力最大的一下 —— 一次点掉所有还能捞回来的笔记,
        // 连历史一起。取消必须真的什么都不做。
        expect(harness.trashedNames()).toHaveLength(2);
        expect(trashRows()).toHaveLength(2);
      } finally {
        unregister();
        resetAppDialogHandlerForTests();
      }
    });

    it("清空失败又拉不回列表时,报的是清空那条错误", async () => {
      harness.seed("One.md", '---\ntitle: "One"\n---\n\na\n');
      renderNotebook();
      await screen.findByRole("button", { name: "One" });
      await deleteActiveNote();
      await waitFor(() => expect(harness.trashedNames()).toEqual(["One.md"]));
      await openTrash();
      await waitFor(() => expect(trashRows()).toHaveLength(1));

      // 清空失败,紧接着那次用来纠正清单的重新拉取也失败。
      harness.failTrashCalls(2);
      const { unregister } = acceptDialogs();
      try {
        fireEvent.click(screen.getByRole("button", { name: "Empty trash" }));

        // 显示的必须是"清空失败",不是"列表拉不回来"。后者是补救动作的副作用,
        // 拿它盖掉原因会让用户以为回收站只是读不出来,而实际上已经清掉了一部分。
        await screen.findByText(/emptying the trash failed/);
        expect(screen.queryByText(/trash is unavailable/)).not.toBeInTheDocument();
      } finally {
        unregister();
        resetAppDialogHandlerForTests();
      }
    });

    it("恢复失败时把错误显示出来,那条留在回收站里", async () => {
      harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\nbye\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Doomed" });
      await deleteActiveNote();
      await waitFor(() => expect(harness.trashedNames()).toEqual(["Doomed.md"]));
      await openTrash();
      await screen.findByText("Doomed.md");

      harness.failTrashCalls();
      fireEvent.click(screen.getByRole("button", { name: "Restore Doomed.md" }));

      // 静默失败最糟:用户以为恢复了,回列表却找不到那条笔记。
      await screen.findByText(/restore failed/);
      expect(harness.trashedNames()).toEqual(["Doomed.md"]);
    });

    it("列表拉不回来时把错误显示出来,而不是显示成空回收站", async () => {
      harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\nbye\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Doomed" });
      await deleteActiveNote();
      await waitFor(() => expect(harness.trashedNames()).toEqual(["Doomed.md"]));

      harness.failTrashCalls();
      await openTrash();

      // 显示成空的话用户会以为自己删掉的笔记真的没了。
      await screen.findByText(/trash is unavailable/);
      expect(screen.queryByText("Nothing in the trash")).not.toBeInTheDocument();
    });

    it("打开回收站会关掉版本历史", async () => {
      const notePath = harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nbody\n');
      harness.seedSnapshot(notePath, '---\ntitle: "Notes"\n---\n\nold\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });
      fireEvent.click(screen.getByRole("button", { name: "Version history" }));
      await screen.findByRole("dialog", { name: /Version history/ });

      await openTrash();

      // 两个都是铺满面板的 overlay。叠着的话下面那个还在接键盘事件(一次 Esc
      // 关掉两个),而用户只看得见上面那个。
      expect(screen.queryByRole("dialog", { name: /Version history/ })).not.toBeInTheDocument();
    });

    it("Esc 关回收站,不往外传", async () => {
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });
      const dialog = await openTrash();

      const outer = vi.fn();
      window.addEventListener("keydown", outer);
      try {
        fireEvent.keyDown(dialog, { key: "Escape", bubbles: true });

        expect(screen.queryByRole("dialog", { name: "Trash" })).not.toBeInTheDocument();
        // 面板外面还有 window 级的 Esc 监听(会去关整个视图)。不拦住的话一次
        // Esc 会同时关掉回收站和它背后的视图。
        expect(outer).not.toHaveBeenCalled();
      } finally {
        window.removeEventListener("keydown", outer);
      }
    });

    it("面板铺在两列外面,贴着随手记面板而不是整个窗口", async () => {
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });

      const dialog = await openTrash();

      const panel = screen.getByRole("region", { name: "Quick Notes" });
      expect(dialog.parentElement).toBe(panel);
      expect(panel.style.position).toBe("relative");
      expect(dialog.style.position).toBe("absolute");
    });

    it("空回收站给一句说明,而不是一片空白", async () => {
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Notes" });

      await openTrash();

      expect(await screen.findByText("Nothing in the trash")).toBeInTheDocument();
    });

    it("回收站按删除时间倒序,新删的在最上面", async () => {
      harness.seed("First.md", '---\ntitle: "First"\n---\n\na\n');
      harness.seed("Second.md", '---\ntitle: "Second"\n---\n\nb\n');
      renderNotebook();
      await screen.findByRole("button", { name: "First" });
      for (const name of ["First", "Second"]) {
        fireEvent.click(screen.getByRole("button", { name }));
        await deleteActiveNote();
      }
      await waitFor(() => expect(harness.trashedNames()).toHaveLength(2));

      await openTrash();
      await waitFor(() => expect(trashRows()).toHaveLength(2));

      // 找回刚删掉的东西是回收站最常见的用途,倒序让它落在第一行。
      expect(trashRows().map((row) => row.textContent)).toEqual([
        expect.stringContaining("Second.md"),
        expect.stringContaining("First.md"),
      ]);
    });
  });

  describe("笔记列表右键菜单", () => {
    /** 在指定笔记那一行上右键,返回打开的菜单。 */
    async function openListMenu(name: string) {
      const row = screen.getByRole("button", { name }).closest("[data-notebook-note-row]");
      if (!row) throw new Error(`no row for ${name}`);
      fireEvent.contextMenu(row);
      return screen.getByRole("menu", { name: "Quick note actions" });
    }

    it("在系统文件夹中打开:传的 allowlist 根是 vault", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Open in System Folder" }));

      // 后端用这个根做 validate_path_within。传成笔记自己的路径就等于没校验,
      // 这个入口会退化成任意路径揭示器 —— 所以两个参数都要钉。
      await waitFor(() => expect(harness.revealCalls).toHaveLength(1));
      expect(harness.revealCalls[0]).toEqual({ path: "/vault/Target.md", projectPath: "/vault" });
    });

    it("复制完整路径", async () => {
      // 调 setup() 是为了装上能往返读写的剪贴板实现(见文件头注释),句柄本身用不上。
      userEvent.setup();
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Copy full path" }));

      await waitFor(async () =>
        expect(await navigator.clipboard.readText()).toBe("/vault/Target.md"),
      );
    });

    it("移入回收站:删的是右键点中的那条,不是当前打开的那条", async () => {
      // 这条是这个菜单最容易写错的地方 —— 沿用原来的 deleteActiveNote 会删错文件。
      const kept = harness.seed("Kept.md", '---\ntitle: "Kept"\n---\n\nkeep me\n');
      const doomed = harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\ndelete me\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Kept" });
      await screen.findByRole("button", { name: "Doomed" });

      // 打开 Kept(它成为 activeNote),然后右键 Doomed。
      fireEvent.click(screen.getByRole("button", { name: "Kept" }));
      await screen.findByDisplayValue("Kept");

      await openListMenu("Doomed");
      fireEvent.click(screen.getByRole("menuitem", { name: "Move to Trash" }));

      await waitFor(() => expect(harness.read(doomed)).toBeUndefined());
      expect(harness.read(kept)).toContain("keep me");
    });

    it("重命名:进入行内改名,不直接改文件名", async () => {
      const notePath = harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Rename quick note" }));

      // 改名走的是列表里那个 input(标题存 frontmatter),文件名保持不动 ——
      // P4 的 wikilink 按文件名互链,静默改名会断链。
      expect(screen.getByRole("textbox", { name: "Rename quick note" })).toHaveValue("Target");
      expect(harness.paths()).toContain(notePath);
    });

    it("版本历史:打开的是右键点中的那条,并把它切成当前笔记", async () => {
      const kept = harness.seed("Kept.md", '---\ntitle: "Kept"\n---\n\nkeep me\n');
      harness.seed("Other.md", '---\ntitle: "Other"\n---\n\nother body\n');
      harness.seedSnapshot(kept, '---\ntitle: "Kept"\n---\n\nold keep\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Other" });
      await screen.findByRole("button", { name: "Kept" });

      // 打开 Other(它成为 activeNote),然后右键 Kept 开历史。
      fireEvent.click(screen.getByRole("button", { name: "Other" }));
      await screen.findByDisplayValue("Other");

      await openListMenu("Kept");
      fireEvent.click(screen.getByRole("menuitem", { name: "Version history" }));

      // 面板标题跟着右键的那条。不切当前笔记的话,用户会看到 Kept 的快照和
      // Other 的正文并排 —— diff 就是两条不同笔记的对比。
      await screen.findByRole("dialog", { name: "Version history — Kept" });
      await waitFor(() =>
        expect(screen.getByTestId("note-history-diff").textContent).toContain("-old keep"),
      );
      expect(screen.getByTestId("note-history-diff").textContent).toContain("+keep me");
      expect(screen.getByTestId("note-history-diff").textContent).not.toContain("other body");
    });

    it("属性:报磁盘上的大小,并把右键的那条切成当前笔记", async () => {
      harness.seed("Other.md", '---\ntitle: "Other"\n---\n\nother body\n');
      // 正文是 8 个 UTF-8 字节的中文 + frontmatter。大小必须按字节算 ——
      // 按字符算的话中文笔记会报成三分之一大。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n中文正文\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Other" });
      await screen.findByRole("button", { name: "Target" });

      fireEvent.click(screen.getByRole("button", { name: "Other" }));
      await screen.findByDisplayValue("Other");

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      expect(sheet).toHaveTextContent("Target");
      // 大小来自磁盘,不是内存里那份笔记(它连字节数都没有)。
      await waitFor(() => expect(sheet).toHaveTextContent(/\d+ B/));
      // 字数走编辑器里的当前文本,所以必须已经切到 Target —— 停在 Other 上的话
      // 这里报的是 "other body" 的字数。
      expect(screen.getByDisplayValue("Target")).toBeInTheDocument();
      expect(sheet).not.toHaveTextContent("Other");
    });

    it("属性:位置显示相对 vault 的路径,完整路径进 title", async () => {
      const notePath = harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      // vault 根往往埋在 `~/Library/Application Support/…` 底下,完整路径在一个
      // 400px 宽的面板里占三行还是看不出笔记在哪个子目录。
      //
      // 断言必须是「显示的正是相对路径」而不是「里面含有 Target.md」—— 完整路径
      // `/vault/Target.md` 也含有它,那条断言在退回完整路径时照样成立。
      const location = sheet.querySelector(`[title="${notePath}"]`);
      expect(location).not.toBeNull();
      expect(location?.textContent).toBe("Target.md");
      // 完整路径只进 title(悬停可看),不占版面。
      expect(sheet.textContent).not.toContain(notePath);
    });

    it("属性:未保存的编辑算进字数,并说明口径", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\none\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });
      fireEvent.click(screen.getByRole("button", { name: "Target" }));
      await screen.findByDisplayValue("Target");
      // 走 EditorView 而不是 user.type:后者会先点一下 contentDOM,而那次点击在
      // jsdom 里会走进 CodeMirror 的 posAtCoords —— 没有真实布局,它读 rect 时炸。
      setEditorValue("one two three");

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      // 三个词是编辑器里的当前文本,磁盘上那份还是一个词 —— 报 1 说明字数走的是
      // 已保存的内容,报 0 说明这条线根本没接上。
      const words = sheet.querySelector('[data-testid="note-properties-words"]');
      expect(words?.textContent).toBe("3");
      // 磁盘那一组和内容那一组口径不同(一个是文件,一个是编辑器里的文本)。
      // 不说清楚的话用户会以为哪个数错了。
      expect(sheet).toHaveTextContent("including unsaved edits");
    });

    it("属性:全库那一组报这篇的标签和被引用数", async () => {
      /* P3 做属性面板时这两行是刻意留空的 —— 那时候标签索引和链接索引都不存在。
         现在两个都有了,这一条守着它们真的接上了。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n#work 一处\n#work 又一处\n#home\n');
      /* 两条链接刻意分在两行:`collectBacklinks` 同一行只留一条(同一行重复两遍
         没有增量信息),写在一行的话这里就成了 2 条,分不出"篇数"和"条数"。 */
      harness.seed("One.md", '---\ntitle: "One"\n---\n\n见 [[Target]]\n又见 [[Target#节]]\n');
      harness.seed("Two.md", '---\ntitle: "Two"\n---\n\n也提到 [[Target]]\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      const tags = () => sheet.querySelector('[data-testid="note-properties-tags"]')?.textContent;
      /* `#work` 两处所以带 ×2,`#home` 一处不带 —— 给每条都缀 ×1 是纯噪声。
         按空白归一后再比:分隔用几个空格是排版,断言写死"两个空格"的话调一下间距
         就要改测试,而那种失败不指向任何真问题。 */
      await waitFor(() => expect(tags()?.replace(/\s+/g, " ").trim()).toBe("#work ×2 #home"));
      // 两篇引用、三条链接(One 里有两条):只报一个数就分不出这两种情况。
      const mentions = sheet.querySelector('[data-testid="note-properties-mentions"]');
      expect(mentions?.textContent).toBe("3 links from 2 notes");
      // 这一组读磁盘,和上面内容那一组口径相反,必须说清。
      expect(sheet).toHaveTextContent("unsaved edits are not counted");
    });

    it("属性:没有标签也没有被引用时是两句话,不是 0", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n光秃秃的正文\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      await waitFor(() => expect(sheet).toHaveTextContent("No inline tags"));
      expect(sheet).toHaveTextContent("No notes link here");
    });

    it("属性:全库扫描失败只弄脏那一组,文件信息照样显示", async () => {
      /* 两组分开加载、分开报错就是为了这个:全库扫描比 stat 慢得多也更容易失败
         (权限、超大文件),让它把"文件多大"一起拖下水是没道理的。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n#work\n');
      harness.failTagScan = true;
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      await waitFor(() => expect(sheet).toHaveTextContent("scanning tags failed"));
      // 磁盘那一组不受影响。
      expect(sheet).toHaveTextContent(/\d+ B/);
      expect(sheet).not.toHaveTextContent("Scanning the vault…");
    });

    it("属性:全库那一组慢的响应回来时不盖掉已经换看的另一条", async () => {
      /* 和 stat 那一路同一个陷阱,但这一路更容易撞上 —— 全库扫描慢得多,用户在它
         飞行途中换看另一条笔记的属性是很自然的操作。

         必须**手工挂住**扫描:默认 harness 同步返回,两次请求永远按发起顺序完成,
         "回来的不是当前那条就丢掉"那条分支根本进不去。第一版这条测试没挂,于是
         去掉那个 noteId 守卫它照样绿 —— 一条守着空气的测试。 */
      harness.seed("Slow.md", '---\ntitle: "Slow"\n---\n\n#slow\n');
      harness.seed("Quick.md", '---\ntitle: "Quick"\n---\n\n#quick\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Slow" });
      await screen.findByRole("button", { name: "Quick" });

      harness.holdTagScans();

      await openListMenu("Slow");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      await screen.findByRole("dialog", { name: "Note properties" });
      await waitFor(() => expect(harness.heldTagScanCount()).toBe(1));

      // Slow 那次还挂着,就换看 Quick。
      await openListMenu("Quick");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      await waitFor(() => expect(harness.heldTagScanCount()).toBe(2));

      // 先放行 Quick(后发起的),再放行 Slow —— 乱序返回。
      harness.releaseTagScan(1);
      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      const tags = () => sheet.querySelector('[data-testid="note-properties-tags"]')?.textContent;
      await waitFor(() => expect(tags()).toContain("#quick"));

      harness.releaseTagScan(0);
      // Slow 那次回来了,但它不是当前看的那条 —— 不能把 Quick 的那一组换掉。
      await new Promise((resolve) => setTimeout(resolve, 20));
      expect(tags()).toContain("#quick");
      expect(tags()).not.toContain("#slow");
    });

    it("属性:读不到文件信息时报错而不是显示 0 字节", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.failNoteStat = true;
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      // 显示 0 B 会被读成"一条空笔记",而真实情况是根本没读到。
      expect(await screen.findByRole("alert")).toHaveTextContent("reading file info failed");
      expect(screen.getByRole("dialog", { name: "Note properties" })).not.toHaveTextContent("0 B");
    });

    it("属性:Esc 关掉面板,不冒泡去关整个视图", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      // 面板外面套一层 Esc 监听,模拟宿主视图身上那个「Esc 关掉整个笔记视图」。
      // 只断言编辑器还在是不够的:这个测试渲染里本来就没有外层监听,那条断言
      // 无论拦不拦都成立。
      const outerEsc = vi.fn();
      render(
        <I18nProvider>
          <div
            onKeyDown={(event) => {
              if (event.key === "Escape") outerEsc();
            }}
          >
            <NotebookPanel />
          </div>
        </I18nProvider>,
      );
      await screen.findByRole("button", { name: "Target" });
      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      const sheet = await screen.findByRole("dialog", { name: "Note properties" });

      fireEvent.keyDown(sheet, { key: "Escape" });

      // 不拦的话外层监听会一起收到,一次 Esc 关掉两层。
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Note properties" })).toBeNull(),
      );
      expect(outerEsc).not.toHaveBeenCalled();
      expect(screen.getByRole("textbox", { name: "Quick note content" })).toBeInTheDocument();
    });

    it("属性:目标笔记被删掉后面板自己收起来,恢复也不把它带回来", async () => {
      harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Doomed" });
      await openListMenu("Doomed");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      await waitFor(() => expect(sheet).toHaveTextContent(/\d+ B/));

      await openListMenu("Doomed");
      fireEvent.click(screen.getByRole("menuitem", { name: "Move to Trash" }));
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Note properties" })).toBeNull(),
      );

      // 从回收站恢复:文件回到**同一个路径**,于是那条 `notes.find(id)` 又能找到
      // 它。状态没清的话面板会自己弹回来,顶着删除前的大小和修改时间 —— 而那份
      // 数字来自一次已经作废的 stat。
      fireEvent.click(screen.getByRole("button", { name: "Trash" }));
      await screen.findByRole("dialog", { name: "Trash" });
      fireEvent.click(screen.getByRole("button", { name: "Restore Doomed.md" }));
      await screen.findByRole("button", { name: "Doomed" });

      expect(screen.queryByRole("dialog", { name: "Note properties" })).toBeNull();
    });

    it("属性:慢的那次响应回来时不盖掉已经换看的另一条", async () => {
      // Slow 的正文长,Quick 的短 —— 两条的字节数必须能分辨,否则「盖没盖」看不出来。
      harness.seed("Slow.md", `---\ntitle: "Slow"\n---\n\n${"x".repeat(400)}\n`);
      harness.seed("Quick.md", '---\ntitle: "Quick"\n---\n\nshort\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Slow" });
      await screen.findByRole("button", { name: "Quick" });

      // 只悬停 Slow 那一次 stat,Quick 的放行。
      const gate: { release: (() => void) | null } = { release: null };
      const originalHandle = harness.handle;
      harness.handle = (command, args) => {
        if (command === "notebook_note_stat" && String(args?.path).endsWith("Slow.md")) {
          return new Promise((resolve) => {
            gate.release = () => resolve(originalHandle(command, args));
          });
        }
        return originalHandle(command, args);
      };

      await openListMenu("Slow");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      await waitFor(() => expect(gate.release).not.toBeNull());
      expect(sheet).toHaveTextContent("Reading file info");

      // Slow 还在飞的时候换看 Quick 的属性。
      await openListMenu("Quick");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      await waitFor(() =>
        expect(screen.getByRole("dialog", { name: "Note properties" })).toHaveTextContent(
          "Quick.md",
        ),
      );
      const quickSize = screen
        .getByRole("dialog", { name: "Note properties" })
        .textContent?.match(/(\d+) B/)?.[1];
      expect(quickSize).toBeDefined();

      // 现在把 Slow 那次放回来。没有 noteId 守卫的话它会把 400+ 字节写进
      // Quick 的面板,标题是 Quick 而大小是 Slow 的。
      await act(async () => {
        gate.release?.();
        await Promise.resolve();
        await Promise.resolve();
      });

      const after = screen.getByRole("dialog", { name: "Note properties" });
      expect(after).toHaveTextContent("Quick.md");
      expect(after).toHaveTextContent(`${quickSize} B`);
      expect(after).not.toHaveTextContent("Reading file info");
    });

    it("属性:慢的那次报错回来时不弄脏已经换看的另一条", async () => {
      harness.seed("Slow.md", '---\ntitle: "Slow"\n---\n\nbody\n');
      harness.seed("Quick.md", '---\ntitle: "Quick"\n---\n\nshort\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Slow" });
      await screen.findByRole("button", { name: "Quick" });

      // Slow 那次悬停住并最终失败,Quick 那次正常返回。
      const gate: { fail: (() => void) | null } = { fail: null };
      const originalHandle = harness.handle;
      harness.handle = (command, args) => {
        if (command === "notebook_note_stat" && String(args?.path).endsWith("Slow.md")) {
          return new Promise((_resolve, reject) => {
            gate.fail = () => reject(new Error("slow stat blew up"));
          });
        }
        return originalHandle(command, args);
      };

      await openListMenu("Slow");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      await screen.findByRole("dialog", { name: "Note properties" });
      await waitFor(() => expect(gate.fail).not.toBeNull());

      await openListMenu("Quick");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      await waitFor(() =>
        expect(screen.getByRole("dialog", { name: "Note properties" })).toHaveTextContent(
          "Quick.md",
        ),
      );

      await act(async () => {
        gate.fail?.();
        await Promise.resolve();
        await Promise.resolve();
      });

      // 错误分支也要认 noteId:不认的话 Quick 的面板会挂上 Slow 的错误,大小那一行
      // 被一条红字顶掉。
      const after = screen.getByRole("dialog", { name: "Note properties" });
      expect(after).not.toHaveTextContent("slow stat blew up");
      expect(screen.queryByRole("alert")).toBeNull();
      expect(after).toHaveTextContent(/\d+ B/);
    });

    it("属性:文件系统不记创建时间时那一行整行不显示", async () => {
      // harness 的假时钟没有创建时间,`createdMs` 恒为 null —— 正好是 Linux 上
      // 部分文件系统的真实情形。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });
      await openListMenu("Target");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      await waitFor(() => expect(sheet).toHaveTextContent(/\d+ B/));
      // 修改时间有,创建时间没有 —— 显示 1970-01-01 比留白糟得多。
      expect(sheet).toHaveTextContent("Modified");
      expect(sheet).not.toHaveTextContent("Created");
      expect(sheet).not.toHaveTextContent("1970");
    });

    it("属性:面板关掉之后飞回来的响应不把它顶开", async () => {
      harness.seed("Slow.md", '---\ntitle: "Slow"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Slow" });

      const gate: { release: (() => void) | null } = { release: null };
      const originalHandle = harness.handle;
      harness.handle = (command, args) => {
        if (command === "notebook_note_stat") {
          return new Promise((resolve) => {
            gate.release = () => resolve(originalHandle(command, args));
          });
        }
        return originalHandle(command, args);
      };

      await openListMenu("Slow");
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      const sheet = await screen.findByRole("dialog", { name: "Note properties" });
      await waitFor(() => expect(gate.release).not.toBeNull());

      fireEvent.keyDown(sheet, { key: "Escape" });
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Note properties" })).toBeNull(),
      );

      // `current` 已经是 null,守卫必须认出来 —— 直接 `{ ...current, stat }` 会
      // 凭空造出一份状态,面板自己弹回来。
      await act(async () => {
        gate.release?.();
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(screen.queryByRole("dialog", { name: "Note properties" })).toBeNull();
    });

    it("菜单和编辑区那个互斥", async () => {
      const user = userEvent.setup();
      renderNotebook();
      await createNote(user);

      // 先开编辑区的菜单。
      fireEvent.contextMenu(screen.getByRole("textbox", { name: "Quick note content" }));
      expect(screen.getByRole("menu", { name: "Text" })).toBeInTheDocument();

      // 再在列表行上右键 —— 两个菜单同时浮着没有意义,前一个要关掉。
      const row = screen
        .getByRole("button", { name: "Untitled quick note" })
        .closest("[data-notebook-note-row]");
      fireEvent.contextMenu(row as Element);
      expect(screen.getByRole("menu", { name: "Quick note actions" })).toBeInTheDocument();
      expect(screen.queryByRole("menu", { name: "Text" })).not.toBeInTheDocument();
    });
  });

  describe("wikilink", () => {
    /** 阅读态里那些被增强过的 wikilink。 */
    function wikiLinks(): HTMLElement[] {
      return Array.from(document.querySelectorAll<HTMLElement>("a.notebook-wikilink"));
    }

    /** 在列表里右键某条笔记,拿到它的菜单。 */
    async function openNoteRowMenu(name: string) {
      const row = screen.getByRole("button", { name }).closest("[data-notebook-note-row]");
      if (!row) throw new Error(`no row for ${name}`);
      fireEvent.contextMenu(row);
      return screen.getByRole("menu", { name: "Quick note actions" });
    }

    /** 切到阅读态并等 HTML 挂上。 */
    async function readMode() {
      fireEvent.click(screen.getByRole("button", { name: "Read" }));
      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")).not.toBeNull(),
      );
    }

    it("阅读态把 [[标题]] 变成可点的链接,点它跳到那条笔记", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n目标正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Target]] 一节\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();

      await waitFor(() => expect(wikiLinks()).toHaveLength(1));
      const link = wikiLinks()[0]!;
      expect(link.textContent).toBe("Target");
      expect(link.title).toBe("Open Target");

      fireEvent.click(link);

      // 跳过去之后标题框换成目标笔记 —— 这条链路要真的经过懒加载正文那一步。
      await screen.findByDisplayValue("Target");
      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")?.textContent).toContain(
          "目标正文",
        ),
      );
    });

    it("按 frontmatter 标题也能解析 —— 文件名与标题不一致时", async () => {
      // 这是 Aeroric 与 Markio 的实质差异:文件名在新建时定死,标题后来改了。
      // 只认文件名 stem 的话,用户写新标题会解析不到自己那篇笔记。
      harness.seed("cao-gao.md", '---\ntitle: "周报"\n---\n\n本周内容\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[周报]]\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();

      await waitFor(() => expect(wikiLinks()).toHaveLength(1));
      fireEvent.click(wikiLinks()[0]!);
      await screen.findByDisplayValue("周报");
    });

    it("目标不存在时是死链,点它不跳也不报错", async () => {
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[还没写]]\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();

      await waitFor(() => expect(wikiLinks()).toHaveLength(1));
      const link = wikiLinks()[0]!;
      expect(link.classList.contains("notebook-wikilink-missing")).toBe(true);
      expect(link.title).toBe("No note named 还没写");

      fireEvent.click(link);
      // 还停在原地,并且没有弹错误条 —— 死链是常态(先写链接后写笔记),不是故障。
      expect(screen.getByDisplayValue("Origin")).toBeInTheDocument();
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("正文没变的重渲染之后,链接还在(不退回字面 [[..]])", async () => {
      /* `dangerouslySetInnerHTML={{ __html: markdownHtml }}` 的属性值如果每次渲染都是
         新对象,React 会重写一遍 innerHTML —— 哪怕 HTML 字符串一个字都没变。预览里的
         子节点被整批换新,增强出来的 `<a>` 全丢,链接退回字面 `[[Target]]`。而增强
         effect 按 `[markdownHtml, mode, linkIndex, t]` 当依赖,这四个都没变,不会重跑,
         于是链接**永久**失效。

         触发这种重渲染的都是日常操作:开一下大纲、切一下侧栏档、一次自动保存回填
         保存状态。这里用开大纲来制造。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n目标正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Target]] 一节\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();
      await waitFor(() => expect(wikiLinks()).toHaveLength(1));

      // 正文没变、视图没变、笔记没变 —— 只是多了一个侧栏。
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      await screen.findByRole("complementary", { name: "Outline" });

      expect(wikiLinks()).toHaveLength(1);
      // 退化的表现是字面量重新出现在正文里。
      expect(document.querySelector(".notebook-markdown-preview")?.textContent).not.toContain(
        "[[Target]]",
      );
      // 而且点了还得能跳 —— 节点在但事件约定丢了也算坏。
      fireEvent.click(wikiLinks()[0]!);
      await screen.findByDisplayValue("Target");
    });

    it("目标笔记被删掉后,活链当场变成死链", async () => {
      /* 这一条钉的是增强 effect 的依赖:它必须跟着链接索引跑,不能只跟
         `markdownHtml`。

         删掉别人那条笔记时,当前笔记的正文一个字都没变、视图也没切 ——
         `markdownHtml` 和 `mode` 都是原值。只按它们当依赖的话,这条链接会一直
         停在"可点、打开 Doomed"的状态,点下去跳到一条已经不存在的笔记。 */
      harness.seed("Doomed.md", '---\ntitle: "Doomed"\n---\n\n目标\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Doomed]]\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();
      await waitFor(() => expect(wikiLinks()).toHaveLength(1));
      expect(wikiLinks()[0]?.classList.contains("notebook-wikilink-missing")).toBe(false);

      // 从列表右键删掉目标。当前笔记不是它,所以选中项不变、视图也不变。
      await openNoteRowMenu("Doomed");
      fireEvent.click(screen.getByRole("menuitem", { name: "Move to Trash" }));
      await waitFor(() => expect(screen.queryByRole("button", { name: "Doomed" })).toBeNull());

      // 仍停在 Origin 的阅读态。
      expect(screen.getByDisplayValue("Origin")).toBeInTheDocument();
      await waitFor(() =>
        expect(wikiLinks()[0]?.classList.contains("notebook-wikilink-missing")).toBe(true),
      );
      expect(wikiLinks()[0]?.title).toBe("No note named Doomed");
    });

    it("改了目标的标题之后,按新标题写的链接能解析到", async () => {
      // 文件名是 `cao-gao.md` 且不会随标题变,所以这条链接只能靠标题索引命中。
      harness.seed("cao-gao.md", '---\ntitle: "旧标题"\n---\n\n内容\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[周报]]\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();
      await waitFor(() =>
        expect(wikiLinks()[0]?.classList.contains("notebook-wikilink-missing")).toBe(true),
      );

      /* 在列表里把那条笔记改名成「周报」(双击进入行内重命名)。
         行上显示的是文件名 stem 而不是 `旧标题` —— 列表只读目录项,没读入的笔记
         拿不到 frontmatter 里的标题。链接解析补得上这一课(靠全库索引),列表
         本身没有,那是另一件事。 */
      fireEvent.dblClick(screen.getByRole("button", { name: "cao-gao" }));
      const input = await screen.findByRole("textbox", { name: "Rename quick note" });
      fireEvent.change(input, { target: { value: "周报" } });
      fireEvent.keyDown(input, { key: "Enter" });
      await screen.findByRole("button", { name: "周报" });

      // 回到 Origin 的阅读态,链接应当已经活了。
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();
      await waitFor(() =>
        expect(wikiLinks()[0]?.classList.contains("notebook-wikilink-missing")).toBe(false),
      );
      expect(wikiLinks()[0]?.title).toBe("Open 周报");
    });

    it("代码块里的 [[...]] 不变成链接", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nx\n');
      harness.seed(
        "Origin.md",
        '---\ntitle: "Origin"\n---\n\n```\n[[Target]]\n```\n\n行内 `[[Target]]` 也不算\n',
      );
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();

      // 代码示例里的双方括号是内容。走 DOM 增强而不是源码正则替换,全部理由就在这。
      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")?.textContent).toContain(
          "[[Target]]",
        ),
      );
      expect(wikiLinks()).toHaveLength(0);
    });

    it("带 #小节 的链接跳过去之后滚到那个标题", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n# 开头\n\n## 第二节\n\n内容\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Target#第二节]]\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();

      await waitFor(() => expect(wikiLinks()).toHaveLength(1));
      const scrolled: string[] = [];
      // jsdom 没有布局,scrollIntoView 是个空实现 —— 用 spy 记下"滚到了哪个标题"。
      const original = Element.prototype.scrollIntoView;
      Element.prototype.scrollIntoView = function (this: Element) {
        scrolled.push(this.textContent ?? "");
      };
      try {
        fireEvent.click(wikiLinks()[0]!);
        await screen.findByDisplayValue("Target");
        // 跳转后正文要先渲染出来,滚动才发生在下一帧。
        await waitFor(() => expect(scrolled).toContain("第二节"));
      } finally {
        Element.prototype.scrollIntoView = original;
      }
    });

    it("别名显示别名,跳转仍按目标", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nx\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Target|换个说法]]\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Origin" });
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();

      await waitFor(() => expect(wikiLinks()).toHaveLength(1));
      expect(wikiLinks()[0]!.textContent).toBe("换个说法");
      fireEvent.click(wikiLinks()[0]!);
      await screen.findByDisplayValue("Target");
    });
  });

  describe("![[嵌入]]", () => {
    /** 阅读态里的嵌入占位。 */
    function embedBlocks(): HTMLElement[] {
      return Array.from(document.querySelectorAll<HTMLElement>(".notebook-embed"));
    }

    /** 切到阅读态并等 HTML 挂上。 */
    async function readMode() {
      fireEvent.click(screen.getByRole("button", { name: "Read" }));
      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")).not.toBeNull(),
      );
    }

    /** 打开某篇笔记的阅读态。 */
    async function openInReadMode(title: string) {
      renderNotebook();
      await screen.findByRole("button", { name: title });
      fireEvent.click(screen.getByRole("button", { name: title }));
      await screen.findByDisplayValue(title);
      await readMode();
    }

    it("把目标笔记的内容嵌进来,头部是它的标题", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被嵌的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n看这段:![[Target]]\n');
      await openInReadMode("Origin");

      await waitFor(() => expect(embedBlocks()).toHaveLength(1));
      const embed = embedBlocks()[0]!;
      await waitFor(() => expect(embed.dataset.embedState).toBe("filled"));
      expect(embed.querySelector(".notebook-embed-head")?.textContent).toBe("Target");
      expect(embed.querySelector(".notebook-embed-body")?.textContent).toContain("被嵌的正文");
      // frontmatter 不该跟着嵌进来。
      expect(embed.textContent).not.toContain("title:");
    });

    it("嵌入取数走只读命令,不会把目标笔记登记成打开", async () => {
      /* 关键约束:`notebook_open_note` 会在后端登记指纹,而嵌入是只读的。用它的话
         后面某次没带基线的保存会拿"嵌入渲染那一刻的指纹"当基线,把盲写放过去。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被嵌的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[Target]]\n');
      await openInReadMode("Origin");

      await waitFor(() => expect(harness.callCount("notebook_peek_note")).toBeGreaterThan(0));
      // Origin 自己是被 open 打开的(1 次);Target 只被 peek 过。
      expect(harness.callCount("notebook_open_note")).toBe(1);
    });

    it("带 #小节 时只嵌那一段", async () => {
      harness.seed(
        "Target.md",
        '---\ntitle: "Target"\n---\n\n# 甲节\n\n甲的内容\n\n# 乙节\n\n乙的内容\n',
      );
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[Target#乙节]]\n');
      await openInReadMode("Origin");

      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("filled"));
      const body = embedBlocks()[0]!.querySelector(".notebook-embed-body")!;
      expect(body.textContent).toContain("乙的内容");
      expect(body.textContent).not.toContain("甲的内容");
      expect(embedBlocks()[0]!.querySelector(".notebook-embed-head")?.textContent).toBe(
        "Target › 乙节",
      );
    });

    it("目标不存在时留下原始语法,不弹错误条", async () => {
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[还没写]]\n');
      await openInReadMode("Origin");

      await waitFor(() => expect(embedBlocks()).toHaveLength(1));
      const embed = embedBlocks()[0]!;
      expect(embed.textContent).toBe("![[还没写]]");
      expect(embed.classList.contains("notebook-wikilink-missing")).toBe(true);
      expect(embed.title).toBe("No note named 还没写");
      // 先写嵌入后写笔记是常态,不是故障。
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("取数失败时留下原始语法并说明原因,不影响其余正文", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被嵌的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n前面一句\n\n![[Target]]\n');
      harness.failPeek = true;
      await openInReadMode("Origin");

      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("error"));
      const embed = embedBlocks()[0]!;
      expect(embed.textContent).toBe("![[Target]]");
      expect(embed.title).toBe("Could not embed Target: reading the note failed");
      // 宿主自己的正文照常渲染 —— 一块嵌入失败不该把整页拖垮。
      expect(document.querySelector(".notebook-markdown-preview")?.textContent).toContain(
        "前面一句",
      );
    });

    it("点嵌入块的头部跳到那篇笔记", async () => {
      // 头部按 enhanceWikiLinks 的约定挂类名和 data-wiki-path,所以面板那个点击
      // 监听不用改一行就能用。这一条钉的正是那个约定。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被嵌的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[Target]]\n');
      await openInReadMode("Origin");

      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("filled"));
      const head = embedBlocks()[0]!.querySelector<HTMLElement>(".notebook-embed-head")!;
      fireEvent.click(head);
      await screen.findByDisplayValue("Target");
    });

    it("嵌入内容里的 [[链接]] 也能点着跳", async () => {
      harness.seed("Third.md", '---\ntitle: "Third"\n---\n\n第三篇\n');
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n再看 [[Third]]\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[Target]]\n');
      await openInReadMode("Origin");

      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("filled"));
      const inner = embedBlocks()[0]!.querySelector<HTMLElement>(
        ".notebook-embed-body a.notebook-wikilink",
      );
      expect(inner?.textContent).toBe("Third");
      fireEvent.click(inner!);
      await screen.findByDisplayValue("Third");
    });

    it("自己嵌自己当场被拦,不套三层", async () => {
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[Origin]]\n');
      await openInReadMode("Origin");

      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("error"));
      expect(embedBlocks()).toHaveLength(1);
      expect(embedBlocks()[0]!.title).toBe(
        "Embed of Origin is nested too deeply or refers back to itself",
      );
    });

    it("嵌入内容的标题不带 id,大纲跳转仍落在宿主的标题上", async () => {
      /* renderNoteMarkdown 每次新建 slug registry,宿主和嵌入会算出同一个 id;
         而大纲跳转用 querySelector 取文档序第一个。嵌入块排在宿主标题之前时,
         点大纲会跳进嵌入里。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n# 同名标题\n\n嵌入的内容\n');
      harness.seed(
        "Origin.md",
        '---\ntitle: "Origin"\n---\n\n![[Target]]\n\n# 同名标题\n\n宿主的内容\n',
      );
      await openInReadMode("Origin");

      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("filled"));
      const preview = document.querySelector(".notebook-markdown-preview")!;
      const withId = Array.from(preview.querySelectorAll("h1[id]"));
      // 只有宿主那个带 id,而且它就是宿主自己的标题。
      expect(withId).toHaveLength(1);
      expect(withId[0]!.closest(".notebook-embed")).toBeNull();
    });

    it("目标笔记出现之后,原来嵌不进来的那块会补上", async () => {
      /* 钉的是"失败的占位要跟着索引变化重试"。做法和 wikilink 那边同一条路径:
         新建笔记 → linkIndex 变 → effect 重跑。 */
      harness.seed("hou-xie.md", '---\ntitle: "旧的"\n---\n\n现在有内容了\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[后写的]]\n');
      await openInReadMode("Origin");
      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("error"));

      // 在列表里把另一篇改名成嵌入指的那个名字(双击进入行内重命名)。
      fireEvent.dblClick(screen.getByRole("button", { name: "hou-xie" }));
      const input = await screen.findByRole("textbox", { name: "Rename quick note" });
      fireEvent.change(input, { target: { value: "后写的" } });
      fireEvent.keyDown(input, { key: "Enter" });
      await screen.findByRole("button", { name: "后写的" });

      // 回到 Origin 的阅读态。
      fireEvent.click(screen.getByRole("button", { name: "Origin" }));
      await screen.findByDisplayValue("Origin");
      await readMode();
      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("filled"));
      expect(embedBlocks()[0]!.textContent).toContain("现在有内容了");
    });

    it("代码块里的 ![[..]] 不嵌任何东西", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被嵌的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n```\n![[Target]]\n```\n');
      await openInReadMode("Origin");

      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")?.textContent).toContain(
          "![[Target]]",
        ),
      );
      expect(embedBlocks()).toHaveLength(0);
      expect(harness.callCount("notebook_peek_note")).toBe(0);
    });

    it("正文没变的重渲染之后,已填好的嵌入不会退回占位", async () => {
      /* 和 wikilink 那条同一个根因(见 `NoteContentArea` 里 `html` 的注释):React 重写
         innerHTML 会把嵌入进来的正文整批扔掉,而 `enhanceNoteEmbeds` 的依赖没变不会重跑。
         嵌入比 wikilink 更疼:它退化后是一片空占位,而重新填充要再过一次 IPC。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被嵌的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[Target]]\n');
      await openInReadMode("Origin");
      await waitFor(() => expect(embedBlocks()[0]?.dataset.embedState).toBe("filled"));
      const peeks = harness.callCount("notebook_peek_note");

      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      await screen.findByRole("complementary", { name: "Outline" });

      expect(embedBlocks()[0]?.dataset.embedState).toBe("filled");
      expect(embedBlocks()[0]!.textContent).toContain("被嵌的正文");
      // 也不该为了补回来再取一次数。
      expect(harness.callCount("notebook_peek_note")).toBe(peeks);
    });
  });

  describe("```notebook-query 查询块", () => {
    /** 阅读态里渲染出的查询块。 */
    function queryBlocks(): HTMLElement[] {
      return Array.from(document.querySelectorAll<HTMLElement>(".notebook-query"));
    }

    async function openInReadMode(title: string) {
      renderNotebook();
      await screen.findByRole("button", { name: title });
      fireEvent.click(screen.getByRole("button", { name: title }));
      await screen.findByDisplayValue(title);
      fireEvent.click(screen.getByRole("button", { name: "Read" }));
      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")).not.toBeNull(),
      );
    }

    /** 一篇带查询块的宿主笔记 + 三篇有 status 字段的笔记。 */
    function seedVault(query: string) {
      harness.seed(
        "Host.md",
        `---\ntitle: "Host"\n---\n\n\`\`\`notebook-query\n${query}\n\`\`\`\n`,
      );
      harness.seed("A.md", '---\ntitle: "Alpha"\nstatus: active\n---\n\n甲\n');
      harness.seed("B.md", '---\ntitle: "Beta"\nstatus: active\n---\n\n乙\n');
      harness.seed("C.md", '---\ntitle: "Gamma"\nstatus: done\n---\n\n丙\n');
    }

    it("把围栏换成结果表,标题取的是 frontmatter 里那个", async () => {
      seedVault("key: status\nvalue: active");
      await openInReadMode("Host");

      await waitFor(() => expect(queryBlocks()).toHaveLength(1));
      const block = queryBlocks()[0]!;
      expect(block.querySelector(".notebook-query-head")?.textContent).toBe(
        "Query · status = active · 2 notes",
      );
      const names = Array.from(block.querySelectorAll("tbody tr td:first-child")).map(
        (td) => td.textContent,
      );
      expect(names).toEqual(["Alpha", "Beta"]);
      // 围栏源码不该还留在页面上。
      expect(document.querySelector('pre[data-language="notebook-query"]')).toBeNull();
    });

    it("点结果里的笔记名跳过去", async () => {
      seedVault("key: status\nvalue: active");
      await openInReadMode("Host");

      await waitFor(() => expect(queryBlocks()).toHaveLength(1));
      const link = queryBlocks()[0]!.querySelector<HTMLAnchorElement>("tbody a")!;
      expect(link.title).toBe("Open Alpha");
      fireEvent.click(link);
      // 跳过去之后编辑器里是 Alpha。
      await screen.findByDisplayValue("Alpha");
    });

    it("limit 截断时表头把总数说出来", async () => {
      seedVault("key: status\nlimit: 1");
      await openInReadMode("Host");

      await waitFor(() => expect(queryBlocks()).toHaveLength(1));
      expect(queryBlocks()[0]!.querySelector(".notebook-query-head")?.textContent).toBe(
        "Query · status · showing 1 of 3",
      );
    });

    it("写错的指令当场报出来,而且不去扫全库", async () => {
      seedVault("keys: status");
      const before = harness.fieldScanCalls;
      await openInReadMode("Host");

      await waitFor(() => expect(queryBlocks()).toHaveLength(1));
      const lines = Array.from(queryBlocks()[0]!.querySelectorAll(".notebook-query-error")).map(
        (el) => el.textContent,
      );
      expect(lines).toEqual([
        "Unknown directive `keys`. Only key, value, sort and limit are understood.",
        "Missing a field name — write at least `key: <field>`.",
      ]);
      expect(harness.fieldScanCalls).toBe(before);
    });

    it("扫描失败时把失败说出来", async () => {
      seedVault("key: status");
      harness.failFieldScan = true;
      await openInReadMode("Host");

      await waitFor(() => expect(queryBlocks()).toHaveLength(1));
      expect(queryBlocks()[0]!.textContent).toContain("Query failed: scanning fields failed");
    });

    it("一条都没匹配上时显示空提示", async () => {
      seedVault("key: nope");
      await openInReadMode("Host");

      await waitFor(() => expect(queryBlocks()).toHaveLength(1));
      expect(queryBlocks()[0]!.querySelector(".notebook-query-empty")).not.toBeNull();
      expect(queryBlocks()[0]!.querySelector("table")).toBeNull();
    });

    it("没有查询块的笔记不会去扫全库字段", async () => {
      harness.seed("Plain.md", '---\ntitle: "Plain"\n---\n\n普通正文\n');
      const before = harness.fieldScanCalls;
      await openInReadMode("Plain");
      expect(harness.fieldScanCalls).toBe(before);
    });
  });

  describe("阅读态勾选任务", () => {
    /** 阅读态里那些可点的任务复选框(解禁过的)。 */
    function liveBoxes(): HTMLInputElement[] {
      return Array.from(
        document.querySelectorAll<HTMLInputElement>(
          ".notebook-markdown-preview li.notebook-task-item input.notebook-task-checkbox",
        ),
      );
    }

    /** 阅读态里所有复选框,含没解禁的。 */
    function allBoxes(): HTMLInputElement[] {
      return Array.from(
        document.querySelectorAll<HTMLInputElement>(
          '.notebook-markdown-preview input[type="checkbox"]',
        ),
      );
    }

    async function readMode() {
      fireEvent.click(screen.getByRole("button", { name: "Read" }));
      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")).not.toBeNull(),
      );
    }

    async function openInReadMode(title: string) {
      renderNotebook();
      await screen.findByRole("button", { name: title });
      fireEvent.click(screen.getByRole("button", { name: title }));
      await screen.findByDisplayValue(title);
      await readMode();
    }

    it("阅读态的复选框是可点的,点一下把源码那一行勾上", async () => {
      const planPath = harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n- [ ] 一\n- [ ] 二\n');
      await openInReadMode("Plan");

      await waitFor(() => expect(liveBoxes()).toHaveLength(2));
      fireEvent.click(liveBoxes()[0]!);

      // 正文改了 → 重渲染 → 第一个框变成已勾。
      await waitFor(() => expect(liveBoxes()[0]?.checked).toBe(true));
      expect(liveBoxes()[1]?.checked).toBe(false);
      // 切回源码态看真正落到正文里的字符。
      fireEvent.click(screen.getByRole("button", { name: "Source" }));
      await waitFor(() => expect(harness.read(planPath)).toContain("- [x] 一"));
      expect(harness.read(planPath)).toContain("- [ ] 二");
    });

    it("再点一下取消勾选", async () => {
      const planPath = harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n- [x] 已经勾了\n');
      await openInReadMode("Plan");

      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      expect(liveBoxes()[0]?.checked).toBe(true);
      fireEvent.click(liveBoxes()[0]!);

      await waitFor(() => expect(liveBoxes()[0]?.checked).toBe(false));
      await waitFor(() => expect(harness.read(planPath)).toContain("- [ ] 已经勾了"));
    });

    it("点第二条只改第二行", async () => {
      const planPath = harness.seed(
        "Plan.md",
        '---\ntitle: "Plan"\n---\n\n- [ ] 一\n- [ ] 二\n- [ ] 三\n',
      );
      await openInReadMode("Plan");

      await waitFor(() => expect(liveBoxes()).toHaveLength(3));
      fireEvent.click(liveBoxes()[1]!);

      await waitFor(() => expect(liveBoxes()[1]?.checked).toBe(true));
      const saved = await waitFor(() => {
        const body = harness.read(planPath) ?? "";
        if (!body.includes("- [x]")) throw new Error("not saved yet");
        return body;
      });
      expect(saved).toContain("- [ ] 一\n- [x] 二\n- [ ] 三");
    });

    /* 围栏里的 `- [ ]` 不产复选框,所以真任务前面没有"幻影"占位。按顺序数复选框的
       实现会在这里把行号算少几行,勾到围栏那一行去(而那一行改了也不会显示成勾上)。 */
    it("围栏里的假任务不参与计数,真任务勾对行", async () => {
      const planPath = harness.seed(
        "Plan.md",
        '---\ntitle: "Plan"\n---\n\n```md\n- [ ] 文档示例\n```\n\n- [ ] 真任务\n',
      );
      await openInReadMode("Plan");

      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      fireEvent.click(liveBoxes()[0]!);

      await waitFor(() => expect(liveBoxes()[0]?.checked).toBe(true));
      const saved = await waitFor(() => {
        const body = harness.read(planPath) ?? "";
        if (!body.includes("- [x]")) throw new Error("not saved yet");
        return body;
      });
      expect(saved).toContain("- [x] 真任务");
      // 围栏里那一行必须原样不动。
      expect(saved).toContain("```md\n- [ ] 文档示例\n```");
    });

    /* 多行 `$$` 会在渲染前被压成一行哨兵。行号如果在那之后才算,公式后面每个任务
       都会往前偏几行 —— 偏到公式内部去,正文被改坏而复选框看着像没反应。 */
    it("公式之后的任务勾的是正确那一行", async () => {
      const planPath = harness.seed(
        "Plan.md",
        '---\ntitle: "Plan"\n---\n\n$$\na = 1\nb = 2\nc = 3\n$$\n\n- [ ] 公式后面\n',
      );
      await openInReadMode("Plan");

      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      fireEvent.click(liveBoxes()[0]!);

      await waitFor(() => expect(liveBoxes()[0]?.checked).toBe(true));
      const saved = await waitFor(() => {
        const body = harness.read(planPath) ?? "";
        if (!body.includes("- [x]")) throw new Error("not saved yet");
        return body;
      });
      expect(saved).toContain("- [x] 公式后面");
      // 公式一个字都不能动。
      expect(saved).toContain("$$\na = 1\nb = 2\nc = 3\n$$");
    });

    it("嵌套任务:点内层只改内层那一行", async () => {
      const planPath = harness.seed(
        "Plan.md",
        '---\ntitle: "Plan"\n---\n\n- [ ] 外层\n  - [ ] 内层\n',
      );
      await openInReadMode("Plan");

      await waitFor(() => expect(liveBoxes()).toHaveLength(2));
      fireEvent.click(liveBoxes()[1]!);

      await waitFor(() => expect(liveBoxes()[1]?.checked).toBe(true));
      expect(liveBoxes()[0]?.checked).toBe(false);
      const saved = await waitFor(() => {
        const body = harness.read(planPath) ?? "";
        if (!body.includes("- [x]")) throw new Error("not saved yet");
        return body;
      });
      expect(saved).toContain("- [ ] 外层\n  - [x] 内层");
    });

    it("嵌进来的别人的任务不可点 —— 行号对不上当前正文", async () => {
      const otherPath = harness.seed("Other.md", '---\ntitle: "Other"\n---\n\n- [ ] 别人的任务\n');
      const planPath = harness.seed(
        "Plan.md",
        '---\ntitle: "Plan"\n---\n\n- [ ] 我的任务\n\n![[Other]]\n',
      );
      await openInReadMode("Plan");

      // 等嵌入填好:那之后 DOM 里有两个复选框,但只有一个是解禁的。
      await waitFor(() => expect(allBoxes()).toHaveLength(2));
      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      expect(liveBoxes()[0]?.getAttribute("aria-label")).toBe("Toggle task: 我的任务");

      const embedded = allBoxes().find((box) => !box.classList.contains("notebook-task-checkbox"));
      expect(embedded?.disabled).toBe(true);
      fireEvent.click(embedded!);
      // 点它不该改任何一篇笔记。
      expect(harness.read(otherPath)).toContain("- [ ] 别人的任务");
      expect(harness.read(planPath)).toContain("- [ ] 我的任务");
    });

    it("点任务里的 wikilink 是跳转,不是勾选", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n目标正文\n');
      const planPath = harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n- [ ] 看 [[Target]]\n');
      await openInReadMode("Plan");

      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      const link = document.querySelector<HTMLElement>(
        ".notebook-markdown-preview a.notebook-wikilink",
      );
      fireEvent.click(link!);

      await screen.findByDisplayValue("Target");
      // 原来那篇的任务没被勾上。
      expect(harness.read(planPath)).toContain("- [ ] 看 [[Target]]");
    });

    it("复选框带无障碍名,文案跟着任务文本走", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n- [ ] 写周报\n- [x] 交周报\n');
      await openInReadMode("Plan");

      await waitFor(() => expect(liveBoxes()).toHaveLength(2));
      expect(liveBoxes().map((box) => box.getAttribute("aria-label"))).toEqual([
        "Toggle task: 写周报",
        "Toggle task: 交周报",
      ]);
    });

    it("源码态不解禁复选框(那边没有预览容器)", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n- [ ] 一\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Plan" });
      fireEvent.click(screen.getByRole("button", { name: "Plan" }));
      await screen.findByDisplayValue("Plan");

      expect(liveBoxes()).toHaveLength(0);
    });

    /* 乐观锁挡下的那一次点击。
     *
     * 复选框上的状态快照来自一次渲染,而正文可能已经被自动保存回填、外部编辑或另一次
     * 点击改过。这里把快照改成与源码不符(等价于"渲染之后正文变了"),点下去必须整个
     * 放弃:不写正文、不落盘,复选框也不能停在已勾的样子 —— 停在那儿会让用户以为勾上了。 */
    it("状态快照与正文不符时,这一次点击整个作废", async () => {
      const planPath = harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n- [ ] 一\n');
      await openInReadMode("Plan");
      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      const savesBefore = harness.callCount("notebook_save_note");

      const li = document.querySelector<HTMLElement>(
        ".notebook-markdown-preview li.notebook-task-item",
      );
      li!.setAttribute("data-task-checked", "1");
      fireEvent.click(liveBoxes()[0]!);

      await new Promise((resolve) => setTimeout(resolve, 1200));
      expect(harness.read(planPath)).toContain("- [ ] 一");
      expect(harness.callCount("notebook_save_note")).toBe(savesBefore);
      // 复选框也不能停在已勾的样子 —— 停在那儿会让用户以为勾上了。
      expect(liveBoxes()[0]?.checked).toBe(false);
    });

    /* 正文没变的重渲染不能让复选框失效。
     *
     * 触发的是日常操作(开大纲、切侧栏档、自动保存回填状态),那时候 `markdownHtml`、
     * `mode`、笔记 id 全是原值。这里用开大纲来制造。
     *
     * 曾经的坏法:`dangerouslySetInnerHTML` 的属性值每次渲染都是新对象,React 会照样
     * 重写一遍 innerHTML,预览里的子节点整批换新、解禁全丢,而解禁 effect 的依赖没变
     * 不会重跑 —— 复选框永久点不动。现在那个对象 memo 掉了(见 `NoteContentArea`),
     * DOM 保持原样。两道保险都在:即便将来 React 的比较语义变了,解禁 effect 也故意
     * 不带依赖数组、每次提交后重跑。 */
    it("正文没变的重渲染之后,复选框仍然可点", async () => {
      const planPath = harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n- [ ] 一\n');
      await openInReadMode("Plan");
      await waitFor(() => expect(liveBoxes()).toHaveLength(1));

      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      await screen.findByRole("complementary", { name: "Outline" });

      // 解禁必须还在,否则下面这一次点击落不到。
      expect(liveBoxes()).toHaveLength(1);
      expect(liveBoxes()[0]?.disabled).toBe(false);
      fireEvent.click(liveBoxes()[0]!);

      await waitFor(() => expect(harness.read(planPath)).toContain("- [x] 一"));
    });

    /* 渲染之后正文又变了的那一种情况。
     *
     * 点击处理的依赖里没有正文(有的话每敲一个字都要重挂监听),所以它闭包里的那份正文
     * 是绑定那一刻的快照。分屏态一边打字一边点复选框就正好命中:如果按快照算出整份新
     * 正文再整块写回,刚敲的字会被抹掉 —— 不是勾错行,是丢别的编辑。所以要在
     * `setNotes` 的 updater 里现读最新正文。 */
    it("分屏态改过正文之后再勾选,不会抹掉刚敲的字", async () => {
      const planPath = harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n- [ ] 一\n\n尾巴\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Plan" });
      fireEvent.click(screen.getByRole("button", { name: "Plan" }));
      await screen.findByDisplayValue("Plan");
      fireEvent.click(screen.getByRole("button", { name: "Split" }));
      await waitFor(() => expect(liveBoxes()).toHaveLength(1));

      setEditorValue("- [ ] 一\n\n尾巴 新增的一段\n");
      // 等预览跟上,确认这次改动已经进了状态。
      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")?.textContent).toContain(
          "新增的一段",
        ),
      );

      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      fireEvent.click(liveBoxes()[0]!);

      const saved = await waitFor(() => {
        const body = harness.read(planPath) ?? "";
        if (!body.includes("- [x]")) throw new Error("not saved yet");
        return body;
      });
      expect(saved).toContain("- [x] 一");
      expect(saved).toContain("新增的一段");
    });

    /* 两篇正文完全相同的笔记渲染出同一个 HTML 字符串。effect 只按 HTML 重挂的话,
       切过去时闭包里还是上一篇的 id —— 点一下会改没显示的那篇。 */
    it("切到正文相同的另一篇后,勾选改的是当前这篇", async () => {
      const pathA = harness.seed("A.md", '---\ntitle: "A"\n---\n\n- [ ] 同样的正文\n');
      const pathB = harness.seed("B.md", '---\ntitle: "B"\n---\n\n- [ ] 同样的正文\n');
      await openInReadMode("A");
      await waitFor(() => expect(liveBoxes()).toHaveLength(1));

      fireEvent.click(screen.getByRole("button", { name: "B" }));
      await screen.findByDisplayValue("B");
      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      fireEvent.click(liveBoxes()[0]!);

      await waitFor(() => expect(harness.read(pathB)).toContain("- [x] 同样的正文"));
      // A 必须一个字都没动 —— 磁盘上没动,内存里的那份也没动。
      expect(harness.read(pathA)).toContain("- [ ] 同样的正文");
      fireEvent.click(screen.getByRole("button", { name: "A" }));
      await screen.findByDisplayValue("A");
      await waitFor(() => expect(liveBoxes()).toHaveLength(1));
      expect(liveBoxes()[0]?.checked).toBe(false);
    });
  });

  describe("wikilink 悬浮预览", () => {
    /** 切到阅读态并等 HTML 挂上。 */
    async function readMode() {
      fireEvent.click(screen.getByRole("button", { name: "Read" }));
      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")).not.toBeNull(),
      );
    }

    /** 打开某篇笔记的阅读态。 */
    async function openInReadMode(title: string) {
      renderNotebook();
      await screen.findByRole("button", { name: title });
      fireEvent.click(screen.getByRole("button", { name: title }));
      await screen.findByDisplayValue(title);
      await readMode();
    }

    function hoverCard(): HTMLElement | null {
      return document.querySelector<HTMLElement>(".notebook-hover-card");
    }

    /** 悬到第一条 wikilink 上并等过出卡延迟。 */
    async function hoverFirstLink() {
      const link = await waitFor(() => {
        const found = document.querySelector<HTMLElement>(
          ".notebook-markdown-preview a.notebook-wikilink[data-wiki-path]",
        );
        if (!found) throw new Error("no resolved wikilink yet");
        return found;
      });
      fireEvent.mouseOver(link);
      return link;
    }

    it("停在链接上弹出目标笔记的开头", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被预览的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Target]]\n');
      await openInReadMode("Origin");
      await hoverFirstLink();

      await waitFor(() => expect(hoverCard()?.style.display).toBe("block"));
      expect(hoverCard()?.querySelector(".notebook-hover-head")?.textContent).toBe("Target");
      await waitFor(() =>
        expect(hoverCard()?.querySelector(".notebook-hover-body")?.textContent).toContain(
          "被预览的正文",
        ),
      );
      // frontmatter 不该出现在预览里。
      expect(hoverCard()?.textContent).not.toContain("title:");
    });

    it("预览取数走只读命令,不会把目标笔记登记成打开", async () => {
      /* 和嵌入同一条约束:`notebook_open_note` 会在后端登记指纹。悬浮只是看一眼,
         登记之后某次没带基线的保存会拿这一刻的指纹当基线,把盲写放过去。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被预览的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Target]]\n');
      await openInReadMode("Origin");
      await hoverFirstLink();

      await waitFor(() => expect(harness.callCount("notebook_peek_note")).toBe(1));
      // Origin 自己是被 open 打开的(1 次);Target 只被 peek 过。
      expect(harness.callCount("notebook_open_note")).toBe(1);
    });

    it("取数失败时卡片里说明加载不出来,不弹错误条", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被预览的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Target]]\n');
      await openInReadMode("Origin");
      harness.failPeek = true;
      await hoverFirstLink();

      await waitFor(() =>
        expect(hoverCard()?.querySelector(".notebook-hover-body")?.textContent).toBe(
          "Could not load a preview",
        ),
      );
      // 看一眼失败是小事,不该占用整个面板的错误条。
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("死链不弹卡", async () => {
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[还没写]]\n');
      await openInReadMode("Origin");

      const link = await waitFor(() => {
        const found = document.querySelector<HTMLElement>(
          ".notebook-markdown-preview a.notebook-wikilink",
        );
        if (!found) throw new Error("no wikilink yet");
        return found;
      });
      expect(link.dataset.wikiPath).toBeUndefined();
      fireEvent.mouseOver(link);

      // 等过出卡延迟(380ms)才有意义 —— 提前查等于什么都没查。
      await new Promise((resolve) => setTimeout(resolve, 450));
      expect(hoverCard()).toBeNull();
      expect(harness.callCount("notebook_peek_note")).toBe(0);
    });

    it("嵌入块的头部不弹卡", async () => {
      // 那块内容就在头部下面摊开着,再弹一张卡挡住它没有意义。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被嵌的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n![[Target]]\n');
      await openInReadMode("Origin");
      await waitFor(() =>
        expect(document.querySelector<HTMLElement>(".notebook-embed")?.dataset.embedState).toBe(
          "filled",
        ),
      );

      const head = document.querySelector<HTMLElement>(".notebook-embed-head")!;
      expect(head.dataset.wikiPath).toBeTruthy();
      fireEvent.mouseOver(head);
      await new Promise((resolve) => setTimeout(resolve, 450));
      expect(hoverCard()).toBeNull();
    });

    it("离开阅读态之后卡片不留在界面上", async () => {
      // 卡片挂在 document.body 上,不摘会浮在编辑区上面。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\n被预览的正文\n');
      harness.seed("Origin.md", '---\ntitle: "Origin"\n---\n\n见 [[Target]]\n');
      await openInReadMode("Origin");
      await hoverFirstLink();
      await waitFor(() => expect(hoverCard()?.style.display).toBe("block"));

      fireEvent.click(screen.getByRole("button", { name: "Source" }));
      await waitFor(() => expect(hoverCard()).toBeNull());
    });
  });

  describe("全屏 ⇄ 半屏", () => {
    /** 宿主态由外面拿着,这里模拟 ProjectPage 那一侧。 */
    function renderWithHost(initial = false) {
      const seen: boolean[] = [];
      function Host() {
        const [full, setFull] = useState(initial);
        return (
          <I18nProvider>
            <NotebookPanel
              fullScreen={full}
              onFullScreenChange={(next) => {
                seen.push(next);
                setFull(next);
              }}
            />
          </I18nProvider>
        );
      }
      render(<Host />);
      return seen;
    }

    /* 按钮里那个图标的名字。
     *
     * 图标是唯一给到明眼用户的状态信号 —— aria-label 只喂给屏读。两态用同一个
     * 图标的话,眼睛看到的是"点了没变化"。lucide 把名字写进 svg 的 class。 */
    function iconOf(button: HTMLElement): string {
      const cls = button.querySelector("svg")?.getAttribute("class") ?? "";
      return cls.split(/\s+/).find((name) => name.startsWith("lucide-")) ?? "";
    }

    it("宿主接了回调时给出开关,点一次报 true 再点报 false", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      const seen = renderWithHost();
      await screen.findByRole("button", { name: "Target" });
      fireEvent.click(screen.getByRole("button", { name: "Target" }));
      await screen.findByDisplayValue("Target");

      const enter = screen.getByRole("button", { name: "Full screen" });
      // 半屏态下 aria-pressed 必须是 false —— 屏读用户要能听出当前在哪一档。
      expect(enter).toHaveAttribute("aria-pressed", "false");
      expect(iconOf(enter)).toBe("lucide-maximize2");
      fireEvent.click(enter);

      // 进全屏后按钮换成回半屏,不是留着同一个标签 —— 否则用户看不出点过之后
      // 发生了什么,也不知道再点一次会去哪。
      const leave = await screen.findByRole("button", { name: "Half screen" });
      expect(leave).toHaveAttribute("aria-pressed", "true");
      expect(iconOf(leave)).toBe("lucide-minimize2");
      expect(screen.queryByRole("button", { name: "Full screen" })).toBeNull();
      fireEvent.click(leave);

      await screen.findByRole("button", { name: "Full screen" });
      expect(seen).toEqual([true, false]);
    });

    it("宿主没接回调时不给这个按钮", async () => {
      /* 全屏要盖掉的是面板外面那圈,面板自己动不了。首页的随手记视图身边没有
         可让的东西 —— 那里画一个点了没反应的开关比不画更糟。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });
      fireEvent.click(screen.getByRole("button", { name: "Target" }));
      await screen.findByDisplayValue("Target");

      expect(screen.queryByRole("button", { name: "Full screen" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Half screen" })).toBeNull();
    });

    it("宿主一开始就是全屏时按钮直接显示回半屏", async () => {
      // 从别的视图切回随手记时全屏偏好还在,那时按钮不能显示成「进全屏」。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderWithHost(true);
      await screen.findByRole("button", { name: "Target" });
      fireEvent.click(screen.getByRole("button", { name: "Target" }));
      await screen.findByDisplayValue("Target");

      const leave = await screen.findByRole("button", { name: "Half screen" });
      expect(iconOf(leave)).toBe("lucide-minimize2");
      expect(screen.queryByRole("button", { name: "Full screen" })).toBeNull();
    });
  });
  describe("自定义图标", () => {
    async function openRowMenu(name: string) {
      const row = screen.getByRole("button", { name }).closest("[data-notebook-note-row]");
      if (!row) throw new Error(`no row for ${name}`);
      fireEvent.contextMenu(row);
      return screen.getByRole("menu", { name: "Quick note actions" });
    }

    /** 打开某条笔记的图标选择器。 */
    async function openPicker(name: string) {
      await openRowMenu(name);
      fireEvent.click(screen.getByRole("menuitem", { name: "Change icon" }));
      return screen.findByRole("dialog", { name: "Change icon" });
    }

    /** 行上那个装饰图标的 lucide 名字。没有就返回空串。 */
    function rowIcon(name: string): string {
      const row = screen.getByRole("button", { name }).closest("[data-notebook-note-row]");
      const svg = row?.querySelector('span[aria-hidden="true"] svg');
      const cls = svg?.getAttribute("class") ?? "";
      return cls.split(/\s+/).find((n) => n.startsWith("lucide-")) ?? "";
    }

    it("挑一个图标:列表当场变,并且真的写进了图标表", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });
      expect(rowIcon("Target")).toBe("");

      await openPicker("Target");
      fireEvent.click(screen.getByRole("button", { name: "Reading" }));

      await waitFor(() => expect(rowIcon("Target")).toBe("lucide-book"));
      // 键是 vault 相对路径,不是绝对路径 —— vault 搬走之后图标还在。
      await waitFor(() => expect(harness.iconTable()).toEqual({ "Target.md": "book" }));
      // 挑完就关,不用再点一次。
      expect(screen.queryByRole("dialog", { name: "Change icon" })).toBeNull();
    });

    it("上次会话留下的图标在面板打开时就显示出来", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.seedIcons({ "Target.md": "flame" });
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await waitFor(() => expect(rowIcon("Target")).toBe("lucide-flame"));
    });

    it("恢复默认把图标去掉,并从表里删掉那个键", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.seed("Other.md", '---\ntitle: "Other"\n---\n\nbody\n');
      harness.seedIcons({ "Target.md": "book", "Other.md": "star" });
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });
      await waitFor(() => expect(rowIcon("Target")).toBe("lucide-book"));

      await openPicker("Target");
      fireEvent.click(screen.getByRole("button", { name: "Reset to default" }));

      await waitFor(() => expect(rowIcon("Target")).toBe(""));
      /* 键要真的被删掉而不是存成空串 —— 空串会一直占着位置,而且下一版如果给空串
         赋了含义就会解释成别的东西。同时别人的图标不能被顺手清掉。 */
      await waitFor(() => expect(harness.iconTable()).toEqual({ "Other.md": "star" }));
    });

    it("选择器开在右键的那个位置", async () => {
      /* 右键菜单是在鼠标处弹的,图标选择器接着它出现,却跑到屏幕左上角的话,
         用户得把鼠标横穿整个窗口才能选。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      const row = (await screen.findByRole("button", { name: "Target" })).closest(
        "[data-notebook-note-row]",
      );
      if (!row) throw new Error("no row");
      fireEvent.contextMenu(row, { clientX: 314, clientY: 159 });
      fireEvent.click(screen.getByRole("menuitem", { name: "Change icon" }));

      const picker = await screen.findByRole("dialog", { name: "Change icon" });
      expect(picker.style.left).toBe("314px");
      expect(picker.style.top).toBe("159px");
      // 菜单本身要让位,两层浮层叠着看不清。
      expect(screen.queryByRole("menu", { name: "Quick note actions" })).toBeNull();
    });

    it("没设过图标时「恢复默认」是禁用的", async () => {
      // 无事可做的按钮点下去只会让人怀疑是不是没生效。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openPicker("Target");
      expect(screen.getByRole("button", { name: "Reset to default" })).toBeDisabled();
    });

    it("当前图标在选择器里是按下态", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.seedIcons({ "Target.md": "book" });
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      await openPicker("Target");
      expect(screen.getByRole("button", { name: "Reading" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      expect(screen.getByRole("button", { name: "Goal" })).toHaveAttribute("aria-pressed", "false");
    });

    it("写盘失败时图标回滚,并报错", async () => {
      /* 留着一个「看起来改了、重开面板又变回去」的图标比当场说失败更难查。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Target" });

      harness.failIconWrite = "write";
      await openPicker("Target");
      fireEvent.click(screen.getByRole("button", { name: "Reading" }));

      expect(await screen.findByText("writing icons failed")).toBeInTheDocument();
      await waitFor(() => expect(rowIcon("Target")).toBe(""));
    });

    it("改图标不把当前笔记顶掉", async () => {
      // 改图标只动列表上的一个符号,把用户手上正在编辑的那篇顶掉是纯粹的打扰。
      harness.seed("Editing.md", '---\ntitle: "Editing"\n---\n\n正在写\n');
      harness.seed("Other.md", '---\ntitle: "Other"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Editing" });
      fireEvent.click(screen.getByRole("button", { name: "Editing" }));
      await screen.findByDisplayValue("Editing");

      await openPicker("Other");
      fireEvent.click(screen.getByRole("button", { name: "Goal" }));

      await waitFor(() => expect(rowIcon("Other")).toBe("lucide-target"));
      // 标题框还是 Editing,不是 Other。
      expect(screen.getByDisplayValue("Editing")).toBeInTheDocument();
    });

    it("图标是装饰,不进行的可及名", async () => {
      /* 加进可及名会让屏读把「书 周报」读成一个整体,而「书」只是用户挑的一个符号。
         这条同时守着测试自己:全库的行查询都按 `{ name: 标题 }` 找,图标一旦进了
         可及名,那些查询会集体失配。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.seedIcons({ "Target.md": "book" });
      renderNotebook();

      const row = await screen.findByRole("button", { name: "Target" });
      expect(row.textContent).toBe("Target");
      const decoration = row
        .closest("[data-notebook-note-row]")
        ?.querySelector('span[aria-hidden="true"] svg');
      expect(decoration).not.toBeNull();
    });

    it("图标表读不出来时面板照常打开,只是没有图标", async () => {
      // 图标是装饰,读失败不该占用那条"你的笔记出事了"的提示。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.seedIcons({ "Target.md": "book" });
      harness.failIconWrite = "read";
      renderNotebook();

      await screen.findByRole("button", { name: "Target" });
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(rowIcon("Target")).toBe("");
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });

  describe("反链", () => {
    /** 打开侧栏并切到反链档。 */
    async function openBacklinks() {
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      // 没扫过之前标签上不带计数(那会看起来像"确实没有")。
      fireEvent.click(await screen.findByRole("button", { name: /^Backlinks/ }));
      return screen.findByRole("complementary", { name: "Backlinks" });
    }

    /* 反链列表里的跳转按钮的可及名。
       限定在反链那个 aside 里找:「未链接的提及」和反链同一档(在它下面),而那些行的
       可及名也是 `{title}, line {line}` —— 全局按名字筛会把两档混在一起,而混起来之后
       这个断言就不再说明"反链里有几条"。 */
    function backlinkNames(): string[] {
      const panel = screen.getByRole("complementary", { name: "Backlinks" });
      return [...panel.querySelectorAll("button[aria-label]")]
        .map((button) => button.getAttribute("aria-label") ?? "")
        .filter((name) => /line \d+$/.test(name));
    }

    it("按标题写的链接也出现在反链里", async () => {
      /* 这是与 Markio 的实质差异:文件名是 `cao-gao`,标题是 Weekly。Markio 的
         `find_backlinks` 按文件名 stem grep,这一类会整片漏掉。 */
      harness.seed("cao-gao.md", '---\ntitle: "Weekly"\n---\n\nbody\n');
      harness.seed("src.md", '---\ntitle: "Source"\n---\n\n见 [[Weekly]] 这里\n');
      renderNotebook();
      /* 点的是文件名 `cao-gao` 而不是标题 —— 列表只读目录项,未读入的笔记行上
         显示的是文件名 stem。这是列表的既有行为,不在反链范围内。 */
      fireEvent.click(await screen.findByRole("button", { name: "cao-gao" }));
      await screen.findByDisplayValue("Weekly");

      await openBacklinks();
      expect(await screen.findByText("见 [[Weekly]] 这里")).toBeInTheDocument();
      // frontmatter 那三行也算进行号:跳转是按整篇源码的行数走的。
      expect(backlinkNames()).toEqual(["Source, line 5"]);
      // 计数进标签,不用点开才知道有没有。
      expect(screen.getByRole("button", { name: "Backlinks (1)" })).toBeInTheDocument();
    });

    it("换到另一篇笔记时反链跟着换,不重扫", async () => {
      /* 扫描结果是全库的,换笔记只是换一个筛选条件。重扫一遍是纯浪费,而反链档
         的扫描是整个面板里最贵的一次 IO。 */
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\nbody\n');
      harness.seed("Beta.md", '---\ntitle: "Beta"\n---\n\nbody\n');
      harness.seed("src.md", '---\ntitle: "Source"\n---\n\n[[Alpha]]\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Alpha" }));
      await openBacklinks();
      await screen.findByText("[[Alpha]]");
      const scans = harness.callCount("notebook_vault_links");

      fireEvent.click(screen.getByRole("button", { name: "Beta" }));
      await waitFor(() => expect(screen.getByText("No note links here yet.")).toBeInTheDocument());
      expect(harness.callCount("notebook_vault_links")).toBe(scans);
    });

    it("没打开反链档时不扫全库", async () => {
      // 读每个文件的全文,而绝大多数时候用户根本没打开反链。
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\n[[Alpha]]\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Alpha" });
      // 侧栏开着但停在大纲档,也不该扫。
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(harness.callCount("notebook_vault_links")).toBe(0);
    });

    it("侧栏整个收起时不扫", async () => {
      /* 门控是两个条件的与。上一条只验了"停在别的档上",这条验另一半 —— 而侧栏
         默认就是收起的,漏掉这一半等于每次开着反链档的会话都在后台白扫。 */
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\n[[Alpha]]\n');
      renderNotebook();
      // 等笔记读进来 —— 侧栏的开关在那之前还不在场。
      await screen.findByRole("button", { name: "Alpha" });
      await openBacklinks();
      await waitFor(() => expect(harness.callCount("notebook_vault_links")).toBe(1));

      fireEvent.click(screen.getByRole("button", { name: "Hide outline" }));
      await waitFor(() =>
        expect(screen.queryByRole("complementary", { name: "Backlinks" })).toBeNull(),
      );
      expect(harness.callCount("notebook_vault_links")).toBe(1);

      // 重新展开时回到同一档,这时才该再扫(收起期间别人可能改过 vault)。
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      await screen.findByRole("complementary", { name: "Backlinks" });
      await waitFor(() => expect(harness.callCount("notebook_vault_links")).toBe(2));
    });

    it("自引用不算,死链不算", async () => {
      harness.seed("self.md", '---\ntitle: "Self"\n---\n\n[[Self]] 指向我自己\n[[根本不存在]]\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Self" }));
      await openBacklinks();

      await waitFor(() => expect(screen.getByText("No note links here yet.")).toBeInTheDocument());
    });

    it("点一条反链跳到来源笔记", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.seed("src.md", '---\ntitle: "Source"\n---\n\n第一行\n见 [[Target]]\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Target" }));
      await screen.findByDisplayValue("Target");
      await openBacklinks();

      fireEvent.click(await screen.findByRole("button", { name: "Source, line 6" }));
      // 标题框换成来源笔记 —— 反链的用途就是走到引用它的地方去。
      expect(await screen.findByDisplayValue("Source")).toBeInTheDocument();

      /* 光标落在那一行的行首。只切笔记不落光标的话用户还得自己找那一行,而一篇
         长笔记里"第 6 行"根本不在视野内。

         注意这个偏移是按**编辑器里的正文**算的,不是按文件:文件第 6 行是
         `见 [[Target]]`,而正文(拆掉 frontmatter 之后)是 `第一行\n见 [[Target]]\n`,
         那一行的行首在 4。两个坐标系差几行取决于 frontmatter 有多长。 */
      await waitFor(() => expect(editorView().state.selection.main.head).toBe(4));
    });

    it("跳到一篇还没读入的笔记也落在那一行", async () => {
      /* 与上一条的差别只在"来源笔记的正文有没有到位",而这恰好是最常见的情形:
         列表只读目录项,除了当前这篇之外都还没读入。正文比编辑器晚到时,落点是在
         编辑器挂好之后才算出来的 —— 只在挂载那一刻读一次 prop 的写法在这里会静默
         把光标留在开头。 */
      /* src 先种、Target 后种:列表按 mtime 倒序,Target 成为挂载时的当前笔记,于是
         src 从头到尾没被读入过(上一条里它恰好是当前笔记,一挂载就读了)。 */
      harness.seed("src.md", '---\ntitle: "Source"\n---\n\n第一行\n见 [[Target]]\n');
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByDisplayValue("Target");
      await openBacklinks();

      fireEvent.click(await screen.findByRole("button", { name: "Source, line 6" }));
      expect(await screen.findByDisplayValue("Source")).toBeInTheDocument();
      await waitFor(() => expect(editorView().state.selection.main.head).toBe(4));
    });

    it("刷新会重扫,能看到外部新加的引用", async () => {
      /* 别人的笔记被外部编辑器改过时,反链只能靠重扫发现 —— 面板不监听整个
         vault 的文件变化。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Target" }));
      await openBacklinks();
      await waitFor(() => expect(screen.getByText("No note links here yet.")).toBeInTheDocument());

      harness.seed("late.md", '---\ntitle: "Late"\n---\n\n[[Target]] 后来加的\n');
      fireEvent.click(screen.getByRole("button", { name: "Rescan the vault for links" }));

      expect(await screen.findByText("[[Target]] 后来加的")).toBeInTheDocument();
    });

    it("扫描失败就地报错,不占用面板那条错误条", async () => {
      // 反链是只读视图,扫不动它不影响读写笔记。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Target" }));
      harness.failLinkScan = true;
      const side = await openBacklinks();

      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent("scanning links failed");
      expect(side).toContainElement(alert);
    });

    it("嵌入也算反链", async () => {
      // `![[..]]` 是更强的引用,漏掉它会让"这篇被谁用了"的答案是错的。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.seed("src.md", '---\ntitle: "Source"\n---\n\n![[Target]]\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Target" }));
      await openBacklinks();

      expect(await screen.findByText("![[Target]]")).toBeInTheDocument();
      expect(backlinkNames()).toEqual(["Source, line 5"]);
      /* 图标也要区分开:`![[..]]` 会把整篇内容搬过去,改标题之类的动作影响面比一条
         链接大。lucide 把图标名写进 svg 的 class,据此断言身份。 */
      const icon = screen.getByRole("button", { name: "Source, line 5" }).querySelector("svg");
      expect(icon?.getAttribute("class")).toContain("lucide-image");
    });

    it("计数行报的是引用条数和来源篇数", async () => {
      /* 两个数不一样才看得出有没有对调 —— 而"3 处引用分布在 1 篇里"和"1 处引用
         分布在 3 篇里"对用户是完全不同的信息。 */
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      harness.seed("src.md", '---\ntitle: "Source"\n---\n\n[[Target]]\n又一处 [[Target]]\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Target" }));
      const side = await openBacklinks();

      await waitFor(() => expect(side).toHaveTextContent("2 references in 1 notes"));
      // 同一篇里的两处各算一条,标签上的计数与计数行一致。
      expect(screen.getByRole("button", { name: "Backlinks (2)" })).toBeInTheDocument();
    });

    it("扫描进行中不能再点刷新", async () => {
      // 重复点击会叠出并发扫描,而扫描是整个面板里最贵的一次 IO。
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Target" }));
      await openBacklinks();
      const refresh = () => screen.getByRole("button", { name: "Rescan the vault for links" });
      await waitFor(() => expect(refresh()).toBeEnabled());

      // 不 await:扫描还挂在飞行中的那一瞬间正是要断言的状态。
      fireEvent.click(refresh());
      expect(refresh()).toBeDisabled();

      await waitFor(() => expect(refresh()).toBeEnabled());
    });

    it("大纲与反链共用一列,切换时只有一个在场", async () => {
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\n# 一级标题\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Alpha" }));
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      expect(await screen.findByRole("complementary", { name: "Outline" })).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: /^Backlinks/ }));
      await screen.findByRole("complementary", { name: "Backlinks" });
      // 面板只有一半宽,两档共用一列;两个都在场意味着正文被挤没了。
      expect(screen.queryByRole("complementary", { name: "Outline" })).toBeNull();
    });
  });

  describe("标签", () => {
    /** 打开侧栏并切到标签档。 */
    async function openTags() {
      // 等笔记读进来 —— 侧栏的开关在笔记加载完之前还不在场。
      fireEvent.click(await screen.findByRole("button", { name: "Show outline" }));
      fireEvent.click(await screen.findByRole("button", { name: "Tags" }));
      return screen.findByRole("complementary", { name: "Tags" });
    }

    /** 标签清单里每一条的可及名。 */
    function tagNames(): string[] {
      return screen
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label") ?? "")
        .filter((name) => /^#\S+, \d+ uses/.test(name));
    }

    it("列出全库的标签,带处数和篇数", async () => {
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 今天\n#work 又一处\n');
      harness.seed("b.md", '---\ntitle: "B"\n---\n\n#home 家里\n');
      renderNotebook();
      const side = await openTags();

      // 处数降序:`#work` 三处不到,两处,排在一处的 `#home` 前面。
      await waitFor(() =>
        expect(tagNames()).toEqual(["#work, 2 uses in 1 notes", "#home, 1 uses in 1 notes"]),
      );
      // 标题行报的是全库总处数和标签个数 —— 两个数不一样才看得出有没有对调。
      expect(side).toHaveTextContent("3 uses across 2 tags");
    });

    it("大小写不同的同一个标签折成一条", async () => {
      /* 折不折是个选择,但必须和重命名一致:数得出来却改不动正是 Markio 的缺陷
         (索引用字符扫、重命名用另一条正则)。 */
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n#Work 大写\n');
      harness.seed("b.md", '---\ntitle: "B"\n---\n\n#work 小写\n');
      renderNotebook();
      await openTags();

      // 显示用第一次见到的写法(`a.md` 在前),但两处算同一条。
      await waitFor(() => expect(tagNames()).toEqual(["#Work, 2 uses in 2 notes"]));
    });

    it("代码块和行内代码里的 # 不算标签", async () => {
      // `#include` 和 shell 注释是最常见的假阳性,而假标签会污染整张标签清单。
      harness.seed(
        "a.md",
        '---\ntitle: "A"\n---\n\n```c\n#include <stdio.h>\n```\n\n`#inline` 也不算\n\n#real 这个算\n',
      );
      renderNotebook();
      await openTags();

      await waitFor(() => expect(tagNames()).toEqual(["#real, 1 uses in 1 notes"]));
    });

    it("点开一条标签就地展开它的引用,再点收起", async () => {
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n第一行\n#work 在这里\n');
      renderNotebook();
      await openTags();

      const entry = await screen.findByRole("button", { name: "#work, 1 uses in 1 notes" });
      expect(entry).toHaveAttribute("aria-expanded", "false");
      fireEvent.click(entry);

      expect(await screen.findByRole("button", { name: "A, line 6" })).toBeInTheDocument();
      expect(entry).toHaveAttribute("aria-expanded", "true");

      /* 侧栏只有一列宽,展开的引用会把标签清单顶下去 —— 不给一条收起的路等于
         要靠滚动找回来。 */
      fireEvent.click(entry);
      expect(screen.queryByRole("button", { name: "A, line 6" })).toBeNull();
    });

    it("点一条引用跳到那一篇的那一行", async () => {
      /* 与反链共用同一条跳转路:两边给的都是"某篇的某一行"。这里也顺带钉住行号的
         坐标系 —— 文件第 6 行是 `#work 在这里`,而正文(拆掉 frontmatter)里那一行的
         行首在 4。 */
      harness.seed("src.md", '---\ntitle: "Source"\n---\n\n第一行\n#work 在这里\n');
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByDisplayValue("Target");
      await openTags();

      fireEvent.click(await screen.findByRole("button", { name: "#work, 1 uses in 1 notes" }));
      fireEvent.click(await screen.findByRole("button", { name: "Source, line 6" }));

      expect(await screen.findByDisplayValue("Source")).toBeInTheDocument();
      await waitFor(() => expect(editorView().state.selection.main.head).toBe(4));
    });

    it("引用上报的是 frontmatter 里的标题,不是文件名", async () => {
      /* 与 Markio 的实质差异之一:文件名是 `cao-gao`、标题是 Weekly,显示文件名会让
         用户以为跳错了地方。 */
      harness.seed("cao-gao.md", '---\ntitle: "Weekly"\n---\n\n#work 周报\n');
      renderNotebook();
      await openTags();

      fireEvent.click(await screen.findByRole("button", { name: "#work, 1 uses in 1 notes" }));
      expect(await screen.findByRole("button", { name: "Weekly, line 5" })).toBeInTheDocument();
    });

    it("筛选框按归一化 key 匹配,大小写和 # 都无关", async () => {
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n#Work #work/deep #home\n');
      renderNotebook();
      await openTags();
      await waitFor(() => expect(tagNames()).toHaveLength(3));

      const filter = screen.getByRole("searchbox", { name: "Filter tags" });
      fireEvent.change(filter, { target: { value: "#WORK" } });
      await waitFor(() =>
        expect(tagNames()).toEqual(["#Work, 1 uses in 1 notes", "#work/deep, 1 uses in 1 notes"]),
      );

      // 子串匹配:层级标签的末段常常才是用户记得的那半。
      fireEvent.change(filter, { target: { value: "deep" } });
      await waitFor(() => expect(tagNames()).toEqual(["#work/deep, 1 uses in 1 notes"]));

      fireEvent.change(filter, { target: { value: "zzz" } });
      await waitFor(() => expect(screen.getByText("No matching tags.")).toBeInTheDocument());
    });

    it("筛选没匹配和全库没标签是两句不同的话", async () => {
      // "没有匹配"要能和"全库确实没有标签"分开 —— 否则用户会以为筛选框坏了。
      harness.seed("a.md", '---\ntitle: "A"\n---\n\nbody\n');
      renderNotebook();
      await openTags();

      expect(await screen.findByText("No inline #tags in this vault yet.")).toBeInTheDocument();
    });

    it("切走再切回来,筛选和展开都还在", async () => {
      // 用户切出去往往正是为了照着正文找该筛什么,回来清空等于白跑一趟。
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 在这里\n#home 别处\n');
      renderNotebook();
      await openTags();
      await waitFor(() => expect(tagNames()).toHaveLength(2));

      fireEvent.change(screen.getByRole("searchbox", { name: "Filter tags" }), {
        target: { value: "work" },
      });
      fireEvent.click(await screen.findByRole("button", { name: "#work, 1 uses in 1 notes" }));
      await screen.findByRole("button", { name: "A, line 5" });

      fireEvent.click(screen.getByRole("button", { name: "Outline" }));
      await screen.findByRole("complementary", { name: "Outline" });
      fireEvent.click(screen.getByRole("button", { name: "Tags" }));

      await screen.findByRole("complementary", { name: "Tags" });
      expect(screen.getByRole("searchbox", { name: "Filter tags" })).toHaveValue("work");
      expect(screen.getByRole("button", { name: "A, line 5" })).toBeInTheDocument();
    });

    it("没打开标签档时不扫全库", async () => {
      // 和反链一样读每个文件的全文,而两档互斥 —— 停在别的档上不该付这次 IO。
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work\n');
      renderNotebook();
      await screen.findByRole("button", { name: "A" });
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      fireEvent.click(await screen.findByRole("button", { name: /^Backlinks/ }));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(harness.tagScanCalls).toBe(0);
      // 反之也成立:开着标签档时不该顺手把链接也扫一遍。
      fireEvent.click(screen.getByRole("button", { name: "Tags" }));
      await waitFor(() => expect(harness.tagScanCalls).toBe(1));
      const linkScans = harness.callCount("notebook_vault_links");
      await act(async () => {
        await Promise.resolve();
      });
      expect(harness.callCount("notebook_vault_links")).toBe(linkScans);
    });

    it("侧栏整个收起时不扫,重新展开才再扫", async () => {
      /* 门控是两个条件的与:停在别的档上不扫(上一条),侧栏收起来也不扫。只验其中
         一个的话另一个可以整条去掉而测试全绿 —— 而侧栏默认就是收起的,漏掉这一半
         等于每次开着标签档的会话都在后台白扫。 */
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work\n');
      renderNotebook();
      await openTags();
      await waitFor(() => expect(harness.tagScanCalls).toBe(1));

      // 收起侧栏:标签档还是"当前那一档",但它不在场了。
      fireEvent.click(screen.getByRole("button", { name: "Hide outline" }));
      await waitFor(() => expect(screen.queryByRole("complementary", { name: "Tags" })).toBeNull());
      expect(harness.tagScanCalls).toBe(1);

      // 重新展开时回到同一档,这时才该再扫一次(收起期间别人可能改过 vault)。
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      await screen.findByRole("complementary", { name: "Tags" });
      await waitFor(() => expect(harness.tagScanCalls).toBe(2));
    });

    it("换笔记不重扫 —— 标签清单是全库的", async () => {
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\n#work\n');
      harness.seed("Beta.md", '---\ntitle: "Beta"\n---\n\nbody\n');
      renderNotebook();
      await openTags();
      await waitFor(() => expect(harness.tagScanCalls).toBe(1));

      fireEvent.click(screen.getByRole("button", { name: "Alpha" }));
      await screen.findByDisplayValue("Alpha");
      // 标签档和另外两档的分工就在这里:它讲全库,不讲当前这一篇。
      expect(harness.tagScanCalls).toBe(1);
      expect(tagNames()).toEqual(["#work, 1 uses in 1 notes"]);
    });

    it("刷新会重扫,能看到外部新加的标签", async () => {
      harness.seed("a.md", '---\ntitle: "A"\n---\n\nbody\n');
      renderNotebook();
      await openTags();
      await waitFor(() =>
        expect(screen.getByText("No inline #tags in this vault yet.")).toBeInTheDocument(),
      );

      harness.seed("late.md", '---\ntitle: "Late"\n---\n\n#later 后来加的\n');
      fireEvent.click(screen.getByRole("button", { name: "Rescan the vault for tags" }));

      expect(
        await screen.findByRole("button", { name: "#later, 1 uses in 1 notes" }),
      ).toBeInTheDocument();
    });

    it("扫描进行中不能再点刷新", async () => {
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work\n');
      renderNotebook();
      await openTags();
      const refresh = () => screen.getByRole("button", { name: "Rescan the vault for tags" });
      await waitFor(() => expect(refresh()).toBeEnabled());

      // 不 await:扫描还挂在飞行中的那一瞬间正是要断言的状态。
      fireEvent.click(refresh());
      expect(refresh()).toBeDisabled();

      await waitFor(() => expect(refresh()).toBeEnabled());
    });

    it("扫描失败就地报错,并留住上一次的结果", async () => {
      /* 清空成"什么都没有"比留着旧结果更糟 —— 那看起来像扫完了、确实没有。 */
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work\n');
      renderNotebook();
      const side = await openTags();
      await waitFor(() => expect(tagNames()).toEqual(["#work, 1 uses in 1 notes"]));

      harness.failTagScan = true;
      fireEvent.click(screen.getByRole("button", { name: "Rescan the vault for tags" }));

      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent("scanning tags failed");
      // 就地显示,不占用面板那条错误条:标签是只读视图,扫不动它不影响读写笔记。
      expect(side).toContainElement(alert);
      expect(tagNames()).toEqual(["#work, 1 uses in 1 notes"]);
    });

    it("三档共用一列,切到标签时另外两个都不在场", async () => {
      harness.seed("a.md", '---\ntitle: "A"\n---\n\n# 一级标题\n\n#work\n');
      renderNotebook();
      await openTags();

      expect(screen.queryByRole("complementary", { name: "Outline" })).toBeNull();
      expect(screen.queryByRole("complementary", { name: "Backlinks" })).toBeNull();
    });

    describe("跨文件重命名", () => {
      /** 打开某个标签那一行的重命名小窗。 */
      async function openRename(tag: string) {
        fireEvent.click(await screen.findByRole("button", { name: `Rename #${tag} everywhere` }));
        return screen.findByRole("dialog", { name: `Rename #${tag}` });
      }

      /** 小窗里输入新名字并执行。 */
      function submitRename(dialog: HTMLElement, next: string) {
        fireEvent.change(screen.getByRole("textbox", { name: "New tag name" }), {
          target: { value: next },
        });
        fireEvent.click(screen.getByRole("button", { name: "Rename" }));
        return dialog;
      }

      it("改完把全库的引用一起换掉,并报出改了几处几篇", async () => {
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 一处\n又一处 #work\n');
        harness.seed("b.md", '---\ntitle: "B"\n---\n\n#work 单独一处\n');
        renderNotebook();
        await openTags();
        const dialog = await openRename("work");

        submitRename(dialog, "job");

        await waitFor(() => expect(dialog).toHaveTextContent("Renamed 3 uses in 2 notes."));
        // 报告不是唯一的凭据 —— 文件真的改了才算改。
        expect(harness.read("/vault/a.md")).toContain("#job 一处");
        expect(harness.read("/vault/a.md")).toContain("又一处 #job");
        expect(harness.read("/vault/b.md")).toContain("#job 单独一处");
      });

      it("传给后端的是归一化 key,不是显示用的写法", async () => {
        /* 面板上显示的是第一次见到的写法(`#Work`),而匹配按小写 key 做。传显示名
           下去的话,后端拿到 `Work` 再归一化一次结果一样 —— 但一旦哪天两边的归一化
           规则不同步,差别就落在"改了几处"上,而那时已经写完盘了。 */
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#Work 大写\n');
        harness.seed("b.md", '---\ntitle: "B"\n---\n\n#work 小写\n');
        renderNotebook();
        await openTags();
        const dialog = await openRename("Work");

        submitRename(dialog, "job");

        await waitFor(() => expect(harness.tagRenameCalls).toHaveLength(1));
        expect(harness.tagRenameCalls[0]).toEqual({ old: "work", next: "job" });
      });

      it("改完重扫,旧名字不再出现在清单里", async () => {
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 一处\n');
        renderNotebook();
        await openTags();
        const before = harness.tagScanCalls;
        const dialog = await openRename("work");

        submitRename(dialog, "job");

        // 不重扫的话清单上还留着 `#work` 那一行,点它会展开一堆跳不到的引用。
        await waitFor(() => expect(tagNames()).toEqual(["#job, 1 uses in 1 notes"]));
        expect(harness.tagScanCalls).toBeGreaterThan(before);
      });

      it("执行完不关窗 —— 报告就是这次操作的结果", async () => {
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 一处\n');
        renderNotebook();
        await openTags();
        const dialog = await openRename("work");

        submitRename(dialog, "job");

        await waitFor(() => expect(dialog).toHaveTextContent("Renamed 1 uses in 1 notes."));
        // 窗还在,而且按钮从「重命名」变成「完成」。
        expect(screen.getByRole("dialog", { name: "Rename #work" })).toBe(dialog);
        expect(screen.getByRole("button", { name: "Done" })).toBeTruthy();
      });

      it("跳过的篇数和理由都报出来", async () => {
        /* "这篇里明明有 #work,怎么没改"是重命名之后最常见的疑问。只报路径不报理由
           的话没人答得上来 —— 答案(在代码块里)只有扫描器知道。 */
        harness.seed("code.md", '---\ntitle: "Code"\n---\n\n```sh\n#work 在代码里\n```\n');
        harness.seed("real.md", '---\ntitle: "Real"\n---\n\n#work 真的\n');
        renderNotebook();
        await openTags();
        const dialog = await openRename("work");

        submitRename(dialog, "job");

        await waitFor(() => expect(dialog).toHaveTextContent("Skipped 1 notes"));
        expect(dialog).toHaveTextContent("Not a tag here (code block, frontmatter, or heading)");
        // 代码块一个字节都没动。
        expect(harness.read("/vault/code.md")).toContain("#work 在代码里");
      });

      it("单篇失败进报告,不当成整次失败", async () => {
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 一处\n');
        harness.tagRenameFailures = [{ path: "/vault/locked.md", message: "permission denied" }];
        renderNotebook();
        await openTags();
        const dialog = await openRename("work");

        submitRename(dialog, "job");

        // 两段同时在场:成功的那些照样报出来,失败的单独一段。
        await waitFor(() => expect(dialog).toHaveTextContent("Failed in 1 notes"));
        expect(dialog).toHaveTextContent("Renamed 1 uses in 1 notes.");
        expect(dialog).toHaveTextContent("permission denied");
      });

      it("整次失败就地报错,文件一个都没动", async () => {
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 一处\n');
        harness.failTagRename = true;
        renderNotebook();
        await openTags();
        const dialog = await openRename("work");

        submitRename(dialog, "job");

        await waitFor(() => expect(dialog).toHaveTextContent("renaming the tag failed"));
        expect(harness.read("/vault/a.md")).toContain("#work 一处");
        // 失败之后还能再试 —— 按钮不该卡在「正在重命名…」。
        expect(screen.getByRole("button", { name: "Rename" })).toBeTruthy();
      });

      it("名字没改时不能提交", async () => {
        // 空操作也会给每篇留一条版本快照,把 30 条的保留窗口冲掉。
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 一处\n');
        renderNotebook();
        await openTags();
        await openRename("work");

        // 初值就是旧名字,开窗即不可提交。
        expect(screen.getByRole("button", { name: "Rename" })).toBeDisabled();
        fireEvent.change(screen.getByRole("textbox", { name: "New tag name" }), {
          target: { value: "  " },
        });
        expect(screen.getByRole("button", { name: "Rename" })).toBeDisabled();
        fireEvent.change(screen.getByRole("textbox", { name: "New tag name" }), {
          target: { value: "job" },
        });
        expect(screen.getByRole("button", { name: "Rename" })).toBeEnabled();
        expect(harness.tagRenameCalls).toHaveLength(0);
      });

      it("开另一个标签的窗时不带着上一次的报告", async () => {
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 一处\n#home 家里\n');
        renderNotebook();
        await openTags();
        const first = await openRename("work");
        submitRename(first, "job");
        await waitFor(() => expect(first).toHaveTextContent("Renamed 1 uses in 1 notes."));
        fireEvent.click(screen.getByRole("button", { name: "Done" }));

        const second = await openRename("home");

        // 留着上一次的报告会让它看起来像这一次的结果。
        expect(second).not.toHaveTextContent("Renamed 1 uses in 1 notes.");
        expect(second).toHaveTextContent("Renames 1 uses of #home across your notes.");
      });

      it("Esc 关窗", async () => {
        /* 只验"关掉"。原本这里还有一句"侧栏不受影响",但那句是空的:面板自己没有
           Esc 处理,宿主那个 window 监听要按修饰键才进,所以侧栏无论如何都还在 ——
           去掉小窗里的 stopPropagation 它照样绿。留一句永远为真的断言比没有更糟,
           它看着像有人在守着那件事。 */
        harness.seed("a.md", '---\ntitle: "A"\n---\n\n#work 一处\n');
        renderNotebook();
        await openTags();
        const dialog = await openRename("work");

        fireEvent.keyDown(dialog, { key: "Escape" });

        await waitFor(() =>
          expect(screen.queryByRole("dialog", { name: "Rename #work" })).toBeNull(),
        );
      });
    });
  });

  describe("frontmatter 字段浏览器", () => {
    /** 打开字段浏览器,等它把扫描结果拉回来。 */
    async function openFields() {
      fireEvent.click(await screen.findByRole("button", { name: "Frontmatter fields" }));
      return screen.findByRole("dialog", { name: "Frontmatter fields" });
    }

    /** 左列里的 key 行(带篇数的那些按钮)。 */
    function keyRows(): string[] {
      const sheet = screen.getByRole("dialog", { name: "Frontmatter fields" });
      return [...sheet.querySelectorAll("button[aria-pressed]")].map(
        (button) => button.textContent ?? "",
      );
    }

    /** 右列里的取值行。 */
    function valueRows(): string[] {
      return [
        ...screen.getByTestId("note-fields-values").querySelectorAll("button[aria-expanded]"),
      ].map((button) => button.textContent ?? "");
    }

    it("列出全库的 key,带篇数,按篇数降序", async () => {
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\n---\n\n正文\n');
      harness.seed("b.md", '---\ntitle: "B"\nstatus: todo\n---\n\n正文\n');
      renderNotebook();
      await openFields();

      // `title` 和 `status` 各两篇,同数按 key 字典序 —— 两个数不一样才看得出有没有
      // 把篇数和取值数搞混。
      await waitFor(() => expect(keyRows()).toEqual(["status2", "title2"]));
    });

    it("选一个 key 才显示取值,取值带命中篇数", async () => {
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\n---\n');
      harness.seed("b.md", '---\ntitle: "B"\nstatus: done\n---\n');
      harness.seed("c.md", '---\ntitle: "C"\nstatus: todo\n---\n');
      renderNotebook();
      const sheet = await openFields();

      // 没选之前右列是提示,不是空白 —— 空白看起来像扫完了确实没有。
      expect(sheet).toHaveTextContent("Pick a field to see its values");
      expect(valueRows()).toEqual([]);

      fireEvent.click(await screen.findByRole("button", { name: /^status/ }));

      // `done` 两篇在前,`todo` 一篇在后。
      await waitFor(() => expect(valueRows()).toEqual(["done2 notes", "todo1 notes"]));
      expect(sheet).toHaveTextContent("Values of status");
    });

    it("点开一个取值列出命中的笔记,再点收起", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\nstatus: done\n---\n');
      harness.seed("b.md", '---\ntitle: "Beta"\nstatus: done\n---\n');
      renderNotebook();
      await openFields();
      fireEvent.click(await screen.findByRole("button", { name: /^status/ }));

      const row = await screen.findByRole("button", { name: /^done/ });
      expect(row.getAttribute("aria-expanded")).toBe("false");
      fireEvent.click(row);

      // 显示的是 frontmatter 里的真标题,不是文件名 —— 改过标题的笔记显示文件名会
      // 让人以为跳错了地方。
      const values = screen.getByTestId("note-fields-values");
      await waitFor(() => expect(values).toHaveTextContent("Alpha"));
      expect(values).toHaveTextContent("Beta");
      expect(screen.getByRole("button", { name: /^done/ }).getAttribute("aria-expanded")).toBe(
        "true",
      );

      fireEvent.click(screen.getByRole("button", { name: /^done/ }));
      await waitFor(() => expect(values).not.toHaveTextContent("Beta"));
    });

    it("点一篇笔记跳过去并把 sheet 收掉", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\nstatus: done\n---\n\nalpha 正文\n');
      harness.seed("b.md", '---\ntitle: "Beta"\nstatus: done\n---\n\nbeta 正文\n');
      renderNotebook();
      /* 等编辑器把第一条读进来,并且断言它现在**不是**目标那条 —— 少了这一句的话
         "跳过去了"和"本来就在那条上"长得一模一样。等笔记列表那个按钮不行:未读入
         的行显示文件名 stem,不是 frontmatter 里的标题。 */
      const editor = await screen.findByRole("textbox", { name: "Quick note content" });
      // 最后 seed 的那条是当前笔记,所以要跳的是另一条。
      await waitFor(() => expect(editor).toHaveTextContent("beta 正文"));

      await openFields();
      fireEvent.click(await screen.findByRole("button", { name: /^status/ }));
      fireEvent.click(await screen.findByRole("button", { name: /^done/ }));

      const values = screen.getByTestId("note-fields-values");
      await waitFor(() => expect(values).toHaveTextContent("Alpha"));
      fireEvent.click(
        [...values.querySelectorAll("button")].find(
          (button) => button.textContent === "Alpha",
        ) as HTMLElement,
      );

      // sheet 铺满面板,留着的话点了笔记什么都看不见。
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Frontmatter fields" })).toBeNull(),
      );
      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note content" })).toHaveTextContent(
          "alpha 正文",
        ),
      );
    });

    it("换 key 时收起已展开的取值", async () => {
      // 两个 key 都有 `done` 这个取值。不收起的话换过去会显示成已展开,而那不是
      // 用户点开的。
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\nphase: done\n---\n');
      renderNotebook();
      await openFields();
      fireEvent.click(await screen.findByRole("button", { name: /^status/ }));
      fireEvent.click(await screen.findByRole("button", { name: /^done/ }));
      await waitFor(() =>
        expect(screen.getByRole("button", { name: /^done/ }).getAttribute("aria-expanded")).toBe(
          "true",
        ),
      );

      fireEvent.click(screen.getByRole("button", { name: /^phase/ }));

      await waitFor(() =>
        expect(screen.getByRole("button", { name: /^done/ }).getAttribute("aria-expanded")).toBe(
          "false",
        ),
      );
    });

    it("有 key 没值的笔记单独一行,点开能看见是哪几篇", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\nstatus:\n---\n');
      harness.seed("b.md", '---\ntitle: "Beta"\nstatus: done\n---\n');
      renderNotebook();
      await openFields();
      fireEvent.click(await screen.findByRole("button", { name: /^status/ }));

      // "没有值"是单独一行,不是一个空串取值。
      await waitFor(() => expect(valueRows()).toEqual(["done1 notes", "(no value)1 notes"]));
      fireEvent.click(screen.getByRole("button", { name: /no value/ }));
      await waitFor(() =>
        expect(screen.getByTestId("note-fields-values")).toHaveTextContent("Alpha"),
      );
    });

    it("筛选按 key 匹配,大小写无关", async () => {
      harness.seed("a.md", '---\ntitle: "A"\nStatus: done\nowner: 我\n---\n');
      renderNotebook();
      await openFields();
      await waitFor(() => expect(keyRows()).toHaveLength(3));

      fireEvent.change(screen.getByRole("textbox", { name: "Filter fields" }), {
        target: { value: "STAT" },
      });

      await waitFor(() => expect(keyRows()).toEqual(["Status1"]));
      fireEvent.change(screen.getByRole("textbox", { name: "Filter fields" }), {
        target: { value: "zzz" },
      });
      await waitFor(() =>
        expect(screen.getByRole("dialog", { name: "Frontmatter fields" })).toHaveTextContent(
          "No field matches that",
        ),
      );
    });

    it("扫描失败就地报错,不显示成空库", async () => {
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\n---\n');
      harness.failFieldScan = true;
      renderNotebook();
      const sheet = await openFields();

      await waitFor(() => expect(sheet).toHaveTextContent("scanning fields failed"));
      // "没有字段"和"扫不动"是两回事 —— 报成空库会让人以为库里真的没有 frontmatter。
      expect(sheet).not.toHaveTextContent("No frontmatter fields in this vault yet");
    });

    it("没有 frontmatter 的库显示空态", async () => {
      harness.seed("a.md", "# 只有标题\n\n正文\n");
      renderNotebook();
      const sheet = await openFields();

      await waitFor(() =>
        expect(sheet).toHaveTextContent("No frontmatter fields in this vault yet"),
      );
    });

    it("只在 sheet 开着时扫全库", async () => {
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\n---\n');
      renderNotebook();
      await screen.findByRole("button", { name: "A" });

      // 扫描要读每个文件的全文,是面板里最贵的一次 IO。没打开就不该扫。
      expect(harness.fieldScanCalls).toBe(0);
      await openFields();
      await waitFor(() => expect(harness.fieldScanCalls).toBe(1));
    });

    it("打开时把焦点挪进来", async () => {
      /* 不挪的话焦点还在编辑器上,那个 onKeyDown 收不到 Esc —— 事件在编辑器那棵子树
         里冒泡,根本不经过 sheet 这个 div。下面那条 Esc 用例测不出这件事:
         `fireEvent.keyDown(sheet)` 是直接派发到元素上的,不看焦点在哪。 */
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\n---\n');
      renderNotebook();
      await openFields();

      await waitFor(() =>
        expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close fields" })),
      );
    });

    it("Esc 关掉 sheet", async () => {
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\n---\n');
      renderNotebook();
      const sheet = await openFields();

      fireEvent.keyDown(sheet, { key: "Escape" });

      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Frontmatter fields" })).toBeNull(),
      );
    });

    it("铺在面板内部而不是整个窗口", async () => {
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\n---\n');
      renderNotebook();
      const sheet = await openFields();

      const panel = screen.getByRole("region", { name: "Quick Notes" });
      expect(sheet.parentElement).toBe(panel);
      expect(sheet.style.position).toBe("absolute");
    });

    it("打开回收站会把字段浏览器收掉", async () => {
      // 四个 overlay 同 z-index,字段浏览器在 JSX 里排在回收站后面 —— 不收掉的话
      // 用户点"回收站"看见的还是字段浏览器。
      harness.seed("a.md", '---\ntitle: "A"\nstatus: done\n---\n');
      renderNotebook();
      await openFields();

      fireEvent.click(screen.getByRole("button", { name: "Trash" }));

      await screen.findByRole("dialog", { name: "Trash" });
      expect(screen.queryByRole("dialog", { name: "Frontmatter fields" })).toBeNull();
    });
  });

  describe("引用图谱", () => {
    /** 打开图谱,等它把扫描结果拉回来。 */
    async function openGraph() {
      fireEvent.click(await screen.findByRole("button", { name: "Link graph" }));
      return screen.findByRole("dialog", { name: "Link graph" });
    }

    /** 图上的节点,按 `data-graph-node` 上的路径取。 */
    function graphNodes(): string[] {
      const sheet = screen.getByRole("dialog", { name: "Link graph" });
      return [...sheet.querySelectorAll("[data-graph-node]")].map(
        (node) => node.getAttribute("data-graph-node") ?? "",
      );
    }

    /** 图上的边,`from|to`。 */
    function graphEdges(): string[] {
      const sheet = screen.getByRole("dialog", { name: "Link graph" });
      return [...sheet.querySelectorAll("[data-graph-edge]")].map(
        (edge) => edge.getAttribute("data-graph-edge") ?? "",
      );
    }

    it("把全库链接画成节点和边", async () => {
      const alpha = harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      const beta = harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n正文\n');
      renderNotebook();
      await openGraph();

      // 两个点一条边,方向是 a → b。
      await waitFor(() => expect(graphNodes()).toHaveLength(2));
      expect(graphEdges()).toEqual([`${alpha}|${beta}`]);
    });

    it("只在开着时扫全库", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n正文\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Beta" });

      // 扫描要读每个文件的全文,是面板里最贵的一次 IO。没打开就不该扫。
      expect(harness.linkScanCalls).toBe(0);
      await openGraph();
      await waitFor(() => expect(harness.linkScanCalls).toBe(1));
    });

    it("反链档已经开着时打开图谱,不再多扫一次", async () => {
      /* 两个视图要的是同一份数据(全库链接)。各自一次 `useVaultScan` 的话,同一份
         库会被读两遍全文,而且"反链里有这条、图里没有"就成了可能 —— 那种偏差没人
         会往取数上想。所以它们共用一次扫描,`enabled` 是"反链档可见 **或** 图谱
         开着":在反链档开着的前提下打开图谱,`enabled` 一直是 true,effect 不重跑。 */
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n正文\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Show outline" }));
      fireEvent.click(await screen.findByRole("button", { name: /^Backlinks/ }));
      await screen.findByRole("complementary", { name: "Backlinks" });
      await waitFor(() => expect(harness.linkScanCalls).toBe(1));

      await openGraph();

      // 图上已经有数据了(= 用的就是反链那一次的结果),而扫描没有第二次。
      await waitFor(() => expect(graphNodes()).toHaveLength(2));
      expect(harness.linkScanCalls).toBe(1);
    });

    it("点一个节点跳过去并把 sheet 收掉", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\nalpha 正文 [[Beta]]\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\nbeta 正文 [[Alpha]]\n');
      renderNotebook();
      const editor = await screen.findByRole("textbox", { name: "Quick note content" });
      // 先钉住"现在不在目标那条上",否则"跳过去了"和"本来就在"长得一样。
      await waitFor(() => expect(editor).toHaveTextContent("beta 正文"));

      await openGraph();
      fireEvent.click(await screen.findByRole("button", { name: "Open Alpha" }));

      await waitFor(() => expect(screen.queryByRole("dialog", { name: "Link graph" })).toBeNull());
      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note content" })).toHaveTextContent(
          "alpha 正文",
        ),
      );
    });

    it("当前这篇是圆心", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      const beta = harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n看 [[Alpha]]\n');
      renderNotebook();
      const sheet = await openGraph();

      // 最后 seed 的那条是当前笔记。
      await waitFor(() => expect(sheet.querySelectorAll("[data-graph-focus]")).toHaveLength(1));
      expect(sheet.querySelector("[data-graph-focus]")?.getAttribute("data-graph-node")).toBe(beta);
    });

    it("报出孤立笔记和失效链接的条数", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]] 和 [[不存在的]]\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n正文\n');
      harness.seed("c.md", '---\ntitle: "Gamma"\n---\n\n谁都不连\n');
      renderNotebook();
      const sheet = await openGraph();

      await waitFor(() => expect(sheet).toHaveTextContent("1 unlinked"));
      // 死链是图谱最该暴露的东西之一,不藏起来。
      expect(sheet).toHaveTextContent("1 broken");
      // 孤立的那篇不画进图里(只报数),所以图上还是两个点。
      expect(graphNodes()).toHaveLength(2);
    });

    it("换跳数会重折图", async () => {
      // a → b → c,焦点在 a 上:1 跳只看得见 b,2 跳才看得见 c。
      harness.seed("c.md", '---\ntitle: "Gamma"\n---\n\n正文\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n看 [[Gamma]]\n');
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      renderNotebook();
      const sheet = await openGraph();
      await waitFor(() => expect(graphNodes()).toHaveLength(3));

      fireEvent.change(screen.getByRole("combobox", { name: "Depth" }), { target: { value: "1" } });

      await waitFor(() => expect(graphNodes()).toHaveLength(2));
      // 裁掉的那篇要报出来,否则用户以为库里就这么点东西。
      expect(sheet).toHaveTextContent("1 out of range");
    });

    it("扫描失败就地报错,不显示成空库", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      harness.failLinkScan = true;
      renderNotebook();
      const sheet = await openGraph();

      await waitFor(() => expect(sheet).toHaveTextContent("scanning links failed"));
      expect(sheet).not.toHaveTextContent("No links between notes yet");
    });

    it("没有链接的库显示空态", async () => {
      harness.seed("a.md", "# 只有标题\n\n正文\n");
      renderNotebook();
      const sheet = await openGraph();

      // 先写笔记后建链接是常态,这不是故障。
      await waitFor(() => expect(sheet).toHaveTextContent("No links between notes yet"));
    });

    it("打开时把焦点挪进来,Esc 关掉", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n正文\n');
      renderNotebook();
      const sheet = await openGraph();

      await waitFor(() =>
        expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close graph" })),
      );
      fireEvent.keyDown(sheet, { key: "Escape" });
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "Link graph" })).toBeNull());
    });

    it("铺在面板内部而不是整个窗口", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      renderNotebook();
      const sheet = await openGraph();

      const panel = screen.getByRole("region", { name: "Quick Notes" });
      expect(sheet.parentElement).toBe(panel);
      expect(sheet.style.position).toBe("absolute");
    });

    it("打开图谱会把字段浏览器收掉,反过来也一样", async () => {
      // 五个 overlay 同 z-index,图谱在 JSX 里排最后 —— 不互斥的话底下那个还在接
      // 键盘事件,而用户只看得见上面那个。
      harness.seed("a.md", '---\ntitle: "Alpha"\nstatus: done\n---\n\n看 [[Beta]]\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Frontmatter fields" }));
      await screen.findByRole("dialog", { name: "Frontmatter fields" });

      fireEvent.click(screen.getByRole("button", { name: "Link graph" }));

      await screen.findByRole("dialog", { name: "Link graph" });
      expect(screen.queryByRole("dialog", { name: "Frontmatter fields" })).toBeNull();

      fireEvent.click(screen.getByRole("button", { name: "Frontmatter fields" }));
      await screen.findByRole("dialog", { name: "Frontmatter fields" });
      expect(screen.queryByRole("dialog", { name: "Link graph" })).toBeNull();
    });

    it("打开回收站会把图谱收掉", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      renderNotebook();
      await openGraph();

      fireEvent.click(screen.getByRole("button", { name: "Trash" }));

      await screen.findByRole("dialog", { name: "Trash" });
      expect(screen.queryByRole("dialog", { name: "Link graph" })).toBeNull();
    });

    it("打开属性面板会把图谱收掉", async () => {
      /* 理由和另外几个一样:图谱在 JSX 里排最后。这一条走右键菜单,是唯一一条不从
         头部按钮进的路径 —— 少了它,`openProperties` 里那一句删掉也没人发现。 */
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      renderNotebook();
      await openGraph();

      const row = screen.getByRole("button", { name: "Alpha" }).closest("[data-notebook-note-row]");
      if (!row) throw new Error("no row for Alpha");
      fireEvent.contextMenu(row);
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));

      await screen.findByRole("dialog", { name: "Note properties" });
      expect(screen.queryByRole("dialog", { name: "Link graph" })).toBeNull();
    });

    it("打开版本历史会把图谱收掉", async () => {
      // 图谱在 JSX 里排在历史后面 —— 不收掉的话用户点"历史"看见的还是图谱。
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      renderNotebook();
      await openGraph();

      fireEvent.click(screen.getByRole("button", { name: "Version history" }));

      await screen.findByRole("dialog", { name: /Version history/ });
      expect(screen.queryByRole("dialog", { name: "Link graph" })).toBeNull();
    });

    it("「整个库」会把连不到当前这篇的笔记也画进来", async () => {
      /* 跳数模式下 `depth` 为 null 的节点是"连不到焦点的",要被裁掉;「整个库」必须
         走另一条路(不传 maxDepth),否则它和最大跳数没区别 —— 而那一对孤岛正是
         用户切到「整个库」想看的东西。 */
      harness.seed("x.md", '---\ntitle: "Xi"\n---\n\n看 [[Upsilon]]\n');
      harness.seed("y.md", '---\ntitle: "Upsilon"\n---\n\n正文\n');
      // 当前笔记(最后 seed 的这条)和上面那对互不相连。
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n看 [[Beta]]\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n正文\n');
      renderNotebook();
      const sheet = await openGraph();

      // 3 跳以内只看得见 a↔b 这一对,x/y 那一对报进"范围外"。
      await waitFor(() => expect(graphNodes()).toHaveLength(2));
      expect(sheet).toHaveTextContent("2 out of range");

      fireEvent.change(screen.getByRole("combobox", { name: "Depth" }), {
        target: { value: "99" },
      });

      await waitFor(() => expect(graphNodes()).toHaveLength(4));
      expect(sheet).not.toHaveTextContent("out of range");
    });
  });

  describe("任务收集箱", () => {
    /** 打开收集箱,等它把扫描结果拉回来。 */
    async function openInbox() {
      fireEvent.click(await screen.findByRole("button", { name: "Task inbox" }));
      return screen.findByRole("dialog", { name: "Task inbox" });
    }

    /** 清单里的任务行(可及名带来源和行号的那些按钮)。 */
    function taskRows(): string[] {
      return [
        ...screen.getByTestId("note-task-inbox-list").querySelectorAll("button[aria-label]"),
      ].map((button) => button.getAttribute("aria-label") ?? "");
    }

    /** 分组标题行的文本(不含任务行)。 */
    function groupLabels(): string[] {
      const list = screen.getByTestId("note-task-inbox-list");
      return [...list.children].map((group) => group.firstElementChild?.textContent ?? "");
    }

    it("列出全库的任务,带来源标题和行号", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] file a task\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n- [ ] file b task\n');
      renderNotebook();
      await openInbox();

      /* 行号按整篇 .md 数(frontmatter 那 3 行也算),和标签 / 反链同一个坐标系。
         组内按 compareTasks 排:两条都是未完成、无优先级、无截止,于是落到文本序。 */
      await waitFor(() =>
        expect(taskRows()).toEqual([
          "file a task — in Alpha line 5",
          "file b task — in Beta line 5",
        ]),
      );
    });

    it("没打开时一次都不扫", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Task inbox" });
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });

      // 扫描要读每个文件的全文,是整个面板里最贵的一次 IO。
      expect(harness.taskScanCalls).toBe(0);

      await openInbox();
      await waitFor(() => expect(harness.taskScanCalls).toBe(1));
    });

    it("重新打开时留着上一次的结果,不闪一下加载中", async () => {
      /* 关掉不清 data(`useVaultScan` 的第二条规则):重开时旧结果先在,后台再重扫。
         清空的话每次切档都要先看一眼"正在扫描",而绝大多数情况下结果没变。 */
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      fireEvent.click(screen.getByRole("button", { name: "Close inbox" }));
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull());

      const sheet = await openInbox();
      expect(taskRows()).toHaveLength(1);
      expect(sheet).not.toHaveTextContent("Scanning tasks");
      // 重开仍然会在后台重扫一遍 —— 关着的这段时间里别人可能改过 vault。
      await waitFor(() => expect(harness.taskScanCalls).toBe(2));
    });

    it("默认藏掉已完成的,开开关就都出来", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] 没做\n- [x] 做完了\n');
      renderNotebook();
      await openInbox();

      await waitFor(() => expect(taskRows()).toEqual(["没做 — in Alpha line 5"]));
      // 计数说的是待办条数,不含已完成的。
      expect(screen.getByRole("dialog", { name: "Task inbox" })).toHaveTextContent("1 open");

      fireEvent.click(screen.getByRole("checkbox", { name: "Include done" }));

      await waitFor(() => expect(taskRows()).toHaveLength(2));
      // 未完成仍排在前面。
      expect(taskRows()[0]).toBe("没做 — in Alpha line 5");
    });

    it("围栏和 frontmatter 里的 `- [ ]` 不算任务", async () => {
      harness.seed(
        "a.md",
        '---\ntitle: "Alpha"\nchecklist:\n  - [ ] YAML 列表项\n---\n\n- [ ] 真的\n\n```md\n- [ ] 教程里的\n```\n',
      );
      renderNotebook();
      await openInbox();

      await waitFor(() => expect(taskRows()).toEqual(["真的 — in Alpha line 7"]));
    });

    it("解析 #标签 / @截止 / !优先级 并显示成徽标", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] 交稿 #写作 @2026-09-01 !high\n');
      renderNotebook();
      const sheet = await openInbox();

      // 可及名里是摘掉标记之后的文本 —— 标记本身在徽标上。
      await waitFor(() => expect(taskRows()).toEqual(["交稿 — in Alpha line 5"]));
      expect(sheet).toHaveTextContent("2026-09-01");
      expect(sheet).toHaveTextContent("#写作");
    });

    it("按时间分组,过期的排在最前", async () => {
      /* 日期钉死而不是相对今天算:2000 年那条永远过期,2099 年那条永远在「以后」,
         用例不会随时间失效,也不受运行机器时区影响。 */
      harness.seed(
        "a.md",
        '---\ntitle: "Alpha"\n---\n\n- [ ] later one @2099-01-01\n- [ ] overdue one @2000-01-01\n- [ ] no date one\n',
      );
      renderNotebook();
      await openInbox();

      /* 组序是固定的桶序,不是"哪个先出现"。过期在最前:它是唯一一类需要用户马上
         做决定的。空桶不出现(这里没有 today / tomorrow / this week)。 */
      await waitFor(() => expect(groupLabels()).toEqual(["Overdue1", "Later1", "No date1"]));
    });

    it("能换成按优先级和按笔记分组", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] urgent !high\n- [ ] plain one\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n- [ ] elsewhere\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(3));

      fireEvent.click(screen.getByRole("button", { name: "Priority" }));
      await waitFor(() => expect(groupLabels()).toEqual(["High1", "No priority2"]));

      fireEvent.click(screen.getByRole("button", { name: "Note" }));
      // 组名是笔记标题,组间按标题字典序。
      await waitFor(() => expect(groupLabels()).toEqual(["Alpha2", "Beta1"]));
    });

    it("按笔记分组时行里不再重复来源标题", async () => {
      // 组标题已经写着笔记名,行里再写一遍是噪声 —— 而这一列的宽度是任务文本要用的。
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      /* 任务行本身的可见文本(不含组标题)。从清单里取而不是从整个 sheet 里取:
         头部那两个图标按钮也带 aria-label。 */
      const rowText = () =>
        screen
          .getByTestId("note-task-inbox-list")
          .querySelector("button[aria-label]")
          ?.textContent?.trim() ?? "";

      // 按时间分组时行里带来源:组标题讲的是时间,不讲在哪。
      expect(rowText()).toBe("a taskAlpha");

      fireEvent.click(screen.getByRole("button", { name: "Note" }));
      await waitFor(() => expect(groupLabels()).toEqual(["Alpha1"]));
      expect(rowText()).toBe("a task");
      // 来源没有消失,只是挪到了组标题上 —— 可及名里仍然带着它。
      expect(taskRows()).toEqual(["a task — in Alpha line 5"]);
    });

    it("筛选匹配任务文本、标签和笔记标题", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] 写周报 #汇报\n');
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n- [ ] 交报销\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(2));

      const filter = screen.getByRole("textbox", { name: "Filter tasks" });
      fireEvent.change(filter, { target: { value: "汇报" } });
      await waitFor(() => expect(taskRows()).toEqual(["写周报 — in Alpha line 5"]));

      fireEvent.change(filter, { target: { value: "Beta" } });
      await waitFor(() => expect(taskRows()).toEqual(["交报销 — in Beta line 5"]));

      // 筛没了显示"没有匹配",不是"库里还没有任务" —— 后者会让人以为扫描出错。
      fireEvent.change(filter, { target: { value: "根本没有" } });
      await waitFor(() =>
        expect(screen.getByRole("dialog", { name: "Task inbox" })).toHaveTextContent(
          "No matching tasks.",
        ),
      );
    });

    it("点一条任务:收掉 sheet,换到那篇,光标落到那一行", async () => {
      /* b 先种、a 后种:列表按 mtime 倒序,Alpha 成为挂载时的当前笔记,于是 Beta 的
         正文从头到尾没被读入过 —— 这才是最常见的情形(跨笔记跳)。 */
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n第一段\n\n- [ ] jump here\n');
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n正文\n');
      renderNotebook();
      await screen.findByDisplayValue("Alpha");
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      fireEvent.click(screen.getByRole("button", { name: "jump here — in Beta line 7" }));

      // sheet 铺满面板,不收掉的话光标落在编辑器里而用户还盯着收集箱。
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull());
      expect(await screen.findByDisplayValue("Beta")).toBeInTheDocument();

      /* 光标落在那一行的行首。偏移按**编辑器里的正文**算,不是按文件:文件第 7 行是
         `- [ ] jump here`,而正文(拆掉 frontmatter 之后)是 `第一段\n\n- [ ] jump here\n`,
         那一行的行首在 5(`第一段` 3 字 + 两个换行)。两个坐标系差几行取决于
         frontmatter 有多长 —— 直接把文件行号喂给编辑器会落在空行上。 */
      await waitFor(() => expect(editorView().state.selection.main.head).toBe(5));
    });

    it("扫描失败就地显示错误,不占面板那条错误条", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      harness.failTaskScan = true;
      renderNotebook();
      const sheet = await openInbox();

      await waitFor(() => expect(sheet).toHaveTextContent("scanning tasks failed"));
      // 只读视图,扫描失败不影响读写笔记,所以不占面板那条错误条。
      expect(sheet).not.toHaveTextContent("No - [ ] tasks in this vault yet.");
    });

    it("刷新会重扫,并带回新增的任务", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] first one\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      // 外部编辑改了别人的笔记 —— 全库扫描的结果只能靠重扫更新。
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n- [ ] second one\n');
      fireEvent.click(screen.getByRole("button", { name: "Rescan" }));

      await waitFor(() => expect(taskRows()).toHaveLength(2));
    });

    it("库里没有任务时说的是「还没有」,不是「没有匹配」", async () => {
      // 两句话指向不同的下一步:一句是"去写任务",一句是"把筛选词删掉"。
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n只是正文\n');
      renderNotebook();
      const sheet = await openInbox();

      await waitFor(() => expect(sheet).toHaveTextContent("No - [ ] tasks in this vault yet."));
    });

    it("Esc 关掉收集箱", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();
      const sheet = await openInbox();

      fireEvent.keyDown(sheet, { key: "Escape" });

      await waitFor(() => expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull());
    });

    it("铺在面板内部而不是整个窗口", async () => {
      // 随手记面板可以只占项目视图的一半,盖住整个窗口会遮掉用户正在参照的另一半。
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();
      const sheet = await openInbox();

      expect(sheet.parentElement).toBe(screen.getByRole("region", { name: "Quick Notes" }));
      expect(sheet.style.position).toBe("absolute");
    });

    it("和字段浏览器、图谱互斥", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\nstatus: done\n---\n\n- [ ] a task\n看 [[Beta]]\n');
      renderNotebook();
      await openInbox();

      fireEvent.click(screen.getByRole("button", { name: "Frontmatter fields" }));
      await screen.findByRole("dialog", { name: "Frontmatter fields" });
      expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull();

      await openInbox();
      expect(screen.queryByRole("dialog", { name: "Frontmatter fields" })).toBeNull();

      fireEvent.click(screen.getByRole("button", { name: "Link graph" }));
      await screen.findByRole("dialog", { name: "Link graph" });
      expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull();

      await openInbox();
      expect(screen.queryByRole("dialog", { name: "Link graph" })).toBeNull();
    });

    it("打开回收站 / 历史 / 属性会把收集箱收掉", async () => {
      /* 收集箱在 JSX 里排最后,z-index 上它盖在这三个之上 —— 不主动收掉的话用户点了
         「回收站」只会看见收集箱没反应。互斥靠 openTrash 那几个 setter,不靠 JSX 顺序。 */
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();

      await openInbox();
      fireEvent.click(screen.getByRole("button", { name: "Trash" }));
      await screen.findByRole("dialog", { name: "Trash" });
      expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull();

      await openInbox();
      fireEvent.click(screen.getByRole("button", { name: "Version history" }));
      await screen.findByRole("dialog", { name: /Version history/ });
      expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull();

      await openInbox();
      const row = screen.getByRole("button", { name: "Alpha" }).closest("[data-notebook-note-row]");
      if (!row) throw new Error("no row for Alpha");
      fireEvent.contextMenu(row);
      fireEvent.click(screen.getByRole("menuitem", { name: "Properties" }));
      await screen.findByRole("dialog", { name: "Note properties" });
      expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull();
    });

    it("右键复制任务文本,复制的是带标记的原文", async () => {
      const user = userEvent.setup();
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] 交稿 #写作 @2026-09-01\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      fireEvent.contextMenu(screen.getByRole("button", { name: /^交稿/ }));
      await user.click(screen.getByRole("menuitem", { name: "Copy task text" }));

      /* 复制原文而不是显示文本:`#标签` 和 `@截止` 通常正是用户想带走的那部分。
         而显示文本("交稿")是这两者都摘掉之后的结果。 */
      await waitFor(async () =>
        expect(await navigator.clipboard.readText()).toBe("交稿 #写作 @2026-09-01"),
      );
    });

    it("右键复制源路径,带行号", async () => {
      const user = userEvent.setup();
      const path = harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] 写周报\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      fireEvent.contextMenu(screen.getByRole("button", { name: /^写周报/ }));
      await user.click(screen.getByRole("menuitem", { name: "Copy source path" }));

      await waitFor(async () => expect(await navigator.clipboard.readText()).toBe(`${path}:5`));
    });

    it("右键「打开源文件」和点行一样,并收掉菜单和 sheet", async () => {
      harness.seed("b.md", '---\ntitle: "Beta"\n---\n\n- [ ] jump here\n');
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n正文\n');
      renderNotebook();
      await screen.findByDisplayValue("Alpha");
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      fireEvent.contextMenu(screen.getByRole("button", { name: /^jump here/ }));
      fireEvent.click(screen.getByRole("menuitem", { name: "Open source file" }));

      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
      expect(screen.queryByRole("dialog", { name: "Task inbox" })).toBeNull();
      expect(await screen.findByDisplayValue("Beta")).toBeInTheDocument();
    });

    it("右键「在系统文件夹中打开」传的 allowlist 根是 vault", async () => {
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      fireEvent.contextMenu(screen.getByRole("button", { name: /^a task/ }));
      fireEvent.click(screen.getByRole("menuitem", { name: "Open in System Folder" }));

      // 后端拿这个根做 validate_path_within。传成任务自己的路径等于没校验。
      await waitFor(() => expect(harness.revealCalls).toHaveLength(1));
      expect(harness.revealCalls[0]).toEqual({ path: "/vault/a.md", projectPath: "/vault" });
    });

    it("关掉 sheet 时右键菜单跟着消失", async () => {
      /* 菜单是 fixed 定位的,不跟着收的话它会孤零零留在屏幕上 —— 而它作用的那条
         任务已经看不见了。 */
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      fireEvent.contextMenu(screen.getByRole("button", { name: /^a task/ }));
      await screen.findByRole("menu", { name: "Task actions" });

      fireEvent.click(screen.getByRole("button", { name: "Close inbox" }));

      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    });

    it("点面板空白处关掉右键菜单", async () => {
      // 复用面板那个 outside-click 监听 —— 靠 data-notebook-context-menu 属性认出自己。
      harness.seed("a.md", '---\ntitle: "Alpha"\n---\n\n- [ ] a task\n');
      renderNotebook();
      await openInbox();
      await waitFor(() => expect(taskRows()).toHaveLength(1));

      fireEvent.contextMenu(screen.getByRole("button", { name: /^a task/ }));
      await screen.findByRole("menu", { name: "Task actions" });

      fireEvent.mouseDown(document.body);

      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
      // sheet 本身不受影响:关菜单不等于关收集箱。
      expect(screen.getByRole("dialog", { name: "Task inbox" })).toBeInTheDocument();
    });
  });

  describe("未链接的提及", () => {
    /** 打开侧栏、切到反链档(提及在它下面),等提及那一块出现。 */
    async function openMentions() {
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      fireEvent.click(await screen.findByRole("button", { name: /^Backlinks/ }));
      return screen.findByRole("region", { name: "Unlinked mentions" });
    }

    /** 提及列表里那些行的可及名。 */
    function mentionRows(): string[] {
      const list = screen.queryByTestId("note-mentions-list");
      if (!list) return [];
      return [...list.querySelectorAll("button[aria-label]")]
        .map((button) => button.getAttribute("aria-label") ?? "")
        .filter((name) => !name.startsWith("Link "));
    }

    /** 「包成链接」那些按钮的可及名。 */
    function linkButtons(): string[] {
      const list = screen.queryByTestId("note-mentions-list");
      if (!list) return [];
      return [...list.querySelectorAll("button[aria-label]")]
        .map((button) => button.getAttribute("aria-label") ?? "")
        .filter((name) => name.startsWith("Link "));
    }

    it("列出提到了这一篇却没链接的地方,行号按整篇源码数", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 Plan 一节\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await screen.findByDisplayValue("Plan");

      await openMentions();
      // frontmatter 那三行也算进行号,和反链同一个坐标系。
      await waitFor(() => expect(mentionRows()).toEqual(["Notes, line 5"]));
      expect(screen.getByText("1 unlinked in 1 notes")).toBeInTheDocument();
    });

    it("已经写成链接的那一处不算提及", async () => {
      // 这一条和下一条一起钉住"按区间判、不按整篇判"。
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 [[Plan]] 一节\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();

      await waitFor(() => expect(harness.callCount("notebook_vault_mentions")).toBe(1));
      expect(screen.getByText("No unlinked mentions of this note.")).toBeInTheDocument();
    });

    it("同一篇里链了一处、另有一处没链时,没链的那一处仍然列出来", async () => {
      /* 这正是 Markio 那份的缺陷:它按整篇 grep `[[stem` 排除,于是这一行一条都
         报不出来 —— 用户以为已经链全了。 */
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n先看 [[Plan]],再看 Plan 附录\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();

      await waitFor(() => expect(mentionRows()).toEqual(["Notes, line 5"]));
    });

    it("frontmatter、围栏、行内代码、标题里的字样都不算", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed(
        "Notes.md",
        [
          "---",
          'title: "Notes"',
          "summary: Plan", // frontmatter
          "---",
          "",
          "## Plan", // ATX 标题
          "```",
          "let Plan = 1;", // 围栏
          "```",
          "`Plan` 是变量", // 行内代码
          "[Plan](./x.md)", // markdown 链接
          "https://x.com/Plan", // 裸 URL
          "真正提到 Plan 了", // 只有这一处
          "",
        ].join("\n"),
      );
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();

      await waitFor(() => expect(mentionRows()).toEqual(["Notes, line 13"]));
    });

    it("按文件名 stem 写的字样也算(不只是 frontmatter 标题)", async () => {
      /* 两个名字都要扫:`resolveLink` 两个都认。只给标题的话按文件名写的那些字样
         一处都扫不出来;只给 stem 就是 Markio 的行为。 */
      harness.seed("cao-gao.md", '---\ntitle: "Weekly"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 cao-gao 也见 Weekly\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "cao-gao" }));
      await screen.findByDisplayValue("Weekly");
      await openMentions();

      await waitFor(() => expect(mentionRows()).toHaveLength(2));
      expect(harness.mentionScanNames.at(-1)).toEqual(["Weekly", "cao-gao"]);
    });

    it("自己那一篇不算(正文里写自己的标题很常见)", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n# Plan\n\n这篇讲 Plan 本身\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();

      await waitFor(() => expect(harness.callCount("notebook_vault_mentions")).toBe(1));
      expect(screen.getByText("No unlinked mentions of this note.")).toBeInTheDocument();
    });

    it("中日韩邻字的那一处标成待确认,不进「全部链接」", async () => {
      /* 这是与 Markio 的实质差异:它会把「原计划表」直接改成「原[[计划]]表」。
         「计划」两侧干净的那一处是 confident,贴着汉字的那一处是 ambiguous ——
         按钮上的数只数前者。 */
      harness.seed("jihua.md", '---\ntitle: "计划"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n原计划表在这\n\n见 计划 一节\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "jihua" }));
      await screen.findByDisplayValue("计划");
      await openMentions();

      await waitFor(() => expect(mentionRows()).toHaveLength(2));
      // 待确认那一条在可及名里就说明白了 —— 颜色和图标对屏读用户不存在。
      expect(mentionRows()).toEqual(["Notes, line 5 (needs a look)", "Notes, line 7"]);
      expect(screen.getByRole("button", { name: "Link 1 clear mentions" })).toBeInTheDocument();
    });

    it("「全部链接」只动明确的那些,报的是处数", async () => {
      harness.seed("jihua.md", '---\ntitle: "计划"\n---\n\nbody\n');
      harness.seed("A.md", '---\ntitle: "A"\n---\n\n见 计划 一节\n\n又见 计划 一次\n');
      harness.seed("B.md", '---\ntitle: "B"\n---\n\n原计划表\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "jihua" }));
      await openMentions();
      await waitFor(() => expect(mentionRows()).toHaveLength(3));

      fireEvent.click(screen.getByRole("button", { name: "Link 2 clear mentions" }));

      /* 报的是**处数**,不是文件数。Markio 那份把文件数说成处数,于是"已链接 12 处"
         实际改了 12 个文件里各一处、剩下几十处还在。 */
      expect(await screen.findByText(/Linked 2 mentions in 1 notes\./)).toBeInTheDocument();
      // A 里两处都包上了 —— 一个文件里的每一处都要动,不是只动第一处。
      expect(harness.read("/vault/A.md")).toContain("见 [[计划]] 一节");
      expect(harness.read("/vault/A.md")).toContain("又见 [[计划]] 一次");
      // B 那一处是待确认的,批量不碰。
      expect(harness.read("/vault/B.md")).toContain("原计划表");
      expect(harness.read("/vault/B.md")).not.toContain("[[");
    });

    it("链接之后重扫,那几处不再出现在列表里", async () => {
      // 闭环:改完就不该再数出来,否则点第二次只会报 alreadyLinked。
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 Plan 一节\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();
      await waitFor(() => expect(mentionRows()).toHaveLength(1));

      fireEvent.click(screen.getByRole("button", { name: "Link 1 clear mentions" }));

      await waitFor(() => expect(mentionRows()).toHaveLength(0));
      expect(screen.getByText("No unlinked mentions of this note.")).toBeInTheDocument();
      // 而反链跟着重扫 —— 刚写进去的是一条真链接。
      await waitFor(() =>
        expect(screen.getByRole("button", { name: "Backlinks (1)" })).toBeInTheDocument(),
      );
    });

    it("逐条链接时只提交那一处", async () => {
      // ambiguous 的不能批量,但逐条点得动 —— 分级是给批量用的,不是禁令。
      harness.seed("jihua.md", '---\ntitle: "计划"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n原计划表\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "jihua" }));
      await openMentions();
      await waitFor(() => expect(linkButtons()).toEqual(['Link "计划" on line 5']));
      // 全是待确认时不摆「全部链接」—— 点了什么都不会变的按钮更糟。
      expect(screen.queryByRole("button", { name: /^Link \d+ clear mentions$/ })).toBeNull();

      fireEvent.click(screen.getByRole("button", { name: 'Link "计划" on line 5' }));

      await waitFor(() => expect(harness.read("/vault/Notes.md")).toContain("原[[计划]]表"));
      expect(harness.mentionLinkCalls).toHaveLength(1);
      expect(harness.mentionLinkCalls[0]).toHaveLength(1);
    });

    it("大小写不同的那一处包完保留正文的写法", async () => {
      /* 链接解析本身大小写不敏感,所以 `PLAN` 包成 `[[PLAN]]` 照样指向《Plan》。
         改用户的用词是没必要的越界 —— 而这条同时钉住"校验用的是原文不是候选名":
         传候选名的话后端会把它报成 vanished,这里就什么都不会改。 */
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 PLAN 一节\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();
      await waitFor(() => expect(linkButtons()).toEqual(['Link "PLAN" on line 5']));

      fireEvent.click(screen.getByRole("button", { name: 'Link "PLAN" on line 5' }));

      await waitFor(() => expect(harness.read("/vault/Notes.md")).toContain("见 [[PLAN]] 一节"));
      expect(harness.mentionLinkCalls[0]?.[0]?.text).toBe("PLAN");
    });

    it("换到另一篇笔记时清空,不显示上一篇的提及", async () => {
      /* 和反链不同:提及的扫描参数里有当前笔记的名字,留着旧结果会在新笔记的标题
         下面显示上一篇的提及,而那些条目点下去会改错地方的正文。 */
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Other.md", '---\ntitle: "Other"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 Plan 一节\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();
      await waitFor(() => expect(mentionRows()).toHaveLength(1));

      fireEvent.click(screen.getByRole("button", { name: "Other" }));

      await waitFor(() => expect(mentionRows()).toHaveLength(0));
      // 而且真的重扫了(名字换了,结果只对当前这一篇成立)。
      await waitFor(() => expect(harness.callCount("notebook_vault_mentions")).toBe(2));
      expect(harness.mentionScanNames.at(-1)).toEqual(["Other"]);
    });

    it("没打开反链档时不扫", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 Plan 一节\n');
      renderNotebook();
      await screen.findByRole("button", { name: "Plan" });
      fireEvent.click(screen.getByRole("button", { name: "Show outline" }));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(harness.callCount("notebook_vault_mentions")).toBe(0);
    });

    it("扫描失败时就地报错,不清掉面板", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.failMentionScan = true;
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();

      expect(await screen.findByRole("alert")).toHaveTextContent(/scanning mentions failed/);
      // 笔记本身照常可读写 —— 提及是只读视图,失败不该影响主编辑区。
      expect(screen.getByDisplayValue("Plan")).toBeInTheDocument();
    });

    it("整次链接失败时就地报错", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 Plan 一节\n');
      harness.failMentionLink = true;
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();
      await waitFor(() => expect(mentionRows()).toHaveLength(1));

      fireEvent.click(screen.getByRole("button", { name: "Link 1 clear mentions" }));

      expect(await screen.findByRole("alert")).toHaveTextContent(/linking mentions failed/);
      // 没写盘。
      expect(harness.read("/vault/Notes.md")).toContain("见 Plan 一节");
    });

    it("报告里带上跳过和失败的那几条", async () => {
      /* 单篇失败在真后端是权限 / 冲突这类外部条件。报告里那一段(哪些没成)恰恰是
         最该有人看的一段 —— 这次操作动的是用户看不见的那些文件。 */
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 Plan 一节\n');
      harness.mentionLinkFailures = [{ path: "/vault/Other.md", message: "conflict" }];
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();
      await waitFor(() => expect(mentionRows()).toHaveLength(1));

      fireEvent.click(screen.getByRole("button", { name: "Link 1 clear mentions" }));

      /* 限定在提及那一档里找:状态栏的保存指示器也是 `role="status"`,全局找会拿到
         那个"Saved"。 */
      const panel = screen.getByRole("region", { name: "Unlinked mentions" });
      const status = await waitFor(() => {
        const found = panel.querySelector('[role="status"]');
        if (!found) throw new Error("report not shown yet");
        return found;
      });
      expect(status).toHaveTextContent(/Linked 1 mentions in 1 notes\./);
      expect(status).toHaveTextContent(/Failed in 1 notes/);
    });

    it("点提及那一行跳到来源笔记的那一行", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n第一行\n见 Plan 一节\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();
      await waitFor(() => expect(mentionRows()).toEqual(["Notes, line 6"]));

      fireEvent.click(screen.getByRole("button", { name: "Notes, line 6" }));

      await screen.findByDisplayValue("Notes");
      /* 文件第 6 行 = 正文第 2 行。正文是 `第一行\n见 Plan 一节\n`,所以行首偏移是 4
         —— 两个坐标系换算对了才落在这里。 */
      await waitFor(() => expect(editorView().state.selection.main.head).toBe(4));
    });

    it("手工重扫", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\nbody\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await openMentions();
      await waitFor(() => expect(harness.callCount("notebook_vault_mentions")).toBe(1));

      // 外部编辑改了别人的笔记时,只能靠重扫发现。
      harness.seed("Notes.md", '---\ntitle: "Notes"\n---\n\n见 Plan 一节\n');
      fireEvent.click(screen.getByRole("button", { name: "Rescan the vault for mentions" }));

      await waitFor(() => expect(mentionRows()).toEqual(["Notes, line 5"]));
    });
  });
});
