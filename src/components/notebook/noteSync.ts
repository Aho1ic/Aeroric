/* 随手记的云盘同步(P8e 前端侧)。
 *
 * 后端已经把「难的部分」做完了:三方 diff、tombstone、退避调度、逐路径的冲突决定。这一层
 * 只做三件后端做不到、而前端弄错就会出事的事:
 *
 * 1. **提交决定时必须回传那两个 hash。** 它们是防覆盖的闸门(见 Rust 侧 `sync::store` 的
 *    模块文档)。前端从 `SyncReport` 的冲突动作上原样取、原样传回去 —— 自己算一个、或者
 *    干脆不传,那道闸门就永远判「对不上」(决定全部作废)或者永远判「对得上」(决定会覆盖
 *    用户没见过的内容)。两种方向都比不做还糟。
 *
 * 2. **决定不立即生效。** 它只是入库,由下一轮同步拿去用。UI 得把这件事说清楚,否则用户点完
 *    看不到变化会以为没生效,然后反复点。
 *
 * 3. **状态是两路数据拼的。** 目标列表(含 `autoSync`)在库里,调度状态(退避、下一轮还有
 *    多久)在进程内存里。后端故意分成两条命令 —— 状态栏高频轮询那一路不该每次开库。
 */

import { invoke } from "@tauri-apps/api/core";

/** 与 Rust 的 `store::RemoteTarget` 对齐。 */
export type SyncRemoteTarget = {
  id: string;
  /** `cloud` / `git` / `p2p`。只有 `cloud` 走这一套命令,`git` 走 `notebook_git_*`。 */
  kind: string;
  root: string;
  connectionId: string;
  lastSyncAt: number;
  seq: number;
  autoSync: boolean;
};

/** 与 Rust 的 `daemon::RemoteStatus` 对齐。 */
export type SyncRemoteStatus = {
  remoteId: string;
  autoSync: boolean;
  /** 连续失败次数。> 0 表示正在退避。 */
  failures: number;
  /** 有本地改动还没同步成功。 */
  dirty: boolean;
  /** 上一轮**开始**的时间。`null` = 这个进程还没跑过。 */
  lastAttemptMs: number | null;
  /**
   * 距下一轮还有多久(毫秒)。`null` = 自动同步**关着**。
   *
   * 此刻就该跑是 `0`,不是 `null` —— 见 Rust 侧 `daemon::status_for`,它把 `Decision::Run`
   * 映射成 `Some(0)`,只有 `Off` 才给 `None`。把 0 当成「关着」会让状态栏在最该显示
   * 「马上就跑」的那一刻说「已关闭」。
   */
  nextRunInMs: number | null;
};

export type ConflictStrategy = "ask" | "local" | "remote";

/** 与 Rust 的 `diff::Resolution` 对齐(serde 的内部 tag 是 `kind`)。 */
export type SyncResolution =
  | { kind: "keepLocal" }
  | { kind: "keepRemote" }
  /** 远端那份另存一份,两边都留。`forkPath` 是 vault 内的相对路径。 */
  | { kind: "fork"; forkPath: string };

/** 与 Rust 的 `diff::Action` 对齐。 */
export type SyncAction =
  | { kind: "upload" }
  | { kind: "download" }
  | { kind: "deleteRemote" }
  | { kind: "deleteLocal" }
  | {
      kind: "conflict";
      /** `null` = 等用户选。 */
      resolution: SyncResolution | null;
      /** 本轮看到的本地 hash。空串 = 本地没有这个文件。**提交决定时要原样回传。** */
      localHash: string;
      /** 本轮看到的远端 hash。空串 = 远端没有这个文件。**提交决定时要原样回传。** */
      remoteHash: string;
    };

export type SyncPlannedAction = {
  path: string;
  action: SyncAction;
  /** 形如 `both_modified` 的稳定标识。文案由 `syncReasonKey` 映射到 i18n。 */
  reason: string;
};

export type SyncPlanSummary = {
  upload: number;
  download: number;
  deleteRemote: number;
  deleteLocal: number;
  conflict: number;
};

export type SyncOutcomeStatus =
  | { kind: "done" }
  | { kind: "pending"; detail: string }
  | { kind: "failed"; error: string };

export type SyncActionOutcome = {
  path: string;
  reason: string;
  status: SyncOutcomeStatus;
};

export type SyncReport = {
  plan: { actions: SyncPlannedAction[]; summary: SyncPlanSummary };
  outcomes: SyncActionOutcome[];
  tombstonesWritten: number;
  /** 成功推进到的逻辑序号。有任何挂起或失败时是 `null`。 */
  seq: number | null;
};

