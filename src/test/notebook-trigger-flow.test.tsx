import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EditorView } from "@uiw/react-codemirror";

import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";
import { NotebookVaultHarness } from "./notebookVaultHarness";

let harness = new NotebookVaultHarness();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: Record<string, unknown>) =>
    harness.handle(command, args ?? {}),
}));

function renderNotebook() {
  return render(
    <I18nProvider>
      <NotebookPanel />
    </I18nProvider>,
  );
}

function editorView(): EditorView {
  const content = screen.getByRole("textbox", { name: "Quick note content" });
  const view = EditorView.findFromDOM(content as HTMLElement);
  if (!view) throw new Error("CodeMirror view not found");
  return view;
}

/**
 * 把整篇内容换成 `value`,光标落在 `|` 的位置(没有 `|` 就落到末尾)。
 *
 * 走 EditorView 事务而不是 fireEvent:CodeMirror 的文档状态在 EditorState 里,
 * 对 contentDOM 派发 input 事件不会更新它。
 */
function typeDoc(value: string) {
  const cursor = value.indexOf("|");
  const text = cursor < 0 ? value : value.slice(0, cursor) + value.slice(cursor + 1);
  const view = editorView();
  act(() => {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
      selection: { anchor: cursor < 0 ? text.length : cursor },
    });
  });
}

function docText(): string {
  return editorView().state.doc.toString();
}

function cursorAt(): number {
  return editorView().state.selection.main.head;
}

/** 键走 contentDOM —— 触发菜单的 keymap 装在编辑器里,不是 window 上。 */
function pressInEditor(key: string) {
  fireEvent.keyDown(screen.getByRole("textbox", { name: "Quick note content" }), { key });
}

function menu(): HTMLElement {
  return screen.getByRole("listbox");
}

function rowLabels(): string[] {
  return within(menu())
    .getAllByRole("option")
    .map((option) => option.textContent ?? "");
}

function selectedLabel(): string {
  const active = menu().getAttribute("aria-activedescendant");
  const node = active ? document.getElementById(active) : null;
  return node?.textContent ?? "";
}

/** 开一篇笔记并等编辑器挂上。 */
async function openNote(body = "body\n") {
  harness.seed("Doc.md", `---\ntitle: "Doc"\n---\n\n${body}`);
  renderNotebook();
  await screen.findByRole("textbox", { name: "Quick note content" });
}

