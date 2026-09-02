/* NotebookPanel — 标签(含跨文件重命名) / frontmatter 字段浏览器
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
});
