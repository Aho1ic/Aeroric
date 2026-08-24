/**
 * 本地通知请求的构造(纯逻辑,不依赖 expo / RN,可直测)。
 *
 * 单独成模块的原因:渠道 id 必须在「创建渠道」与「投递通知」两处保持一致,
 * 而 Android 只能通过 trigger 指定渠道 —— trigger 传 null 时原生侧
 * (expo-notifications 的 BaseNotificationBuilder)会直接落到
 * expo_notifications_fallback_notification_channel,自建渠道就成了系统设置里
 * 调了也不生效的空壳。把构造抽出来才能用测试把这个约束钉住。
 */

import type { TaskNotification } from "./notification-gate";

/** 任务状态通知的 Android 渠道 id;创建与投递必须用同一个值。 */
export const TASK_NOTIFICATION_CHANNEL_ID = "task-status";

export interface TaskNotificationRequest {
  content: {
    title: string;
    body: string;
    data: { taskId: string; projectId: string };
  };
  /** iOS 忽略 channelId,立即投递语义与 null trigger 一致。 */
  trigger: { channelId: string };
}

export function taskNotificationRequest(
  note: TaskNotification,
  projectId?: string,
): TaskNotificationRequest {
  return {
    content: {
      title: note.title,
      body: note.body,
      data: { taskId: note.taskId, projectId: projectId ?? "" },
    },
    trigger: { channelId: TASK_NOTIFICATION_CHANNEL_ID },
  };
}
