/* `useAttachmentImages` 的三件事:扫 DOM 换 src、换笔记回收 blob、给编辑器一个
 * 身份稳定但读得到当前目录的 facet 值。
 *
 * 面板测试只能看到"图显示出来了",这几条契约在那一层是不可见的。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { useAttachmentImages } from "../components/notebook/useAttachmentImages";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

let created: string[];
let revoked: string[];

beforeEach(() => {
  created = [];
  revoked = [];
  let counter = 0;
  vi.spyOn(URL, "createObjectURL").mockImplementation(() => {
    const url = `blob:hook-${(counter += 1)}`;
    created.push(url);
    return url;
  });
  vi.spyOn(URL, "revokeObjectURL").mockImplementation((url: string) => {
    revoked.push(url);
  });
  invoke.mockReset();
  invoke.mockResolvedValue(new Uint8Array([1, 2, 3]).buffer);
});

afterEach(() => {
  vi.restoreAllMocks();
});

/** 造一个挂在文档上的预览容器,里面放 markdown 渲染出来的 `<img>`。 */
function hostWith(html: string) {
  const host = document.createElement("div");
  host.innerHTML = html;
  document.body.appendChild(host);
  const ref = createRef<HTMLElement>();
  (ref as { current: HTMLElement | null }).current = host;
  return { host, ref };
}

function imgIn(host: HTMLElement): HTMLImageElement {
  const img = host.querySelector("img");
  if (!img) throw new Error("img not found");
  return img;
}

describe("useAttachmentImages", () => {
  it("相对路径换成 blob URL", async () => {
    const { host, ref } = hostWith(`<img src="attachments/x.png">`);

    renderHook(() => useAttachmentImages("/vault/notes/A.md", ref, "k1"));

    await waitFor(() => expect(imgIn(host).getAttribute("src")).toBe("blob:hook-1"));
    expect(invoke).toHaveBeenCalledWith("notebook_attachment_read", {
      path: "/vault/notes/attachments/x.png",
    });
  });

  it("解析期间相对路径不留在 src 上", async () => {
    let release: ((value: ArrayBuffer) => void) | undefined;
    invoke.mockReturnValue(
      new Promise<ArrayBuffer>((resolve) => {
        release = resolve;
      }),
    );
    const { host, ref } = hostWith(`<img src="attachments/slow.png">`);

    renderHook(() => useAttachmentImages("/vault/notes/A.md", ref, "k1"));
    await act(async () => {
      await Promise.resolve();
    });

    // 留着的话 WebView 会按 `tauri://localhost/attachments/slow.png` 发一次注定
    // 404 的请求,img 的 error 事件把它画成坏图 —— 等 blob 到了图已经是坏的了。
    expect(imgIn(host).getAttribute("src")).toBeNull();
    expect(imgIn(host).dataset.notebookSrc).toBe("attachments/slow.png");

    await act(async () => {
      release?.(new Uint8Array([1]).buffer);
      await Promise.resolve();
    });
    await waitFor(() => expect(imgIn(host).getAttribute("src")).toBe("blob:hook-1"));
  });

  it("网络图片一个字都不动", async () => {
    const { host, ref } = hostWith(`<img src="https://example.com/a.png">`);

    renderHook(() => useAttachmentImages("/vault/notes/A.md", ref, "k1"));
    await act(async () => {
      await Promise.resolve();
    });

    // 把 src 摘下来再异步放回去会让浏览器取消已经在进行的加载、重新发一次请求 ——
    // 网络图会闪一下。而且这一步会往节点上留一个没用的 data 属性。
    expect(imgIn(host).getAttribute("src")).toBe("https://example.com/a.png");
    expect(imgIn(host).dataset.notebookSrc).toBeUndefined();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("读不出来的图标成坏图,不弹全局错误", async () => {
    invoke.mockRejectedValue(new Error("reading the attachment failed"));
    const { host, ref } = hostWith(`<img src="attachments/gone.png">`);

    renderHook(() => useAttachmentImages("/vault/notes/A.md", ref, "k1"));

    // 没有标记的话页面上是一个不解释自己的空白框,用户不知道是没加载完还是丢了。
    await waitFor(() => expect(imgIn(host).dataset.notebookError).toBe("1"));
    expect(imgIn(host).getAttribute("src")).toBeNull();
  });

  it("同一个容器被扫两次不会重复读", async () => {
    const { host, ref } = hostWith(`<img src="attachments/x.png">`);
    const { rerender } = renderHook(
      ({ key }: { key: string }) => useAttachmentImages("/vault/notes/A.md", ref, key),
      { initialProps: { key: "k1" } },
    );
    await waitFor(() => expect(imgIn(host).getAttribute("src")).toBe("blob:hook-1"));

    // 公式 / Mermaid 渲染完会再触发一轮扫描。
    rerender({ key: "k2" });
    await act(async () => {
      await Promise.resolve();
    });

    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("换笔记回收上一篇的 blob", async () => {
    const { host, ref } = hostWith(`<img src="attachments/x.png">`);
    const { rerender } = renderHook(
      ({ note }: { note: string }) => useAttachmentImages(note, ref, "k1"),
      { initialProps: { note: "/vault/notes/A.md" } },
    );
    await waitFor(() => expect(imgIn(host).getAttribute("src")).toBe("blob:hook-1"));

    host.innerHTML = `<img src="attachments/y.png">`;
    rerender({ note: "/vault/notes/B.md" });
    await waitFor(() => expect(imgIn(host).getAttribute("src")).toBe("blob:hook-2"));

    // 不回收的话翻二十篇图多的笔记会攒下几百个 blob,而它们只有页面卸载才会被
    // 浏览器收走 —— 内存一路涨,没有任何报错。
    expect(revoked).toEqual(["blob:hook-1"]);
    expect(created).toEqual(["blob:hook-1", "blob:hook-2"]);
  });

  it("卸载时回收", async () => {
    const { host, ref } = hostWith(`<img src="attachments/x.png">`);
    const { unmount } = renderHook(() => useAttachmentImages("/vault/notes/A.md", ref, "k1"));
    await waitFor(() => expect(imgIn(host).getAttribute("src")).toBe("blob:hook-1"));

    unmount();

    expect(revoked).toEqual(["blob:hook-1"]);
  });

  it("facet 值身份不变,但读到的目录跟着笔记走", async () => {
    const { ref } = hostWith("");
    const { result, rerender } = renderHook(
      ({ note }: { note: string }) => useAttachmentImages(note, ref, "k1"),
      { initialProps: { note: "/vault/notes/A.md" } },
    );
    const first = result.current;
    expect(first.noteDir).toBe("/vault/notes");

    rerender({ note: "/vault/deep/nested/B.md" });

    // 身份必须稳:它进了 CodeMirror 的 extension 数组,换新对象会重建 view ——
    // 光标和撤销栈全丢。但目录又必须是当前那一篇的,否则换笔记后编辑态里的图会
    // 按上一篇的目录去解析。
    expect(result.current).toBe(first);
    expect(first.noteDir).toBe("/vault/deep/nested");
  });

  it("没有打开笔记时目录是空的", () => {
    const { ref } = hostWith("");
    const { result } = renderHook(() => useAttachmentImages("", ref, "k1"));

    expect(result.current.noteDir).toBe("");
  });
});
