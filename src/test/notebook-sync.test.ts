/* 云盘同步的前端逻辑(P8e)。
 *
 * 这一层三个失败模式都不报错:
 *
 * 1. 提交决定时那两个 hash 传错(自己算的、或者只传一侧),后端那道防覆盖闸门就永远判
 *    「对不上」或者永远判「对得上」—— 前者是决定全部失效,后者是覆盖用户没见过的内容。
 * 2. `stale` 判成 `decided`,用户会一直等一个后端已经作废的决定。
 * 3. fork 路径拼在扩展名后面(`a.md.conflict`),那份内容在列表、搜索、反链里全都不出现,
 *    而选 fork 的意思正是「这份我要留着看」。
 */

import { describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  clearConflictResolution,
  conflictHashes,
  conflictRowState,
  defaultForkPath,
  nextRunSeconds,
  pendingConflicts,
  resolveConflict,
  resolutionLabelKey,
  syncFailures,
  syncPendingKey,
  syncReasonKey,
  syncVerdict,
  type StoredResolution,
  type SyncPlannedAction,
  type SyncReport,
} from "../components/notebook/noteSync";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";

function conflict(path: string, localHash: string, remoteHash: string): SyncPlannedAction {
  return {
    path,
    action: { kind: "conflict", resolution: null, localHash, remoteHash },
    reason: "both_modified",
  };
}

function decided(path: string, localHash: string, remoteHash: string): StoredResolution {
  return {
    path,
    resolution: { kind: "keepRemote" },
    localHash,
    remoteHash,
    decidedAt: 1_760_000_000_000,
  };
}

function report(over: Partial<SyncReport> = {}): SyncReport {
  return {
    plan: {
      actions: [],
      summary: { upload: 0, download: 0, deleteRemote: 0, deleteLocal: 0, conflict: 0 },
    },
    outcomes: [],
    tombstonesWritten: 0,
    seq: null,
    ...over,
  };
}

describe("冲突行的三档状态", () => {
  it("没决定过就是未决定", () => {
    expect(conflictRowState(conflict("a.md", "L", "R"), [])).toEqual({ kind: "undecided" });
  });

  it("两侧都对得上才算已决定", () => {
    const state = conflictRowState(conflict("a.md", "L", "R"), [decided("a.md", "L", "R")]);
    expect(state).toEqual({ kind: "decided", resolution: { kind: "keepRemote" } });
  });

  it("本地那侧变了就是作废,不是已决定", () => {
    // 判成 decided 的话用户会一直等一个后端下一轮就会丢掉的决定。
    const state = conflictRowState(conflict("a.md", "L-new", "R"), [decided("a.md", "L", "R")]);
    expect(state.kind).toBe("stale");
  });

  it("远端那侧变了同样是作废", () => {
    // 只比本地一侧的话这一条会漏 —— 而它恰好是「别的设备又改了」这个最常见的场景。
    const state = conflictRowState(conflict("a.md", "L", "R-new"), [decided("a.md", "L", "R")]);
    expect(state.kind).toBe("stale");
  });

  it("别的路径的决定不算在这一行上", () => {
    const state = conflictRowState(conflict("a.md", "L", "R"), [decided("other.md", "L", "R")]);
    expect(state).toEqual({ kind: "undecided" });
  });

  it("空 hash 的单边冲突也能对上", () => {
    // 「对面删了 / 这边改了」远端那侧是空串。拿某个占位值去比的话,这类冲突的决定永远显示
    // 成作废 —— 而它恰好是最需要用户拍板的一类。
    const state = conflictRowState(conflict("a.md", "L", ""), [decided("a.md", "L", "")]);
    expect(state.kind).toBe("decided");
  });
});

