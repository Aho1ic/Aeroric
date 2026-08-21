import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DshJobView, DshLiveSessionState, DshQueueItem, DshTodoItem } from "../types";
import { useI18n } from "../i18n";
import { DshSessionInsights } from "./DshSessionInsights";
import { DshSessionLogExportButton } from "./DshSessionLogExport";
import { dshInsightTabs, useDshTrajectory } from "./DshTrajectoryHost";
import {
  Circle,
  Check,
  CheckCircle2,
  Edit2,
  Loader2,
  Target,
  Pause,
  ListChecks,
  Layers,
  Send,
  X,
} from "lucide-react";

/**
 * Live status bars rendered above the dsh terminal/composer when a dsh session
 * is active. Surfaces what the dsh web UI shows in its conversation header:
 * the goal bar, the todo checklist, plan-mode chip, background-job strip, and
 * the pending-prompt queue. All data comes from the `useDshLiveSessions` hook
 * which consumes the projection/jobs/queue push frames the backend already
 * forwards (no new RPC needed).
 */
/**
 * The session-scoped header actions, rendered at the end of the task's meta row.
 *
 * Session log / trajectory / the panel's own views all belong to the same
 * session as the badge they sit next to, and folding them into that row is what
 * gives the terminal back the strip they used to occupy on their own.
 */
export function DshTerminalHeaderActions({ sessionId }: { sessionId: string }) {
  const { t } = useI18n();
  const { openAt } = useDshTrajectory();
  return (
    <div className="dsh-terminal-header-actions">
      <DshSessionLogExportButton sessionId={sessionId} />
      <DshSessionInsights />
      {/* The trajectory view itself already has the trigger above, so the rest of
          the panel's tabs open straight into their own view. */}
      {dshInsightTabs
        .filter((item) => item.id !== "trajectory")
        .map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              type="button"
              className="dsh-view-trigger"
              title={t(item.labelKey)}
              onClick={() => openAt(item.id)}
            >
              <Icon size={13} />
              <span>{t(item.labelKey)}</span>
            </button>
          );
        })}
    </div>
  );
}

export function DshLiveBars({
  sessionId,
  live,
}: {
  sessionId: string;
  live: DshLiveSessionState | undefined;
}) {
  const { t } = useI18n();

  const hasTodo = live?.todo && live.todo.length > 0;
  const hasGoal = live?.goal != null;
  const hasJobs =
    live?.jobs && live.jobs.some((j) => j.status === "running" || j.status === "stopping");
  const hasQueue = live?.queue && live.queue.length > 0;
  // The header actions moved into the meta row, so with nothing live to show
  // this strip has no content of its own left: drop it instead of leaving an
  // empty bordered band above the terminal.
  if (!hasGoal && !live?.planMode && !hasTodo && !hasJobs && !hasQueue) return null;

  return (
    <div
      className="dsh-live-bars"
      style={{
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        gap: 6,
        padding: "6px 14px",
        borderBottom: "1px solid var(--border-dim)",
        background: "var(--bg-panel)",
        fontSize: 12,
        color: "var(--text-secondary)",
      }}
    >
      {hasGoal && live?.goal && <GoalRow goal={live.goal} />}
      {live?.planMode && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            color: "var(--primary-action-bg)",
          }}
        >
          <Target size={13} strokeWidth={2} />
          <span>{t("dsh.live.planMode")}</span>
        </div>
      )}
      {hasTodo && <TodoRow items={live!.todo!} />}
      {hasJobs && <JobsRow jobs={live!.jobs!} />}
      {hasQueue && <QueueRow sessionId={sessionId} items={live!.queue!} />}
    </div>
  );
}

function GoalRow({ goal }: { goal: NonNullable<DshLiveSessionState["goal"]> }) {
  const { t } = useI18n();
  const phaseKey = `dsh.live.goalPhase.${goal.phase}`;
  const phaseLabel = t(phaseKey);
  const Icon =
    goal.phase === "active"
      ? Target
      : goal.phase === "paused"
        ? Pause
        : goal.phase === "complete"
          ? CheckCircle2
          : Loader2;
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <Icon size={13} strokeWidth={2} />
      <span style={{ fontWeight: 600, color: "var(--text-primary)" }}>
        {goal.objective ?? goal.title ?? goal.id}
      </span>
      <span style={{ color: "var(--text-hint)" }}>· {phaseLabel}</span>
    </div>
  );
}

function TodoRow({ items }: { items: DshTodoItem[] }) {
  const { t } = useI18n();
  const completed = items.filter((i) => i.status === "completed").length;
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6, paddingTop: 1 }}>
        <ListChecks size={13} strokeWidth={2} />
        <span style={{ color: "var(--text-hint)" }}>
          {t("dsh.live.todoProgress", { completed, total: items.length })}
        </span>
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
        {items.slice(0, 8).map((item, idx) => {
          const Icon =
            item.status === "completed"
              ? CheckCircle2
              : item.status === "in_progress"
                ? Loader2
                : Circle;
          const color =
            item.status === "completed"
              ? "var(--success)"
              : item.status === "in_progress"
                ? "var(--primary-action-bg)"
                : "var(--text-hint)";
          return (
            <span
              key={idx}
              title={item.content}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 4,
                padding: "2px 7px",
                borderRadius: 4,
                background: "var(--bg-hover)",
                border: "1px solid var(--border-dim)",
                color: item.status === "completed" ? "var(--text-hint)" : "var(--text-secondary)",
                textDecoration: item.status === "completed" ? "line-through" : "none",
                maxWidth: 220,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              <Icon size={11} strokeWidth={2} style={{ color }} />
              <span>{item.activeForm ?? item.content}</span>
            </span>
          );
        })}
      </div>
    </div>
  );
}

