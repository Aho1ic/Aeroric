import { describe, expect, it } from "vitest";
import { TaskNotificationGate } from "./notification-gate";
import { TASK_NOTIFICATION_CHANNEL_ID, taskNotificationRequest } from "./notification-request";

describe("taskNotificationRequest", () => {
  const note = { title: "任务已完成", body: "build docs", taskId: "task-1" };

  // 回归:曾经传 trigger: null,Android 因此忽略自建渠道,落到
  // expo_notifications_fallback_notification_channel。
  it("routes delivery through the created Android channel", () => {
    expect(taskNotificationRequest(note).trigger).toEqual({
      channelId: TASK_NOTIFICATION_CHANNEL_ID,
    });
  });

  it("never leaves the trigger null or channel-less", () => {
    const { trigger } = taskNotificationRequest(note, "proj-1");
    expect(trigger).not.toBeNull();
    expect(trigger.channelId).toBeTruthy();
  });

  it("carries the deep-link payload and falls back to an empty projectId", () => {
    expect(taskNotificationRequest(note, "proj-1").content).toEqual({
      title: "任务已完成",
      body: "build docs",
      data: { taskId: "task-1", projectId: "proj-1" },
    });
    expect(taskNotificationRequest(note).content.data.projectId).toBe("");
  });

  it("accepts what the gate actually emits", () => {
    const gate = new TaskNotificationGate();
    const emitted = gate.evaluate("task-9", "done", "ship it");
    expect(emitted).not.toBeNull();
    const request = taskNotificationRequest(emitted!, "proj-9");
    expect(request.content.data.taskId).toBe("task-9");
    expect(request.trigger.channelId).toBe(TASK_NOTIFICATION_CHANNEL_ID);
  });
});
