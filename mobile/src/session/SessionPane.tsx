/**
 * 会话面板:结构化消息流 + 审批卡片 + prompt 输入框。
 *
 * 数据流:进入(或重连)时 `session.messages` 全量拉取;`session.appended`
 * 推送增量追加。加载期间到达的推送只标记 dirty,加载完成后再拉一次,
 * 避免「全量已含 + 增量又推」的重叠(见 remote/session_push.rs 注释)。
 */

import { ChevronDown, ChevronRight, Paperclip, Wrench } from "lucide-react-native";
import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ActivityIndicator, ScrollView, StyleSheet, Text, TextInput, View } from "react-native";
import { t } from "../i18n";
import { useConnection } from "../state/connection-context";
import type {
  SessionAppendedPush,
  SessionContent,
  SessionMessage,
  SessionMessagesResult,
  Task,
} from "../types";
import { radii, theme } from "../ui/theme";
import { AnimatedPressable } from "../ui/AnimatedPressable";
import { MarkdownText } from "./MarkdownText";
import { mergeAppended } from "./messages";

function ToolUseCard({ name, input }: { name: string; input: string }) {
  const [expanded, setExpanded] = useState(false);
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <View style={styles.toolCard}>
      <AnimatedPressable style={styles.toolHeader} onPress={() => setExpanded((prev) => !prev)}>
        <Chevron size={13} color={theme.textHint} />
        <Wrench size={13} color={theme.textSecondary} />
        <Text style={styles.toolName} numberOfLines={1}>
          {name}
        </Text>
      </AnimatedPressable>
      {expanded ? (
        <Text style={styles.toolInput} selectable>
          {input}
        </Text>
      ) : null}
    </View>
  );
}

function ToolResultCard({ output }: { output: string }) {
  const [expanded, setExpanded] = useState(false);
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <View style={styles.toolCard}>
      <AnimatedPressable style={styles.toolHeader} onPress={() => setExpanded((prev) => !prev)}>
        <Chevron size={13} color={theme.textHint} />
        <Text style={styles.toolName} numberOfLines={1}>
          {t("session.toolResult")}
        </Text>
      </AnimatedPressable>
      {expanded ? (
        <Text style={styles.toolInput} selectable>
          {output}
        </Text>
      ) : null}
    </View>
  );
}

function ThinkingBlock({ thinking }: { thinking: string }) {
  const [expanded, setExpanded] = useState(false);
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <View>
      <AnimatedPressable style={styles.thinkingToggle} onPress={() => setExpanded((prev) => !prev)}>
        <Chevron size={13} color={theme.textHint} />
        <Text style={styles.thinkingLabel}>{t("session.thinking")}</Text>
      </AnimatedPressable>
      {expanded ? (
        <Text style={styles.thinkingText} selectable>
          {thinking}
        </Text>
      ) : null}
    </View>
  );
}

function SystemBlock({ content }: { content: string }) {
  return content.trim() ? <Text style={styles.systemText}>{content}</Text> : null;
}

function AttachmentBlock({ name, mediaType }: { name?: string; mediaType?: string }) {
  return (
    <View style={styles.attachmentBlock}>
      <Paperclip size={15} color={theme.textSecondary} />
      <View style={styles.attachmentText}>
        <Text style={styles.attachmentTitle} numberOfLines={1}>
          {name?.trim() || t("session.attachment")}
        </Text>
        {mediaType ? <Text style={styles.attachmentMeta}>{mediaType}</Text> : null}
      </View>
    </View>
  );
}

function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

