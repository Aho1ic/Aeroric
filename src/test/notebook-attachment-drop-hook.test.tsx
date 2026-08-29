/* `useNoteAttachmentDrop` 里面板走不到的两条路。
 *
 * 一条是"编辑器在但笔记没了"(面板里 activeNote 为空时编辑器根本不渲染,所以只能
 * 直接测 hook);另一条是系统文件管理器拖入 —— 那个事件由 Tauri 发给整个窗口,
 * 在面板测试里没有真实来源。
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useNoteAttachmentDrop } from "../components/notebook/useNoteAttachmentDrop";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

/** 注册进 hook 的原生拖放回调。测试直接调它来模拟一次系统拖入。 */
let dropListener: ((event: unknown) => void) | null = null;
const unlisten = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (event: unknown) => void) => {
      dropListener = handler;
      return Promise.resolve(unlisten);
    },
  }),
}));

function imageFile(name: string): File {
  const file = new File([new Uint8Array([1])], name, { type: "image/png" });
  Object.defineProperty(file, "arrayBuffer", {
    value: () => Promise.resolve(new Uint8Array([1]).buffer),
  });
  return file;
}

type Options = Parameters<typeof useNoteAttachmentDrop>[0];

function setup(overrides: Partial<Options> = {}) {
  const calls = {
    inserted: [] as string[],
    errors: [] as string[],
    insertPoints: [] as number[],
    saved: 0,
  };
  const options: Options = {
    notePath: "/vault/Note.md",
    setInsertPoint: (at) => calls.insertPoints.push(at),
    insert: (markdown) => calls.inserted.push(markdown),
    posAtClientPoint: () => 7,
    onSaved: () => {
      calls.saved += 1;
    },
    onError: (message) => calls.errors.push(message),
    noNoteMessage: "open a note first",
    tooManyMessage: "skipped {count}",
    ...overrides,
  };
  const view = renderHook(() => useNoteAttachmentDrop(options));
  return { ...view, calls };
}

