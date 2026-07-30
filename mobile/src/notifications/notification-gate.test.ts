import { beforeEach, describe, expect, it } from "vitest";
import { setLanguage } from "../i18n";
import { TaskNotificationGate } from "./notification-gate";

describe("TaskNotificationGate", () => {
  beforeEach(() => {
    setLanguage("zh");
  });

  it("notifies on input_required/done/failed and stays silent otherwise", () => {
    const gate = new TaskNotificationGate();
    expect(gate.evaluate("t1", "running", "修 bug")).toBeNull();
    const note = gate.evaluate("t1", "input_required", "修 bug");
    expect(note).not.toBeNull();
    expect(note!.title).toBe("任务等待确认");
    expect(note!.body).toBe("修 bug");
    expect(gate.evaluate("t1", "done", "修 bug")!.title).toBe("任务已完成");
    expect(gate.evaluate("t2", "failed", undefined)!.title).toBe("任务失败");
  });

  it("dedupes the same status but re-notifies after a round trip", () => {
    const gate = new TaskNotificationGate();
    expect(gate.evaluate("t1", "input_required", "a")).not.toBeNull();
    // 补发 + 实时重复推同一状态 → 静默
    expect(gate.evaluate("t1", "input_required", "a")).toBeNull();
    // 状态往复后再次 input_required → 重新提醒
    expect(gate.evaluate("t1", "running", "a")).toBeNull();
    expect(gate.evaluate("t1", "input_required", "a")).not.toBeNull();
  });

  it("falls back to a short task id and localizes to English", () => {
    setLanguage("en");
    const gate = new TaskNotificationGate();
    const note = gate.evaluate("0123456789abcdef", "failed");
    expect(note!.title).toBe("Task failed");
    expect(note!.body).toBe("Task 01234567");
  });
});
