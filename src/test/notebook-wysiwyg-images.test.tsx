/* 编辑态图片的 src 是**异步**定下来的:vault 里的相对路径要先读成 blob URL。
 *
 * 这一层的坑集中在"widget 还活着吗"的判定上,而那个判定只有在真实的 CodeMirror
 * 挂载时序下才会暴露,所以这些测试挂真编辑器而不是直接 new widget。
 */

import { describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { forceParsing } from "@codemirror/language";
import { EditorView } from "@uiw/react-codemirror";
import { NoteSourceEditor } from "../components/notebook/NoteSourceEditor";
import type { AttachmentContext } from "../components/notebook/wysiwyg";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: () => Promise.resolve(undefined),
}));

type Resolver = {
  context: AttachmentContext;
  /** 每次 resolve 的入参。 */
  calls: { url: string; noteDir: string }[];
  /** 手动放行第 n 次 resolve。 */
  settle: (index: number, value: string) => void;
  reject: (index: number, message: string) => void;
};

function makeResolver(noteDir = "/vault/notes"): Resolver {
  const calls: { url: string; noteDir: string }[] = [];
  const pending: { resolve: (v: string) => void; reject: (e: Error) => void }[] = [];
  return {
    calls,
    context: {
      noteDir,
      resolve: (url, dir) => {
        calls.push({ url, noteDir: dir });
        return new Promise<string>((resolve, reject) => {
          pending.push({ resolve, reject });
        });
      },
    },
    settle: (index, value) => {
      act(() => {
        pending[index]!.resolve(value);
      });
    },
    reject: (index, message) => {
      act(() => {
        pending[index]!.reject(new Error(message));
      });
    },
  };
}

function mount(value: string, attachments?: AttachmentContext) {
  const view = render(
    <NoteSourceEditor
      value={value}
      onChange={() => {}}
      themeVariant="light"
      ariaLabel="Body"
      wysiwyg
      attachments={attachments}
    />,
  );
  const content = screen.getByRole("textbox", { name: "Body" });
  const editor = EditorView.findFromDOM(content as HTMLElement);
  if (!editor) throw new Error("view not found");
  act(() => {
    forceParsing(editor, editor.state.doc.length, 5000);
    editor.dispatch({});
  });
  return { ...view, content, editor };
}

function widgetImg(content: HTMLElement): HTMLImageElement {
  const img = content.querySelector<HTMLImageElement>(".cm-md-img-widget img");
  if (!img) throw new Error("image widget not found");
  return img;
}

describe("编辑态的图片 src", () => {
  it("相对路径先不进 src,解析完才填", async () => {
    const resolver = makeResolver();
    const { content } = mount("![shot](attachments/shot.png)\n", resolver.context);

    // 相对路径**不能**先塞进 src 顶一下:WebView 的 base 是 tauri://localhost,
    // 那次加载注定失败,而 error 事件会立刻打上 error 类 —— 于是解析成功之后图
    // 还是显示成坏的。
    expect(widgetImg(content).getAttribute("src")).toBeNull();
    expect(resolver.calls).toEqual([{ url: "attachments/shot.png", noteDir: "/vault/notes" }]);

    resolver.settle(0, "blob:one");

    await waitFor(() => expect(widgetImg(content).getAttribute("src")).toBe("blob:one"));
    expect(content.querySelector(".cm-md-img-error")).toBeNull();
  });

  it("解析失败标成坏图而不是空白", async () => {
    const resolver = makeResolver();
    const { content } = mount("![shot](attachments/gone.png)\n", resolver.context);

    resolver.reject(0, "no such file");

    await waitFor(() => expect(content.querySelector(".cm-md-img-error")).not.toBeNull());
  });

  it("同步 resolved 的绝对地址也能拿到 src", async () => {
    // 已经 resolved 的 promise 是最容易被"判活"逻辑误伤的一种:它的 then 紧贴着
    // 挂载时序跑。这条守着的是"网络图片在最快的那条路上也别丢 src"。
    const context: AttachmentContext = {
      noteDir: "/vault/notes",
      resolve: (url) => Promise.resolve(url),
    };
    const { content } = mount("![net](https://example.com/a.png)\n", context);

    await waitFor(() =>
      expect(widgetImg(content).getAttribute("src")).toBe("https://example.com/a.png"),
    );
  });

  it("widget 被换掉之后落地的解析不写进旧节点", async () => {
    const resolver = makeResolver();
    const { content, editor } = mount("![shot](attachments/a.png)\n", resolver.context);
    const stale = widgetImg(content);

    // 改地址会让 eq() 不成立,CodeMirror 重建 widget 并 destroy 旧的那个。
    act(() => {
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: "text\n" } });
      forceParsing(editor, editor.state.doc.length, 5000);
      editor.dispatch({});
    });
    resolver.settle(0, "blob:stale");
    // 必须先把微任务放干再断言。用 waitFor 等一个"应该保持为空"的值是自欺:它在
    // 第一次检查就通过并返回,那时候那次写入还没发生。
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(stale.getAttribute("src")).toBeNull();
  });

  it("没接 attachments 时不动 URL", async () => {
    const { content } = mount("![net](https://example.com/a.png)\n");

    // facet 的默认值原样返回 —— 缺了它,不带 attachments 的编辑器(比如别处复用
    // NoteSourceEditor)里所有图都会变成空白。
    await waitFor(() =>
      expect(widgetImg(content).getAttribute("src")).toBe("https://example.com/a.png"),
    );
  });
});
