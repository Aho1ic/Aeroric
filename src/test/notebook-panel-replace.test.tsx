/* NotebookPanel — 全库替换 —— 单独一个文件:这一组 15 个测试占 153.5s,是全部 8 个文件里最慢的一箱
 *
 * 从原来 7374 行的 notebook-panel.test.tsx 拆出来的一份。拆分理由是并行度:
 * vitest 按文件并行,379 个测试挤在一个文件里只能占一个核,跑 14 分 45 秒。
 * 共用的渲染 / 编辑器辅助在 ./notebookPanelKit,断言与行为逐字未改。 */
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HARNESS_VAULT, NotebookVaultHarness } from "./notebookVaultHarness";
import { editorValue, renderNotebook, setEditorValue } from "./notebookPanelKit";

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

  describe("全库替换", () => {
    async function openGlobalSearch() {
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "F", shiftKey: true, metaKey: true });
      return screen.getByRole("textbox", { name: "Search all notes" });
    }

    /** 填查询 + 填替换文本,然后点预览。 */
    function preview(query: string, replacement: string) {
      const search = screen.getByRole("textbox", { name: "Search all notes" });
      fireEvent.change(search, { target: { value: query } });
      const replace = screen.getByRole("textbox", { name: "Replace across all notes" });
      fireEvent.change(replace, { target: { value: replacement } });
      fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    }

    const applyButton = () => screen.getByRole("button", { name: "Replace all" });

    /* 替换条自己的状态行。对话框里有两条状态行(搜索一条、替换一条),按 `aria-live`
       取会拿到文档顺序里的第一个 —— 那是搜索那条。替换那条是 `role="status"`,搜索
       那条只有裸 `aria-live`,所以按 role 取正好只命中替换。 */
    function replaceStatus(): string {
      const dialog = screen.getByRole("dialog", { name: "Search all notes" });
      return within(dialog).getByRole("status").textContent ?? "";
    }

    it("预览列出全库命中,落笔后真的写进磁盘", async () => {
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\ncat sat\n还有 cat\n');
      harness.seed("Beta.md", '---\ntitle: "Beta"\n---\n\none cat\n');
      harness.seed("Gamma.md", '---\ntitle: "Gamma"\n---\n\nno match\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 3 matches in 2 notes"));

      fireEvent.click(applyButton());
      await waitFor(() => expect(replaceStatus()).toContain("Replaced 3 matches in 2 notes"));

      expect(harness.read(`${HARNESS_VAULT}/Alpha.md`)).toBe(
        '---\ntitle: "Alpha"\n---\n\ndog sat\n还有 dog\n',
      );
      expect(harness.read(`${HARNESS_VAULT}/Beta.md`)).toBe('---\ntitle: "Beta"\n---\n\none dog\n');
      // 没命中的那篇一个字节都不该动。
      expect(harness.read(`${HARNESS_VAULT}/Gamma.md`)).toBe(
        '---\ntitle: "Gamma"\n---\n\nno match\n',
      );
    });

    it("不预览就不能落笔", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      /* 全库替换一次能改几十个文件,而且不进撤销栈(改的是磁盘)。必须先看过命中
         才允许落笔 —— 这是这块界面唯一的安全阀。 */
      expect(applyButton()).toBeDisabled();
      fireEvent.click(applyButton());
      await waitFor(() =>
        expect(harness.read(`${HARNESS_VAULT}/Doc.md`)).toBe('---\ntitle: "Doc"\n---\n\ncat\n'),
      );
    });

    it("勾掉的文件一个字节都不改", async () => {
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\ncat sat\n');
      harness.seed("Beta.md", '---\ntitle: "Beta"\n---\n\none cat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 2 matches in 2 notes"));

      fireEvent.click(screen.getByRole("checkbox", { name: "Include Beta.md in the replacement" }));
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());

      await waitFor(() => expect(replaceStatus()).toContain("Replaced 1 matches in 1 notes"));
      expect(harness.read(`${HARNESS_VAULT}/Alpha.md`)).toContain("dog sat");
      expect(harness.read(`${HARNESS_VAULT}/Beta.md`)).toBe('---\ntitle: "Beta"\n---\n\none cat\n');
    });

    it("不碰 vault 私有目录里的 .md", async () => {
      /* 回收站和历史快照里放的也是 `.md`,而后端遍历时只跳 `.git` / `node_modules` /
         `dist` / `target`。不传排除模式的话「全库替换」会把已删除的笔记和历史版本
         一起改写 —— 历史版本被改写之后,回滚就再也拿不回替换前的正文了。 */
      harness.seed("Alpha.md", '---\ntitle: "Alpha"\n---\n\ncat sat\n');
      harness.seed(".notebook/trash/Gone.md", "cat in the trash\n");
      harness.seed(".notebook/history/Alpha/1.md", "cat in a snapshot\n");
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());
      await waitFor(() => expect(replaceStatus()).toContain("Replaced 1 matches in 1 notes"));

      expect(harness.read(`${HARNESS_VAULT}/.notebook/trash/Gone.md`)).toBe("cat in the trash\n");
      expect(harness.read(`${HARNESS_VAULT}/.notebook/history/Alpha/1.md`)).toBe(
        "cat in a snapshot\n",
      );
    });

    it("落笔后当前这篇的编辑器跟着换成新正文", async () => {
      /* 替换是后端直接改文件,内存里那份还是旧正文。不重读的话下一次自动保存会把
         旧正文整篇写回去,替换静默消失。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat sat\n');
      renderNotebook();
      await screen.findByDisplayValue("Doc");
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());
      await waitFor(() => expect(replaceStatus()).toContain("Replaced 1 matches"));

      fireEvent.keyDown(screen.getByRole("dialog", { name: "Search all notes" }), {
        key: "Escape",
      });
      await waitFor(() => expect(editorValue()).toBe("dog sat\n"));
    });

    it("落笔后接着打字,不会被延迟的外部更新覆盖", async () => {
      /* 和回滚那条同一个危险:`@uiw/react-codemirror` 对外部 value 变化有一道「打字闩」——
         本地刚改过文档的 200ms 内,外部更新存进 pendingUpdate 等闩到期,而那个闭包
         **捕获了当时的 value**。于是"替换落笔 → 立刻接着打字"会在闩到期时把用户刚打的字
         换成替换后的正文,静默丢编辑。面板靠落笔时 bump `editorEpoch` 重建编辑器绕开它。

         只断言"编辑器换成了新正文"是不够的 —— 闩没点起来时受控 value 自己就会生效,
         去掉重建照样能过。必须先点起闩,再在闩的窗口内打字,然后等它过期。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat sat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));

      /* 先本地改一次,把闩点起来。要在命中**之后**追加 —— 那样不动 `cat` 的偏移和字节,
         落笔前的 settleSave 把这次编辑冲到盘上之后乐观锁仍然对得上,替换才会真的落笔
         (落不了笔就不会重读、不会 bump epoch,这条测试也就测不到东西)。 */
      setEditorValue("cat sat!\n");
      fireEvent.click(applyButton());
      await waitFor(() => expect(replaceStatus()).toContain("Replaced 1 matches"));

      // 闩的窗口内接着打字。
      setEditorValue("typed right after the replace\n");

      // 等到闩肯定过期,确认那段字还在 —— 被覆盖的话这里会变回 "dog sat!"。
      await new Promise((resolve) => setTimeout(resolve, 1200));
      expect(editorValue()).toBe("typed right after the replace\n");
    });

    it("命中 frontmatter 里的标题时标题跟着变", async () => {
      /* 替换按整个文件的偏移走,frontmatter 也在范围里 —— 而标题就存在那儿。只把
         `body` 换掉的写法会让标题框停在旧标题上,下一次保存再把旧标题写回去。 */
      harness.seed("Doc.md", '---\ntitle: "cat notes"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByDisplayValue("cat notes");
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());

      await waitFor(() => expect(screen.getByDisplayValue("dog notes")).toBeInTheDocument());
    });

    it("先把内存里没落盘的编辑等落完再预览", async () => {
      /* 后端读的是磁盘。内存里改了没落盘时两边不是同一份正文,预览会按旧文本给出
         偏移 —— 那些偏移拿去落笔,乐观锁会全部对不上(好一点),或者落在错的位置
         (坏一点)。这条从"编辑器里刚打的字"出发,它只有 settleSave 之后才在盘上。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nplain\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      setEditorValue("cat sat\n");
      await openGlobalSearch();

      preview("cat", "dog");

      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());
      await waitFor(() =>
        expect(harness.read(`${HARNESS_VAULT}/Doc.md`)).toBe('---\ntitle: "Doc"\n---\n\ndog sat\n'),
      );
    });

    it("预览之后又改了正文,落笔前把那次编辑等落完", async () => {
      /* 这是最危险的一条,而且失败起来是静默的。
         面板盖住编辑器,但它是 absolute 覆盖、不卸载编辑器,所以预览之后正文仍然可能
         被改(命令面板、快捷键等入口都还在)。

         落笔前**不**等落盘的话:磁盘上还是旧正文 → 乐观锁比对通过 → 替换写进磁盘 →
         随后那条挂起的自动保存到期,把内存里(改过、但没有替换)的整篇正文写回去,
         替换静默消失。
         等落盘之后:磁盘已经是用户改过的新正文,那几处偏移对不上原文,后端跳过并如实
         报 skipped —— 用户看得见"这次没改成",而不是以为改好了。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat sat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));

      /* 要在命中**之前**插字,不能在后面追加 —— 追加不改变 `cat` 的偏移,也不改变那
         三个字节,乐观锁本来就该通过。那样测的是"锁没坏",不是"等没等落盘"。 */
      setEditorValue("前面加了一句\ncat sat\n");
      fireEvent.click(applyButton());

      await waitFor(() => expect(replaceStatus()).toContain("1 skipped"));
      // 用户那次编辑必须在盘上 —— 它是被 settleSave 冲下去的。
      expect(harness.read(`${HARNESS_VAULT}/Doc.md`)).toBe(
        '---\ntitle: "Doc"\n---\n\n前面加了一句\ncat sat\n',
      );
    });

    it("中文正文按字节偏移替换,不写坏", async () => {
      /* 预览给的是**字节**偏移。前端若拿 `String.length`(UTF-16 码元)重算一次,
         中文笔记上就会切在字符中间,写出乱码 —— 而且是静默的。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\n中文里的 cat 和后面的字\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      preview("cat", "猫");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());

      await waitFor(() =>
        expect(harness.read(`${HARNESS_VAULT}/Doc.md`)).toBe(
          '---\ntitle: "Doc"\n---\n\n中文里的 猫 和后面的字\n',
        ),
      );
    });

    it("预览之后文件被外部改过,那几处跳过而不是写坏", async () => {
      /* 乐观锁的验收点。预览之后文件在外面被改了,偏移就不再指向原来那段文本 ——
         照旧偏移写下去会把无关内容切掉。后端比对命中处原文,对不上就跳过。 */
      const path = harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat sat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      harness.externalWrite(path, '---\ntitle: "Doc"\n---\n\n完全不一样的内容\n');

      fireEvent.click(applyButton());

      await waitFor(() => expect(replaceStatus()).toContain("1 skipped"));
      expect(harness.read(path)).toBe('---\ntitle: "Doc"\n---\n\n完全不一样的内容\n');
    });

    it("落笔后预览清掉,不能拿过期偏移再点一次", async () => {
      /* 落笔之后所有偏移都变了。留着预览的话再点一次「全部替换」会拿旧偏移去写,
         而乐观锁只在原文恰好不同时才拦得住 —— 「cat→cat dog」这种就拦不住。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat sat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());
      await waitFor(() => expect(replaceStatus()).toContain("Replaced 1 matches"));

      expect(applyButton()).toBeDisabled();
      expect(screen.queryByRole("checkbox", { name: /Include Doc\.md/ })).not.toBeInTheDocument();
    });

    it("查询为空时预览点不动", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\ncat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      expect(screen.getByRole("button", { name: "Preview" })).toBeDisabled();
    });

    it("替换成空串就是删掉命中", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nremove cat here\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      // 空串是合法的替换目标,不能被当成"还没填"而拦下来。
      preview("cat", "");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());

      await waitFor(() =>
        expect(harness.read(`${HARNESS_VAULT}/Doc.md`)).toBe(
          '---\ntitle: "Doc"\n---\n\nremove  here\n',
        ),
      );
    });

    it("搜索条件跟着生效", async () => {
      /* 替换用的是搜索那一行的条件(大小写 / 全词 / 正则)。不传的话「区分大小写」
         开着也照样改小写命中,而用户看到的预览是按开着算的。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nCat and cat\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      await openGlobalSearch();

      fireEvent.click(screen.getByRole("button", { name: "Match case" }));
      preview("cat", "dog");
      await waitFor(() => expect(replaceStatus()).toContain("Will replace 1 matches in 1 notes"));
      fireEvent.click(applyButton());

      await waitFor(() =>
        expect(harness.read(`${HARNESS_VAULT}/Doc.md`)).toBe(
          '---\ntitle: "Doc"\n---\n\nCat and dog\n',
        ),
      );
    });
  });
});
