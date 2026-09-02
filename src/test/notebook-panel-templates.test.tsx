/* NotebookPanel — 日记与模板 / 快速捕获 / 自定义模板
 *
 * 从原来 7374 行的 notebook-panel.test.tsx 拆出来的一份。拆分理由是并行度:
 * vitest 按文件并行,379 个测试挤在一个文件里只能占一个核,跑 14 分 45 秒。
 * 共用的渲染 / 编辑器辅助在 ./notebookPanelKit,断言与行为逐字未改。 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
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

  describe("日记与模板", () => {
    /** 从命令面板跑一条命令。整条路都走:⌘K → 打字 → 回车。 */
    async function runCommand(query: string) {
      const region = screen.getByRole("region", { name: "Quick Notes" });
      fireEvent.keyDown(region, { key: "k", metaKey: true });
      const input = screen.getByRole("combobox", { name: "Command palette" });
      fireEvent.change(input, { target: { value: query } });
      fireEvent.keyDown(input, { key: "Enter" });
    }

    function pad2(value: number): string {
      return String(value).padStart(2, "0");
    }

    /** 今天的日记路径。测试跟着系统时钟走 —— 面板用的是 `new Date()`。 */
    function todayPath(offsetDays = 0): string {
      const now = new Date();
      const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offsetDays);
      return `${HARNESS_VAULT}/Daily/${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(
        date.getDate(),
      )}.md`;
    }

    it("「今天的日记」建到 Daily/YYYY-MM-DD.md", async () => {
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Today's daily note");

      await waitFor(() => expect(harness.read(todayPath())).toBeDefined());
      const content = harness.read(todayPath()) ?? "";
      // 标题存 frontmatter,文件名是日期 —— 两者一致,`[[2026-08-28]]` 才指得到。
      expect(content).toContain(`title: "${todayPath().slice(-13, -3)}"`);
      expect(content).toContain("## To do");
    });

    it("日记建完就是当前笔记", async () => {
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Today's daily note");

      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue(
          todayPath().slice(-13, -3),
        ),
      );
      expect(editorValue()).toContain("## To do");
    });

    it("再开一次今天的日记不会建第二个文件", async () => {
      /* 后端分配文件名那条路会去重(`2026-08-28-2.md`),日记必须每天恒定一个文件。
         这条钉的就是「路径由前端定 + ALREADY_EXISTS 当正常分支」。 */
      renderNotebook();
      await screen.findAllByText("No quick notes yet");
      await runCommand("Today's daily note");
      await waitFor(() => expect(harness.read(todayPath())).toBeDefined());

      await runCommand("Today's daily note");
      // 第二次没有新文件要等,给它一拍让可能的 IPC 落地。
      await waitFor(() =>
        expect(harness.paths().filter((path) => path.includes("/Daily/"))).toHaveLength(1),
      );
    });

    it("磁盘上已经有今天的日记时读磁盘那份,不拿模板盖掉", async () => {
      /* 用户在别处(同步盘 / 另一个窗口)写过今天的日记,内容不能被模板覆盖。
         `openOrCreateNoteAt` 在 ALREADY_EXISTS 时走 loadNoteByPath —— 这条盯的是
         那个分支。

         文件必须在**挂载之后**才出现在磁盘上:挂载前 seed 的话初次扫盘就把它收进
         列表了,于是 `openDailyNote` 走的是「已经在列表里,只切过去」那条早退,
         ALREADY_EXISTS 这条分支一次都不会执行 —— 测试会因为另一个原因通过。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      harness.externalWrite(todayPath(), "# 手写的\n\n昨天写的内容\n");

      await runCommand("Today's daily note");

      await waitFor(() => expect(editorValue()).toContain("昨天写的内容"));
      expect(editorValue()).not.toContain("## To do");
      expect(harness.read(todayPath())).toBe("# 手写的\n\n昨天写的内容\n");
    });

    it("连点两次也只在列表里出现一条", async () => {
      /* 第一次的 IPC 还没回来时列表还是空的,第二次也过得了「已经在列表里」那道
         早退 —— 两条都会走到入列那一步。没有去重的话列表里会出现两行同一篇日记,
         而它们指向同一个文件。 */
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Today's daily note");
      await runCommand("Today's daily note");

      const name = todayPath().slice(-13, -3);
      await waitFor(() => expect(harness.read(todayPath())).toBeDefined());
      await waitFor(() => {
        const region = screen.getByRole("region", { name: "Quick Notes" });
        expect(within(region).getAllByRole("button", { name })).toHaveLength(1);
      });
    });

    it("日记已经在列表里时只切过去,一趟 IPC 都不发", async () => {
      /* 内容安全**不是**这条早退提供的:去掉它之后建会撞名、转去读磁盘,而入列那步
         的去重又会把读回来的内容丢掉,所以用户打的字照样在。早退真正省下的是那趟
         白跑的 IPC(一次 create + 一次 open),所以这条钉的是调用次数。

         内容那一面另有测试守着(下一条),两者不是同一件事。 */
      renderNotebook();
      await screen.findAllByText("No quick notes yet");
      await runCommand("Today's daily note");
      await waitFor(() => expect(harness.read(todayPath())).toBeDefined());
      await waitFor(() => expect(editorValue()).toContain("## To do"));
      const creates = harness.callCount("notebook_create_note");
      const opens = harness.callCount("notebook_open_note");

      await runCommand("Today's daily note");
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument(),
      );

      expect(harness.callCount("notebook_create_note")).toBe(creates);
      expect(harness.callCount("notebook_open_note")).toBe(opens);
    });

    it("再开一次不会把未落盘的编辑冲掉", async () => {
      renderNotebook();
      await screen.findAllByText("No quick notes yet");
      await runCommand("Today's daily note");
      await waitFor(() => expect(harness.read(todayPath())).toBeDefined());
      await waitFor(() => expect(editorValue()).toContain("## To do"));
      setEditorValue("还没落盘的字\n");

      await runCommand("Today's daily note");

      // 内容被磁盘上那份换掉的话,这里会变回模板正文。
      await waitFor(() => expect(editorValue()).toBe("还没落盘的字\n"));
    });

    it("「前一天」从今天退一天", async () => {
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Previous daily note");

      await waitFor(() => expect(harness.read(todayPath(-1))).toBeDefined());
    });

    it("「后一天」从今天进一天", async () => {
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Next daily note");

      await waitFor(() => expect(harness.read(todayPath(1))).toBeDefined());
    });

    it("在日记上按「前一天」以它为基准,能连着翻", async () => {
      /* 以「今天」为基准的话,连按两次会一直停在昨天。这条按两次,断言落到前天。 */
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Previous daily note");
      await waitFor(() => expect(harness.read(todayPath(-1))).toBeDefined());
      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue(
          todayPath(-1).slice(-13, -3),
        ),
      );

      await runCommand("Previous daily note");

      await waitFor(() => expect(harness.read(todayPath(-2))).toBeDefined());
    });

    it("当前不是日记时「前一天」从今天算", async () => {
      // 名字长得像日期但不在 Daily/ 下的那种,不该被当成基准。
      harness.seed("2020-01-01.md", '---\ntitle: "2020-01-01"\n---\n\nnope\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      await runCommand("Previous daily note");

      await waitFor(() => expect(harness.read(todayPath(-1))).toBeDefined());
    });

    it("模板命令建出带正文的笔记", async () => {
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Meeting notes");

      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note content" })).toBeInTheDocument(),
      );
      await waitFor(() => expect(editorValue()).toContain("## Action items"));
      // 文件名由后端从标题分配,所以这里只断言磁盘上确实多了一个含模板正文的文件。
      await waitFor(() =>
        expect(harness.paths().some((path) => harness.read(path)?.includes("## Agenda"))).toBe(
          true,
        ),
      );
    });

    it("同一个模板用两次得到两个文件", async () => {
      // 一天可以有好几场会。模板走后端分配文件名那条路,撞名自动加序号。
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Meeting notes");
      await waitFor(() => expect(harness.paths()).toHaveLength(1));
      await runCommand("Meeting notes");

      await waitFor(() => expect(harness.paths()).toHaveLength(2));
    });

    it("模板正文里的日期占位符被展开", async () => {
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Meeting notes");

      await waitFor(() => expect(editorValue()).toContain(todayPath().slice(-13, -3)));
      expect(editorValue()).not.toMatch(/\{\w+\}/);
    });

    it("模板能按说明里的词搜到", async () => {
      /* 一行说明进了 keywords:记不住模板叫什么、只记得里面有什么的人也搜得到。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "k", metaKey: true });
      const input = screen.getByRole("combobox", { name: "Command palette" });

      fireEvent.change(input, { target: { value: "three key results" } });

      const list = screen.getByRole("listbox", { name: "Command palette" });
      expect(
        within(list)
          .getAllByRole("option")
          .some((option) => (option.textContent ?? "").includes("Quarterly OKR")),
      ).toBe(true);
    });

    it("模板与日记归在「模板」分组下", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "k", metaKey: true });
      const input = screen.getByRole("combobox", { name: "Command palette" });

      fireEvent.change(input, { target: { value: "Weekly report" } });

      const list = screen.getByRole("listbox", { name: "Command palette" });
      expect(within(list).getAllByRole("option")[0]?.textContent ?? "").toContain("Templates");
    });

    it("新建的日记出现在笔记列表里", async () => {
      /* 日记在子目录下,而列表是平铺的 —— `flattenTree` 会把目录丢掉但保留里面的
         笔记。落在 Daily/ 下之后在面板里看不见的话,这个功能就只是往磁盘写文件。 */
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("Today's daily note");

      const name = todayPath().slice(-13, -3);
      await waitFor(() => {
        const list = screen.getByRole("region", { name: "Quick Notes" });
        expect(within(list).getAllByRole("button", { name }).length).toBeGreaterThan(0);
      });
    });
  });

  describe("快速捕获", () => {
    function pad2(value: number): string {
      return String(value).padStart(2, "0");
    }

    function todayPath(): string {
      const now = new Date();
      return `${HARNESS_VAULT}/Daily/${now.getFullYear()}-${pad2(now.getMonth() + 1)}-${pad2(
        now.getDate(),
      )}.md`;
    }

    const INBOX = `${HARNESS_VAULT}/Inbox.md`;

    /** ⌘⇧K 唤出捕获窗,返回那个 textarea。 */
    function openCapture(): HTMLTextAreaElement {
      const region = screen.getByRole("region", { name: "Quick Notes" });
      fireEvent.keyDown(region, { key: "K", shiftKey: true, metaKey: true });
      return screen.getByRole("textbox", { name: "What to capture" }) as HTMLTextAreaElement;
    }

    /** 打字并提交。 */
    function capture(text: string, target?: "Inbox") {
      const area = openCapture();
      if (target) fireEvent.click(screen.getByRole("radio", { name: target }));
      fireEvent.change(area, { target: { value: text } });
      fireEvent.click(screen.getByRole("button", { name: "Capture" }));
    }

    it("⌘⇧K 开捕获窗", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      openCapture();

      expect(screen.getByRole("dialog", { name: "Quick capture" })).toBeInTheDocument();
    });

    it("⌘⇧K 不会顺带开命令面板", async () => {
      /* ⌘K 的判断不看 shiftKey,排在 ⌘⇧K 前面的话它会把捕获整个吞掉。这条钉的是
         那个顺序。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      openCapture();

      expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();
    });

    it("捕获到今天的日记,没有就按模板建出来", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      capture("记得回邮件");

      await waitFor(() => expect(harness.read(todayPath())).toBeDefined());
      const content = harness.read(todayPath()) ?? "";
      expect(content).toContain("## To do");
      expect(content).toMatch(/## \d{2}:\d{2}\n\n记得回邮件\n$/);
    });

    it("捕获到收集箱,落在单个 Inbox.md 上", async () => {
      /* 收集箱刻意不是一篇篇新笔记:捕获多半是一句话,一句话一篇会让笔记列表在
         一周内变得没法用。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      capture("一个想法", "Inbox");

      await waitFor(() => expect(harness.read(INBOX)).toBeDefined());
      expect(harness.read(INBOX) ?? "").toMatch(/## \d{2}:\d{2}\n\n一个想法\n$/);
      expect(harness.read(todayPath())).toBeUndefined();
    });

    it("第二次捕获追加在后面,不覆盖第一次", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      capture("第一条", "Inbox");
      await waitFor(() => expect(harness.read(INBOX) ?? "").toContain("第一条"));
      capture("第二条", "Inbox");

      await waitFor(() => expect(harness.read(INBOX) ?? "").toContain("第二条"));
      const content = harness.read(INBOX) ?? "";
      expect(content).toContain("第一条");
      expect(content.indexOf("第一条")).toBeLessThan(content.indexOf("第二条"));
    });

    it("捕获不切走当前笔记", async () => {
      /* 捕获的意义就是不打断手上的事。切走会把编辑器的滚动位置和用户的注意力
         一起带到另一篇上。 */
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      capture("一个想法", "Inbox");

      await waitFor(() => expect(harness.read(INBOX)).toBeDefined());
      expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue("Doc");
      expect(editorValue()).toContain("body");
    });

    it("成功后窗自己关掉", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });

      capture("一个想法", "Inbox");

      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Quick capture" })).not.toBeInTheDocument(),
      );
    });

    it("先把目标笔记未落盘的编辑落下去,再追加", async () => {
      /* 追加读的是磁盘。不先落盘的话追加会接在旧正文后面,而这次保存又会把用户刚打的
         字整篇覆盖掉 —— 捕获成功了,当前编辑没了。 */
      harness.seed("Inbox.md", "旧正文\n");
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      // 切到 Inbox 并改内容,先不等它落盘。
      fireEvent.click(screen.getAllByRole("button", { name: "Inbox" })[0] as HTMLElement);
      await waitFor(() => expect(editorValue()).toContain("旧正文"));
      setEditorValue("刚打的字\n");

      capture("捕获的一句", "Inbox");

      await waitFor(() => expect(harness.read(INBOX) ?? "").toContain("捕获的一句"));
      expect(harness.read(INBOX) ?? "").toContain("刚打的字");
      expect(harness.read(INBOX) ?? "").not.toContain("旧正文");
    });

    it("捕获到当前打开的这篇时,编辑器跟着更新", async () => {
      harness.seed("Inbox.md", "旧正文\n");
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.click(screen.getAllByRole("button", { name: "Inbox" })[0] as HTMLElement);
      await waitFor(() => expect(editorValue()).toContain("旧正文"));

      capture("捕获的一句", "Inbox");

      await waitFor(() => expect(editorValue()).toContain("捕获的一句"));
    });

    it("捕获之后接着打的字不会被抹掉", async () => {
      /* CodeMirror 有个 200ms 的输入闩:局部改动后的 200ms 内,外部传进来的 value 会
         被存成一个挂起的更新,而那个闭包捕获的是**当时**的 value。闩过期时它会把用户
         之后打的字盖掉。追加完 bump `editorEpoch` 重建编辑器就是为了扔掉它。

         只断言「编辑器内容变了」抓不到这件事:闩没上膛时受控 value 自己就生效了。 */
      harness.seed("Inbox.md", "旧正文\n");
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.click(screen.getAllByRole("button", { name: "Inbox" })[0] as HTMLElement);
      await waitFor(() => expect(editorValue()).toContain("旧正文"));
      // 上膛:局部改动。追加在末尾,所以捕获仍然读到「旧正文 + 这句」。
      setEditorValue("旧正文\n又加了一句\n");

      capture("捕获的一句", "Inbox");
      /* 不能拿「编辑器内容出现了那句」当等待条件:默认 50ms 一轮的轮询,轮到的时候
         距离上膛往往已经超过 200ms,闩自己过期、挂起的更新在我们打字**之前**就平静
         生效了 —— 于是有没有重建编辑器在 DOM 上没区别。改成等窗关闭(它和 bump 在
         同一个 await 续体里,批到同一次渲染),1ms 一轮,真实时间还远没走到 200ms。 */
      await waitFor(
        () => expect(screen.queryByRole("dialog", { name: "Quick capture" })).toBeNull(),
        {
          interval: 1,
        },
      );
      expect(editorValue()).toContain("捕获的一句");

      setEditorValue("重建之后打的字\n");
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 1200));
      });

      expect(editorValue()).toBe("重建之后打的字\n");
    });

    it("保存失败时窗不关,文字还在", async () => {
      /* 捕获的那句话只存在窗里的 textarea 上,关掉就没了。所以失败必须留在窗里报,
         而不是走面板那条错误提示。 */
      harness.seed("Inbox.md", "旧正文\n");
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      harness.failNextSave = true;

      capture("会失败的一句", "Inbox");

      /* 必须在**窗里**找那条 alert:笔记列表的错误横幅也是 role="alert",不限定范围的
         `getByRole("alert")` 在「错误跑去面板上报了」时同样只找到一个,于是断言通过而
         窗里其实什么都没显示。 */
      const dialog = await screen.findByRole("dialog", { name: "Quick capture" });
      await waitFor(() => expect(within(dialog).getByRole("alert")).toBeInTheDocument());
      // 反过来:面板那边不该同时也报一遍。
      const list = screen.getByRole("region", { name: "Quick Notes" });
      expect(
        within(list)
          .queryAllByRole("alert")
          .filter((node) => !dialog.contains(node)),
      ).toHaveLength(0);
      expect(screen.getByRole("textbox", { name: "What to capture" })).toHaveValue("会失败的一句");
      expect(harness.read(INBOX)).toBe("旧正文\n");
    });

    it("冲突时不覆盖,提示重来", async () => {
      /* 读出基线之后、写回之前磁盘又变了(外部编辑器 / 同步盘)。这时候强写会把别人
         的改动吃掉,所以报错让用户重新捕获一次 —— 那句话还在窗里。 */
      harness.seed("Inbox.md", "旧正文\n");
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      harness.conflictNextSave = true;

      capture("会撞上的一句", "Inbox");

      const dialog = await screen.findByRole("dialog", { name: "Quick capture" });
      await waitFor(() =>
        expect(within(dialog).getByRole("alert")).toHaveTextContent("changed on disk"),
      );
      expect(harness.read(INBOX)).toBe("旧正文\n");
      expect(screen.getByRole("textbox", { name: "What to capture" })).toHaveValue("会撞上的一句");
    });

    it("命令面板里也能唤出捕获", async () => {
      harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
      renderNotebook();
      const content = await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.keyDown(content, { key: "k", metaKey: true });
      const input = screen.getByRole("combobox", { name: "Command palette" });

      fireEvent.change(input, { target: { value: "Quick capture" } });
      fireEvent.keyDown(input, { key: "Enter" });

      expect(await screen.findByRole("dialog", { name: "Quick capture" })).toBeInTheDocument();
      expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();
    });

    it("捕获出来的笔记出现在列表里", async () => {
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      capture("一个想法", "Inbox");

      await waitFor(() => {
        const region = screen.getByRole("region", { name: "Quick Notes" });
        expect(within(region).getAllByRole("button", { name: "Inbox" }).length).toBeGreaterThan(0);
      });
    });

    it("捕获之后那篇笔记的基线是新的,自动保存不会把捕获写回去", async () => {
      /* 不把结果写回内存的话,下一次自动保存会拿改之前的正文整篇覆盖 —— 捕获静默消失
         (和全库替换那边同一个坑)。这条改一次标题触发保存,然后看捕获还在不在。 */
      harness.seed("Inbox.md", "旧正文\n");
      renderNotebook();
      await screen.findByRole("textbox", { name: "Quick note content" });
      fireEvent.click(screen.getAllByRole("button", { name: "Inbox" })[0] as HTMLElement);
      await waitFor(() => expect(editorValue()).toContain("旧正文"));

      capture("捕获的一句", "Inbox");
      await waitFor(() => expect(harness.read(INBOX) ?? "").toContain("捕获的一句"));

      // 改标题会 scheduleSave 整篇。写回去的必须是含捕获的那份。
      fireEvent.change(screen.getByRole("textbox", { name: "Quick note name" }), {
        target: { value: "收集箱" },
      });
      await waitFor(() => expect(harness.read(INBOX) ?? "").toContain("收集箱"));
      expect(harness.read(INBOX) ?? "").toContain("捕获的一句");
    });
  });

  describe("自定义模板", () => {
    /** 从命令面板跑一条命令。 */
    async function runCommand(query: string) {
      const region = screen.getByRole("region", { name: "Quick Notes" });
      fireEvent.keyDown(region, { key: "k", metaKey: true });
      const input = screen.getByRole("combobox", { name: "Command palette" });
      fireEvent.change(input, { target: { value: query } });
      fireEvent.keyDown(input, { key: "Enter" });
    }

    /** 命令面板里当前列出来的候选文本。可以连着调 —— 已经开着就只换查询词。 */
    async function paletteLabels(query: string): Promise<string[]> {
      /* 只在还没开的时候按 ⌘K:那个键是**开关**,对着开着的面板再按一次是关掉,
         于是第二次调用会在一个不存在的输入框上等到超时。 */
      if (screen.queryByRole("combobox", { name: "Command palette" }) === null) {
        const region = screen.getByRole("region", { name: "Quick Notes" });
        fireEvent.keyDown(region, { key: "k", metaKey: true });
      }
      const input = await screen.findByRole("combobox", { name: "Command palette" });
      fireEvent.change(input, { target: { value: query } });
      // `queryAll`:一个命中不到的查询是合法输入(下面那条就靠它),`getAll` 会直接抛。
      return screen.queryAllByRole("option").map((node) => node.textContent ?? "");
    }

    it("磁盘上的模板出现在命令面板里", async () => {
      harness.userTemplates = [
        { id: "duty", title: "值班记录", name: "{{date}} 值班", body: "# {{title}}\n\n## 事件\n" },
      ];
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      expect((await paletteLabels("值班")).join("\n")).toContain("值班记录");
    });

    it("按自定义模板新建:标题展开日期,正文的 title 用最终标题", async () => {
      harness.userTemplates = [
        { id: "duty", title: "值班记录", name: "{{date}} 值班", body: "# {{title}}\n\n## 事件\n" },
      ];
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      await runCommand("值班记录");

      const now = new Date();
      const stamp = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(
        now.getDate(),
      ).padStart(2, "0")}`;
      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Quick note name" })).toHaveValue(
          `${stamp} 值班`,
        ),
      );
      /* `# {{title}}` 是模板最常见的首行,它必须和笔记标题一致 —— 留着占位符或者
         填成空的话,用户看到的是一篇标题栏有名字、正文第一行是 `#` 的笔记。 */
      expect(editorValue()).toContain(`# ${stamp} 值班`);
      expect(editorValue()).not.toContain("{{title}}");
    });

    it("文件名 stem 也能搜到", async () => {
      // 用户记得的可能是文件叫什么,而不是 frontmatter 里写的显示名。
      harness.userTemplates = [{ id: "duty-log", title: "值班记录", name: "值班", body: "body\n" }];
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      expect((await paletteLabels("duty-log")).join("\n")).toContain("值班记录");
    });

    it("读模板失败时只是少几条命令,不弹错误提示", async () => {
      /* 那条提示条是用来说「你的笔记出事了」的。模板读不到最坏只是命令面板里少几条,
         占用它会让用户以为笔记库坏了。 */
      harness.failUserTemplates = true;
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      // 内置模板还在,说明面板本身是好的。
      expect((await paletteLabels("Weekly")).join("\n")).toContain("Weekly");
      const region = screen.getByRole("region", { name: "Quick Notes" });
      expect(within(region).queryByRole("alert")).toBeNull();
    });

    it("没有模板目录时不凭空多出模板命令", async () => {
      /* 绝大多数 vault 没有 `.notebook/templates/`,那是正常状态 —— 空表要真的是空表,
         而不是回落到一份内置的示例清单。 */
      renderNotebook();
      await screen.findAllByText("No quick notes yet");

      expect(await paletteLabels("值班")).toEqual([]);
      // 内置模板照旧在,证明这条不是因为面板整个空了才通过。
      expect((await paletteLabels("Weekly")).join("\n")).toContain("Weekly");
    });
  });
});
