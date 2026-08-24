/**
 * 本地通知桥:订阅连接层推送(实时 + 重连补发共用一条通道),
 * 经 TaskNotificationGate 去重后调 expo-notifications 弹本地通知;
 * 点击通知深链到对应任务详情页。无 UI,挂在根布局。
 */

import * as Notifications from "expo-notifications";
import { router } from "expo-router";
import { useEffect, useRef } from "react";
import { Platform } from "react-native";
import { useConnection } from "../state/connection-context";
import type { ApprovalRequest } from "../types";
import { TaskNotificationGate } from "./notification-gate";
import { TASK_NOTIFICATION_CHANNEL_ID, taskNotificationRequest } from "./notification-request";
import { taskMeta } from "./task-name-cache";

Notifications.setNotificationHandler({
  // App 在前台时也展示横幅(用户可能正盯着别的任务)
  handleNotification: async () => ({
    shouldShowBanner: true,
    shouldShowList: true,
    shouldPlaySound: false,
    shouldSetBadge: false,
  }),
});

export function NotificationsBridge() {
  const { onPush } = useConnection();
  const gateRef = useRef(new TaskNotificationGate());

  useEffect(() => {
    void Notifications.requestPermissionsAsync().catch(() => {});
    if (Platform.OS === "android") {
      void Notifications.setNotificationChannelAsync(TASK_NOTIFICATION_CHANNEL_ID, {
        name: "Aeroric tasks",
        importance: Notifications.AndroidImportance.HIGH,
      }).catch(() => {});
    }
  }, []);

  useEffect(() => {
    return onPush((push, data) => {
      if (push !== "task-status") return;
      const payload = data as {
        task_id?: string;
        status?: string;
        approval?: ApprovalRequest;
      };
      if (!payload?.task_id || !payload.status) return;
      const meta = taskMeta(payload.task_id);
      const note = gateRef.current.evaluate(
        payload.task_id,
        payload.status,
        meta?.name,
        payload.approval,
      );
      if (!note) return;
      void Notifications.scheduleNotificationAsync(
        taskNotificationRequest(note, meta?.projectId),
      ).catch(() => {});
    });
  }, [onPush]);

  // 点通知 → 打开任务详情
  useEffect(() => {
    const subscription = Notifications.addNotificationResponseReceivedListener((response) => {
      const data = response.notification.request.content.data as {
        taskId?: string;
        projectId?: string;
      };
      if (typeof data?.taskId === "string" && data.taskId) {
        router.push({
          pathname: "/task/[taskId]",
          params: { taskId: data.taskId, projectId: data.projectId ?? "" },
        });
      }
    });
    return () => subscription.remove();
  }, []);

  return null;
}
