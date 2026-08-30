import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EditorView } from "@uiw/react-codemirror";

import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";
import { NotebookVaultHarness } from "./notebookVaultHarness";

let harness = new NotebookVaultHarness();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: Record<string, unknown>) =>
    harness.handle(command, args ?? {}),
}));

function editorView(): EditorView {
  const content = screen.getByRole("textbox", { name: "Quick note content" });
  const view = EditorView.findFromDOM(content as HTMLElement);
  if (!view) throw new Error("CodeMirror view not found");
  return view;
}

function docText(): string {
  return editorView().state.doc.toString();
}

/** 选中 `[from, to)`,再抬手 —— 气泡挂在"动作结束"上,只 dispatch 不抬手不会出现。 */
function dragSelect(from: number, to: number) {
  act(() => {
    editorView().dispatch({ selection: { anchor: from, head: to } });
  });
  fireEvent.mouseUp(screen.getByRole("textbox", { name: "Quick note content" }));
}

function bubble(): HTMLElement | null {
  return screen.queryByRole("toolbar", { name: "Formatting" });
}

async function openNote(body = "hello world\n") {
  harness.seed("Doc.md", `---\ntitle: "Doc"\n---\n\n${body}`);
  render(
    <I18nProvider>
      <NotebookPanel />
    </I18nProvider>,
  );
  await screen.findByRole("textbox", { name: "Quick note content" });
}

/**
 * 点气泡上的一个按钮。走 mouseDown —— 组件挂的是它,不是 click。
 *
 * 必须限定在气泡里找:Markdown 工具栏上有同名按钮("Bold" 之类),全局找会撞。
 */
function clickBubble(label: string) {
  const root = bubble();
  if (!root) throw new Error("bubble not open");
  fireEvent.mouseDown(within(root).getByRole("button", { name: label }));
}

