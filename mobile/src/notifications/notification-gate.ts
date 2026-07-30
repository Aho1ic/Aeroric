/**
 * 任务状态通知的纯决策逻辑(与 expo-notifications 解耦,vitest 直测)。
 * 语义:input_required / done / failed 触发本地通知;同一任务同一状态只报
 * 一次,状态往复(running → input_required → running → input_required)重新报。
 * 实时推送与重连补发(events.since 回放)走同一入口,断线期间的待审批不会漏。
 */

import { t } from "../i18n";

const NOTIFY_STATUSES = new Set(["input_required", "done", "failed"]);

export interface TaskNotification {
  title: string;
  body: string;
  taskId: string;
}

export class TaskNotificationGate {
  private lastStatus = new Map<string, string>();

  /** 返回 null = 不通知。 */
  evaluate(taskId: string, status: string, taskName?: string): TaskNotification | null {
    const previous = this.lastStatus.get(taskId);
    this.lastStatus.set(taskId, status);
    if (!NOTIFY_STATUSES.has(status)) return null;
    if (previous === status) return null;
    const title =
      status === "input_required"
        ? t("notify.inputRequired")
        : status === "done"
          ? t("notify.done")
          : t("notify.failed");
    const body = t("notify.body", {
      name: taskName?.trim() || `${t("common.task")} ${taskId.slice(0, 8)}`,
    });
    return { title, body, taskId };
  }
}