describe("提交决定时回传的 hash", () => {
  it("原样取自冲突动作", () => {
    const got = conflictHashes(conflict("a.md", "L", "R").action);
    expect(got).toEqual({ localHash: "L", remoteHash: "R" });
  });

  it("不是冲突就没有 hash", () => {
    expect(conflictHashes({ kind: "upload" })).toBeNull();
  });

  it("resolveConflict 把两个 hash 都发给后端", async () => {
    // 少发一个,后端那侧就拿空串去比,于是决定永远作废。这条断言盯的是**两个都在**。
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    await resolveConflict({
      vault: "/v",
      remoteId: "r1",
      path: "a.md",
      resolution: { kind: "keepLocal" },
      localHash: "L",
      remoteHash: "R",
    });
    expect(invoke).toHaveBeenCalledWith("notebook_sync_resolve", {
      vault: "/v",
      remoteId: "r1",
      path: "a.md",
      resolution: { kind: "keepLocal" },
      localHash: "L",
      remoteHash: "R",
    });
  });

  it("撤回是同一条命令传 null,不是另一条命令", async () => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    await clearConflictResolution("/v", "r1", "a.md");
    expect(invoke).toHaveBeenCalledWith("notebook_sync_resolve", {
      vault: "/v",
      remoteId: "r1",
      path: "a.md",
      resolution: null,
      localHash: null,
      remoteHash: null,
    });
  });
});

describe("fork 的默认落点", () => {
  it("拼在扩展名之前,结果还是一篇 md", () => {
    // 拼成 `a.md.conflict` 的话随手记不索引它,那份内容在列表、搜索、反链里全都不出现。
    expect(defaultForkPath("a.md")).toBe("a.conflict.md");
  });

  it("带目录的路径只动文件名", () => {
    expect(defaultForkPath("年报/2026/一季度.md")).toBe("年报/2026/一季度.conflict.md");
  });

  it("没有扩展名就直接接在后面", () => {
    expect(defaultForkPath("README")).toBe("README.conflict");
  });

  it("目录里有点、文件名没有扩展名时不会切错", () => {
    // `lastIndexOf(".")` 会命中目录上那个点。按它切的话结果是 `v1.conflict.2/note`,
    // 一个不存在的目录。
    expect(defaultForkPath("v1.2/note")).toBe("v1.2/note.conflict");
  });

  it("以点开头的文件名不会被切成空名字", () => {
    expect(defaultForkPath(".gitkeep")).toBe(".gitkeep.conflict");
  });
});

describe("一轮的总体结果", () => {
  it("seq 推进了就是全部落定", () => {
    expect(syncVerdict(report({ seq: 7 }))).toBe("settled");
  });

  it("有失败就是失败", () => {
    const got = report({
      outcomes: [
        { path: "a.md", reason: "local_modified", status: { kind: "failed", error: "boom" } },
      ],
    });
    expect(syncVerdict(got)).toBe("failed");
  });

  it("只是等用户不算失败", () => {
    // 混成「失败」的话状态栏会红着,而实际上什么都没坏 —— 用户会去查一个不存在的故障。
    const got = report({
      outcomes: [
        {
          path: "a.md",
          reason: "both_modified",
          status: { kind: "pending", detail: "awaiting_user" },
        },
      ],
    });
    expect(syncVerdict(got)).toBe("awaitingUser");
  });

  it("失败盖过挂起", () => {
    const got = report({
      outcomes: [
        {
          path: "a.md",
          reason: "both_modified",
          status: { kind: "pending", detail: "awaiting_user" },
        },
        { path: "b.md", reason: "local_modified", status: { kind: "failed", error: "boom" } },
      ],
    });
    expect(syncVerdict(got)).toBe("failed");
  });

  it("没跑过就没有结论", () => {
    expect(syncVerdict(null)).toBeNull();
  });
});

describe("待处理的冲突", () => {
  it("只列还卡着的那些", () => {
    // 已经按决定执行完的冲突不该再出现在面板上,否则用户会以为自己上次没点成功。
    const got = report({
      plan: {
        actions: [conflict("stuck.md", "L", "R"), conflict("done.md", "L", "R")],
        summary: { upload: 0, download: 0, deleteRemote: 0, deleteLocal: 0, conflict: 2 },
      },
      outcomes: [
        {
          path: "stuck.md",
          reason: "both_modified",
          status: { kind: "pending", detail: "awaiting_user" },
        },
        { path: "done.md", reason: "both_modified", status: { kind: "done" } },
      ],
    });
    expect(pendingConflicts(got).map((a) => a.path)).toEqual(["stuck.md"]);
  });

  it("执行失败的冲突也要列出来", () => {
    // 决定还在库里等下一轮,面板上得看得到它 —— 否则用户以为自己的选择丢了。
    const got = report({
      plan: {
        actions: [conflict("a.md", "L", "R")],
        summary: { upload: 0, download: 0, deleteRemote: 0, deleteLocal: 0, conflict: 1 },
      },
      outcomes: [
        { path: "a.md", reason: "both_modified", status: { kind: "failed", error: "boom" } },
      ],
    });
    expect(pendingConflicts(got).map((a) => a.path)).toEqual(["a.md"]);
  });

  it("非冲突的失败不混进冲突列表", () => {
    const got = report({
      plan: {
        actions: [{ path: "a.md", action: { kind: "upload" }, reason: "local_modified" }],
        summary: { upload: 1, download: 0, deleteRemote: 0, deleteLocal: 0, conflict: 0 },
      },
      outcomes: [
        { path: "a.md", reason: "local_modified", status: { kind: "failed", error: "boom" } },
      ],
    });
    expect(pendingConflicts(got)).toEqual([]);
    expect(syncFailures(got).map((o) => o.path)).toEqual(["a.md"]);
  });
});