describe("随手记触发菜单", () => {
  beforeEach(() => {
    localStorage.clear();
    harness = new NotebookVaultHarness();
  });

  describe("斜杠菜单", () => {
    it("行首打 / 开菜单", async () => {
      await openNote();
      typeDoc("/|");

      expect(await screen.findByRole("listbox", { name: "Insert" })).toBeInTheDocument();
    });

    it("打字过滤候选", async () => {
      await openNote();
      typeDoc("/quote|");

      await waitFor(() => expect(rowLabels().length).toBeGreaterThan(0));
      expect(rowLabels().some((label) => label.includes("Quote"))).toBe(true);
      expect(rowLabels().some((label) => label.includes("Heading 1"))).toBe(false);
    });

    it("回车插入,并吃掉触发序列", async () => {
      await openNote("");
      typeDoc("/quote|");
      await screen.findByRole("listbox");

      pressInEditor("Enter");

      // 关键是 `/quote` 整段不见了 —— 只删 `/` 会留下一串残字。
      await waitFor(() => expect(docText()).toBe("> "));
      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    });

    it("插入代码块时光标落在围栏中间", async () => {
      await openNote("");
      typeDoc("/code|");
      await screen.findByRole("listbox");

      pressInEditor("Enter");

      await waitFor(() => expect(docText()).toBe("```\n\n```\n"));
      // 光标要在两条围栏之间那一行,不是整段末尾。
      expect(cursorAt()).toBe(4);
    });

    it("正文中间的 / 不开菜单", async () => {
      /* 行中间的 `/` 绝大多数是路径和日期(`src/lib`、`2026/08`),弹菜单纯属打扰,
         而且它还会把方向键和回车抢走。 */
      await openNote("");
      typeDoc("see src/|");

      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    });

    it("下键换选中项,回车插的是新选中的那条", async () => {
      await openNote("");
      typeDoc("/head|");
      await screen.findByRole("listbox");
      const first = selectedLabel();

      pressInEditor("ArrowDown");
      expect(selectedLabel()).not.toBe(first);
      const second = docText();
      expect(second).toBe("/head");

      pressInEditor("Enter");
      await waitFor(() => expect(docText()).toBe("## "));
    });

    it("上键到顶就停,不绕回最后一项", async () => {
      // 和命令面板同一个 `moveSelection` —— 夹住而不是环绕,一路按到底不会突然跳回。
      await openNote("");
      typeDoc("/head|");
      await screen.findByRole("listbox");
      expect(rowLabels().length).toBeGreaterThan(1);
      const first = selectedLabel();

      pressInEditor("ArrowUp");

      expect(selectedLabel()).toBe(first);
    });

    it("下键到底也停住", async () => {
      await openNote("");
      typeDoc("/head|");
      await screen.findByRole("listbox");
      const count = rowLabels().length;

      for (let i = 0; i < count + 3; i += 1) pressInEditor("ArrowDown");

      expect(selectedLabel()).toBe(rowLabels()[count - 1]);
    });

    it("Tab 和回车一样提交", async () => {
      await openNote("");
      typeDoc("/quote|");
      await screen.findByRole("listbox");

      pressInEditor("Tab");

      await waitFor(() => expect(docText()).toBe("> "));
    });

    it("Esc 关菜单但不动文档", async () => {
      await openNote("");
      typeDoc("/quote|");
      await screen.findByRole("listbox");

      pressInEditor("Escape");

      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
      expect(docText()).toBe("/quote");
    });

    it("没有候选时回车交还给编辑器", async () => {
      /* 菜单空着还霸占回车,那一行就永远换不了行 —— 表现是"打错字之后编辑器卡住"。
         用回车而不是方向键来验:`cursorLineDown` 要量排版,jsdom 里量不出来。 */
      await openNote("");
      typeDoc("/zzzz|");
      expect(screen.queryAllByRole("option")).toHaveLength(0);

      pressInEditor("Enter");

      // keymap 交还后由 defaultKeymap 的 insertNewlineAndIndent 接手。
      await waitFor(() => expect(docText()).toBe("/zzzz\n"));
    });

    it("列表项里的 / 也开菜单,起点算在标记之后", async () => {
      /* `- ` 要留着(斜杠的起点是标记之后,不是行首),而且**不能**在中间插换行 ——
         那会把列表项拆成两行。 */
      await openNote("");
      typeDoc("- /quote|");
      await screen.findByRole("listbox");

      pressInEditor("Enter");

      await waitFor(() => expect(docText()).toBe("- > "));
    });

    it("代码块里的 / 不开菜单", async () => {
      await openNote("");
      typeDoc("```\n/quote|\n```\n");

      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    });
  });

  describe("[[ 补全", () => {
    it("列出别的笔记,插入 wikilink", async () => {
      harness.seed("Target.md", '---\ntitle: "Target"\n---\n\nx\n');
      await openNote("");
      typeDoc("see [[Tar|");

      const list = await screen.findByRole("listbox", { name: "Link to a note" });
      expect(within(list).getAllByRole("option")[0]!.textContent).toContain("Target");

      pressInEditor("Enter");

      await waitFor(() => expect(docText()).toBe("see [[Target]] "));
    });

    it("不把自己列出来", async () => {
      // 自链没有意义,而它排第一会挡住真正想链的那篇。
      await openNote("");
      typeDoc("[[Doc|");

      await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
      expect(
        within(menu())
          .queryAllByRole("option")
          .some((option) => (option.textContent ?? "").includes("Doc")),
      ).toBe(false);
    });
  });

  describe("# 补全", () => {
    it("从库里已有的标签补全", async () => {
      harness.seed("Other.md", '---\ntitle: "Other"\n---\n\n#project/alpha\n');
      await openNote("");
      typeDoc("#proj|");

      const list = await screen.findByRole("listbox", { name: "Tag" });
      // 文案就是 `#project/alpha` 一个井号 —— 图标位不再重复放一个 `#`。
      await waitFor(() =>
        expect(
          within(list)
            .getAllByRole("option")
            .map((o) => o.textContent ?? ""),
        ).toContain("#project/alpha"),
      );

      pressInEditor("Enter");

      await waitFor(() => expect(docText()).toBe("#project/alpha "));
    });

    it("纯数字不算标签", async () => {
      // `#42` 是 issue 号。Rust 侧的 `tag_hits` 也这么判,不能只在前端放宽。
      await openNote("");
      typeDoc("fix #42|");

      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    });

    it("紧跟在字符后面的 # 不触发", async () => {
      await openNote("");
      typeDoc("a#tag|");

      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    });
  });

  describe(": emoji 补全", () => {
    it("按英文关键词补,插入的是 emoji 本身", async () => {
      await openNote("");
      typeDoc("ship :rocket|");
      await screen.findByRole("listbox", { name: "Emoji" });

      pressInEditor("Enter");

      await waitFor(() => expect(docText()).toBe("ship 🚀 "));
    });
  });

  describe("@ 补全", () => {
    it("从当前正文里已出现的 @ 词补全", async () => {
      /* 没有后端人名索引,候选只能来自正文。这条同时守着「不要凭空造人」。 */
      await openNote("");
      typeDoc("cc @alice and @al|");

      const list = await screen.findByRole("listbox", { name: "Mention" });
      expect(within(list).getAllByRole("option")[0]!.textContent).toContain("@alice");

      pressInEditor("Enter");

      await waitFor(() => expect(docText()).toBe("cc @alice and @alice "));
    });
  });

  describe("菜单生命周期", () => {
    it("光标离开触发序列就关", async () => {
      await openNote("");
      typeDoc("/quote|");
      await screen.findByRole("listbox");

      act(() => {
        editorView().dispatch({ selection: { anchor: 0 } });
      });

      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    });

    it("选中一段文本时不开", async () => {
      await openNote("");
      typeDoc("/quote");
      act(() => {
        editorView().dispatch({ selection: { anchor: 0, head: 6 } });
      });

      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    });

    it("鼠标点行提交", async () => {
      await openNote("");
      typeDoc("/quote|");
      const list = await screen.findByRole("listbox");

      fireEvent.mouseDown(within(list).getAllByRole("option")[0]!);

      await waitFor(() => expect(docText()).toBe("> "));
    });

    it("换笔记时收起 —— start 是上一篇里的偏移", async () => {
      /* 不收的话菜单原样留在屏幕上,而它的 `trigger.start` 指向上一篇的位置:
         新的那篇更短时那个偏移直接越界,提交就会插到错的地方。 */
      harness.seed("Long.md", `---\ntitle: "Long"\n---\n\n${"x".repeat(200)}\n/quote`);
      harness.seed("Short.md", '---\ntitle: "Short"\n---\n\nhi\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      fireEvent.click(await screen.findByRole("button", { name: "Long" }));
      await waitFor(() => expect(docText()).toContain("/quote"));
      act(() => {
        editorView().dispatch({ selection: { anchor: editorView().state.doc.length } });
      });
      await screen.findByRole("listbox", { name: "Insert" });

      fireEvent.click(screen.getByRole("button", { name: "Short" }));

      await waitFor(() => expect(docText()).toBe("hi\n"));
      expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    });

    it("命令面板开着时让位", async () => {
      /* 两个都是 listbox,同时在场会让 `getByRole("listbox")` 变歧义,更要紧的是
         方向键会被两处同时抢。 */
      await openNote("");
      typeDoc("/quote|");
      await screen.findByRole("listbox", { name: "Insert" });

      fireEvent.keyDown(screen.getByRole("textbox", { name: "Quick note content" }), {
        key: "k",
        metaKey: true,
      });

      expect(screen.getByRole("listbox", { name: "Command palette" })).toBeInTheDocument();
      expect(screen.queryByRole("listbox", { name: "Insert" })).not.toBeInTheDocument();
    });
  });
});
