/* 附件在面板里的完整走法:粘贴 / 拖入 → 落盘 → 插入正文 → 出现在附件分区。
 *
 * 后端语义由 Rust 侧覆盖(`notebook::tests` 里的附件那一段),这里验的是**接线**:
 * 粘贴事件有没有被接手、markdown 插到了哪、失败了会不会静默、附件分区展开前不扫盘。
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { EditorView } from "@uiw/react-codemirror";
import { I18nProvider } from "../i18n";
import { NotebookPanel } from "../components/notebook/NotebookPanel";
import { NotebookVaultHarness } from "./notebookVaultHarness";

let harness: NotebookVaultHarness;

/* invoke 声明成 async:真实的 Tauri invoke 永远返回 promise,失败是 reject 而不是
   同步抛。写成 `Promise.resolve(harness.handle(...))` 的话 harness 里的 throw 会同步
   逃出去,把「命令失败」变成「渲染期异常」—— 那测的就不是产品代码的降级路径了。 */
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: Record<string, unknown>) =>
    harness.handle(command, args ?? {}),
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: () => Promise.resolve(""),
}));

function renderNotebook() {
  return render(
    <I18nProvider>
      <NotebookPanel />
    </I18nProvider>,
  );
}

async function createNote(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "New quick note" }));
  await screen.findByRole("textbox", { name: "Quick note name" });
}

function editorView(): EditorView {
  const content = screen.getByRole("textbox", { name: "Quick note content" });
  const view = EditorView.findFromDOM(content as HTMLElement);
  if (!view) throw new Error("CodeMirror view not found");
  return view;
}

function editorValue(): string {
  return editorView().state.doc.toString();
}

/** 造一个带图片的 paste 事件。jsdom 的 File 没有 arrayBuffer,补一个。 */
function imageFile(name: string, type = "image/png"): File {
  const file = new File([new Uint8Array([1, 2, 3, 4])], name, { type });
  // jsdom 的 File.arrayBuffer 在某些版本上缺失,补一个可用实现。
  Object.defineProperty(file, "arrayBuffer", {
    value: () => Promise.resolve(new Uint8Array([1, 2, 3, 4]).buffer),
  });
  return file;
}

/**
 * 往编辑器派发一次粘贴。
 *
 * 手工构造 `clipboardData`:jsdom 的 ClipboardEvent 不带 `files`,而 userEvent 的
 * paste 只处理文本。事件要打在 CodeMirror 的 contentDOM 上 —— handler 是通过
 * `EditorView.domEventHandlers` 挂在那里的。
 */
function pasteFiles(files: File[]): void {
  const view = editorView();
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: { files, items: [], getData: () => "" },
  });
  act(() => {
    view.contentDOM.dispatchEvent(event);
  });
}

