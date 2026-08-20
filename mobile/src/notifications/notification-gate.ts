/**
 * 任务状态通知的纯决策逻辑(与 expo-notifications 解耦,vitest 直测)。
 * 语义:input_required / done / failed 触发本地通知;同一任务同一状态只报
 * 一次,状态往复(running → input_required → running → input_required)重新报。
 * 实时推送与重连补发(events.since 回放)走同一入口,断线期间的待审批不会漏。
 *
 * `input_required` 有两层含义,标题也因此分开:带 `approval` 的是真实审批请求,
 * 不带的是交互式回合跑完把会话交回用户(Claude/Codex 的 `Stop`、DSH 的
 * `turn/end` 都落在这一类)。后者同样值得提醒——远程盯任务时"该你了"是最有
 * 用的那条信号——但一个 `input_required` 区间内只提醒一次:审批已经报过之后,
 * 紧随其后的普通空闲态不再重复打断。
 */

import { t } from "../i18n";
import type { ApprovalRequest } from "../types";

const NOTIFY_STATUSES = new Set(["input_required", "done", "failed"]);

export interface TaskNotification {
  title: string;
  body: string;
  taskId: string;
}

export class TaskNotificationGate {
  private lastStatus = new Map<string, { status: string; approvalRequestId?: string }>();

  /** 返回 null = 不通知。 */
  evaluate(
    taskId: string,
    status: string,
    taskName?: string,
    approval?: ApprovalRequest,
  ): TaskNotification | null {
    const approvalRequestId =
      typeof approval?.requestId === "string" ? approval.requestId.trim() || undefined : undefined;
    const previous = this.lastStatus.get(taskId);
    // 同一个 `input_required` 区间内只提醒一次。新的审批请求(requestId 变化)
    // 算新事件;普通空闲态只要区间没断过就一律去重,这样"审批 → 解决 → 回合
    // 结束"不会连着响两次。
    const duplicate =
      previous?.status === status &&
      (status !== "input_required" ||
        !approvalRequestId ||
        previous.approvalRequestId === approvalRequestId);
    this.lastStatus.set(taskId, { status, approvalRequestId });
    if (!NOTIFY_STATUSES.has(status)) return null;
    if (duplicate) return null;
    const title =
      status === "done"
        ? t("notify.done")
        : status === "failed"
          ? t("notify.failed")
          : approvalRequestId
            ? t("notify.inputRequired")
            : t("notify.turnIdle");
    const body = t("notify.body", {
      name: taskName?.trim() || `${t("common.task")} ${taskId.slice(0, 8)}`,
    });
    return { title, body, taskId };
  }
}
