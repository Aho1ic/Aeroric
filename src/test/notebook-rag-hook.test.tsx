/* `useNoteRag` 的取数与进度。
 *
 * 三件"错了不报错、只是界面看起来卡住"的事:
 *   1. 进度监听必须在建索引**之前**就挂上 —— `notebook_rag_index` 会一直 await 到这
 *      一轮跑完,等它返回再挂就一条进度都收不到。
 *   2. 进度只认自己那个 vault —— 事件按 vault 平铺,不过滤的话另一个库的进度会画到
 *      这个面板上。
 *   3. 建索引失败(库都打不开,一条事件都没来过)时进度必须清掉,否则永远卡在
 *      scanning。
 */

import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

import { DEFAULT_RAG_CONFIG, type RagIndexProgress } from "../components/notebook/noteRag";
import { useNoteRag } from "../components/notebook/useNoteRag";

type Subscription = {
  event: string;
  handler: (event: { payload: RagIndexProgress }) => void;
  unlisten: ReturnType<typeof vi.fn>;
};

const subscriptions: Subscription[] = [];
const invokeMock = vi.fn();
/** listen 落地的时机由测试控制:要能验"监听还没挂上时命令就已经发出去了"。 */
let listenGate: (() => void) | null = null;

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event: string, handler: (payload: { payload: RagIndexProgress }) => void) => {
    if (listenGate) await new Promise<void>((resolve) => (listenGate = resolve));
    const unlisten = vi.fn();
    subscriptions.push({ event, handler, unlisten });
    return unlisten;
  },
}));

const stats = {
  docs: 2,
  indexed: 2,
  pending: 0,
  failed: 0,
  stale: 0,
  chunks: 9,
  failures: [],
};

const progress = (over: Partial<RagIndexProgress> = {}): RagIndexProgress => ({
  vault: "/v",
  phase: "embedding",
  total: 8,
  done: 3,
  failed: 0,
  current: null,
  error: null,
  ...over,
});

beforeEach(() => {
  subscriptions.length = 0;
  invokeMock.mockReset();
  listenGate = null;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "notebook_rag_stats") return stats;
    if (command === "notebook_rag_index")
      return { indexed: 2, skipped: 0, failed: 0, removed: 0, cancelled: false };
    if (command === "notebook_rag_cancel") return true;
    if (command === "notebook_rag_clear") return null;
    if (command === "notebook_rag_search") return { hits: [], degraded: [], vectorsMissing: false };
    throw new Error(`未预期的命令 ${command}`);
  });
});

function mount(vault: string | null = "/v", enabled = true) {
  return renderHook(() => useNoteRag(vault, enabled, DEFAULT_RAG_CONFIG));
}

describe("useNoteRag 的取数", () => {
  it("面板开着时读一次索引状态", async () => {
    const { result } = mount();
    await waitFor(() => expect(result.current.stats).not.toBeNull());
    expect(result.current.stats?.chunks).toBe(9);
  });

  it("面板关着时不读 —— 那是一次开库读表", async () => {
    mount("/v", false);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(invokeMock).not.toHaveBeenCalled();
    expect(subscriptions).toHaveLength(0);
  });

  it("没有 vault 时什么都不做", async () => {
    mount(null, true);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("读状态失败时报出来", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "notebook_rag_stats") throw new Error("库打不开");
      return null;
    });
    const { result } = mount();
    await waitFor(() => expect(result.current.error).toBe("库打不开"));
  });
});

describe("进度事件", () => {
  it("订阅的是约定好的那个 topic", async () => {
    mount();
    await waitFor(() => expect(subscriptions).toHaveLength(1));
    expect(subscriptions[0]!.event).toBe("notebook-rag-index-progress");
  });

  it("面板一开就订阅,不等到点了建索引才订阅", async () => {
    /* 这一条是这个 hook 存在的主要理由。`notebook_rag_index` 会 await 到整轮跑完,
       监听挂晚了就一条进度都收不到 —— 表现是"点了之后很久没反应,然后突然好了"。
       所以订阅不能挂在"正在建索引"这个条件上,而 `listen` 本身是异步的,那一段
       沉默期正好是最前面几条进度。 */
    const { result } = mount();
    await waitFor(() => expect(subscriptions).toHaveLength(1));
    // 一次 index 都没调过,而进度已经能收到了。
    expect(invokeMock.mock.calls.some((c) => c[0] === "notebook_rag_index")).toBe(false);
    act(() => subscriptions[0]!.handler({ payload: progress({ done: 1 }) }));
    expect(result.current.progress?.done).toBe(1);
  });

  it("第一条事件到之前先手动置 scanning", async () => {
    /* 开库 + 走完 vault 是最久的一段,期间一条事件都没有。不先置的话点了按钮界面
       完全没反应,用户会再点一次(而后端会拒绝,看起来像坏了)。 */
    listenGate = () => {};
    const { result } = mount();
    act(() => result.current.index("all"));
    expect(result.current.progress?.phase).toBe("scanning");
    expect(result.current.progress?.total).toBe(0);
  });

  it("只认自己那个 vault 的进度", async () => {
    // 不过滤的话另一个库的进度会画到这个面板上。
    const { result } = mount();
    await waitFor(() => expect(subscriptions).toHaveLength(1));
    act(() => subscriptions[0]!.handler({ payload: progress({ vault: "/other", done: 7 }) }));
    expect(result.current.progress).toBeNull();
    act(() => subscriptions[0]!.handler({ payload: progress({ done: 3 }) }));
    expect(result.current.progress?.done).toBe(3);
  });

  it("终态事件把进度收掉", async () => {
    // done / cancelled / failed 留在场的话进度条会永远停在最后一格。
    const { result } = mount();
    await waitFor(() => expect(subscriptions).toHaveLength(1));
    act(() => subscriptions[0]!.handler({ payload: progress() }));
    expect(result.current.progress).not.toBeNull();
    act(() => subscriptions[0]!.handler({ payload: progress({ phase: "done" }) }));
    expect(result.current.progress).toBeNull();
  });

  it("整轮失败的原因从事件里接住", async () => {
    /* `runRagIndex` 对"跑完了但整轮失败"是正常返回的,原因只在事件里 —— 不接住
       这里就没有任何地方会显示它。 */
    const { result } = mount();
    await waitFor(() => expect(subscriptions).toHaveLength(1));
    act(() =>
      subscriptions[0]!.handler({ payload: progress({ phase: "failed", error: "维度对不上" }) }),
    );
    expect(result.current.error).toBe("维度对不上");
    expect(result.current.progress).toBeNull();
  });

  it("卸载时退订", async () => {
    const { unmount } = mount();
    await waitFor(() => expect(subscriptions).toHaveLength(1));
    unmount();
    await waitFor(() => expect(subscriptions[0]!.unlisten).toHaveBeenCalled());
  });
});