function renderContentPart(
  part: SessionContent,
  index: number,
  role: SessionMessage["role"],
): ReactNode {
  const key = "id" in part && part.id ? `${part.type}-${part.id}` : `${part.type}-${index}`;
  switch (part.type) {
    case "thinking":
      return part.thinking.trim() ? <ThinkingBlock key={key} thinking={part.thinking} /> : null;
    case "tool_use":
      return <ToolUseCard key={key} name={part.name} input={part.input} />;
    case "tool_result":
      return <ToolResultCard key={key} output={part.output} />;
    case "attachment":
      return <AttachmentBlock key={key} name={part.name} mediaType={part.mediaType} />;
    case "opaque":
      return <SystemBlock key={key} content={`${part.name}: ${safeJson(part.value)}`} />;
    case "text":
      if (!part.text.trim()) return null;
      if (role === "user") {
        return (
          <View key={key} style={styles.userBubble}>
            <Text style={styles.userText} selectable>
              {part.text}
            </Text>
          </View>
        );
      }
      if (role === "system") return <SystemBlock key={key} content={part.text} />;
      return <MarkdownText key={key} text={part.text} />;
    default: {
      const unknown = part as unknown as Record<string, unknown>;
      const kind = typeof unknown.type === "string" ? unknown.type : "unknown";
      return (
        <SystemBlock
          key={`unknown-${index}`}
          content={t("session.unknownContent", { type: kind })}
        />
      );
    }
  }
}

/**
 * memo:agent 流式输出时 `session.appended` 推送很密,不 memo 会让整列表连同所有
 * markdown 子树逐帧重渲染,把 JS 线程占满 —— 底部输入框的 native→JS→native 回写
 * 随之变慢,打字回显发钝甚至打断 IME 的 composing region。
 * `mergeAppended` 保持既有消息的引用稳定(只有被合并的末条会换新对象),memo 因此有实效。
 */
const MessageBlock = memo(function MessageBlock({ message }: { message: SessionMessage }) {
  if (message.content.length === 0) return null;
  const content = message.content.map((part, index) =>
    renderContentPart(part, index, message.role),
  );
  if (message.role === "user") {
    return (
      <View style={styles.userRow}>
        <View style={styles.userMessage}>{content}</View>
      </View>
    );
  }
  if (message.role === "system") {
    return <View style={styles.systemBlock}>{content}</View>;
  }
  return <View style={styles.assistantBlock}>{content}</View>;
});

/**
 * 输入框独立成 memo 组件,draft/sending 下沉到内部:消息推送不再触达输入框,
 * 输入框自身的 setState 也不再触发整列表重渲染。
 *
 * 输入框恒可编辑 —— 之前 `editable` 跟着 `canSend`(即任务状态)实时门控,agent 一跑完
 * 状态转 done/interrupted,正在打字的输入框瞬间变不可编辑:键盘被收、composing 内容
 * 丢弃、后续键入无效,表现就是「打了字不显示」。现在只有发送按钮和提交时判定能不能发,
 * 用户可以先把字打完再等状态允许。
 */
const Composer = memo(function Composer({
  canSend,
  onSubmit,
}: {
  canSend: boolean;
  onSubmit: (text: string) => Promise<void>;
}) {
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);

  const submit = useCallback(() => {
    const text = draft.trim();
    if (!text || sending || !canSend) return;
    setSending(true);
    onSubmit(text)
      .then(() => {
        // 只在草稿仍是刚发出的内容时清空:输入框在往返期间可编辑,
        // 用户可能已经接着打了下一句,不能连带抹掉。
        setDraft((current) => (current.trim() === text ? "" : current));
      })
      .catch(() => {
        // 错误由父组件展示,这里保留草稿供重试
      })
      .finally(() => setSending(false));
  }, [canSend, draft, onSubmit, sending]);

  const disabled = !canSend || !draft.trim() || sending;
  return (
    <View style={styles.composer}>
      <TextInput
        style={styles.input}
        value={draft}
        onChangeText={setDraft}
        placeholder={canSend ? t("session.sendPlaceholder") : t("session.cannotSend")}
        placeholderTextColor={theme.textHint}
        multiline
      />
      <AnimatedPressable
        style={[styles.sendButton, disabled && styles.sendDisabled]}
        disabled={disabled}
        onPress={submit}
      >
        <Text style={styles.sendText}>{sending ? "…" : t("session.send")}</Text>
      </AnimatedPressable>
    </View>
  );
});

