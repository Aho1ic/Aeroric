import { beforeEach, describe, expect, it } from "vitest";
import { setLanguage } from "../i18n";
import { TaskNotificationGate } from "./notification-gate";

describe("TaskNotificationGate", () => {
  beforeEach(() => {
    setLanguage("zh");
  });

  it("separates a real approval from a turn handing the session back", () => {
    const gate = new TaskNotificationGate();
    expect(gate.evaluate("t1", "running", "修 bug")).toBeNull();
    // 交互式回合跑完(Claude/Codex 的 Stop、DSH 的 turn/end)不带 approval,
    // 仍然提醒,但标题是"等待输入"而不是"等待确认"。
    const idle = gate.evaluate("t1", "input_required", "修 bug");
    expect(idle!.title).toBe("任务等待输入");
    expect(idle!.body).toBe("修 bug");
    expect(gate.evaluate("t1", "running", "修 bug")).toBeNull();
    const approval = gate.evaluate("t1", "input_required", "修 bug", {
      requestId: "approval-1",
      kind: "permission",
    });
    expect(approval!.title).toBe("任务等待确认");
    expect(gate.evaluate("t1", "done", "修 bug")!.title).toBe("任务已完成");
  });

  it("notifies on failure", () => {
    const gate = new TaskNotificationGate();
    expect(gate.evaluate("t2", "failed", "构建")!.title).toBe("任务失败");
    // 同状态重复推送仍然去重。
    expect(gate.evaluate("t2", "failed", "构建")).toBeNull();
  });

  it("dedupes the same status but re-notifies after a round trip", () => {
    const gate = new TaskNotificationGate();
    const approval = { requestId: "approval-1", kind: "permission" };
    expect(gate.evaluate("t1", "input_required", "a", approval)).not.toBeNull();
    // 补发 + 实时重复推同一状态 → 静默
    expect(gate.evaluate("t1", "input_required", "a", approval)).toBeNull();
    // 状态往复后再次 input_required → 重新提醒
    expect(gate.evaluate("t1", "running", "a")).toBeNull();
    expect(gate.evaluate("t1", "input_required", "a", approval)).not.toBeNull();
  });

  it("does not let an ordinary idle event suppress a later approval", () => {
    const gate = new TaskNotificationGate();
    expect(gate.evaluate("t1", "input_required", "a")).not.toBeNull();
    expect(
      gate.evaluate("t1", "input_required", "a", {
        requestId: "approval-1",
        kind: "permission",
      }),
    ).not.toBeNull();
  });

  it("does not repeat itself when a resolved approval settles into idle", () => {
    const gate = new TaskNotificationGate();
    expect(
      gate.evaluate("t1", "input_required", "a", {
        requestId: "approval-1",
        kind: "permission",
      }),
    ).not.toBeNull();
    // 审批已经报过,同一个 input_required 区间内紧随的普通空闲态不再打断。
    expect(gate.evaluate("t1", "input_required", "a")).toBeNull();
  });

  it("treats a malformed approval payload as an ordinary idle event", () => {
    const gate = new TaskNotificationGate();
    const note = gate.evaluate("t1", "input_required", "a", {
      requestId: 123 as unknown as string,
      kind: "permission",
    });
    expect(note!.title).toBe("任务等待输入");
    // 空字符串 requestId 同样不算审批。
    const blank = new TaskNotificationGate();
    expect(
      blank.evaluate("t1", "input_required", "a", { requestId: "  ", kind: "permission" })!.title,
    ).toBe("任务等待输入");
  });

  it("notifies a new approval even when the status does not change", () => {
    const gate = new TaskNotificationGate();
    expect(
      gate.evaluate("t1", "input_required", "a", {
        requestId: "approval-1",
        kind: "permission",
      }),
    ).not.toBeNull();
    expect(
      gate.evaluate("t1", "input_required", "a", {
        requestId: "approval-2",
        kind: "permission",
      }),
    ).not.toBeNull();
  });

  it("falls back to a short task id and localizes to English", () => {
    setLanguage("en");
    const gate = new TaskNotificationGate();
    const note = gate.evaluate("0123456789abcdef", "done");
    expect(note!.title).toBe("Task completed");
    expect(note!.body).toBe("Task 01234567");
  });
});