/** 与 Rust 的 `store::StoredResolution` 对齐。 */
export type StoredResolution = {
  path: string;
  resolution: SyncResolution;
  localHash: string;
  remoteHash: string;
  decidedAt: number;
};

/**
 * 后台守护线程跑完一轮、并且**确实改了东西**之后发的事件。
 *
 * 两件事得记住:
 *
 * - **载荷是空的**(Rust 侧 `app.emit(SYNC_EVENT, ())`),里面没有 vault。所以监听方没法
 *   按 vault 过滤,只能收到就去重查自己那个 vault。
 * - **只有守护线程发它。** `notebook_sync_run`(手动那一路)不发 —— 所以在这个事件里回头
 *   调 `runSync` 不会自激。
 */
export const SYNC_EVENT = "notebook-sync-updated";

export async function listSyncRemotes(vault: string): Promise<SyncRemoteTarget[]> {
  return invoke<SyncRemoteTarget[]>("notebook_sync_remotes", { vault });
}

export async function syncStatus(vault: string, remoteIds: string[]): Promise<SyncRemoteStatus[]> {
  return invoke<SyncRemoteStatus[]>("notebook_sync_status", { vault, remoteIds });
}

export async function setSyncAuto(
  vault: string,
  remoteId: string,
  enabled: boolean,
): Promise<void> {
  return invoke<void>("notebook_sync_set_auto", { vault, remoteId, enabled });
}

/**
 * 跑一轮。
 *
 * `strategy` 省略时按 `ask` —— 冲突挂起等用户,不动文件。传 `local` / `remote` 是 **vault
 * 级**的一律处理,会作用到这一轮**所有**冲突文件上;想逐个处理用 [`resolveConflict`]。
 */
export async function runSync(
  vault: string,
  remoteId: string,
  strategy?: ConflictStrategy,
): Promise<SyncReport> {
  return invoke<SyncReport>("notebook_sync_run", {
    vault,
    remoteId,
    strategy: strategy ?? null,
  });
}

/**
 * 对一个冲突路径做决定。下一轮同步时执行。
 *
 * `localHash` / `remoteHash` 必须是冲突动作上那两个字段的**原值**。用 [`conflictHashes`]
 * 取,不要自己算。
 */
export async function resolveConflict(args: {
  vault: string;
  remoteId: string;
  path: string;
  resolution: SyncResolution;
  localHash: string;
  remoteHash: string;
}): Promise<void> {
  return invoke<void>("notebook_sync_resolve", {
    vault: args.vault,
    remoteId: args.remoteId,
    path: args.path,
    resolution: args.resolution,
    localHash: args.localHash,
    remoteHash: args.remoteHash,
  });
}

/** 撤回一条决定。 */
export async function clearConflictResolution(
  vault: string,
  remoteId: string,
  path: string,
): Promise<void> {
  return invoke<void>("notebook_sync_resolve", {
    vault,
    remoteId,
    path,
    resolution: null,
    localHash: null,
    remoteHash: null,
  });
}

/** 这个远端上还存着的决定。面板重开之后要靠它恢复「已决定,等下一轮」那些行。 */
export async function listConflictResolutions(
  vault: string,
  remoteId: string,
): Promise<StoredResolution[]> {
  return invoke<StoredResolution[]>("notebook_sync_resolutions", { vault, remoteId });
}

/** 从一条计划动作里取那两个 hash。不是冲突就返回 `null`。 */
export function conflictHashes(
  action: SyncAction,
): { localHash: string; remoteHash: string } | null {
  if (action.kind !== "conflict") return null;
  return { localHash: action.localHash, remoteHash: action.remoteHash };
}

/** 这一轮里还等着用户处理的那些冲突。 */
export function pendingConflicts(report: SyncReport | null): SyncPlannedAction[] {
  if (!report) return [];
  const stuck = new Set(
    report.outcomes
      .filter((o) => o.status.kind === "pending" || o.status.kind === "failed")
      .map((o) => o.path),
  );
  return report.plan.actions.filter((a) => a.action.kind === "conflict" && stuck.has(a.path));
}

/** 这一轮有没有失败的动作。 */
export function syncFailures(report: SyncReport | null): SyncActionOutcome[] {
  if (!report) return [];
  return report.outcomes.filter((o) => o.status.kind === "failed");
}

/**
 * 一轮的总体结果。
 *
 * 和后端 `daemon::classify` 同一套口径:`seq` 推进了就是完整落定;否则区分「有失败」和
 * 「只是等用户」—— 后者不是故障,不该显示成错误。
 */
export type SyncVerdict = "settled" | "awaitingUser" | "failed";

export function syncVerdict(report: SyncReport | null): SyncVerdict | null {
  if (!report) return null;
  if (report.seq !== null) return "settled";
  if (report.outcomes.some((o) => o.status.kind === "failed")) return "failed";
  return "awaitingUser";
}

