/* NotebookPanel — 阅读态勾选任务 / 任务收集箱 / ```notebook-query 查询块 / 看板视图
 *
 * 从原来 7374 行的 notebook-panel.test.tsx 拆出来的一份。拆分理由是并行度:
 * vitest 按文件并行,379 个测试挤在一个文件里只能占一个核,跑 14 分 45 秒。
 * 共用的渲染 / 编辑器辅助在 ./notebookPanelKit,断言与行为逐字未改。 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NotebookVaultHarness } from "./notebookVaultHarness";
import { editorValue, editorView, renderNotebook, setEditorValue } from "./notebookPanelKit";

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

describe("NotebookPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    harness = new NotebookVaultHarness();
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

  describe("看板视图(frontmatter view: kanban)", () => {
    const BOARD = [
      "---",
      'title: "Plan"',
      "view: kanban",
      "---",
      "",
      "## 待办",
      "- [ ] 写周报",
      "- [ ] 修 bug",
      "",
      "## 完成",
      "- [x] 开周会",
      "",
    ].join("\n");

    async function openBoard(title = "Plan") {
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: title }));
      await screen.findByDisplayValue(title);
      fireEvent.click(screen.getByRole("button", { name: "Read" }));
      return waitFor(() => {
        const board = document.querySelector(".notebook-kanban");
        if (!board) throw new Error("board not rendered yet");
        return board;
      });
    }

    /** 看板上的复选框,按 DOM 顺序。 */
    function cards(): HTMLInputElement[] {
      return Array.from(
        document.querySelectorAll<HTMLInputElement>('.notebook-kanban input[type="checkbox"]'),
      );
    }

    /* 列必须在**板内**找。面板根节点本身是 `<section aria-label="Quick Notes">`,
       也是一个 region,而且在文档序里排在最前 —— `screen.getAllByRole("region")[0]`
       拿到的是整个面板。 */
    function boardColumns(): HTMLElement[] {
      const board = document.querySelector(".notebook-kanban-board");
      if (!board) throw new Error("board not rendered");
      return Array.from(board.querySelectorAll<HTMLElement>(".notebook-kanban-col"));
    }

    it("阅读态渲染看板而不是 Markdown 预览", async () => {
      harness.seed("Plan.md", BOARD);
      await openBoard();

      expect(document.querySelector(".notebook-markdown-preview")).toBeNull();
      expect(screen.getByRole("heading", { name: "待办" })).toBeTruthy();
      expect(screen.getByRole("heading", { name: "完成" })).toBeTruthy();
      expect(screen.getByText("1 of 3 done · 33%")).toBeTruthy();
    });

    it("没写 view 的笔记照旧走 Markdown 预览", async () => {
      harness.seed("Plan.md", '---\ntitle: "Plan"\n---\n\n## 待办\n- [ ] 写周报\n');
      renderNotebook();
      fireEvent.click(await screen.findByRole("button", { name: "Plan" }));
      await screen.findByDisplayValue("Plan");
      fireEvent.click(screen.getByRole("button", { name: "Read" }));

      await waitFor(() =>
        expect(document.querySelector(".notebook-markdown-preview")).not.toBeNull(),
      );
      expect(document.querySelector(".notebook-kanban")).toBeNull();
    });

    it("view 的值大小写不敏感", async () => {
      harness.seed("Plan.md", BOARD.replace("view: kanban", "View: Kanban"));
      await openBoard();
      expect(screen.getByRole("heading", { name: "待办" })).toBeTruthy();
    });

    it("勾一张卡片,源码那一行真的被勾上", async () => {
      const planPath = harness.seed("Plan.md", BOARD);
      await openBoard();

      await waitFor(() => expect(cards()).toHaveLength(3));
      fireEvent.click(cards()[0]!);

      await waitFor(() => expect(cards()[0]?.checked).toBe(true));
      await waitFor(() => expect(harness.read(planPath)).toContain("- [x] 写周报"));
      // 没动别人那两行。
      expect(harness.read(planPath)).toContain("- [ ] 修 bug");
      expect(harness.read(planPath)).toContain("- [x] 开周会");
    });

    it("勾第二张只改第二行", async () => {
      const planPath = harness.seed("Plan.md", BOARD);
      await openBoard();

      await waitFor(() => expect(cards()).toHaveLength(3));
      fireEvent.click(cards()[1]!);

      await waitFor(() => expect(harness.read(planPath)).toContain("- [x] 修 bug"));
      expect(harness.read(planPath)).toContain("- [ ] 写周报");
    });

    it("在某列添加任务,写到那一列末尾", async () => {
      const planPath = harness.seed("Plan.md", BOARD);
      await openBoard();

      fireEvent.click(within(boardColumns()[0]!).getByRole("button", { name: "+ Add task" }));
      const input = screen.getByRole("textbox", { name: "New task in 待办" });
      fireEvent.change(input, { target: { value: "新任务 #work" } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() => {
        const saved = harness.read(planPath) ?? "";
        if (!saved.includes("新任务")) throw new Error("not saved yet");
        // 插在「修 bug」之后、「## 完成」之前 —— 不是文末。
        expect(saved).toContain("- [ ] 修 bug\n- [ ] 新任务 #work\n\n## 完成");
      });
      // 写回之后看板重画,新卡片在第一列。
      await waitFor(() => expect(cards()).toHaveLength(4));
      expect(within(boardColumns()[0]!).getByText("新任务")).toBeTruthy();
    });

    it("切回源码态能看到原文,看板不霸占其他视图", async () => {
      harness.seed("Plan.md", BOARD);
      await openBoard();

      fireEvent.click(screen.getByRole("button", { name: "Source" }));
      await waitFor(() => expect(editorValue()).toContain("## 待办"));
      expect(document.querySelector(".notebook-kanban")).toBeNull();
    });

    it("没有列时给出写法说明", async () => {
      harness.seed(
        "Plan.md",
        ["---", 'title: "Plan"', "view: kanban", "---", "", "就是一段正文", ""].join("\n"),
      );
      await openBoard();
      expect(screen.getByText("This board has no columns yet.")).toBeTruthy();
    });
  });
});
