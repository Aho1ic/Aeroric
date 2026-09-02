/* NotebookPanel — wikilink / ![[嵌入]] / 悬浮预览 / 反链 / 引用图谱 / 未链接的提及
 *
 * 从原来 7374 行的 notebook-panel.test.tsx 拆出来的一份。拆分理由是并行度:
 * vitest 按文件并行,379 个测试挤在一个文件里只能占一个核,跑 14 分 45 秒。
 * 共用的渲染 / 编辑器辅助在 ./notebookPanelKit,断言与行为逐字未改。 */
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NotebookVaultHarness } from "./notebookVaultHarness";
import { editorView, renderNotebook } from "./notebookPanelKit";

/* 注意 `async` 不是可以省的:harness 的失败分支是同步 `throw`,而真实 `invoke`
 * 只会以 rejection 的形式报错。写成 `Promise.resolve(harness.handle(...))` 的话
 * 抛错会在 promise 生成之前同步逃出调用点,凡是"发出去不等结果、用 .catch 收错"
 * 的写法都会变成未捕获异常 —— 测到的是 mock 的怪癖,不是产品行为。
 *
 * `harness` 与两个 `vi.mock` 每个文件各一份,不能提到 kit 里:`vi.mock` 按文件
 * 提升,从被 import 的模块里调用对当前文件无效;工厂闭包读的就是这个 `harness`。 */
let harness: NotebookVaultHarness;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: Record<string, unknown>) =>
    harness.handle(command, args ?? {}),
}));

/* 本文件没有测剪贴板的用例,但这个 mock 仍然是必需的:`NotebookPanel.tsx` 在
 * 模块顶层 import 了 `plugin-clipboard-manager` 的 `readText`,而 jsdom 里没有
 * Tauri 插件 —— 不 mock 的话面板根本 import 不进来。 */
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: () => navigator.clipboard.readText(),
}));

describe("NotebookPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    harness = new NotebookVaultHarness();
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