describe("后端标识到文案 key", () => {
  it("四条冲突原因各有自己的文案", () => {
    const keys = [
      "both_modified",
      "remote_deleted_local_modified",
      "local_tombstone_remote_modified",
      "both_present_no_baseline",
    ].map(syncReasonKey);
    expect(new Set(keys).size).toBe(4);
    expect(keys.every((k) => k !== "notebook.sync.reason.unknown")).toBe(true);
  });

  it("认不出的原因落到通用文案,不原样透出", () => {
    // 后端加了新 reason 时,「冲突」比 `some_new_reason` 好懂,而且中文界面下不会冒出英文。
    expect(syncReasonKey("something_new")).toBe("notebook.sync.reason.unknown");
  });

  it("挂起原因同样有映射", () => {
    expect(syncPendingKey("oversize_not_hashable")).toBe(
      "notebook.sync.pending.oversizeNotHashable",
    );
    expect(syncPendingKey("whatever")).toBe("notebook.sync.pending.unknown");
  });

  it("三种决定各有自己的按钮文案", () => {
    const keys = [
      resolutionLabelKey({ kind: "keepLocal" }),
      resolutionLabelKey({ kind: "keepRemote" }),
      resolutionLabelKey({ kind: "fork", forkPath: "a.conflict.md" }),
    ];
    expect(new Set(keys).size).toBe(3);
  });
});

describe("映射出来的 key 都在文案目录里", () => {
  // `i18n-keys.test.ts` 那条 guard 只扫字面量 `t("...")`。这些 key 是从映射函数返回的,
  // 调用点写成 `t(syncReasonKey(reason))` —— 它扫不到,所以缺失时不会有任何报错,UI 上
  // 直接露出 `notebook.sync.reason.bothModified` 这种原始串。这里逐个断言。
  const reasons = [
    "both_modified",
    "remote_deleted_local_modified",
    "local_tombstone_remote_modified",
    "both_present_no_baseline",
    "something_new",
  ];
  const details = [
    "oversize_not_hashable",
    "local_gone_during_sync",
    "local_appeared_during_sync",
    "awaiting_user",
    "something_new",
  ];

  it.each(reasons)("冲突原因 %s", (reason) => {
    const key = syncReasonKey(reason);
    expect(en, key).toHaveProperty(key);
    expect(zh, key).toHaveProperty(key);
  });

  it.each(details)("挂起原因 %s", (detail) => {
    const key = syncPendingKey(detail);
    expect(en, key).toHaveProperty(key);
    expect(zh, key).toHaveProperty(key);
  });

  it("三种决定的按钮文案", () => {
    for (const resolution of [
      { kind: "keepLocal" } as const,
      { kind: "keepRemote" } as const,
      { kind: "fork", forkPath: "a.conflict.md" } as const,
    ]) {
      const key = resolutionLabelKey(resolution);
      expect(en, key).toHaveProperty(key);
      expect(zh, key).toHaveProperty(key);
    }
  });
});

describe("倒计时", () => {
  it("向上取整", () => {
    // 向下取整会显示「0 秒后跑」并停在那儿,看起来像卡住了。
    const status = {
      remoteId: "r1",
      autoSync: true,
      failures: 0,
      dirty: true,
      lastAttemptMs: 1,
      nextRunInMs: 1200,
    };
    expect(nextRunSeconds(status)).toBe(2);
  });

  it("关着的时候没有倒计时", () => {
    const status = {
      remoteId: "r1",
      autoSync: false,
      failures: 0,
      dirty: false,
      lastAttemptMs: null,
      nextRunInMs: null,
    };
    expect(nextRunSeconds(status)).toBeNull();
  });
});