describe("建索引", () => {
  it("按 scope 发起并在结束后重读状态", async () => {
    const { result } = mount();
    await waitFor(() => expect(result.current.stats).not.toBeNull());
    const before = invokeMock.mock.calls.filter((c) => c[0] === "notebook_rag_stats").length;

    await act(async () => {
      result.current.index("failedOnly");
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(invokeMock).toHaveBeenCalledWith("notebook_rag_index", {
      vault: "/v",
      config: DEFAULT_RAG_CONFIG,
      scope: "failedOnly",
    });
    await waitFor(() =>
      expect(
        invokeMock.mock.calls.filter((c) => c[0] === "notebook_rag_stats").length,
      ).toBeGreaterThan(before),
    );
  });

  it("命令抛出时进度也要清掉", async () => {
    /* 库打不开的话一条事件都没来过。不在 finally 里清的话进度永远卡在 scanning,
       而取消按钮点了也没用 —— 界面等于死了。 */
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "notebook_rag_stats") return stats;
      if (command === "notebook_rag_index") throw new Error("索引库打不开");
      return null;
    });
    const { result } = mount();
    await act(async () => {
      result.current.index("all");
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await waitFor(() => expect(result.current.error).toBe("索引库打不开"));
    expect(result.current.progress).toBeNull();
  });
});

describe("检索与清空", () => {
  it("空查询不发请求", async () => {
    // 空查询的检索没有意义,发出去只会拿回一份空结果并把 searched 置上。
    const { result } = mount();
    await waitFor(() => expect(result.current.stats).not.toBeNull());
    act(() => result.current.setQuery("   "));
    act(() => result.current.search());
    expect(invokeMock.mock.calls.some((c) => c[0] === "notebook_rag_search")).toBe(false);
    expect(result.current.searched).toBe(false);
  });

  it("检索结果连同降级一起接住", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "notebook_rag_stats") return stats;
      if (command === "notebook_rag_search")
        return {
          hits: [],
          degraded: [{ stage: "vector", detail: "refused" }],
          vectorsMissing: true,
        };
      return null;
    });
    const { result } = mount();
    act(() => result.current.setQuery("问题"));
    await act(async () => {
      result.current.search();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await waitFor(() => expect(result.current.searched).toBe(true));
    expect(result.current.degraded).toEqual([{ stage: "vector", detail: "refused" }]);
    expect(result.current.vectorsMissing).toBe(true);
  });

  it("清空索引之后命中和上下文也清掉", async () => {
    /* 它们讲的是刚被删掉的那个索引。留着点进去会跳到一份可能已经变了的正文上。 */
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "notebook_rag_stats") return stats;
      if (command === "notebook_rag_search")
        return {
          hits: [
            {
              path: "/v/a.md",
              title: "甲",
              heading: "",
              body: "正文",
              score: 1,
              source: "fts",
              charStart: 0,
              charEnd: 2,
              bodySpans: [],
              sourceSpans: [],
            },
          ],
          degraded: [],
          vectorsMissing: false,
        };
      return null;
    });
    const { result } = mount();
    act(() => result.current.setQuery("问题"));
    await act(async () => {
      result.current.search();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await waitFor(() => expect(result.current.hits).toHaveLength(1));

    await act(async () => {
      result.current.clear();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await waitFor(() => expect(result.current.hits).toHaveLength(0));
    expect(result.current.searched).toBe(false);
    expect(result.current.vectorsMissing).toBe(true);
  });
});
