//! App 装配层测试的 fake core：把 `apply_task_intent` 的语义按**客户端自己的纯函数**
//! 应用到内存 store，并记录每次"落盘"。权威规则本身由 core 的门禁测（tasks/storage），
//! 这里只保证装配层能对着 `savedTasksFor` 继续断言"盘上最后是什么"。

import type { Task } from "../types";
import { normalizeInterruptedTasksOnStartup } from "../appProjectState";
import {
  archiveTasksInList,
  unarchiveTasksInList,
  renameTaskInList,
  toggleTaskStarInList,
} from "../state/app/taskMutations";
import { updateTodoTaskInList } from "../state/app/taskNaming";
import { applyTaskStatusTransition } from "../state/app/taskStatus";
import type { TaskIntent, TaskIntentOutcome } from "../taskIntentQueue";
import type { TaskStatus } from "../types";

type AnyTask = Task & Record<string, unknown>;

/** 换体后的七条启动/续跑命令——命令名与 JS 参数不变，落盘与 spawn 都在命令体内完成。 */
const LAUNCH_COMMANDS = new Set([
  "run_task",
  "run_remote_task",
  "run_wsl_task",
  "run_dsh_task",
  "resume_task",
  "resume_remote_task",
  "resume_wsl_task",
]);

function sessionPathOf(task: Task): string | null {
  const family = task.sessionFamily ?? (task.sessionCodexLike ? "codex" : "claude");
  const paths: Record<string, string | undefined> = {
    claude: task.claudeSessionPath,
    codex: task.codexSessionPath,
    dsh: task.dshSessionPath,
    omp: task.ompSessionPath,
  };
  return paths[family] ?? null;
}

export interface FakeCore {
  invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  savedTasksFor: (projectId: string) => Task[];
  store: Record<string, Task[]>;
  /** 真观察通道的镜像（接线 §12 ③）：core 决策并提交，announce 由测试的事件桥单独驱动。 */
  observeTaskStatus: (
    projectId: string,
    taskId: string,
    status: TaskStatus,
    failureReason?: string,
  ) => { changed: boolean };
}

