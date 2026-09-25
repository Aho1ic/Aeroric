//! ADR-022 的保存队列：排的是**意图**（`op` + 载荷），不是整项目快照。
//!
//! 与旧的 `taskPersistence.ts` 的三点不同，每一点都有 ADR 背书：
//! 1. 不做 350 ms 快照合并；同键合并只发生在显式给了 `operationKey` 且**还没发出**的
//!    条目上（ADR-017：重试身份是调用方的操作键。落过盘的键不算"排着的同键"）。
//! 2. 失败按 ADR-026 分两类：元数据 op 在 `REVISION_CONFLICT` 时**重放一次**，
//!    状态 op 永不重放、直接把错误交回调用方。
//! 3. 成功返回的就是权威快照（`TaskIntentOutcome.tasks`），宿主采纳它而不是继续持有
//!    自己算出的整表。

import { invoke } from "@tauri-apps/api/core";
import type { Task } from "./types";

/** `constants.taskIntents.metadata` 那份分类的 TS 侧镜像；与产物的一致性由
 *  `contracts-parity.test.ts` 与本目录的队列用例守着，这里不许长出第四份名单。 */
export const METADATA_TASK_INTENT_OPS = [
  "rename",
  "toggleStar",
  "archive",
  "unarchive",
  "attachSession",
  "setWorktree",
  "discardWorktree",
  "recordDiffStats",
  "normalizeAfterBoot",
] as const;

/** 一行的会话字段族（11 键）。`SwitchConfig` 的三路合并要 previous/resolved 两份。 */
export interface TaskSessionSnapshot {
  claudeSessionId?: string;
  claudeSessionPath?: string;
  codexSessionId?: string;
  codexSessionPath?: string;
  dshSessionId?: string;
  dshSessionPath?: string;
  ompSessionId?: string;
  ompSessionPath?: string;
  sessionAgent?: string;
  sessionCodexLike?: boolean;
  sessionFamily?: string;
}

/** Q71（ADR-037 决策 4）：LLM 生成名字的过期判定。前置四样由 core 在权威快照上判等，
 *  不等 ⇒ `changed:false`、不报错、不烧 revision。 */
export interface RenameExpectation {
  name: string;
  prompt: string;
  status: string;
  sessionPath?: string;
}

export type TaskIntent =
  | { op: "rename"; taskId: string; name: string; expect?: RenameExpectation }
  | { op: "toggleStar"; taskId: string }
  | { op: "archive"; taskIds: string[] }
  | { op: "unarchive"; taskIds: string[] }
  | {
      op: "attachSession";
      taskId: string;
      family: string;
      sessionAgent: string;
      sessionId?: string;
      sessionPath?: string;
    }
  | { op: "setWorktree"; taskId: string; path: string; branch: string; baseBranch: string }
  | { op: "discardWorktree"; taskId: string }
  | { op: "recordDiffStats"; taskId: string; additions: number; deletions: number }
  | {
      op: "create";
      id: string;
      prompt: string;
      agent: string;
      permissionMode: string;
      immediate: boolean;
      selectedModel?: string;
      reasoningEffort?: string;
      speed?: string;
      dshAgentPreset?: string;
    }
  | { op: "delete"; taskIds: string[] }
  | { op: "normalizeAfterBoot"; liveTaskIds: string[] }
  | {
      op: "switchConfig";
      taskId: string;
      agent: string;
      selectedModel?: string;
      reasoningEffort?: string;
      speed?: string;
      permissionMode: string;
      previousSession: TaskSessionSnapshot;
      resolvedSession: TaskSessionSnapshot;
    }
  | {
      op: "resumeWithSession";
      taskId: string;
      agent: string;
      family: string;
      sessionId?: string;
      sessionPath?: string;
    };

export interface TaskIntentOutcome {
  tasks: Task[];
  revision: string | null;
  changed: boolean;
  committedByCore: boolean;
}

type Entry = {
  intent: TaskIntent;
  operationKey?: string;
  onAdopt?: (tasks: Task[]) => void;
  onError?: (msg: string) => void;
  formatError?: (error: string, projectId: string) => string;
  replayed: boolean;
};

export type TaskIntentInvoker = (
  projectId: string,
  intent: TaskIntent,
) => Promise<TaskIntentOutcome>;

export interface TaskIntentQueue {
  enqueue: (
    projectId: string,
    intent: TaskIntent,
    opts?: {
      operationKey?: string;
      onAdopt?: (tasks: Task[]) => void;
      onError?: (msg: string) => void;
      formatError?: (error: string, projectId: string) => string;
    },
  ) => void;
  flush: (projectId: string) => Promise<void>;
  flushAll: () => Promise<void>;
  pendingCount: () => number;
}

function isMetadataOp(op: TaskIntent["op"]): boolean {
  return (METADATA_TASK_INTENT_OPS as readonly string[]).includes(op);
}

