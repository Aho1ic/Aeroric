/**
 * 会话消息的增量合并逻辑(纯函数,vitest 覆盖)。
 *
 * 桌面 watcher 按「批」推送新解析的消息(见 remote/session_push.rs):
 * codex 解析器会把同一回合的 assistant 内容合并进上一条消息,但批与批之间
 * 无法合并 —— 这里在手机端补上同等语义,保证增量追加后的视图与
 * 重新全量拉取的视图一致。claude 每条 assistant 消息独立,不做合并。
 */

import type { SessionContent, SessionMessage } from "../types";

export function mergeAppended(
  existing: SessionMessage[],
  incoming: SessionMessage[],
  mergeAdjacentAssistant: boolean,
): SessionMessage[] {
  if (incoming.length === 0) return existing;
  const next = [...existing];
  for (const message of incoming) {
    if (!message || (message.role !== "user" && message.role !== "assistant")) continue;
    const content = Array.isArray(message.content) ? message.content : [];
    if (content.length === 0) continue;
    const last = next[next.length - 1];
    if (
      mergeAdjacentAssistant &&
      last &&
      last.role === "assistant" &&
      message.role === "assistant"
    ) {
      next[next.length - 1] = { ...last, content: [...last.content, ...content] };
    } else {
      next.push({ role: message.role, content });
    }
  }
  return next;
}

/** 消息里可直接显示的纯文本(user 气泡、通知预览用)。 */
export function messageText(message: SessionMessage): string {
  return message.content
    .filter((c): c is Extract<SessionContent, { type: "text" }> => c.type === "text")
    .map((c) => c.text)
    .join("\n");
}