export function SessionPane({
  projectId,
  task,
  active,
  canSend,
}: {
  projectId: string;
  task: Task;
  active: boolean;
  canSend: boolean;
}) {
  const { status, request, onPush, capabilitiesReady, hasCapability } = useConnection();
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [unavailableReason, setUnavailableReason] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [responding, setResponding] = useState<"approve" | "deny" | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const approvalsSupported = !capabilitiesReady || hasCapability("tasks.approvals");

  const loadSeq = useRef(0);
  const loadingRef = useRef(false);
  const dirtyRef = useRef(false);
  const isCodexRef = useRef(false);
  const scrollRef = useRef<ScrollView>(null);
  const nearBottomRef = useRef(true);

  const load = useCallback(() => {
    if (status !== "online") return;
    if (capabilitiesReady && !hasCapability("session.structured")) {
      setUnavailableReason("unsupported");
      setMessages([]);
      setLoading(false);
      return;
    }
    const seq = ++loadSeq.current;
    loadingRef.current = true;
    setLoading(true);
    setError(null);
    request<SessionMessagesResult>("session.messages", { projectId, taskId: task.id })
      .then((result) => {
        if (loadSeq.current !== seq) return;
        loadingRef.current = false;
        isCodexRef.current = !!result.isCodex;
        setUnavailableReason(result.available ? null : (result.reason ?? "no_session"));
        setMessages(result.messages ?? []);
        setLoading(false);
        if (dirtyRef.current) {
          // 加载间隙有增量推送到达:再拉一次全量补齐
          dirtyRef.current = false;
          load();
        }
      })
      .catch((err) => {
        if (loadSeq.current !== seq) return;
        loadingRef.current = false;
        setLoading(false);
        setError(err instanceof Error ? err.message : String(err));
      });
  }, [capabilitiesReady, hasCapability, projectId, request, status, task.id]);

  // 首次进入 + 重连后重新同步
  useEffect(() => {
    if (active && status === "online") load();
  }, [active, load, status]);

  // 增量推送(tab 未激活也追加,切回时无需重拉)
  useEffect(() => {
    return onPush((push, data) => {
      if (push !== "session.appended") return;
      const payload = data as Partial<SessionAppendedPush>;
      if (payload?.task_id !== task.id || !Array.isArray(payload.messages)) return;
      if (loadingRef.current) {
        dirtyRef.current = true;
        return;
      }
      setUnavailableReason(null);
      setMessages((prev) => mergeAppended(prev, payload.messages ?? [], isCodexRef.current));
    });
  }, [onPush, task.id]);

  // 引用稳定(只依赖 request 与 task.id),Composer 的 memo 才拦得住父组件重渲染。
  // 失败时上抛,让 Composer 保留草稿。
  const submitPrompt = useCallback(
    (text: string) => {
      setActionError(null);
      return request("task.input", { taskId: task.id, text }).then(
        () => undefined,
        (err: unknown) => {
          setActionError(err instanceof Error ? err.message : String(err));
          throw err;
        },
      );
    },
    [request, task.id],
  );

  const respond = useCallback(
    (action: "approve" | "deny") => {
      if (responding || !task.approval) return;
      if (!approvalsSupported) {
        setActionError(t("session.approvalUnsupported"));
        return;
      }
      setResponding(action);
      setActionError(null);
      request("task.respond", {
        taskId: task.id,
        requestId: task.approval.requestId,
        action,
      })
        .catch((err) => setActionError(err instanceof Error ? err.message : String(err)))
        .finally(() => setResponding(null));
    },
    [approvalsSupported, request, responding, task.approval, task.id],
  );

  // 两种 `input_required` 的展示分家:
  // - 带 `approval`:真审批,agent 被卡住等人点按钮 → 保留醒目告警卡片。
  // - 不带 `approval`:本轮已结束、可以继续输入 → 一条细窄状态条。输入框就在
  //   下方,再用整块告警卡片喊一遍既占屏又误导(听起来像 agent 卡了)。
  const attention = active && task.status === "input_required";
  const pendingApproval = attention ? task.approval : undefined;
  const showTurnSettled = attention && !pendingApproval;
  const approvalTool = useMemo(() => {
    for (let messageIndex = messages.length - 1; messageIndex >= 0; messageIndex -= 1) {
      const content = messages[messageIndex]?.content ?? [];
      for (let contentIndex = content.length - 1; contentIndex >= 0; contentIndex -= 1) {
        const part = content[contentIndex];
        if (part?.type === "tool_use") return part;
      }
    }
    return null;
  }, [messages]);

  return (
    <View style={styles.pane}>
      {showTurnSettled ? (
        <View style={styles.settledStrip}>
          <View style={styles.settledDot} />
          <Text style={styles.settledText} numberOfLines={1}>
            {t("session.turnSettled")}
          </Text>
        </View>
      ) : null}
      {pendingApproval ? (
        <View style={styles.approvalCard}>
          <Text style={styles.approvalTitle}>{t("session.approvalTitle")}</Text>
          {pendingApproval.toolName || approvalTool ? (
            <Text style={styles.approvalDetail} numberOfLines={2}>
              {t("session.tool")}:{pendingApproval.toolName ?? approvalTool?.name}
              {approvalTool?.input ? `\n${approvalTool.input}` : ""}
            </Text>
          ) : null}
          {approvalsSupported ? (
            <View style={styles.approvalButtons}>
              <AnimatedPressable
                style={[styles.approvalButton, styles.approveButton]}
                disabled={responding !== null}
                onPress={() => respond("approve")}
              >
                <Text style={styles.approveText}>
                  {responding === "approve" ? t("session.sending") : t("session.approve")}
                </Text>
              </AnimatedPressable>
              <AnimatedPressable
                style={[styles.approvalButton, styles.denyButton]}
                disabled={responding !== null}
                onPress={() => respond("deny")}
              >
                <Text style={styles.denyText}>
                  {responding === "deny" ? t("session.sending") : t("session.deny")}
                </Text>
              </AnimatedPressable>
            </View>
          ) : null}
          <Text style={styles.approvalHint}>
            {approvalsSupported ? t("session.approvalStale") : t("session.approvalUnsupported")}
          </Text>
        </View>
      ) : null}
      {actionError ? <Text style={styles.actionError}>{actionError}</Text> : null}

      <ScrollView
        ref={scrollRef}
        style={styles.scroll}
        contentContainerStyle={styles.scrollContent}
        onScroll={(e) => {
          const { contentOffset, contentSize, layoutMeasurement } = e.nativeEvent;
          nearBottomRef.current =
            contentOffset.y + layoutMeasurement.height >= contentSize.height - 120;
        }}
        scrollEventThrottle={120}
        onContentSizeChange={() => {
          if (nearBottomRef.current) scrollRef.current?.scrollToEnd({ animated: false });
        }}
      >
        {loading ? (
          <View style={styles.centerWrap}>
            <ActivityIndicator color={theme.textSecondary} />
          </View>
        ) : null}
        {error ? <Text style={styles.errorText}>{t("session.loadFailed", { error })}</Text> : null}
        {!loading && !error && unavailableReason ? (
          <Text style={styles.hintText}>
            {unavailableReason === "ssh"
              ? t("session.sshUnavailable")
              : unavailableReason === "unsupported"
                ? t("session.unsupported")
                : t("session.notStarted")}
          </Text>
        ) : null}
        {!loading && !error && !unavailableReason && messages.length === 0 ? (
          <Text style={styles.hintText}>{t("session.empty")}</Text>
        ) : null}
        {messages.map((message, index) => (
          <MessageBlock key={message.messageId ?? message.id ?? index} message={message} />
        ))}
      </ScrollView>

      <Composer canSend={canSend} onSubmit={submitPrompt} />
    </View>
  );
}