/** `REVISION_CONFLICT` 与指纹不符都归 ADR-015/026 的"被拒的写"。匹配失败按普通错误
 *  处理——宁可对未知错误保守，也不把"永不重放"的状态 op 卷进重试。 */
function isStaleWrite(error: unknown): boolean {
  const text = String(error);
  return /REVISION_CONFLICT|stale write/i.test(text);
}

export function createTaskIntentQueue(
  invokeIntent: TaskIntentInvoker,
  defaultOnError?: (msg: string) => void,
): TaskIntentQueue {
  const queues = new Map<string, Entry[]>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const draining = new Map<string, Promise<void>>();

  function report(entry: Entry, projectId: string, error: unknown): void {
    const text = entry.formatError
      ? entry.formatError(String(error), projectId)
      : String(error);
    (entry.onError ?? defaultOnError ?? ((m) => console.error(m)))(text);
  }

  function queueFor(projectId: string): Entry[] {
    let queue = queues.get(projectId);
    if (!queue) {
      queue = [];
      queues.set(projectId, queue);
    }
    return queue;
  }

  async function drain(projectId: string): Promise<void> {
    const queue = queueFor(projectId);
    while (queue.length > 0) {
      const entry = queue.shift() as Entry;
      try {
        const outcome = await invokeIntent(projectId, entry.intent);
        entry.onAdopt?.(outcome.tasks);
      } catch (error) {
        if (isStaleWrite(error) && isMetadataOp(entry.intent.op) && !entry.replayed) {
          // ADR-026：元数据重放一次。"权威快照"不用宿主补——下一次 invoke 里 core 自己 load。
          entry.replayed = true;
          queue.unshift(entry);
          continue;
        }
        report(entry, projectId, error);
      }
    }
    if (queues.get(projectId) === queue) {
      queues.delete(projectId);
    }
  }

  function startDrain(projectId: string): Promise<void> {
    const running = draining.get(projectId);
    if (running) return running;
    const promise = drain(projectId)
      .catch((error) => console.error("intent queue drain failed", projectId, error))
      .finally(() => {
        draining.delete(projectId);
        const timer = timers.get(projectId);
        if (timer) {
          clearTimeout(timer);
          timers.delete(projectId);
          void startDrain(projectId);
        }
      });
    draining.set(projectId, promise);
    return promise;
  }

  function schedule(projectId: string): void {
    if (draining.has(projectId) || timers.has(projectId)) return;
    // 微任务窗口：同步的一阵 enqueue 先合并再发（旧队列 350 ms 防抖的意图版——
    // 合并的判据是"同一个 operationKey 还没发出"，不再是"整表覆盖整表"）。
    timers.set(
      projectId,
      setTimeout(() => {
        timers.delete(projectId);
        void startDrain(projectId);
      }, 0)
    );
  }

  return {
    enqueue(projectId, intent, opts = {}) {
      const entry: Entry = {
        intent,
        operationKey: opts.operationKey,
        onAdopt: opts.onAdopt,
        onError: opts.onError,
        formatError: opts.formatError,
        replayed: false,
      };
      const queue = queueFor(projectId);
      if (opts.operationKey) {
        const pendingIndex = queue.findIndex(
          (item) => item.operationKey === opts.operationKey,
        );
        if (pendingIndex >= 0) {
          queue[pendingIndex] = entry;
          return;
        }
      }
      queue.push(entry);
      schedule(projectId);
    },
    async flush(projectId) {
      for (;;) {
        const timer = timers.get(projectId);
        if (timer) {
          clearTimeout(timer);
          timers.delete(projectId);
        }
        const running = draining.get(projectId);
        if (running) {
          await running;
          continue;
        }
        if (!queues.has(projectId) || queues.get(projectId)!.length === 0) return;
        await startDrain(projectId);
      }
    },
    async flushAll() {
      // 与旧 `flushAll` 同一姿态：排空期间被顺手 enqueue 的也要等完（退出前的那份）。
      for (;;) {
        const projects = new Set([...queues.keys(), ...draining.keys(), ...timers.keys()]);
        if (projects.size === 0) return;
        for (const projectId of projects) {
          await this.flush(projectId);
        }
      }
    },
    pendingCount() {
      let n = 0;
      for (const queue of queues.values()) n += queue.length;
      return n;
    },
  };
}

export const taskIntentQueue: TaskIntentQueue = createTaskIntentQueue(
  (projectId, intent) =>
    invoke<TaskIntentOutcome>("apply_task_intent", { projectId, intent }),
  (msg) => console.error(msg),
);

export function queueTaskIntent(
  projectId: string,
  intent: TaskIntent,
  opts?: Parameters<TaskIntentQueue["enqueue"]>[2],
): void {
  taskIntentQueue.enqueue(projectId, intent, opts);
}

export function flushProjectIntents(projectId: string): Promise<void> {
  return taskIntentQueue.flush(projectId);
}

export function flushAllProjectIntents(): Promise<void> {
  return taskIntentQueue.flushAll();
}
