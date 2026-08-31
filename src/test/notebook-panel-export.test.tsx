/* 面板把导出接对了没有。
 *
 * 和另外三组的分工:`notebook-export-flow.test.ts` 验每条通道自己(渲染、内联图、
 * 落盘调用),`notebook-export-run.test.ts` 验结果文案,`notebook-export-sheet.test.tsx`
 * 验窗的画法。这边只验**接线**:命令面板里能不能搜到、点下去有没有真的落盘、正文取的
 * 是内存里那份还是磁盘上那份、整库导出的产物长什么样。
 *
 * 单开一个文件而不是塞进 `notebook-panel.test.tsx`:那个文件已经六千多行,而且有一条
 * 已知的顺序相关失败还没定位完 —— 新用例混进去分不清是谁的问题。
 */

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EditorView } from "@uiw/react-codemirror";

import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";
import { NotebookVaultHarness } from "./notebookVaultHarness";

let harness: NotebookVaultHarness;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: Record<string, unknown>) =>
    harness.handle(command, args ?? {}),
}));

/** 保存对话框选的路径。null = 用户取消。 */
let savePath: string | null = "/Users/me/Desktop/out.html";
/** 选目录对话框选的目录。null = 用户取消。 */
let openDir: string | null = "/Users/me/Desktop/site";
const saveCalls: unknown[] = [];

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: async (options?: unknown) => {
    saveCalls.push(options);
    return savePath;
  },
  open: async () => openDir,
}));

/** 剪贴板写侧。导出用的是 Tauri 插件那条(不是 navigator.clipboard)。 */
const clipboardWrites: string[] = [];
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: async () => clipboardWrites[clipboardWrites.length - 1] ?? "",
  writeText: async (text: string) => {
    clipboardWrites.push(text);
  },
}));

function renderNotebook() {
  return render(
    <I18nProvider>
      <NotebookPanel />
    </I18nProvider>,
  );
}

/** ⌘K 唤出命令面板,返回输入框。 */
async function openPalette() {
  const content = await screen.findByRole("textbox", { name: "Quick note content" });
  fireEvent.keyDown(content, { key: "k", metaKey: true });
  return screen.getByRole("combobox", { name: "Command palette" });
}

/** 从命令面板跑一条命令:打字过滤 → 回车。 */
async function runCommand(query: string) {
  const input = await openPalette();
  fireEvent.change(input, { target: { value: query } });
  fireEvent.keyDown(input, { key: "Enter" });
}

function optionLabels(): string[] {
  const list = screen.getByRole("listbox", { name: "Command palette" });
  return within(list)
    .getAllByRole("option")
    .map((option) => option.textContent ?? "");
}

/* 按钮文案取自 en 词条,而不是在测试里另写一份 —— 改了文案就该在这里显式地改,
   而不是让用例悄悄按旧文案找不到节点然后报"没有这个按钮"。 */
const HTML_LABEL = "Export as single-file HTML";
const MD_LABEL = "Export as Markdown";
const COPY_HTML_LABEL = "Copy as HTML";
const COPY_MD_LABEL = "Copy as Markdown";
const SITE_LABEL = "Export the whole library as a static site";

function exportDialog(): HTMLElement {
  return screen.getByRole("dialog", { name: "Export" });
}

