/* `useNoteSync` 的取数与事件。
 *
 * 五件"错了不报错、只是悄悄做错事"的:
 *
 *   1. **非 cloud 的远端不能进来。** `git` 走另一套命令,选中它之后按「立即同步」会拿一个
 *      `notebook_sync_run` 处理不了的目标去调。
 *   2. **守护线程的事件不能触发一轮同步。** 那会写文件 —— 面板开着这件事本身不该让后台的
 *      一轮变成两轮写。它只该标记过期 + 重查。
 *   3. **提交决定时那两个 hash 要原样来自本轮报告。** 自己造一个会让防覆盖的闸门失效。
 *   4. **换远端要丢掉上一轮的报告。** 否则用户对着 A 的冲突清单给 B 做决定。
 *   5. **状态轮询失败不能写进 `error`。** 那个位置是给用户动作的失败留的。
 */

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

import { useNoteSync } from "../components/notebook/useNoteSync";
import type { SyncReport } from "../components/notebook/noteSync";

type Handler = () => void;

const invokeMock = vi.fn();
const handlers: { event: string; handler: Handler }[] = [];
const unlisten = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event: string, handler: Handler) => {
    handlers.push({ event, handler });
    return unlisten;
  },
}));

const cloudRemote = {
  id: "r-cloud",
  kind: "cloud",
  root: "/dav/notes",
  connectionId: "c1",
  lastSyncAt: 1_756_700_000_000,
  seq: 4,
  autoSync: true,
};

const gitRemote = { ...cloudRemote, id: "r-git", kind: "git", root: "/git/notes" };

const status = (over: Record<string, unknown> = {}) => ({
  remoteId: "r-cloud",
  autoSync: true,
  failures: 0,
  dirty: false,
  lastAttemptMs: 1_756_700_000_000,
  nextRunInMs: 30_000,
  ...over,
});

/** 一轮的报告:一个冲突,挂起等用户。 */
const report = (over: Partial<SyncReport> = {}): SyncReport => ({
  plan: {
    actions: [
      {
        path: "a.md",
        reason: "both_modified",
        action: { kind: "conflict", resolution: null, localHash: "111", remoteHash: "222" },
      },
    ],
    summary: { upload: 0, download: 0, deleteRemote: 0, deleteLocal: 0, conflict: 1 },
  },
  outcomes: [
    { path: "a.md", reason: "both_modified", status: { kind: "pending", detail: "awaiting_user" } },
  ],
  tombstonesWritten: 0,
  seq: null,
  ...over,
});

let remotes = [cloudRemote];
let resolutions: unknown[] = [];

beforeEach(() => {
  handlers.length = 0;
  invokeMock.mockReset();
  unlisten.mockReset();
  remotes = [cloudRemote];
  resolutions = [];
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "notebook_sync_remotes") return remotes;
    if (command === "notebook_sync_status") return [status()];
    if (command === "notebook_sync_resolutions") return resolutions;
    if (command === "notebook_sync_run") return report();
    if (command === "notebook_sync_set_auto") return null;
    if (command === "notebook_sync_resolve") return null;
    throw new Error(`unexpected command ${command}`);
  });
});

afterEach(() => {
  vi.useRealTimers();
});

const callsTo = (command: string) => invokeMock.mock.calls.filter((call) => call[0] === command);

describe("只认云盘远端", () => {
  it("git 远端不进列表", async () => {
    remotes = [gitRemote, cloudRemote];
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.remotes).toHaveLength(1));
    expect(result.current.remotes[0]?.target.id).toBe("r-cloud");
    expect(result.current.activeId).toBe("r-cloud");
  });

  it("一个云盘远端都没有时不查状态", async () => {
    remotes = [gitRemote];
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(callsTo("notebook_sync_remotes")).toHaveLength(1));
    expect(result.current.active).toBeNull();
    // 没有 id 可查,那条命令一次都不该发出去。
    expect(callsTo("notebook_sync_status")).toHaveLength(0);
  });

  it("关着的时候一次请求都不发", async () => {
    renderHook(() => useNoteSync("/v", false));
    await act(async () => {});
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("守护线程那条事件", () => {
  it("标记报告过期,但不跑第二轮", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    await act(async () => result.current.sync());
    await waitFor(() => expect(result.current.report).not.toBeNull());

    const runsBefore = callsTo("notebook_sync_run").length;
    const sub = handlers.find((item) => item.event === "notebook-sync-updated");
    expect(sub).toBeDefined();
    await act(async () => sub?.handler());

    await waitFor(() => expect(result.current.stale).toBe(true));
    /* 这是这条测试的重点:事件不该引出一次写。多跑一轮会把守护线程刚做过的写再做一遍,
       而用户并没有要求同步。 */
    expect(callsTo("notebook_sync_run")).toHaveLength(runsBefore);
  });

  it("重查了目标和决定", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    const before = callsTo("notebook_sync_remotes").length;

    const sub = handlers.find((item) => item.event === "notebook-sync-updated");
    await act(async () => sub?.handler());

    await waitFor(() => expect(callsTo("notebook_sync_remotes").length).toBeGreaterThan(before));
  });

  it("还没有报告时不标过期", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    const sub = handlers.find((item) => item.event === "notebook-sync-updated");
    await act(async () => sub?.handler());
    // 界面上没有"这一轮的结果",没有东西可过期 —— 标出来只会让用户去按一个没意义的刷新。
    expect(result.current.stale).toBe(false);
  });

  it("新报告落地后过期标记收掉", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    await act(async () => result.current.sync());
    await waitFor(() => expect(result.current.report).not.toBeNull());
    const sub = handlers.find((item) => item.event === "notebook-sync-updated");
    await act(async () => sub?.handler());
    await waitFor(() => expect(result.current.stale).toBe(true));

    await act(async () => result.current.sync());
    await waitFor(() => expect(result.current.stale).toBe(false));
  });
});

