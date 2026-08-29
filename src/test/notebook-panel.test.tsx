import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) =>
    Promise.resolve(harness.handle(command, args ?? {})),
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
});