describe("随手记的附件", () => {
  beforeEach(() => {
    localStorage.clear();
    harness = new NotebookVaultHarness();
  });

  it("粘贴图片会落盘并在光标处插入 markdown", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note content" }), "before");

    pasteFiles([imageFile("shot.png")]);

    await waitFor(() => expect(harness.attachments).toHaveLength(1));
    // 插在光标处(打完字光标在末尾),而且是图片语法。
    await waitFor(() => expect(editorValue()).toMatch(/^before!\[shot\]\(attachments\/.+\.png\)$/));
  });

  it("粘贴多张图之间空一行", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);

    pasteFiles([imageFile("a.png"), imageFile("b.png")]);

    await waitFor(() => expect(harness.attachments).toHaveLength(2));
    // 紧挨着写的话 markdown 会把两张图渲染进同一个段落,挤在一行里。
    await waitFor(() => expect(editorValue()).toMatch(/\)\n\n!\[/));
  });

  it("粘贴纯文本不走附件那条路", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);

    pasteFiles([]);

    // 不过滤的话复制粘贴文字会莫名多出一张图(有些应用会在 dataTransfer 里塞
    // 一份 HTML 快照的文件)。
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(harness.attachments).toHaveLength(0);
  });

  it("一次粘太多会截断并说明跳过了几张", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);

    pasteFiles(Array.from({ length: 12 }, (_, index) => imageFile(`a${index}.png`)));

    await waitFor(() => expect(harness.attachments).toHaveLength(10));
    // 静默截断的话用户以为十二张都插进去了,实际少了两张。
    expect(await screen.findByText(/skipped 2/)).toBeInTheDocument();
  });

  it("多张图里有一张失败时,成功的照样插进去且错误可见", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    harness.failAttachmentSaves(1);

    pasteFiles([imageFile("a.png"), imageFile("b.png")]);

    // 第一张就失败,循环中断 —— 但已经存下来的(这里是 0 张)不该丢,而错误必须
    // 说出来:静默的话用户会以为图插进去了。
    expect(await screen.findByText(/saving the attachment failed/)).toBeInTheDocument();
    expect(harness.attachments).toHaveLength(0);
  });

  it("附件分区展开前不扫盘,展开后列出附件", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    pasteFiles([imageFile("shot.png")]);
    await waitFor(() => expect(harness.attachments).toHaveLength(1));

    // 折叠态一次都没列过:扫的是整个 vault,图多的仓库这一下不便宜。
    expect(screen.queryByText(/attachments$/i)).not.toBeNull();
    const toggle = screen.getByRole("button", { name: "Attachments in this notebook" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    await user.click(toggle);

    const name = harness.attachments[0]!.name;
    expect(
      await screen.findByRole("button", { name: `Show ${name} in the file manager` }),
    ).toBeInTheDocument();
  });

  it("从附件分区插入的链接相对笔记目录", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    pasteFiles([imageFile("shot.png")]);
    await waitFor(() => expect(harness.attachments).toHaveLength(1));
    // 先把正文清空,好断言插进去的就是那一段。
    const view = editorView();
    act(() => {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "" } });
    });

    await user.click(screen.getByRole("button", { name: "Attachments in this notebook" }));
    const name = harness.attachments[0]!.name;
    await user.click(screen.getByRole("button", { name: `Insert ${name} into the note` }));

    // 笔记在 vault 根下,所以没有 `../`。子目录的情形由 linkFromNote 的单测覆盖。
    expect(editorValue()).toBe(`![${name}](attachments/${name})`);
  });

  it("附件列不出来时报错而不是显示成空", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    harness.failAttachmentList = true;

    await user.click(screen.getByRole("button", { name: "Attachments in this notebook" }));

    // 显示成空列表的话用户会以为附件都没了。
    expect(await screen.findByRole("alert")).toHaveTextContent("listing attachments failed");
  });

  it("阅读态把相对路径的图换成能显示的 URL", async () => {
    const user = userEvent.setup();
    const created: string[] = [];
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => {
      const url = `blob:read-${created.length + 1}`;
      created.push(url);
      return url;
    });
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    try {
      renderNotebook();
      await createNote(user);
      pasteFiles([imageFile("shot.png")]);
      await waitFor(() => expect(harness.attachments).toHaveLength(1));

      await user.click(screen.getByRole("button", { name: "Read" }));

      // 不换的话浏览器会按 `tauri://localhost/attachments/x.png` 去取,永远 404
      // —— 而页面上只是一个空白框,不报任何错。
      await waitFor(() => {
        const img = document.querySelector<HTMLImageElement>(".notebook-markdown-preview img");
        expect(img?.getAttribute("src")).toBe("blob:read-1");
      });
      expect(harness.attachmentReads).toHaveLength(1);
    } finally {
      vi.restoreAllMocks();
    }
  });

  it("阅读态也认 title 里的宽度标注", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    const view = editorView();
    act(() => {
      view.dispatch({
        changes: { from: 0, insert: '![a](https://example.com/a.png "width=320")\n' },
      });
    });

    await user.click(screen.getByRole("button", { name: "Read" }));

    // 编辑态的 widget 自己会按宽度缩放,阅读态没人做这件事 —— 于是同一张图切一下
    // 视图就从 320px 变成撑满整行。
    await waitFor(() => {
      const img = document.querySelector<HTMLImageElement>(".notebook-markdown-preview img");
      expect(img?.style.width).toBe("320px");
    });
  });

  it("拖入图片插到落点而不是原来的光标处", async () => {
    const user = userEvent.setup();
    renderNotebook();
    await createNote(user);
    await user.type(screen.getByRole("textbox", { name: "Quick note content" }), "abcdef");
    const view = editorView();
    // 光标停在末尾,但拖放要插到文档开头。
    act(() => {
      view.dispatch({ selection: { anchor: 6 } });
    });
    vi.spyOn(view, "posAtCoords").mockReturnValue(0);

    const event = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "dataTransfer", {
      value: { files: [imageFile("drop.png")], items: [], getData: () => "" },
    });
    Object.defineProperty(event, "clientX", { value: 10 });
    Object.defineProperty(event, "clientY", { value: 10 });
    act(() => {
      view.contentDOM.dispatchEvent(event);
    });

    await waitFor(() => expect(harness.attachments).toHaveLength(1));
    // 插到光标处的话图会跑到 `abcdef` 后面 —— 用户拖到哪就该插到哪。
    await waitFor(() => expect(editorValue()).toMatch(/^!\[drop\]\(attachments\/.+\)abcdef$/));
  });
});
