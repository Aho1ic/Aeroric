/* NotebookPanel — 查找栏 / 全库搜索 / 命令面板
 *
 * 从原来 7374 行的 notebook-panel.test.tsx 拆出来的一份。拆分理由是并行度:
 * vitest 按文件并行,379 个测试挤在一个文件里只能占一个核,跑 14 分 45 秒。
 * 共用的渲染 / 编辑器辅助在 ./notebookPanelKit,断言与行为逐字未改。 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HARNESS_VAULT, NotebookVaultHarness } from "./notebookVaultHarness";
import { editorValue, editorView, renderNotebook } from "./notebookPanelKit";

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

  describe("查找栏的大小写 / 整词 / 正则", () => {
    /**
     * 正文从磁盘种进去,而**不是**用 `setEditorValue` 打进编辑器。
     *
     * `@uiw/react-codemirror` 有个「打字闸」:凡是编辑器自己产生的文档变更,都会开一个
     * 200 拍的倒计时,闸没到期之前外部传进来的 `value` 只入队、不落到文档上
     * (`useCodeMirror.js` 的 `typingLatch` / `pendingUpdate`)。那 200 拍走的是
     * `setInterval(…, 1)` —— 真实浏览器里 200ms 就过去了,用户敲完字再去点「全部替换」
     * 早就过期了;但 jsdom 在整份测试文件的负载下 1ms 定时器会被饿到几秒一拍,于是替换
     * 明明已经写进 state,编辑器里还是旧文本,断言就超时了。
     *
     * 单跑这一组时闸走得快,所以这类用例会「单独跑过、整文件跑挂」。种到磁盘上就压根
     * 不产生编辑器侧的变更,闸不会开。
     */
    async function openFind(body: string) {
      // 标题别叫「Find」—— 会和查找框的可及名字撞在一起,按名字取元素时很难看。
      harness.seed("Doc.md", `---\ntitle: "Doc"\n---\n\n${body}`);
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Doc" }));
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      await waitFor(() => expect(editorValue()).toBe(body));
      fireEvent.keyDown(content, { key: "h", metaKey: true });
      return { content };
    }

    function typeQuery(value: string) {
      fireEvent.change(screen.getByRole("textbox", { name: "Find" }), { target: { value } });
    }

    function setReplacement(value: string) {
      fireEvent.change(screen.getByRole("textbox", { name: "Replace" }), { target: { value } });
    }

    function status(): string {
      // 状态栏是那个 aria-live 的 span,固定在上下按钮左边。
      const live = document.querySelector('[aria-live="polite"]');
      return live?.textContent ?? "";
    }

    it("区分大小写开关会改命中数", async () => {
      await openFind("Alpha alpha ALPHA");
      typeQuery("alpha");
      await waitFor(() => expect(status()).toBe("1/3"));

      fireEvent.click(screen.getByRole("button", { name: "Match case" }));
      await waitFor(() => expect(status()).toBe("1/1"));
    });

    it("正则模式下替换能引用捕获组", async () => {
      await openFind("2024-01-02 和 2025-03-04");
      fireEvent.click(screen.getByRole("button", { name: "Use regular expression" }));
      typeQuery("(\\d{4})-(\\d{2})-(\\d{2})");
      await waitFor(() => expect(status()).toBe("1/2"));

      setReplacement("$3/$2/$1");
      fireEvent.click(screen.getByRole("button", { name: "Replace all" }));

      await waitFor(() => expect(editorValue()).toBe("02/01/2024 和 04/03/2025"));
    });

    it("普通模式下 $ 是字面量,不当捕获组用", async () => {
      await openFind("price here");
      typeQuery("price");
      await waitFor(() => expect(status()).toBe("1/1"));

      // `$&` 是唯一在普通模式下能验出「$ 没被当成引用」的记号 —— `$1` 在普通模式下
      // 一律越界,展开与不展开的结果相同。
      setReplacement("$1.50 / $&");
      fireEvent.click(screen.getByRole("button", { name: "Replace all" }));

      await waitFor(() => expect(editorValue()).toBe("$1.50 / $& here"));
    });

    it("半截正则报错而不是崩,也不显示无匹配", async () => {
      await openFind("abc");
      fireEvent.click(screen.getByRole("button", { name: "Use regular expression" }));
      typeQuery("(");

      await waitFor(() => expect(screen.getByText("Invalid regex")).toBeInTheDocument());
      expect(screen.queryByText("No matches")).not.toBeInTheDocument();
    });

    it("整词过滤掉词内命中", async () => {
      await openFind("cat scatter cat");
      typeQuery("cat");
      await waitFor(() => expect(status()).toBe("1/3"));

      fireEvent.click(screen.getByRole("button", { name: "Match whole word" }));
      await waitFor(() => expect(status()).toBe("1/2"));
    });

    it("切开关回到第一处命中", async () => {
      // 命中数不变的开关才验得出这条:数一变,那个「序号夹到上界」的 effect 会顺手
      // 把序号带回合法范围,于是复位有没有做都看不出来。
      await openFind("cat cat cat");
      typeQuery("cat");
      await waitFor(() => expect(status()).toBe("1/3"));

      fireEvent.click(screen.getByRole("button", { name: "Next match" }));
      fireEvent.click(screen.getByRole("button", { name: "Next match" }));
      await waitFor(() => expect(status()).toBe("3/3"));

      fireEvent.click(screen.getByRole("button", { name: "Match whole word" }));
      // 三处都是整词,总数不变;序号该回到第一处。
      await waitFor(() => expect(status()).toBe("1/3"));
    });

    it("整词在中文上给出放宽提示,而不是 0 命中", async () => {
      await openFind("本周计划表");
      typeQuery("计划");
      fireEvent.click(screen.getByRole("button", { name: "Match whole word" }));

      await waitFor(() => expect(status()).toBe("1/1"));
      expect(screen.getByText("Whole word relaxed")).toBeInTheDocument();
    });

    it("大小写折叠改长度时替换不串位", async () => {
      // 旧实现在小写化后的串上取偏移,这里会写成 "İstanbul 的 cDOG"。
      await openFind("İstanbul 的 cat");
      typeQuery("cat");
      await waitFor(() => expect(status()).toBe("1/1"));

      setReplacement("dog");
      fireEvent.click(screen.getByRole("button", { name: "Replace all" }));

      await waitFor(() => expect(editorValue()).toBe("İstanbul 的 dog"));
    });

    it("单处替换只动当前那一处", async () => {
      await openFind("cat cat cat");
      typeQuery("cat");
      await waitFor(() => expect(status()).toBe("1/3"));

      fireEvent.click(screen.getByRole("button", { name: "Next match" }));
      await waitFor(() => expect(status()).toBe("2/3"));

      setReplacement("dog");
      // `name` 是全等匹配,所以这里不会连上「Replace all」。
      fireEvent.click(screen.getByRole("button", { name: "Replace" }));

      await waitFor(() => expect(editorValue()).toBe("cat dog cat"));
    });
  });

  describe("全库搜索", () => {
    /** ⌘⇧F 从编辑器里唤出全库搜索。返回那个输入框。 */
    async function openGlobalSearch() {
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "F", shiftKey: true, metaKey: true });
      return screen.getByRole("textbox", { name: "Search all notes" });
    }

    function submit(input: HTMLElement, query: string) {
      fireEvent.change(input, { target: { value: query } });
      fireEvent.keyDown(input, { key: "Enter" });
    }

    /* 限定在对话框里取状态行:查找栏也有一个 aria-live,虽然两者互斥,但按文档
       顺序取第一个会在将来加了别的 live 区域时静默指到别处去。 */
    function status(): string {
      const dialog = screen.getByRole("dialog", { name: "Search all notes" });
      return dialog.querySelector('[aria-live="polite"]')?.textContent ?? "";
    }

    /* 分组也要限定在对话框里数:面板根节点自己就是个 `region`(`aria-label="Quick
       Notes"`),在全局数会永远多出来一个。 */
    function hitGroupLabels(): (string | null)[] {
      const dialog = screen.getByRole("dialog", { name: "Search all notes" });
      return within(dialog)
        .queryAllByRole("region")
        .map((group) => group.getAttribute("aria-label"));
    }

    it("⌘⇧F 开全库搜索,而不是当前这篇的查找栏", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });

      fireEvent.keyDown(content, { key: "F", shiftKey: true, metaKey: true });

      expect(screen.getByRole("dialog", { name: "Search all notes" })).toBeInTheDocument();
      expect(screen.queryByRole("textbox", { name: "Find" })).not.toBeInTheDocument();
    });

    it("查找栏开着时按 ⌘⇧F 会把它收掉", async () => {
      /* 上一条从"查找栏本来就没开"出发,验不出这次收拢 —— 要先真的开着。两个查找框
         同时在场时 Escape 该关谁没有直觉答案,而它们的输入框长得几乎一样。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "f", metaKey: true });
      expect(screen.getByRole("textbox", { name: "Find" })).toBeInTheDocument();

      fireEvent.keyDown(content, { key: "F", shiftKey: true, metaKey: true });

      expect(screen.getByRole("dialog", { name: "Search all notes" })).toBeInTheDocument();
      expect(screen.queryByRole("textbox", { name: "Find" })).not.toBeInTheDocument();
    });

    it("⌘F 仍然只开当前这篇的查找栏", async () => {
      // ⇧ 那一支排在前面,不带 ⇧ 的路径不能被它吃掉。
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });

      fireEvent.keyDown(content, { key: "f", metaKey: true });

      expect(screen.getByRole("textbox", { name: "Find" })).toBeInTheDocument();
      expect(screen.queryByRole("dialog", { name: "Search all notes" })).not.toBeInTheDocument();
    });

    it("搜到的命中按文件分组列出来", async () => {
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\ncat sat\n还有 cat\n');
      harness.seed("Beta.md", '---\ntitle: "Beta"\n---\n\nno match here\n');
      harness.seed("Gamma.md", '---\ntitle: "Gamma"\n---\n\none cat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "cat");

      // 3 处命中(Alpha 两处 + Gamma 一处),分布在 2 篇里。Beta 不该出现。
      await waitFor(() => expect(status()).toContain("3 matches across 2 notes"));
      expect(hitGroupLabels()).toEqual(["Alpha.md", "Gamma.md"]);
    });

    it("点一条命中跳到那篇笔记的那一行", async () => {
      /* 这是这一项的验收点:「全文搜索命中可定位到行」。行号是**文件行号**
         (frontmatter 算在内),而光标要落在**正文**坐标系里 —— 两者差几行取决于
         frontmatter 有多长,所以这条同时钉住那次换算。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      harness.seed("Hit.md", '---\ntitle: "Hit"\n---\n\n第一行\n有 needle 在这\n');
      renderNotebook();
      await screen.findByDisplayValue("Hit");
      const input = await openGlobalSearch();

      submit(input, "needle");
      fireEvent.click(await screen.findByRole("button", { name: /Hit\.md line 6/ }));

      expect(await screen.findByDisplayValue("Hit")).toBeInTheDocument();
      // 面板收掉:它铺满整个面板,留着的话用户点了一条却什么都看不见。
      expect(screen.queryByRole("dialog", { name: "Search all notes" })).not.toBeInTheDocument();
      /* 文件第 6 行是 `有 needle 在这`;正文(拆掉 frontmatter)是
         `第一行\n有 needle 在这\n`,那一行的行首在 4。 */
      await waitFor(() => expect(editorView().state.selection.main.head).toBe(4));
    });

    it("跳到一篇还没读入的笔记也落在那一行", async () => {
      /* 和反链那条同样的理由:列表只读目录项,除当前这篇之外都还没读入,正文比
         编辑器晚到。只在挂载那一刻读一次 prop 的写法会静默把光标留在开头。
         src 先种、Doc 后种,于是 Doc 是挂载时的当前笔记。 */
      harness.seed("src.md", '---\ntitle: "Source"\n---\n\n第一行\n有 needle 在这\n');
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByDisplayValue("Doc");
      const input = await openGlobalSearch();

      submit(input, "needle");
      fireEvent.click(await screen.findByRole("button", { name: /src\.md line 6/ }));

      expect(await screen.findByDisplayValue("Source")).toBeInTheDocument();
      await waitFor(() => expect(editorView().state.selection.main.head).toBe(4));
    });

    it("中文在命中前面时高亮不串位", async () => {
      /* 后端给的 `column` 是**字节**偏移。`标题 ` 是 3 汉字 + 空格 = 10 字节,
         所以 `abc` 的列是 11;直接拿它当 JS 下标会切在「题」上,高亮整体左移。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\n标题 abc def\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "abc");

      await waitFor(() => expect(document.querySelector("mark")?.textContent).toBe("abc"));
    });

    it("正则开关传到后端", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\n2024-01-02\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      // 纯文本模式下 `\d{4}` 一处都不该命中(它会被转义成字面量)。
      submit(input, "\\d{4}");
      await waitFor(() => expect(status()).toContain("No matching notes"));

      fireEvent.click(screen.getByRole("button", { name: "Use regular expression" }));
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(status()).toContain("1 matches across 1 notes"));
    });

    it("区分大小写开关传到后端", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nAlpha alpha\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "alpha");
      await waitFor(() => expect(status()).toContain("2 matches across 1 notes"));

      fireEvent.click(screen.getByRole("button", { name: "Match case" }));
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(status()).toContain("1 matches across 1 notes"));
    });

    it("后端报错时原样显示,不显示成「没有结果」", async () => {
      /* 半个正则写到一半就回车是常态。这时候说「没有结果」是在骗人 —— 用户会以为
         库里真的没有,而实际是模式不合法。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      harness.failTextSearch = true;
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "(unclosed");

      await waitFor(() => expect(status()).toContain("regex parse error"));
      expect(status()).not.toContain("No matching notes");
    });

    it("报错时把上一批结果清掉", async () => {
      /* 「搜到 3 条 → 改成半个正则 → 报错」是真实顺序。旧命中留在列表里的话,状态行
         说出错、下面却列着三条结果,用户没法判断哪个当真。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat sat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "cat");
      await waitFor(() => expect(hitGroupLabels()).toEqual(["Doc.md"]));

      harness.failTextSearch = true;
      submit(input, "(unclosed");

      await waitFor(() => expect(status()).toContain("regex parse error"));
      expect(hitGroupLabels()).toHaveLength(0);
    });

    it("空查询不发请求,并清掉上一批结果", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat sat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "cat");
      await waitFor(() => expect(status()).toContain("1 matches across 1 notes"));

      // 清空再回车:留着上一批结果会像「已经清空了还搜得到」。
      submit(input, "   ");
      await waitFor(() => expect(status()).toContain("press Enter to search"));
      expect(hitGroupLabels()).toHaveLength(0);
    });

    it("先发的搜索后回来时不会盖掉后发的结果", async () => {
      /* 改了条件立刻重搜是常态,而两次搜索的耗时取决于命中多少 —— 前一次(范围更宽)
         很可能后回来。不认序号的话它会把新结果盖掉,而列表上看不出任何异常:用户
         搜的是 `needle`,看到的却是 `e` 的那一批。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nneedle here\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      harness.holdTextSearches();
      submit(input, "e");
      submit(input, "needle");
      await waitFor(() => expect(harness.heldTextSearchCount()).toBe(2));

      // 故意反序放行:后发的先回,先发的后回。
      harness.releaseTextSearch(1);
      await waitFor(() => expect(status()).toContain("1 matches across 1 notes"));

      /* 放行后要用 `act` 把 promise 和随之而来的 state 更新都冲干净再断言。
         `waitFor` 的第一次检查是同步跑的,会在旧值上就通过 —— 那样这条测试对
         「旧结果盖掉新结果」是瞎的。 */
      await act(async () => {
        harness.releaseTextSearch(0);
      });

      // `e` 那一批命中远多于 1 处,盖上来的话摘要会立刻变。
      expect(status()).toContain("1 matches across 1 notes");
    });

    it("先发的搜索失败时不会给后发的成功结果扣上报错", async () => {
      /* 半个正则搜完立刻补全再搜是常见手速。那次失败要是后回来,状态行会在一片
         正常结果上面挂一条 regex parse error —— 用户会以为新的这次也炸了。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nneedle here\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      harness.holdTextSearches();
      harness.failTextSearch = true;
      submit(input, "(unclosed");
      harness.failTextSearch = false;
      submit(input, "needle");
      await waitFor(() => expect(harness.heldTextSearchCount()).toBe(2));

      harness.releaseTextSearch(1);
      await waitFor(() => expect(status()).toContain("1 matches across 1 notes"));
      await act(async () => {
        harness.releaseTextSearch(0);
      });

      expect(status()).not.toContain("regex parse error");
      expect(status()).toContain("1 matches across 1 notes");
    });

    it("先发的搜索回来时不会把后发那次的进行中状态抹掉", async () => {
      /* loading 也归序号管。旧的那次结束时顺手把 loading 关掉,状态行就会在新搜索
         还在路上时说"没有匹配的笔记" —— 一条尚未返回的搜索被显示成搜完了。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nneedle here\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      harness.holdTextSearches();
      submit(input, "needle");
      submit(input, "here");
      await waitFor(() => expect(harness.heldTextSearchCount()).toBe(2));

      // 只放行先发的那次,后发的还挂着。
      await act(async () => {
        harness.releaseTextSearch(0);
      });

      expect(status()).toContain("Searching");
      expect(status()).not.toContain("No matching notes");
    });

    it("命中触顶时摘要标出结果被截断", async () => {
      /* 上限是 500。触顶时必须说出来 —— 用户看到 500 条会以为那就是全部,而
         「改个更精确的词再搜」这个决定完全取决于知不知道结果被截了。 */
      const lines = Array.from({ length: 600 }, () => "needle").join("\n");
      harness.seed("Doc.md", `---\ntitle: "Doc"\n---\n\n${lines}\n`);
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "needle");

      await waitFor(() => expect(status()).toContain("500 matches"));
      expect(status()).toContain("Limit reached");
    });

    it("没触顶就不提截断", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nneedle\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "needle");

      await waitFor(() => expect(status()).toContain("1 matches across 1 notes"));
      expect(status()).not.toContain("Limit reached");
    });

    it("Escape 关掉面板", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      fireEvent.keyDown(input, { key: "Escape" });

      expect(screen.queryByRole("dialog", { name: "Search all notes" })).not.toBeInTheDocument();
    });

    it("重开面板时上一批结果还在", async () => {
      // 关掉再开常常是"刚搜的那批还想再点一条"。
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat sat\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "cat");
      await waitFor(() => expect(status()).toContain("1 matches across 1 notes"));
      fireEvent.keyDown(input, { key: "Escape" });

      fireEvent.keyDown(content, { key: "F", shiftKey: true, metaKey: true });
      expect(status()).toContain("1 matches across 1 notes");
    });

    it("命中所在文件不在列表里时明说,而不是静默不动", async () => {
      /* 后端会 canonicalize 根目录,而列表是另一条路给的路径。真的对不上时(文件刚被
         移走/删掉)必须说出来 —— 静默 return 会让用户以为面板坏了。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      harness.searchGhostHit = true;
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      const input = await openGlobalSearch();

      submit(input, "needle");
      fireEvent.click(await screen.findByRole("button", { name: /Ghost\.md line 5/ }));

      await waitFor(() => expect(status()).toContain("not in the current note list"));
      // 面板留着:没跳成功就关掉面板等于把用户手里的结果一起收走。
      expect(screen.getByRole("dialog", { name: "Search all notes" })).toBeInTheDocument();
    });
  });

  describe("命令面板", () => {
    /** ⌘K 从编辑器里唤出命令面板。返回那个输入框。 */
    async function openPalette() {
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "k", metaKey: true });
      return screen.getByRole("combobox", { name: "Command palette" });
    }

    function optionLabels(): string[] {
      const list = screen.getByRole("listbox", { name: "Command palette" });
      return within(list)
        .getAllByRole("option")
        .map((option) => option.textContent ?? "");
    }

    it("⌘K 开命令面板", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await openPalette();

      expect(screen.getByRole("dialog", { name: "Command palette" })).toBeInTheDocument();
    });

    it("再按一次 ⌘K 关掉", async () => {
      // ⌘K 是开关。不接的话第二次按会落到 WebView 或面板外的监听上。
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "k", metaKey: true });
      fireEvent.keyDown(content, { key: "k", metaKey: true });

      expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();
    });

    it("空查询时列出命令", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await openPalette();

      expect(optionLabels().some((label) => label.includes("Link graph"))).toBe(true);
    });

    it("打字过滤掉不相关的命令", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const input = await openPalette();

      fireEvent.change(input, { target: { value: "graph" } });

      expect(optionLabels().some((label) => label.includes("Link graph"))).toBe(true);
      expect(optionLabels().some((label) => label.includes("Trash"))).toBe(false);
    });

    it("回车执行选中的命令", async () => {
      /* 验收点:命令面板真的能开出别的面板来。这条走「⌘K → 打字 → 回车」整条路,
         而不是直接调处理函数。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const input = await openPalette();

      fireEvent.change(input, { target: { value: "graph" } });
      fireEvent.keyDown(input, { key: "Enter" });

      expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();
      expect(await screen.findByRole("dialog", { name: "Link graph" })).toBeInTheDocument();
    });

    it("执行命令时先关面板,后开那个 overlay", async () => {
      /* 顺序反了的话,`run` 里开的 overlay 会被命令面板的关闭逻辑连带盖掉 ——
         表现是点了一下什么都没发生。这条盯的就是那个顺序。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const input = await openPalette();

      fireEvent.change(input, { target: { value: "Search all" } });
      fireEvent.keyDown(input, { key: "Enter" });

      expect(await screen.findByRole("dialog", { name: "Search all notes" })).toBeInTheDocument();
    });

    it("笔记也在候选里,回车跳过去", async () => {
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\na\n');
      harness.seed("Beta.md", '---\ntitle: "Beta"\n---\n\nb\n');
      renderNotebook();
      const input = await openPalette();

      fireEvent.change(input, { target: { value: "Beta" } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("Beta"),
      );
    });

    it("没有笔记时也能开,并且能新建", async () => {
      // 空库正是最需要它的时候 —— 那时面板上几乎没有别的入口。
      renderNotebook();
      // 「空库」这句话在笔记列表和正文区各有一处,用 findAllByText 避开歧义。
      await screen.findAllByText("No quick notes yet");
      const region = screen.getByRole("region", { name: "Quick Notes" });
      fireEvent.keyDown(region, { key: "k", metaKey: true });
      const input = screen.getByRole("combobox", { name: "Command palette" });

      fireEvent.change(input, { target: { value: "New quick note" } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note name" })).toBeInTheDocument(),
      );
    });

    it("没有笔记时「删除」这条灰着", async () => {
      renderNotebook();
      // 「空库」这句话在笔记列表和正文区各有一处,用 findAllByText 避开歧义。
      await screen.findAllByText("No quick notes yet");
      const region = screen.getByRole("region", { name: "Quick Notes" });
      fireEvent.keyDown(region, { key: "k", metaKey: true });
      const input = screen.getByRole("combobox", { name: "Command palette" });

      fireEvent.change(input, { target: { value: "Delete" } });

      const list = screen.getByRole("listbox", { name: "Command palette" });
      const option = within(list).getAllByRole("option")[0];
      expect(option).toHaveAttribute("aria-disabled", "true");
    });

    it("灰着的那条回车不执行,也不关面板", async () => {
      // 关掉会让人以为它执行了,而它什么都没做。
      renderNotebook();
      // 「空库」这句话在笔记列表和正文区各有一处,用 findAllByText 避开歧义。
      await screen.findAllByText("No quick notes yet");
      const region = screen.getByRole("region", { name: "Quick Notes" });
      fireEvent.keyDown(region, { key: "k", metaKey: true });
      const input = screen.getByRole("combobox", { name: "Command palette" });

      fireEvent.change(input, { target: { value: "Delete" } });
      fireEvent.keyDown(input, { key: "Enter" });

      expect(screen.getByRole("dialog", { name: "Command palette" })).toBeInTheDocument();
    });

    it("Escape 关掉,并且清掉查询", async () => {
      /* 清查询:下次 ⌘K 是一次新的检索,留着上次的词等于要先删一遍 —— 而那个词
         看起来还像是当前的过滤条件。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const input = await openPalette();
      fireEvent.change(input, { target: { value: "graph" } });

      fireEvent.keyDown(input, { key: "Escape" });
      expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();

      const reopened = await openPalette();
      expect(reopened).toHaveValue("");
    });

    it("⌘K 会把全库搜索收掉", async () => {
      /* 命令面板 z-index 最高,不收的话下面那层还在接键盘事件 —— Escape 会一次
         关掉两层,而用户只看得见最上面这层。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "F", shiftKey: true, metaKey: true });
      expect(screen.getByRole("dialog", { name: "Search all notes" })).toBeInTheDocument();

      fireEvent.keyDown(content, { key: "k", metaKey: true });

      expect(screen.queryByRole("dialog", { name: "Search all notes" })).not.toBeInTheDocument();
      expect(screen.getByRole("dialog", { name: "Command palette" })).toBeInTheDocument();
    });

    it("打开过的笔记进「最近」,空查询时列在命令后面", async () => {
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\na\n');
      harness.seed("Beta.md", '---\ntitle: "Beta"\n---\n\nb\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      // 切到 Beta,它就该进最近名单。
      // 精确名字:标签页和列表行都叫 Beta,`/Beta/` 会同时命中两个。
      fireEvent.click(screen.getAllByRole("button", { name: "Beta" })[0] as HTMLElement);
      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("Beta"),
      );

      const content = screen.getByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "k", metaKey: true });

      expect(optionLabels().some((label) => label.includes("Beta"))).toBe(true);
    });

    it("最近名单按 vault 相对路径落盘", async () => {
      /* 存绝对路径的话,vault 换个位置整份名单会静默失效。这条盯的是键的形状 ——
         它是 localStorage 里的数据,改坏了没有任何报错。 */
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\na\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      await waitFor(() => {
        const raw = localStorage.getItem(`aeroric:notebookRecents:${HARNESS_VAULT}`);
        expect(raw).not.toBeNull();
        expect(JSON.parse(raw ?? "[]")).toContain("Alpha.md");
      });
    });
  });
});