function JobsRow({ jobs }: { jobs: DshJobView[] }) {
  const { t } = useI18n();
  const live = jobs.filter((j) => j.status === "running" || j.status === "stopping");
  if (live.length === 0) return null;
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <Layers size={13} strokeWidth={2} />
      <span style={{ color: "var(--text-hint)" }}>
        {t("dsh.live.jobsRunning", { count: live.length })}
      </span>
      <span style={{ display: "inline-flex", gap: 4 }}>
        {live.slice(0, 4).map((j) => (
          <span
            key={j.id}
            title={`${j.kind}: ${j.label ?? j.id}`}
            style={{
              padding: "1px 6px",
              borderRadius: 4,
              background: "var(--bg-hover)",
              border: "1px solid var(--border-dim)",
            }}
          >
            {j.label ?? j.kind}
          </span>
        ))}
      </span>
    </div>
  );
}

function QueueRow({ sessionId, items }: { sessionId: string; items: DshQueueItem[] }) {
  const { t } = useI18n();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingText, setEditingText] = useState("");
  const [busyId, setBusyId] = useState<string | null>(null);
  const updateItem = useCallback(
    async (itemId: string, action: unknown) => {
      if (!itemId || busyId) return;
      setBusyId(itemId);
      try {
        await invoke("update_dsh_session_queue", { sessionId, itemId, action });
      } finally {
        setBusyId(null);
      }
    },
    [busyId, sessionId],
  );
  const cancelItem = useCallback(
    async (itemId: string) => {
      try {
        await updateItem(itemId, { kind: "remove" });
      } catch {
        // The dsh web instance owns the queue; a stale id is benign.
      }
    },
    [updateItem],
  );
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
      <span style={{ color: "var(--text-hint)" }}>
        {t("dsh.live.queued", { count: items.length })}
      </span>
      {items.slice(0, 3).map((q) => (
        <span
          key={q.id ?? q.itemId}
          title={q.text ?? queueItemText(q)}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 4,
            padding: "2px 6px 2px 8px",
            borderRadius: 4,
            background: "var(--bg-hover)",
            border: "1px solid var(--border-dim)",
            maxWidth: 260,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {editingId === (q.id ?? q.itemId) ? (
            <input
              value={editingText}
              onChange={(event) => setEditingText(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  const id = q.id ?? q.itemId ?? "";
                  void updateItem(id, {
                    kind: "edit",
                    content: [{ type: "text", text: editingText }],
                  });
                  setEditingId(null);
                }
                if (event.key === "Escape") setEditingId(null);
              }}
              autoFocus
              style={{
                width: 150,
                minWidth: 0,
                border: "1px solid var(--border-medium)",
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                borderRadius: 3,
                padding: "1px 4px",
                font: "inherit",
              }}
            />
          ) : (
            <span style={{ color: "var(--text-secondary)" }}>{queueItemText(q)}</span>
          )}
          {editingId === (q.id ?? q.itemId) ? (
            <button
              type="button"
              aria-label={t("dsh.live.saveQueue")}
              title={t("dsh.live.saveQueue")}
              onClick={() => {
                const id = q.id ?? q.itemId ?? "";
                void updateItem(id, {
                  kind: "edit",
                  content: [{ type: "text", text: editingText }],
                });
                setEditingId(null);
              }}
              style={queueButtonStyle}
            >
              <Check size={11} strokeWidth={2} />
            </button>
          ) : (
            <button
              type="button"
              aria-label={t("dsh.live.editQueue")}
              title={t("dsh.live.editQueue")}
              onClick={() => {
                setEditingId(q.id ?? q.itemId ?? null);
                setEditingText(queueItemText(q));
              }}
              style={queueButtonStyle}
            >
              <Edit2 size={11} strokeWidth={2} />
            </button>
          )}
          <button
            type="button"
            aria-label={t("dsh.live.steerQueue")}
            title={t("dsh.live.steerQueue")}
            onClick={() => void updateItem(q.id ?? q.itemId ?? "", { kind: "steer" })}
            style={queueButtonStyle}
          >
            <Send size={11} strokeWidth={2} />
          </button>
          <button
            type="button"
            aria-label={t("dsh.live.cancelQueue")}
            onClick={() => cancelItem(q.id ?? q.itemId ?? "")}
            style={queueButtonStyle}
          >
            <X size={11} strokeWidth={2} />
          </button>
        </span>
      ))}
    </div>
  );
}

const queueButtonStyle = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  width: 16,
  height: 16,
  padding: 0,
  background: "transparent",
  border: "none",
  color: "var(--text-hint)",
  cursor: "pointer",
  borderRadius: 3,
} as const;

function queueItemText(item: DshQueueItem): string {
  if (item.text) return item.text;
  return (item.message?.content ?? [])
    .map((part) => {
      if (typeof part === "string") return part;
      if (part && typeof part === "object" && "text" in part) {
        const text = (part as { text?: unknown }).text;
        return typeof text === "string" ? text : "";
      }
      return "";
    })
    .join("");
}
