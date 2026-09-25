import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import type { Task } from "../types";
import {
  createTaskIntentQueue,
  METADATA_TASK_INTENT_OPS,
  type TaskIntent,
  type TaskIntentOutcome,
} from "../taskIntentQueue";

function outcome(projectId: string, ids: string[]): TaskIntentOutcome {
  const tasks: Task[] = ids.map((id) => ({
    id,
    projectId,
    prompt: id,
    agent: "claude",
    permissionMode: "ask",
    status: "todo",
    createdAt: 1,
  }));
  return { tasks, revision: "7", changed: true, committedByCore: true };
}

const rename = (taskId: string, name: string): TaskIntent => ({
  op: "rename",
  taskId,
  name,
});

describe("createTaskIntentQueue", () => {
  it("sends intents in FIFO order per project and hands back the authoritative snapshot", async () => {
    const sent: Array<[string, TaskIntent]> = [];
    const queue = createTaskIntentQueue(async (projectId, intent) => {
      sent.push([projectId, intent]);
      return outcome(projectId, ["t1", "t2"]);
    });
    const adopted = vi.fn();

    queue.enqueue("p1", rename("t1", "第一个"));
    queue.enqueue("p1", { op: "toggleStar", taskId: "t2" }, { onAdopt: adopted });
    await queue.flush("p1");

    expect(sent.map(([, intent]) => intent.op)).toEqual(["rename", "toggleStar"]);
    expect(adopted).toHaveBeenCalledTimes(1);
    expect(adopted.mock.calls[0][0].map((t: Task) => t.id)).toEqual(["t1", "t2"]);
    expect(queue.pendingCount()).toBe(0);
  });

  it("replays a rejected metadata intent exactly once", async () => {
    const calls: string[] = [];
    const queue = createTaskIntentQueue(async (projectId, intent) => {
      calls.push(intent.op);
      if (calls.length === 1) throw new Error("REVISION_CONFLICT: stale write refused");
      return outcome(projectId, ["t1"]);
    });
    const onError = vi.fn();

    queue.enqueue("p1", rename("t1", "重放一次"), { onError });
    await queue.flush("p1");

    expect(calls).toEqual(["rename", "rename"]);
    expect(onError).not.toHaveBeenCalled();
  });

  it("never replays a state intent and reports the refusal to the caller", async () => {
    const calls: string[] = [];
    const queue = createTaskIntentQueue(async (_projectId, intent) => {
      calls.push(intent.op);
      throw new Error("REVISION_CONFLICT: stale write refused");
    });
    const onError = vi.fn();

    queue.enqueue("p1", { op: "delete", taskIds: ["t1"] }, { onError });
    // 与旧快照队列同语义：永久失败必须能被 await flush 的调用方看见（远程 launch 门）。
    await expect(queue.flush("p1")).rejects.toThrow(/REVISION_CONFLICT/);

    expect(calls, "ADR-026：状态 op 永不自动重放").toEqual(["delete"]);
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("reports a non-conflict error once without any replay, metadata included", async () => {
    const calls: string[] = [];
    const queue = createTaskIntentQueue(async (_projectId, intent) => {
      calls.push(intent.op);
      throw new Error("disk is full");
    });
    const onError = vi.fn();

    queue.enqueue("p1", rename("t1", "网络断了"), { onError });
    await expect(queue.flush("p1")).rejects.toThrow("disk is full");

    expect(calls, "重放只属于 REVISION_CONFLICT，不是所有失败").toEqual(["rename"]);
    expect(onError).toHaveBeenCalledWith(expect.stringContaining("disk is full"));
  });

  it("does not attribute an older, already-reported failure to a later flush", async () => {
    let fail = true;
    const queue = createTaskIntentQueue(async (projectId) => {
      if (fail) throw new Error("REVISION_CONFLICT: refused");
      return outcome(projectId, ["t1"]);
    });
    const onError = vi.fn();
    queue.enqueue("p1", { op: "delete", taskIds: ["t1"] }, { onError });
    await vi.waitFor(() => expect(onError).toHaveBeenCalled());
    fail = false;
    // 陈旧失败属于上一个调用点（toast 已报）；这次 flush 只看见新失败或不失败。
    await expect(queue.flush("p1")).resolves.toBeUndefined();
  });

  it("coalesces same-key intents enqueued before the window drains, keeping the latest payload", async () => {
    const sent: TaskIntent[] = [];
    const queue = createTaskIntentQueue(async (projectId, intent) => {
      sent.push(intent);
      return outcome(projectId, ["t1"]);
    });

    queue.enqueue("p1", { op: "recordDiffStats", taskId: "t1", additions: 1, deletions: 1 }, {
      operationKey: "diff-run-42",
    });
    // 还没发出 ⇒ 同键换载荷不换位置（今天的 `pendingRef` 语义在队列内的化身）。
    queue.enqueue("p1", { op: "recordDiffStats", taskId: "t1", additions: 9, deletions: 9 }, {
      operationKey: "diff-run-42",
    });
    queue.enqueue("p1", rename("t1", "不同键照常排队"));
    await queue.flush("p1");

    expect(sent.map((intent) => intent.op)).toEqual(["recordDiffStats", "rename"]);
    const stats = sent.find((intent) => intent.op === "recordDiffStats");
    expect(stats, "最后一次算出的数字才是那一趟运行的事实").toMatchObject({
      additions: 9,
      deletions: 9,
    });
  });

  it("keeps a second send of an already-applied key as its own intent", async () => {
    const sent: TaskIntent[] = [];
    const queue = createTaskIntentQueue(async (projectId, intent) => {
      sent.push(intent);
      return outcome(projectId, ["t1"]);
    });

    queue.enqueue("p1", { op: "recordDiffStats", taskId: "t1", additions: 1, deletions: 1 }, {
      operationKey: "k",
    });
    await queue.flush("p1");
    queue.enqueue("p1", { op: "recordDiffStats", taskId: "t1", additions: 1, deletions: 1 }, {
      operationKey: "k",
    });
    await queue.flush("p1");

    expect(sent.length, "落过盘的键不再是'排着的同键'——幂等属于事实层，不靠队列抹掉第二次真写").toBe(2);
  });

  it("flushAll drains projects enqueued while another project's flush is in flight", async () => {
    const sent: string[] = [];
    const queue = createTaskIntentQueue(async (projectId, intent) => {
      sent.push(`${projectId}:${intent.op}`);
      if (projectId === "p1") {
        // 模拟退出路径：排空 p1 的过程中，另一个项目被顺手 enqueue（旧 flushAll 同样要等它）。
        queue.enqueue("p2", rename("t2", "退出前补上"));
      }
      return outcome(projectId, []);
    });

    queue.enqueue("p1", rename("t1", "先来的"));
    await queue.flushAll();

    expect(sent).toContain("p1:rename");
    expect(sent).toContain("p2:rename");
    expect(queue.pendingCount()).toBe(0);
  });

  it("mirrors the contract's split: ten metadata ops are replay-eligible, the vocabulary has fourteen", () => {
    const published = JSON.parse(
      readFileSync("contracts/native/v1/envelope.schema.json", "utf8"),
    ).constants.taskIntents;
    expect([...METADATA_TASK_INTENT_OPS].sort()).toEqual([...published.metadata].sort());
    expect(published.ops.length).toBe(14);
  });
});