describe("面板接导出", () => {
  beforeEach(() => {
    localStorage.clear();
    harness = new NotebookVaultHarness();
    savePath = "/Users/me/Desktop/out.html";
    openDir = "/Users/me/Desktop/site";
    saveCalls.length = 0;
    clipboardWrites.length = 0;
  });

  it("命令面板里搜得到导出", async () => {
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
    renderNotebook();
    const input = await openPalette();

    fireEvent.change(input, { target: { value: "export" } });

    const labels = optionLabels();
    expect(labels.some((label) => label.includes("Export as PDF"))).toBe(true);
    expect(labels.some((label) => label.includes(SITE_LABEL))).toBe(true);
  });

  it("搜「pdf」直接命中,不用先开导出面板再找一遍", async () => {
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
    renderNotebook();
    const input = await openPalette();

    fireEvent.change(input, { target: { value: "pdf" } });

    expect(optionLabels().some((label) => label.includes("Export as PDF"))).toBe(true);
  });

  it("从命令面板开导出窗", async () => {
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
    renderNotebook();

    await runCommand("Open the export panel");

    expect(await screen.findByRole("dialog", { name: "Export" })).toBeInTheDocument();
  });

  it("导出 HTML:落盘内容带样式和正文", async () => {
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\n# Heading\n\n**bold**\n');
    renderNotebook();
    await screen.findByRole("textbox", { name: "Quick note content" });

    fireEvent.click(within(await openExport()).getByText(HTML_LABEL));

    await waitFor(() => expect(harness.exportWrites).toHaveLength(1));
    const written = harness.exportWrites[0]!;
    expect(written.path).toBe("/Users/me/Desktop/out.html");
    expect(written.content).toContain("<h1");
    expect(written.content).toContain("<strong>bold</strong>");
    // 独立文件:样式内联,不引外部资源。
    expect(written.content).toContain("<style>");
    expect(written.content).not.toContain("<link");
  });

  it("导出成功后报出落盘路径", async () => {
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(HTML_LABEL));

    await waitFor(() =>
      expect(within(exportDialog()).getByRole("status").textContent).toContain(
        "/Users/me/Desktop/out.html",
      ),
    );
  });

  it("保存对话框的默认文件名来自标题", async () => {
    harness.seed("Doc.md", '---\ntitle: "Release notes"\n---\n\nbody\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(HTML_LABEL));

    await waitFor(() => expect(saveCalls).toHaveLength(1));
    expect(JSON.stringify(saveCalls[0])).toContain("Release notes.html");
  });

  it("对话框取消:不落盘,也不报错", async () => {
    savePath = null;
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(HTML_LABEL));

    await waitFor(() => expect(saveCalls).toHaveLength(1));
    expect(harness.exportWrites).toHaveLength(0);
    expect(within(exportDialog()).queryByRole("alert")).toBeNull();
  });

  it("落盘失败报到窗里,不静默", async () => {
    harness.failExportWrite = true;
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(HTML_LABEL));

    await waitFor(() =>
      expect(within(exportDialog()).getByRole("alert").textContent).toContain("failed"),
    );
  });

  it("导出 Markdown:原样落盘,不经渲染", async () => {
    savePath = "/Users/me/Desktop/out.md";
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\n# Heading\n\n- a\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(MD_LABEL));

    await waitFor(() => expect(harness.exportWrites).toHaveLength(1));
    const written = harness.exportWrites[0]!;
    expect(written.content).toContain("# Heading");
    expect(written.content).not.toContain("<h1");
  });

  it("复制为 Markdown 走剪贴板,不落盘", async () => {
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\n# Heading\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(COPY_MD_LABEL));

    await waitFor(() => expect(clipboardWrites).toHaveLength(1));
    expect(clipboardWrites[0]).toContain("# Heading");
    expect(harness.exportWrites).toHaveLength(0);
  });

  it("复制为 HTML 进剪贴板的是渲染后的片段,不带 <html> 外壳", async () => {
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\n**bold**\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(COPY_HTML_LABEL));

    await waitFor(() => expect(clipboardWrites).toHaveLength(1));
    expect(clipboardWrites[0]).toContain("<strong>bold</strong>");
    expect(clipboardWrites[0]).not.toContain("<!doctype");
  });

  it("导出的正文是内存里那份 —— 含还没落盘的编辑", async () => {
    /* 这条盯的是「导出会不会拿磁盘上的旧内容」。用户改了字、自动保存(800ms 防抖)
       还没跑,这时候导出必须是眼下看到的内容,否则导出物和屏幕不一致,而用户完全
       无从察觉。

       判据是 `failPeek` 而不是「自动保存还没跑」:后者要靠 800ms 的时间窗,整套测试
       一起跑的时候机器负载上去,防抖会在导出那几个 await 之间醒来,于是磁盘上也变成
       了新内容 —— 断言照样绿,但已经什么都没验到(踩过一次,全量跑时挂在这条)。
       让读盘直接失败反而是确定的:笔记已经读入过,正确实现根本不会去读盘;真去读了
       就报错,一眼看得出来。 */
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nold body\n');
    renderNotebook();
    const content = await screen.findByRole("textbox", { name: "Quick note content" });
    await waitFor(() => expect(content.textContent).toContain("old body"));

    const view = EditorView.findFromDOM(content as HTMLElement);
    if (!view) throw new Error("CodeMirror view not found");
    act(() => {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "new body" } });
    });

    harness.failPeek = true;
    savePath = "/Users/me/Desktop/out.md";
    await openExport();
    fireEvent.click(screen.getByText(MD_LABEL));

    await waitFor(() => expect(harness.exportWrites).toHaveLength(1));
    expect(harness.exportWrites[0]!.content).toContain("new body");
    expect(harness.exportWrites[0]!.content).not.toContain("old body");
  });

  it("整库导出:每篇一页,外加首页", async () => {
    harness.seed("A.md", '---\ntitle: "A"\n---\n\n# A\n');
    harness.seed("B.md", '---\ntitle: "B"\n---\n\n# B\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(SITE_LABEL));

    await waitFor(() => expect(harness.exportSiteWrites).toHaveLength(3));
    const rels = harness.exportSiteWrites.map((entry) => entry.relPath);
    expect(rels).toContain("A.html");
    expect(rels).toContain("B.html");
    expect(rels).toContain("index.html");
    for (const entry of harness.exportSiteWrites) {
      expect(entry.outDir).toBe("/Users/me/Desktop/site");
    }
  });

  it("整库导出完成后报页数", async () => {
    harness.seed("A.md", '---\ntitle: "A"\n---\n\n# A\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(SITE_LABEL));

    await waitFor(() =>
      expect(within(exportDialog()).getByRole("status").textContent).toContain("Exported 1 pages"),
    );
  });

  it("选目录取消:一页都不写", async () => {
    openDir = null;
    harness.seed("A.md", '---\ntitle: "A"\n---\n\n# A\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(SITE_LABEL));

    // 等按钮解禁 —— 那是这条流程跑完的信号。
    await waitFor(() => expect(siteButton().disabled).toBe(false));
    expect(harness.exportSiteWrites).toHaveLength(0);
  });

  it("站内链接改成相对 .html,不再是 wikilink", async () => {
    harness.seed("A.md", '---\ntitle: "A"\n---\n\nsee [[B]]\n');
    harness.seed("B.md", '---\ntitle: "B"\n---\n\n# B\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(SITE_LABEL));

    await waitFor(() => expect(harness.exportSiteWrites.length).toBeGreaterThanOrEqual(3));
    const pageA = harness.exportSiteWrites.find((entry) => entry.relPath === "A.html");
    expect(pageA?.content).toContain('href="B.html"');
    expect(pageA?.content).not.toContain("[[B]]");
  });

  it("首页列出全部页面", async () => {
    harness.seed("A.md", '---\ntitle: "A"\n---\n\n# A\n');
    harness.seed("B.md", '---\ntitle: "B"\n---\n\n# B\n');
    renderNotebook();
    await openExport();

    fireEvent.click(screen.getByText(SITE_LABEL));

    await waitFor(() => expect(harness.exportSiteWrites).toHaveLength(3));
    const index = harness.exportSiteWrites.find((entry) => entry.relPath === "index.html");
    expect(index?.content).toContain('href="A.html"');
    expect(index?.content).toContain('href="B.html"');
  });

  it("Esc 关掉导出窗", async () => {
    harness.seed("Doc.md", '---\ntitle: "Doc"\n---\n\nbody\n');
    renderNotebook();
    await openExport();

    fireEvent.keyDown(exportDialog(), { key: "Escape" });

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Export" })).toBeNull());
  });
});

/** 开导出窗(走命令面板整条路),返回那个 dialog。 */
async function openExport(): Promise<HTMLElement> {
  await runCommand("Open the export panel");
  return screen.findByRole("dialog", { name: "Export" });
}

function siteButton(): HTMLButtonElement {
  const label = screen.getByText(SITE_LABEL);
  return label.closest("button") as HTMLButtonElement;
}