describe("useNoteAttachmentDrop", () => {
  beforeEach(() => {
    dropListener = null;
    invoke.mockReset();
    unlisten.mockReset();
    let counter = 0;
    invoke.mockImplementation((command: string) => {
      counter += 1;
      if (
        command === "notebook_attachment_save" ||
        command === "notebook_attachment_save_from_path"
      )
        return Promise.resolve({
          path: `/vault/attachments/a${counter}.png`,
          name: `a${counter}.png`,
          link: `attachments/a${counter}.png`,
          markdown: `![a${counter}](attachments/a${counter}.png)`,
          size: 1,
        });
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });
  });

  it("没有打开笔记时提示,并且仍然接手事件", () => {
    const { result, calls } = setup({ notePath: "" });

    const handled = result.current.handleFiles([imageFile("x.png")], 3);

    // 返回 false 的话浏览器会接着走默认行为,把图片的**文件名**当纯文本插进笔记。
    expect(handled).toBe(true);
    expect(calls.errors).toEqual(["open a note first"]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("先定插入点再存,不用出发时的偏移", async () => {
    const { result, calls } = setup();

    act(() => {
      result.current.handleFiles([imageFile("x.png")], 42);
    });

    // 存附件要等写盘,那期间用户可能继续打字。先把插入点变成一个选区,让
    // CodeMirror 跟着后续编辑一起映射它 —— 拿着 42 去替换会插错位置。
    expect(calls.insertPoints).toEqual([42]);
    await waitFor(() => expect(calls.inserted).toHaveLength(1));
  });

  it("系统拖入按落点插入,并换算设备像素比", async () => {
    const posAt = vi.fn().mockReturnValue(5);
    const { calls } = setup({ posAtClientPoint: posAt });
    await waitFor(() => expect(dropListener).not.toBeNull());
    Object.defineProperty(window, "devicePixelRatio", { value: 2, configurable: true });

    act(() => {
      dropListener!({
        payload: { type: "drop", paths: ["/tmp/shot.png"], position: { x: 100, y: 60 } },
      });
    });

    // 事件给的是物理像素,DOM 的 rect 是 CSS 像素。不换算的话在 Retina 屏上落点
    // 会偏到两倍远的地方。
    expect(posAt).toHaveBeenCalledWith(50, 30);
    await waitFor(() => expect(calls.inserted).toHaveLength(1));
    expect(calls.insertPoints).toEqual([5]);
    expect(calls.errors).toEqual([]);
  });

  it("系统拖入在没有打开笔记时静默忽略", async () => {
    const { calls } = setup({ notePath: "" });
    await waitFor(() => expect(dropListener).not.toBeNull());

    act(() => {
      dropListener!({
        payload: { type: "drop", paths: ["/tmp/shot.png"], position: { x: 1, y: 1 } },
      });
    });

    // 和 handleFiles 不一样:那条路是用户明确往编辑器里粘,该给提示;这条是整个
    // 窗口的事件,别的面板里的拖放不该弹随手记的错误。
    expect(calls.errors).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("系统拖入落在编辑器外就整个忽略", async () => {
    const { calls } = setup({ posAtClientPoint: () => null });
    await waitFor(() => expect(dropListener).not.toBeNull());

    act(() => {
      dropListener!({
        payload: { type: "drop", paths: ["/tmp/shot.png"], position: { x: 1, y: 1 } },
      });
    });

    // 这个事件是**整个窗口**的。不判落点的话把文件拖到笔记列表、甚至别的面板上
    // 都会往正文里插图。
    expect(invoke).not.toHaveBeenCalled();
    expect(calls.inserted).toEqual([]);
  });

  it("系统拖入只认图片扩展名", async () => {
    setup();
    await waitFor(() => expect(dropListener).not.toBeNull());

    act(() => {
      dropListener!({
        payload: {
          type: "drop",
          paths: ["/tmp/notes.md", "/tmp/archive.zip"],
          position: { x: 1, y: 1 },
        },
      });
    });

    // 拖一个 .md 进来是"想打开它",不是"想把它变成附件"。
    expect(invoke).not.toHaveBeenCalled();
  });

  it("系统拖入走路径而不是 base64", async () => {
    setup();
    await waitFor(() => expect(dropListener).not.toBeNull());

    act(() => {
      dropListener!({
        payload: { type: "drop", paths: ["/tmp/shot.png"], position: { x: 1, y: 1 } },
      });
    });

    // 一张 8MB 的图 base64 之后是 11MB 的字符串,来回穿 IPC 纯浪费 —— 而这条路
    // 本来就有磁盘路径可用。
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("notebook_attachment_save_from_path", {
        note: "/vault/Note.md",
        src: "/tmp/shot.png",
      }),
    );
  });

  it("hover / cancel 之类的负载不当成 drop", async () => {
    setup();
    await waitFor(() => expect(dropListener).not.toBeNull());

    act(() => {
      dropListener!({ payload: { type: "over", position: { x: 1, y: 1 } } });
      dropListener!({ payload: { type: "leave" } });
    });

    expect(invoke).not.toHaveBeenCalled();
  });

  it("卸载时摘掉监听", async () => {
    const { unmount } = setup();
    await waitFor(() => expect(dropListener).not.toBeNull());

    unmount();

    // 不摘的话面板每开一次就多一个监听,一次拖入会被处理好几遍。
    expect(unlisten).toHaveBeenCalled();
  });

  it("handleFiles 的身份稳定,不会让编辑器重建", () => {
    const { result, rerender } = setup();
    const first = result.current.handleFiles;

    rerender();

    // 它进了 CodeMirror 的 extension 数组:每次渲染换一个新函数会重建 view,
    // 光标和撤销栈全丢。
    expect(result.current.handleFiles).toBe(first);
  });
});