export function createFakeCore(
  seed: Record<string, Task[]>,
  opts: { now?: () => number } = {},
): FakeCore {
  const store: Record<string, Task[]> = Object.fromEntries(
    Object.entries(seed).map(([pid, rows]) => [pid, rows.map((r) => ({ ...r }))]),
  );
  const lastSaved: Record<string, Task[]> = {};
  const now = opts.now ?? Date.now;

  function commit(projectId: string): void {
    lastSaved[projectId] = store[projectId].map((r) => ({ ...r }));
  }

  function applyIntent(projectId: string, intent: TaskIntent): TaskIntentOutcome {
    const rows: Task[] = store[projectId] ?? [];
    let changed = false;
    switch (intent.op) {
      case "rename": {
        const expect = intent.expect;
        const current = rows.find((r) => r.id === intent.taskId);
        const guardOk =
          !expect ||
          (!!current &&
            (current.name ?? "") === expect.name &&
            current.prompt === expect.prompt &&
            current.status === expect.status &&
            sessionPathOf(current) === (expect.sessionPath ?? null));
        if (expect && !guardOk) break;
        const next = renameTaskInList(rows, intent.taskId, intent.name);
        changed = next !== rows;
        store[projectId] = next;
        break;
      }
      case "toggleStar":
        store[projectId] = toggleTaskStarInList(rows, intent.taskId);
        changed = true;
        break;
      case "archive":
        store[projectId] = archiveTasksInList(rows, intent.taskIds, now());
        changed = true;
        break;
      case "unarchive":
        store[projectId] = unarchiveTasksInList(rows, intent.taskIds);
        changed = true;
        break;
      case "editTodo": {
        const next = updateTodoTaskInList(rows, intent.taskId, {
          prompt: intent.prompt,
          agent: intent.agent as Task["agent"],
          permissionMode: intent.permissionMode as Task["permissionMode"],
        });
        changed = next !== rows;
        store[projectId] = next;
        break;
      }
      case "normalizeAfterBoot": {
        const live = new Set(intent.liveTaskIds);
        const { tasks } = normalizeInterruptedTasksOnStartup(rows, live);
        const cleaned = tasks.map((t) =>
          t.speed === "fast" && t.agent === ("dsh" as Task["agent"])
            ? { ...t, speed: "standard" as Task["speed"] }
            : t,
        );
        changed = JSON.stringify(cleaned) !== JSON.stringify(rows);
        store[projectId] = cleaned;
        break;
      }
      case "attachSession": {
        const next = rows.map((t) => {
          if (t.id !== intent.taskId) return t;
          if (!intent.sessionId && !intent.sessionPath) return t;
          const cleared: AnyTask = {
            ...t,
            claudeSessionId: undefined,
            claudeSessionPath: undefined,
            codexSessionId: undefined,
            codexSessionPath: undefined,
            dshSessionId: undefined,
            dshSessionPath: undefined,
            ompSessionId: undefined,
            ompSessionPath: undefined,
            sessionAgent: intent.sessionAgent,
            sessionCodexLike: intent.family === "codex",
            sessionFamily: intent.family as Task["sessionFamily"],
          };
          if (intent.family === "claude") {
            cleared.claudeSessionId = intent.sessionId;
            cleared.claudeSessionPath = intent.sessionPath;
          }
          if (intent.family === "codex") {
            cleared.codexSessionId = intent.sessionId;
            cleared.codexSessionPath = intent.sessionPath;
          }
          if (intent.family === "dsh") {
            cleared.dshSessionId = intent.sessionId;
            cleared.dshSessionPath = intent.sessionPath;
          }
          if (intent.family === "omp") {
            cleared.ompSessionId = intent.sessionId;
            cleared.ompSessionPath = intent.sessionPath;
          }
          return cleared;
        });
        changed = JSON.stringify(next) !== JSON.stringify(rows);
        store[projectId] = next;
        break;
      }
      case "setWorktree":
        store[projectId] = rows.map((t) =>
          t.id === intent.taskId
            ? {
                ...t,
                worktreePath: intent.path,
                worktreeBranch: intent.branch,
                baseBranch: intent.baseBranch,
              }
            : t,
        );
        changed = true;
        break;
      case "discardWorktree":
        store[projectId] = rows.map((t) =>
          t.id === intent.taskId ? { ...t, worktreeDiscarded: true } : t,
        );
        changed = true;
        break;
      case "recordDiffStats":
        store[projectId] = rows.map((t) =>
          t.id === intent.taskId
            ? { ...t, additions: intent.additions, deletions: intent.deletions }
            : t,
        );
        changed = true;
        break;
      case "create": {
        if (rows.some((r) => r.id === intent.id)) throw new Error("DUPLICATE_ID");
        const created: Task = {
          id: intent.id,
          projectId,
          prompt: intent.prompt,
          name: intent.prompt.trim() ? undefined : intent.agent,
          agent: intent.agent as Task["agent"],
          selectedModel: intent.selectedModel,
          reasoningEffort: intent.reasoningEffort,
          speed: intent.speed as Task["speed"],
          dshAgentPreset: intent.dshAgentPreset,
          permissionMode: intent.permissionMode as Task["permissionMode"],
          status: intent.immediate ? "pending" : "todo",
          createdAt: now(),
        };
        store[projectId] = [created, ...rows];
        changed = true;
        break;
      }
      case "delete": {
        const ids = new Set(intent.taskIds);
        store[projectId] = rows.filter((r) => !(ids.has(r.id) && !r.starred));
        changed = true;
        break;
      }
      case "resumeWithSession":
      case "switchConfig": {
        store[projectId] = rows.map((t) => {
          if (t.id !== intent.taskId) return t;
          const base: AnyTask = {
            ...t,
            agent: intent.agent as Task["agent"],
            status: "pending" as Task["status"],
            attentionRequestedAt: undefined,
            failureReason: undefined,
          };
          if (intent.op === "resumeWithSession") {
            const family = intent.family as "claude" | "codex" | "dsh" | "omp";
            base.sessionAgent = intent.agent;
            base.sessionCodexLike = family === "codex";
            base.sessionFamily = family;
            base.claudeSessionId = family === "claude" ? intent.sessionId : undefined;
            base.claudeSessionPath = family === "claude" ? intent.sessionPath : undefined;
            base.codexSessionId = family === "codex" ? intent.sessionId : undefined;
            base.codexSessionPath = family === "codex" ? intent.sessionPath : undefined;
            base.dshSessionId = family === "dsh" ? intent.sessionId : undefined;
            base.dshSessionPath = family === "dsh" ? intent.sessionPath : undefined;
            base.ompSessionId = family === "omp" ? intent.sessionId : undefined;
            base.ompSessionPath = family === "omp" ? intent.sessionPath : undefined;
            return base;
          }
          const resolved = intent.resolvedSession as Record<string, unknown>;
          const previous = intent.previousSession as Record<string, unknown>;
          for (const key of Object.keys(resolved)) {
            if (resolved[key] !== previous[key]) base[key] = resolved[key];
          }
          base.selectedModel = intent.selectedModel;
          base.reasoningEffort = intent.reasoningEffort;
          base.speed = intent.speed;
          base.permissionMode = intent.permissionMode as Task["permissionMode"];
          return base;
        });
        changed = true;
        break;
      }
      default:
        break;
    }
    if (changed) commit(projectId);
    return {
      tasks: store[projectId].map((r) => ({ ...r })),
      revision: changed ? String(Object.keys(lastSaved).length) : null,
      changed,
      committedByCore: true,
    };
  }

  /** 换体后的启动命令镜像：真命令体由 core 的 begin 先落 pending 再 spawn，返回权威快照。 */
  function launchMirror(args?: Record<string, unknown>): TaskIntentOutcome {
    const taskId = String(args?.taskId);
    const pid = Object.keys(store).find((project) =>
      store[project].some((row) => row.id === taskId),
    );
    if (!pid) throw new Error(`fake core: no project owns task ${taskId}`);
    // prepare_for_launch 的镜像：pending + 清 attentionRequestedAt（Resume 还清 failureReason）。
    store[pid] = store[pid].map((row) =>
      row.id === taskId
        ? ({
            ...row,
            status: "pending",
            attentionRequestedAt: undefined,
            failureReason: undefined,
          } as Task)
        : row,
    );
    commit(pid);
    return {
      tasks: store[pid].map((r) => ({ ...r })),
      revision: String(Object.keys(lastSaved).length),
      changed: true,
      committedByCore: true,
    };
  }

  async function invoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
    if (command === "apply_task_intent") {
      return applyIntent(String(args?.projectId), args?.intent as TaskIntent);
    }
    // `save_project_tasks` 已退役（§12 ④）：装配层任何一次整表快照写都会在这里炸出来，
    // 而不是悄悄走第二条车道。
    if (LAUNCH_COMMANDS.has(command)) {
      return launchMirror(args);
    }
    throw new Error(`fake core: unexpected command ${command}`);
  }

  const observeTaskStatus = (
    projectId: string,
    taskId: string,
    status: TaskStatus,
    failureReason?: string,
  ) => {
    const { tasks, changed } = applyTaskStatusTransition(
      store[projectId] ?? [],
      taskId,
      status,
      undefined,
      failureReason,
      now(),
    );
    if (changed) {
      store[projectId] = tasks;
      commit(projectId);
    }
    return { changed };
  };

  return {
    invoke,
    store,
    savedTasksFor: (projectId) => lastSaved[projectId] ?? [],
    observeTaskStatus,
  };
}
