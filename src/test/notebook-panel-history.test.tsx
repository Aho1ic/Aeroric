/* NotebookPanel — 版本历史 / 回收站 / 笔记列表右键菜单
 *
 * 从原来 7374 行的 notebook-panel.test.tsx 拆出来的一份。拆分理由是并行度:
 * vitest 按文件并行,379 个测试挤在一个文件里只能占一个核,跑 14 分 45 秒。
 * 共用的渲染 / 编辑器辅助在 ./notebookPanelKit,断言与行为逐字未改。 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NotebookVaultHarness } from "./notebookVaultHarness";
import { registerAppDialogHandler, resetAppDialogHandlerForTests } from "../lib/appDialog";
import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";
import { createNote, editorValue, renderNotebook, setEditorValue } from "./notebookPanelKit";

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
});