describe("随手记选区浮动气泡", () => {
  beforeEach(() => {
    localStorage.clear();
    harness = new NotebookVaultHarness();
    /* jsdom 没有排版引擎,`coordsAtPos` 恒为 null,而气泡的位置完全来自它 ——
       不桩掉的话 `selectionRect()` 永远返回 null,气泡在测试里永远不出现,整套交互
       都没法验。桩的是坐标这一件事,选区状态仍然走真实的 EditorState。 */
    vi.spyOn(EditorView.prototype, "coordsAtPos").mockImplementation(function (
      this: EditorView,
      pos: number,
    ) {
      return { left: 100 + pos, right: 108 + pos, top: 200, bottom: 216 };
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("出现与收起", () => {
    it("拖选一段文字后出现", async () => {
      await openNote();
      expect(bubble()).not.toBeInTheDocument();

      dragSelect(0, 5);

      expect(bubble()).toBeInTheDocument();
    });

    it("空选区不出现 —— 只把光标点到某处不算选中", async () => {
      await openNote();

      dragSelect(3, 3);

      expect(bubble()).not.toBeInTheDocument();
    });

    it("两端坐标都拿不到时干净地不出现,而不是抛异常", async () => {
      /* 整段选区滚出可视区时 CodeMirror 给不出坐标。
         只断言"气泡没出现"是不够的:去掉那道判断之后 `head` 就是 null,读 `head.top`
         抛 TypeError,气泡同样不出现 —— 两种结果在 DOM 上一模一样。而抛出来的地方是
         mouseUp 监听器里,jsdom 会把它吞掉,线上表现是"偶尔按一下就没反应了"。
         所以这里必须盯着未捕获异常。 */
      await openNote();
      vi.mocked(EditorView.prototype.coordsAtPos).mockReturnValue(null);
      const errors: unknown[] = [];
      const onError = (event: ErrorEvent) => {
        event.preventDefault();
        errors.push(event.error);
      };
      window.addEventListener("error", onError);

      try {
        dragSelect(0, 5);
      } finally {
        window.removeEventListener("error", onError);
      }

      expect(errors).toEqual([]);
      expect(bubble()).not.toBeInTheDocument();
    });

    it("选区变空就收起,不等下一次抬手", async () => {
      /* 不收的话气泡停在上一段选区的位置上,而那段已经不是选区了 —— 点它就是对空选区
         执行命令,什么都不会发生,但用户以为点到了。 */
      await openNote();
      dragSelect(0, 5);
      expect(bubble()).toBeInTheDocument();

      act(() => {
        editorView().dispatch({ selection: { anchor: 8 } });
      });

      expect(bubble()).not.toBeInTheDocument();
    });

    it("换笔记时收起 —— 坐标是上一篇里的", async () => {
      harness.seed("A.md", '---\ntitle: "A"\n---\n\naaaa\n');
      harness.seed("B.md", '---\ntitle: "B"\n---\n\nbbbb\n');
      render(
        <I18nProvider>
          <NotebookPanel />
        </I18nProvider>,
      );
      await screen.findByRole("textbox", { name: "Quick note content" });

      fireEvent.click(await screen.findByRole("button", { name: "A" }));
      await waitFor(() => expect(docText()).toBe("aaaa\n"));
      dragSelect(0, 4);
      expect(bubble()).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "B" }));

      await waitFor(() => expect(docText()).toBe("bbbb\n"));
      expect(bubble()).not.toBeInTheDocument();
    });

    it("换视图时收起 —— 同一个编辑器,选区还在,但坐标全变了", async () => {
      /* 特意用 源码 → 实时(不是 → 阅读):阅读态把编辑器整个卸掉,选区随之消失,那条
         路由靠 `handleSelectionChange` 就收住了。而 源码 ↔ 实时 是**同一个** CodeMirror
         实例(key 只带笔记 id),选区原样留着 —— 但实时态把标记符号藏了,同样的偏移落在
         完全不同的坐标上,气泡会停在一个和选区无关的位置。这一条只有挂在 `mode` 上的
         effect 能收。 */
      await openNote();
      dragSelect(0, 5);
      expect(bubble()).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Live" }));

      // 选区确实还在 —— 说明收起不是"选区没了"顺带的结果。
      expect(editorView().state.selection.main.empty).toBe(false);
      expect(bubble()).not.toBeInTheDocument();
    });

    it("只有一端拿到坐标时照样出现", async () => {
      /* 选区一半滚出可视区是常态(选了一大段再往下滚)。这时另一端的坐标够用了,
         按"拿不到就不画"处理会让长选区上的气泡随机消失。 */
      await openNote();
      vi.mocked(EditorView.prototype.coordsAtPos).mockImplementation((pos: number) =>
        pos === 0 ? null : { left: 300, right: 308, top: 200, bottom: 216 },
      );

      dragSelect(0, 5);

      expect(bubble()).toBeInTheDocument();
    });

    it("跨行选区横向对齐首行,不取整块外接矩形", async () => {
      /* 跨行时按两端并集居中,气泡会停在中间几行的正中,离用户手上那一端很远。
         纵向也只用首行:气泡挂在选区顶边上方。 */
      await openNote("aaaa\nbbbb\n");
      vi.mocked(EditorView.prototype.coordsAtPos).mockImplementation((pos: number) =>
        pos < 5
          ? { left: 100, right: 108, top: 200, bottom: 216 }
          : { left: 600, right: 608, top: 300, bottom: 316 },
      );

      dragSelect(0, 7);

      // 首行 [100,108] 的中点 104 → left = 104 - 336/2,夹到 EDGE=8。
      expect(bubble()!.style.left).toBe("8px");
      // 顶边取首行的 200,不是末行的 300。
      expect(bubble()!.style.top).toBe(`${200 - 34 - 8}px`);
    });

    it("和触发菜单天然互斥 —— 打出 `/` 时气泡已经收了", async () => {
      /* 触发菜单只在**空选区**下弹(`NoteSourceEditor` 的 updateListener 里那条
         `if (!range.empty) report(null)`),气泡只在**非空选区**下画。所以两者不会同时
         在场,靠的是选区那一条,不需要在挂载处再加一道 `!trigger`。 */
      await openNote("");
      dragSelect(0, 0);
      act(() => {
        editorView().dispatch({
          changes: { from: 0, insert: "abc" },
          selection: { anchor: 0, head: 3 },
        });
      });
      fireEvent.mouseUp(screen.getByRole("textbox", { name: "Quick note content" }));
      expect(bubble()).toBeInTheDocument();

      act(() => {
        editorView().dispatch({
          changes: { from: 0, to: 3, insert: "/quote" },
          selection: { anchor: 6 },
        });
      });

      expect(await screen.findByRole("listbox", { name: "Insert" })).toBeInTheDocument();
      expect(bubble()).not.toBeInTheDocument();
    });

    it("命令面板开着时让位", async () => {
      // 面板一开焦点就离开编辑器,留着气泡只是挡视线,而它的按钮还会去改那段选区。
      await openNote();
      dragSelect(0, 5);
      expect(bubble()).toBeInTheDocument();

      fireEvent.keyDown(screen.getByRole("textbox", { name: "Quick note content" }), {
        key: "k",
        metaKey: true,
      });

      expect(screen.getByRole("listbox", { name: "Command palette" })).toBeInTheDocument();
      expect(bubble()).not.toBeInTheDocument();
    });
  });

  describe("命令", () => {
    it("加粗包住选中的那段", async () => {
      await openNote();
      dragSelect(0, 5);

      clickBubble("Bold");

      await waitFor(() => expect(docText()).toBe("**hello** world\n"));
    });

    it("行内代码用单反引号,不是三个", async () => {
      await openNote();
      dragSelect(0, 5);

      clickBubble("Inline code");

      await waitFor(() => expect(docText()).toBe("`hello` world\n"));
    });

    it("链接留下 url 占位,光标能接着改", async () => {
      await openNote();
      dragSelect(0, 5);

      clickBubble("Link");

      await waitFor(() => expect(docText()).toBe("[hello](url) world\n"));
    });

    it("引用逐行加 `> `,标题的井号不剥", async () => {
      /* `applyLinePrefix` 会先剥 `#{1,6}`(改标题层级时是对的),而引用一个标题时
         剥掉井号就是把标题降成了正文。 */
      await openNote("# 标题\n正文\n");
      dragSelect(0, 7);

      clickBubble("Quote");

      await waitFor(() => expect(docText()).toBe("> # 标题\n> 正文\n"));
    });

    it("已经是引用的行不再叠一层", async () => {
      await openNote("> 引用\n");
      dragSelect(0, 4);

      clickBubble("Quote");

      await waitFor(() => expect(docText()).toBe("> 引用\n"));
    });

    it("执行完收起 —— 选区位置已经变了", async () => {
      await openNote();
      dragSelect(0, 5);

      clickBubble("Bold");

      await waitFor(() => expect(docText()).toBe("**hello** world\n"));
      expect(bubble()).not.toBeInTheDocument();
    });
  });
});
