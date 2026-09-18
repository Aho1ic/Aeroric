/**
 * DSH 宿主事件的 App 层订阅簇:审批 / 提问请求与它们的 resolved 清理,加上宿主
 * 失效通道到浏览器事件总线的转发。载体是 /api/remote.mux 上的 `$events` 流
 * (events.host 已经没了),Rust 侧把 waterfall / emit 帧翻成下面这些 Tauri 事件名。
 * 原来内联在 App.tsx 的挂载期大 effect 里;这批监听与其余监听(任务状态、设置
 * 变更)互不耦合,只是共用同一个 effect,拆出来后注册时机还略早于原先(更不漏
 * 早期事件)。
 *
 * "dsh-host-refresh" 的转发约定:App 不理解 payload,只加一层(eventName,
 * payload)原样重发,由各面板自己刷新自己的快照。
 */
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  DSH_APPROVAL_REQUESTED_EVENT,
  DSH_APPROVAL_RESOLVED_EVENT,
  DSH_HOST_AGENT_ERROR_EVENT,
  DSH_HOST_ARCHIVED_SESSIONS_CHANGED_EVENT,
  DSH_HOST_SESSION_ADDED_EVENT,
  DSH_HOST_SESSION_REMOVED_EVENT,
  DSH_HOST_SESSION_STATUS_EVENT,
  DSH_HOST_WORKSPACE_CHANGED_EVENT,
  DSH_HOST_WORKSPACE_ORDER_CHANGED_EVENT,
  DSH_HOST_WORKSPACE_REMOVED_EVENT,
  DSH_QUESTION_REQUESTED_EVENT,
  DSH_QUESTION_RESOLVED_EVENT,
} from "../tauriEvents";
import type { DshApprovalRequest } from "../components/DshApprovalDialog";
import type { DshQuestionRequest } from "../components/DshQuestionDialog";
import { useToast } from "../components/Toast";

export function useDshHostEvents() {
  const { showToast } = useToast();
  // DSH approval / question dialogs — the agent pauses until the client responds.
  const [dshApprovalRequests, setDshApprovalRequests] = useState<DshApprovalRequest[]>([]);
  const [dshQuestionRequests, setDshQuestionRequests] = useState<DshQuestionRequest[]>([]);

  useEffect(() => {
    const p6 = listen<{
      type: string;
      eventId: string;
      clientId: string;
      sessionId: string;
      toolName: string;
      callId?: string;
      reason?: string;
    }>(DSH_APPROVAL_REQUESTED_EVENT, (e) => {
      setDshApprovalRequests((prev) => {
        const request = {
          eventId: e.payload.eventId,
          clientId: e.payload.clientId,
          sessionId: e.payload.sessionId,
          toolName: e.payload.toolName,
          callId: e.payload.callId,
          reason: e.payload.reason,
        } satisfies DshApprovalRequest;
        const next = prev.filter((item) => item.eventId !== request.eventId);
        return [...next, request];
      });
    });
    const p7 = listen<{
      type: string;
      eventId: string;
      clientId: string;
      sessionId: string;
      questions: Array<{
        id: string;
        question: string;
        detail?: string;
        header?: string;
        options?: Array<{ label: string; description?: string }>;
        multiSelect?: boolean;
      }>;
    }>(DSH_QUESTION_REQUESTED_EVENT, (e) => {
      setDshQuestionRequests((prev) => {
        const request = {
          eventId: e.payload.eventId,
          clientId: e.payload.clientId,
          sessionId: e.payload.sessionId,
          questions: e.payload.questions,
        } satisfies DshQuestionRequest;
        const next = prev.filter((item) => item.eventId !== request.eventId);
        return [...next, request];
      });
    });
    // resolved 只按 eventId 匹配:waterfall 的 eventId 全局唯一,再叠 sessionId
    // 不会更准,只会在 Rust 侧漏带 sessionId 时撤不掉框。
    const p8 = listen<{ sessionId?: string; eventId?: string }>(
      DSH_APPROVAL_RESOLVED_EVENT,
      (e) => {
        setDshApprovalRequests((prev) => prev.filter((item) => item.eventId !== e.payload.eventId));
      },
    );
    const p9 = listen<{ sessionId?: string; eventId?: string }>(
      DSH_QUESTION_RESOLVED_EVENT,
      (e) => {
        setDshQuestionRequests((prev) => prev.filter((item) => item.eventId !== e.payload.eventId));
      },
    );
    // 宿主失效帧(`$events` 的 emit)是设置 / 会话面板的实时失效通道。原样再发一条
    // 浏览器事件,面板各自刷自己的快照,不用把 App 和它们的 state 绑在一起。
    const dispatchDshHostRefresh = (eventName: string, payload: unknown) => {
      window.dispatchEvent(new CustomEvent("dsh-host-refresh", { detail: { eventName, payload } }));
    };
    const p10 = listen(DSH_HOST_SESSION_ADDED_EVENT, (e) =>
      dispatchDshHostRefresh("session-added", e.payload),
    );
    const p11 = listen(DSH_HOST_SESSION_REMOVED_EVENT, (e) =>
      dispatchDshHostRefresh("session-removed", e.payload),
    );
    const p12 = listen(DSH_HOST_SESSION_STATUS_EVENT, (e) =>
      dispatchDshHostRefresh("session-status", e.payload),
    );
    const p13 = listen(DSH_HOST_WORKSPACE_CHANGED_EVENT, (e) =>
      dispatchDshHostRefresh("workspace-changed", e.payload),
    );
    const p14 = listen(DSH_HOST_WORKSPACE_REMOVED_EVENT, (e) =>
      dispatchDshHostRefresh("workspace-removed", e.payload),
    );
    const p15 = listen(DSH_HOST_WORKSPACE_ORDER_CHANGED_EVENT, (e) =>
      dispatchDshHostRefresh("workspace-order-changed", e.payload),
    );
    const p16 = listen(DSH_HOST_ARCHIVED_SESSIONS_CHANGED_EVENT, (e) =>
      dispatchDshHostRefresh("archived-sessions-changed", e.payload),
    );
    const p17 = listen<{ message?: string; error?: string }>(DSH_HOST_AGENT_ERROR_EVENT, (e) => {
      const msg = e.payload?.message ?? e.payload?.error ?? "DSH agent error";
      showToast(msg, "error");
    });
    return () => {
      p6.then((fn) => fn());
      p7.then((fn) => fn());
      p8.then((fn) => fn());
      p9.then((fn) => fn());
      p10.then((fn) => fn());
      p11.then((fn) => fn());
      p12.then((fn) => fn());
      p13.then((fn) => fn());
      p14.then((fn) => fn());
      p15.then((fn) => fn());
      p16.then((fn) => fn());
      p17.then((fn) => fn());
    };
  }, [showToast]);

  // 对话框队列一次只显示队首;关闭即弹出队首,由 resolved 事件负责精确清除。
  const dismissApproval = useCallback(() => setDshApprovalRequests((prev) => prev.slice(1)), []);
  const dismissQuestion = useCallback(() => setDshQuestionRequests((prev) => prev.slice(1)), []);

  return { dshApprovalRequests, dshQuestionRequests, dismissApproval, dismissQuestion };
}