describe("提交决定", () => {
  it("回传的是本轮报告上那两个 hash", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    await act(async () => result.current.sync());
    await waitFor(() => expect(result.current.report).not.toBeNull());

    await act(async () => result.current.decide("a.md", { kind: "keepLocal" }));

    const call = callsTo("notebook_sync_resolve").at(-1);
    expect(call?.[1]).toMatchObject({
      vault: "/v",
      remoteId: "r-cloud",
      path: "a.md",
      resolution: { kind: "keepLocal" },
      localHash: "111",
      remoteHash: "222",
    });
  });

  it("不在本轮冲突里的路径不提交", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    await act(async () => result.current.sync());
    await waitFor(() => expect(result.current.report).not.toBeNull());

    await act(async () => result.current.decide("nope.md", { kind: "keepRemote" }));

    /* 拿不到 hash 就不能提交:凭空传空串的话,后端那道"两侧 hash 都要对得上"的闸门会把
       一条本该作废的决定放过去。 */
    expect(callsTo("notebook_sync_resolve")).toHaveLength(0);
  });

  it("还没跑过就做决定时什么也不发", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    await act(async () => result.current.decide("a.md", { kind: "keepLocal" }));
    expect(callsTo("notebook_sync_resolve")).toHaveLength(0);
  });

  it("乐观记一份,不等重查", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    await act(async () => result.current.sync());
    await waitFor(() => expect(result.current.report).not.toBeNull());

    await act(async () => result.current.decide("a.md", { kind: "keepRemote" }));

    /* 点完到重查落地之间那一段,行上不能显示"还没决定" —— 用户会以为没点上然后再点一次。 */
    await waitFor(() =>
      expect(result.current.decided).toEqual([
        expect.objectContaining({
          path: "a.md",
          resolution: { kind: "keepRemote" },
          localHash: "111",
          remoteHash: "222",
        }),
      ]),
    );
  });

  it("撤回把本地那份也去掉", async () => {
    resolutions = [
      {
        path: "a.md",
        resolution: { kind: "keepLocal" },
        localHash: "111",
        remoteHash: "222",
        decidedAt: 1_756_700_000_000,
      },
    ];
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.decided).toHaveLength(1));

    await act(async () => result.current.undecide("a.md"));
    await waitFor(() => expect(result.current.decided).toHaveLength(0));
    expect(callsTo("notebook_sync_resolve").at(-1)?.[1]).toMatchObject({
      path: "a.md",
      resolution: null,
    });
  });
});

describe("换远端", () => {
  it("把上一轮的报告和过期标记一起丢掉", async () => {
    remotes = [cloudRemote, { ...cloudRemote, id: "r-two", root: "/dav/other" }];
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.remotes).toHaveLength(2));
    await act(async () => result.current.sync());
    await waitFor(() => expect(result.current.report).not.toBeNull());

    await act(async () => result.current.selectRemote("r-two"));

    /* 那份报告讲的是 r-cloud 的冲突。留着的话用户会对着 A 的清单给 B 做决定,而每条决定
       都会带着 A 那两个 hash 存到 B 上面去。 */
    await waitFor(() => expect(result.current.report).toBeNull());
    expect(result.current.stale).toBe(false);
  });

  it("选中的远端不在了就落回第一个", async () => {
    remotes = [cloudRemote, { ...cloudRemote, id: "r-two", root: "/dav/other" }];
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.remotes).toHaveLength(2));
    await act(async () => result.current.selectRemote("r-two"));
    await waitFor(() => expect(result.current.activeId).toBe("r-two"));

    // r-two 被解绑了。
    remotes = [cloudRemote];
    await act(async () => result.current.refresh());

    // 落回而不是显示空白 —— 而且 `activeId` 自己也要跟着改,不能只让 `active` 兜。
    await waitFor(() => expect(result.current.activeId).toBe("r-cloud"));
    expect(result.current.active?.target.id).toBe("r-cloud");
  });
});

describe("失败的去处", () => {
  it("状态轮询失败不写进 error", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "notebook_sync_remotes") return remotes;
      if (command === "notebook_sync_resolutions") return [];
      if (command === "notebook_sync_status") throw new Error("poll boom");
      throw new Error(`unexpected command ${command}`);
    });

    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(warn).toHaveBeenCalled());
    /* 那个位置是给用户动作的失败留的。被一次瞬时的查询失败占住之后,真正的失败原因会被
       后面的轮询覆盖掉。 */
    expect(result.current.error).toBeNull();
    warn.mockRestore();
  });

  it("跑一轮失败时报出来", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "notebook_sync_remotes") return remotes;
      if (command === "notebook_sync_status") return [status()];
      if (command === "notebook_sync_resolutions") return [];
      if (command === "notebook_sync_run") throw new Error("remote unreachable");
      throw new Error(`unexpected command ${command}`);
    });

    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    await act(async () => result.current.sync());
    await waitFor(() => expect(result.current.error).toBe("remote unreachable"));
    expect(result.current.running).toBe(false);
  });
});

describe("默认策略", () => {
  it("不传时按 ask —— 那是唯一不动冲突文件的一档", async () => {
    const { result } = renderHook(() => useNoteSync("/v", true));
    await waitFor(() => expect(result.current.active).not.toBeNull());
    await act(async () => result.current.sync());
    expect(callsTo("notebook_sync_run").at(-1)?.[1]).toMatchObject({ strategy: "ask" });
  });
});