/** 冲突面板上一行的展示状态。 */
export type ConflictRowState =
  /** 还没决定。 */
  | { kind: "undecided" }
  /** 已经决定,等下一轮执行。 */
  | { kind: "decided"; resolution: SyncResolution }
  /**
   * 决定过,但两侧内容在那之后又变了 —— 后端下一轮会作废它并重新挂起。
   *
   * 这一档必须单独显示。混进 `decided` 的话用户会一直等一个永远不会执行的决定;混进
   * `undecided` 的话他会以为自己上次没点成功。
   */
  | { kind: "stale"; resolution: SyncResolution };

/**
 * 把「存着的决定」和「本轮看到的冲突」对起来。
 *
 * 判 `stale` 的口径必须和 Rust 侧 `diff::decided_for` 一致:**两侧 hash 都要相等**。只比
 * 一侧的话,另一侧变了时前端说「已决定」而后端作废了它,两边永远对不上。
 */
export function conflictRowState(
  action: SyncPlannedAction,
  decided: StoredResolution[],
): ConflictRowState {
  const hashes = conflictHashes(action.action);
  const found = decided.find((d) => d.path === action.path);
  if (!found || !hashes) return { kind: "undecided" };
  const fresh = found.localHash === hashes.localHash && found.remoteHash === hashes.remoteHash;
  return fresh
    ? { kind: "decided", resolution: found.resolution }
    : { kind: "stale", resolution: found.resolution };
}

/**
 * fork 的默认落点:`笔记.conflict.md`。
 *
 * 拼在扩展名**之前**,这样它还是一篇 `.md` —— 随手记只索引 `.md`,拼成 `a.md.conflict`
 * 的话那份内容在列表、搜索、反链里全都不出现,而用户选 fork 的意思正是「这份我要留着看」。
 */
export function defaultForkPath(path: string): string {
  const dot = path.lastIndexOf(".");
  const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (dot <= slash + 1) return `${path}.conflict`;
  return `${path.slice(0, dot)}.conflict${path.slice(dot)}`;
}

/**
 * 退避 / 下一轮的倒计时文案用的秒数。向上取整 —— 显示 0 秒但还没跑会显得卡住了。
 *
 * `elapsedMs` 是「拿到这份状态之后又过去了多久」。`nextRunInMs` 是取的那一刻的快照,不减掉
 * 这一段的话数字会卡在快照上不动,直到下一次轮询才跳一大格。
 */
export function nextRunSeconds(
  status: SyncRemoteStatus | null,
  elapsedMs: number = 0,
): number | null {
  if (!status || status.nextRunInMs === null) return null;
  return Math.max(0, Math.ceil((status.nextRunInMs - elapsedMs) / 1000));
}

/**
 * `reason` → i18n key。
 *
 * 后端那些是稳定标识(`both_modified` 之类),不是文案。直接显示的话用户看到的是
 * snake_case 的英文标识,而且中文界面下也是英文。认不出的落到通用文案而不是原样透出 ——
 * 后端加了新 reason 时,「冲突」比 `some_new_reason` 好懂。
 */
const REASON_KEYS: Record<string, string> = {
  both_modified: "notebook.sync.reason.bothModified",
  remote_deleted_local_modified: "notebook.sync.reason.remoteDeletedLocalModified",
  local_tombstone_remote_modified: "notebook.sync.reason.localTombstoneRemoteModified",
  both_present_no_baseline: "notebook.sync.reason.bothPresentNoBaseline",
};

export function syncReasonKey(reason: string): string {
  return REASON_KEYS[reason] ?? "notebook.sync.reason.unknown";
}

/** `Pending` 的 `detail` → i18n key。同上,那些也是标识不是文案。 */
const PENDING_KEYS: Record<string, string> = {
  oversize_not_hashable: "notebook.sync.pending.oversizeNotHashable",
  local_gone_during_sync: "notebook.sync.pending.localGoneDuringSync",
  local_appeared_during_sync: "notebook.sync.pending.localAppearedDuringSync",
  awaiting_user: "notebook.sync.pending.awaitingUser",
};

export function syncPendingKey(detail: string): string {
  return PENDING_KEYS[detail] ?? "notebook.sync.pending.unknown";
}

/** resolution → 按钮文案的 i18n key。 */
export function resolutionLabelKey(resolution: SyncResolution): string {
  switch (resolution.kind) {
    case "keepLocal":
      return "notebook.sync.keepLocal";
    case "keepRemote":
      return "notebook.sync.keepRemote";
    case "fork":
      return "notebook.sync.fork";
  }
}