const styles = StyleSheet.create({
  pane: { flex: 1, backgroundColor: theme.bg },
  scroll: { flex: 1 },
  scrollContent: { paddingHorizontal: 14, paddingVertical: 12, gap: 14 },
  centerWrap: { paddingVertical: 24, alignItems: "center" },
  errorText: { color: theme.danger, fontSize: 12.5, paddingVertical: 8 },
  hintText: { color: theme.textSecondary, fontSize: 13, lineHeight: 20, paddingVertical: 8 },
  userRow: { flexDirection: "row", justifyContent: "flex-end" },
  userMessage: { width: "82%", alignItems: "stretch", gap: 8 },
  userBubble: {
    backgroundColor: "rgba(68,147,248,0.14)",
    borderColor: "rgba(68,147,248,0.28)",
    borderWidth: 1,
    borderRadius: radii.card,
    paddingHorizontal: 13,
    paddingVertical: 9,
  },
  userText: { color: theme.text, fontSize: 13.5, lineHeight: 20 },
  systemText: {
    color: theme.textHint,
    fontSize: 12,
    lineHeight: 18,
    paddingHorizontal: 10,
    paddingVertical: 6,
    borderLeftWidth: 2,
    borderLeftColor: theme.border,
  },
  systemBlock: { gap: 6 },
  attachmentBlock: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    borderRadius: radii.button,
    paddingHorizontal: 10,
    paddingVertical: 8,
    backgroundColor: theme.bgCard,
  },
  attachmentText: { flex: 1, minWidth: 0 },
  attachmentTitle: { color: theme.text, fontSize: 12.5, fontWeight: "600" },
  attachmentMeta: { color: theme.textHint, fontSize: 11.5, marginTop: 2 },
  assistantBlock: { gap: 8 },
  toolCard: {
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    borderRadius: radii.button,
    overflow: "hidden",
  },
  toolHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: 10,
    paddingVertical: 7,
    backgroundColor: theme.bgCard,
  },
  toolName: { color: theme.textSecondary, fontSize: 12.5, fontWeight: "600", flex: 1 },
  toolInput: {
    color: theme.textSecondary,
    fontSize: 11.5,
    lineHeight: 17,
    padding: 10,
    backgroundColor: theme.bg,
  },
  thinkingToggle: { paddingVertical: 2, flexDirection: "row", alignItems: "center", gap: 4 },
  thinkingLabel: { color: theme.textHint, fontSize: 12, fontStyle: "italic" },
  thinkingText: {
    color: theme.textHint,
    fontSize: 12.5,
    lineHeight: 19,
    fontStyle: "italic",
    borderLeftWidth: 2,
    borderLeftColor: theme.border,
    paddingLeft: 10,
    marginTop: 4,
  },
  approvalCard: {
    margin: 10,
    marginBottom: 0,
    padding: 12,
    borderRadius: radii.row,
    borderWidth: 1,
    borderColor: theme.warning,
    backgroundColor: "rgba(210,153,34,0.10)",
    gap: 10,
  },
  // 「本轮已结束」状态条:单行、无边框、无警告色。够传达状态,不抢屏幕。
  settledStrip: {
    flexDirection: "row",
    alignItems: "center",
    gap: 7,
    paddingHorizontal: 14,
    paddingVertical: 6,
  },
  settledDot: {
    width: 5,
    height: 5,
    borderRadius: 2.5,
    backgroundColor: theme.textHint,
  },
  settledText: { color: theme.textHint, fontSize: 11.5, flex: 1 },
  approvalTitle: { color: theme.warning, fontSize: 14, fontWeight: "700" },
  approvalDetail: { color: theme.text, fontSize: 12.5 },
  approvalButtons: { flexDirection: "row", gap: 10 },
  approvalButton: {
    flex: 1,
    borderRadius: radii.button,
    paddingVertical: 10,
    alignItems: "center",
  },
  approveButton: { backgroundColor: theme.success },
  denyButton: { backgroundColor: theme.bgElevated, borderWidth: 1, borderColor: theme.border },
  approveText: { color: "#fff", fontSize: 14, fontWeight: "700" },
  denyText: { color: theme.text, fontSize: 14, fontWeight: "600" },
  approvalHint: { color: theme.textHint, fontSize: 11.5 },
  actionError: {
    color: theme.danger,
    fontSize: 12,
    paddingHorizontal: 14,
    paddingTop: 8,
  },
  composer: {
    flexDirection: "row",
    alignItems: "flex-end",
    gap: 8,
    paddingHorizontal: 10,
    paddingVertical: 8,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: theme.border,
    backgroundColor: theme.bgCard,
  },
  input: {
    flex: 1,
    minHeight: 38,
    maxHeight: 120,
    borderRadius: radii.input,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: theme.border,
    backgroundColor: theme.bgElevated,
    color: theme.text,
    paddingHorizontal: 12,
    paddingVertical: 9,
    fontSize: 14,
  },
  sendButton: {
    backgroundColor: theme.accent,
    borderRadius: radii.button,
    paddingHorizontal: 16,
    paddingVertical: 10,
  },
  sendDisabled: { opacity: 0.4 },
  sendText: { color: "#fff", fontSize: 14, fontWeight: "600" },
});
