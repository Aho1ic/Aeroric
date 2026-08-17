/**
 * DSH Goals panel — create and manage goals for a selected session.
 *
 * Goals are exposed through the session projection stream. The history tail
 * carries the same projection baseline, so this panel can restore an existing
 * goal after reload and then keep local state responsive to mutations.
 */
import { useCallback, useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CheckCircle, Pause, Pencil, Play, Plus, RefreshCw, Trash2 } from "lucide-react";
import type { DshGoal, DshSessionSummary } from "../../types";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import "./DshPluginsPanel.css";

function errorMessage(e: unknown): string {
  if (typeof e === "string" && e.trim()) return e;
  if (e instanceof Error) return e.message;
  return String(e || "Unknown error");
}

interface GoalRowProps {
  goal: DshGoal;
  sessionId: string;
  t: (k: string) => string;
  onUpdate: (updated: DshGoal) => void;
  onCleared: () => void;
}

function GoalRow({ goal, sessionId, t, onUpdate, onCleared }: GoalRowProps) {
  const [acting, setActing] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState(goal.title);

  async function act(cmd: string) {
    if (acting) return;
    setActing(true);
    try {
      const updated = await invoke<DshGoal>(cmd, {
        sessionId,
        goalId: goal.goalId,
        revision: goal.revision,
      });
      onUpdate(updated);
    } finally {
      setActing(false);
    }
  }

  async function commitEdit() {
    const next = editTitle.trim();
    if (!next || next === goal.title || acting) {
      setEditing(false);
      return;
    }
    setActing(true);
    try {
      const updated = await invoke<DshGoal>("edit_dsh_goal", {
        sessionId,
        goalId: goal.goalId,
        revision: goal.revision,
        title: next,
      });
      onUpdate(updated);
      setEditing(false);
    } finally {
      setActing(false);
    }
  }

  const isPaused = goal.status === "paused";
  const isRunning = goal.status === "running" || goal.status === "active";
  const isDone = goal.status === "completed" || goal.status === "done";

  return (
    <div
      className="dsh-config-card"
      style={{
        padding: "11px 14px",
        display: "flex",
        alignItems: "center",
        gap: 10,
        opacity: isDone ? 0.6 : 1,
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        {editing ? (
          <div style={{ display: "flex", gap: 6 }}>
            <input
              autoFocus
              type="text"
              value={editTitle}
              onChange={(e) => setEditTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void commitEdit();
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  setEditTitle(goal.title);
                  setEditing(false);
                }
              }}
              style={{
                flex: 1,
                minWidth: 0,
                height: 26,
                padding: "0 8px",
                border: "1px solid var(--border-medium)",
                borderRadius: "var(--radius-sm)",
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                fontSize: 12.5,
                fontFamily: "inherit",
                outline: "none",
              }}
            />
            <Button variant="outline" size="xs" disabled={acting} onClick={() => void commitEdit()}>
              {t("appSettings.dshGoalSave")}
            </Button>
          </div>
        ) : (
          <>
            <div style={{ fontSize: 12.5, fontWeight: 600, color: "var(--text-primary)" }}>
              {goal.title}
            </div>
            <div style={{ marginTop: 2, fontSize: 10.5, color: "var(--text-hint)" }}>
              {goal.goalId.slice(0, 10)} · {goal.status}
            </div>
          </>
        )}
      </div>
      <div style={{ display: "flex", gap: 5, flexShrink: 0 }}>
        {!isDone && !editing && (
          <>
            <Button
              variant="ghost"
              size="icon-xs"
              icon={Pencil}
              title={t("appSettings.dshGoalEdit")}
              disabled={acting}
              onClick={() => {
                setEditTitle(goal.title);
                setEditing(true);
              }}
            />
            {isPaused ? (
              <Button
                variant="ghost"
                size="icon-xs"
                icon={Play}
                title={t("appSettings.dshGoalResume")}
                disabled={acting}
                onClick={() => void act("resume_dsh_goal")}
              />
            ) : isRunning ? (
              <Button
                variant="ghost"
                size="icon-xs"
                icon={Pause}
                title={t("appSettings.dshGoalPause")}
                disabled={acting}
                onClick={() => void act("pause_dsh_goal")}
              />
            ) : null}
            <Button
              variant="ghost"
              size="icon-xs"
              icon={CheckCircle}
              title={t("appSettings.dshGoalComplete")}
              disabled={acting}
              style={{ color: "var(--success, #22c55e)" }}
              onClick={() => void act("complete_dsh_goal")}
            />
          </>
        )}
        <Button
          variant="ghost"
          size="icon-xs"
          icon={Trash2}
          title={t("appSettings.dshGoalClear")}
          disabled={acting}
          style={{ color: "var(--danger)" }}
          onClick={onCleared}
        />
      </div>
    </div>
  );
}

// ── Main panel ────────────────────────────────────────────────────────────────

export function DshGoalsPanel() {
  const { t } = useI18n();
  const [sessions, setSessions] = useState<DshSessionSummary[]>([]);
  const [selectedSessionId, setSelectedSessionId] = useState<string>("");
  const [goals, setGoals] = useState<DshGoal[]>([]);
  const [newGoalTitle, setNewGoalTitle] = useState("");
  const [creating, setCreating] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [loadingSessions, setLoadingSessions] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadSessions = useCallback(async () => {
    setLoadingSessions(true);
    try {
      const list = await invoke<DshSessionSummary[]>("list_dsh_sessions");
      setSessions(list);
      if (list.length > 0 && !selectedSessionId) {
        setSelectedSessionId(list[0].sessionId);
      }
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoadingSessions(false);
    }
  }, [selectedSessionId]);

  useEffect(() => {
    void loadSessions();
  }, [loadSessions]);

  useEffect(() => {
    if (!selectedSessionId) {
      setGoals([]);
      return;
    }
    let cancelled = false;
    void invoke<{ projections?: { values?: Record<string, unknown> } }>("get_dsh_session_history", {
      sessionId: selectedSessionId,
      maxMessages: 1,
    })
      .then((history) => {
        if (cancelled) return;
        const projection = history.projections?.values?.goal;
        const raw =
          projection && typeof projection === "object"
            ? (projection as { goal?: unknown }).goal
            : null;
        if (!raw || typeof raw !== "object") {
          setGoals([]);
          return;
        }
        const value = raw as {
          id?: unknown;
          objective?: unknown;
          title?: unknown;
          revision?: unknown;
          phase?: unknown;
        };
        if (typeof value.id !== "string" || typeof value.revision !== "number") {
          setGoals([]);
          return;
        }
        setGoals([
          {
            goalId: value.id,
            title:
              typeof value.objective === "string"
                ? value.objective
                : typeof value.title === "string"
                  ? value.title
                  : value.id,
            revision: value.revision,
            status: typeof value.phase === "string" ? value.phase : "active",
          },
        ]);
      })
      .catch(() => {
        if (!cancelled) setGoals([]);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedSessionId]);

  async function handleCreate() {
    if (!newGoalTitle.trim() || !selectedSessionId || creating) return;
    setCreating(true);
    setError(null);
    try {
      const goal = await invoke<DshGoal>("create_dsh_goal", {
        sessionId: selectedSessionId,
        title: newGoalTitle.trim(),
      });
      setGoals((prev) => [goal, ...prev]);
      setNewGoalTitle("");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setCreating(false);
    }
  }

  async function handleClearAll() {
    if (!selectedSessionId || clearing) return;
    setClearing(true);
    setError(null);
    try {
      const current = goals[0];
      if (!current) return;
      await invoke("clear_dsh_goals", {
        sessionId: selectedSessionId,
        goalId: current.goalId,
        revision: current.revision,
      });
      setGoals([]);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setClearing(false);
    }
  }

  function handleUpdate(updated: DshGoal) {
    setGoals((prev) =>
      prev.map((g) =>
        g.goalId === updated.goalId
          ? {
              ...g,
              ...updated,
              title: updated.title || g.title,
              status: updated.status || g.status,
            }
          : g,
      ),
    );
  }

  function handleRemoveLocal(goalId: string) {
    setGoals((prev) => prev.filter((g) => g.goalId !== goalId));
  }

  return (
    <div className="dsh-settings-panel">
      <div className="dsh-page">
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
            gap: 12,
          }}
        >
          <header className="dsh-section-heading" style={{ flex: 1 }}>
            <h2>{t("appSettings.dshGoalsTitle")}</h2>
            <p>{t("appSettings.dshGoalsIntro")}</p>
          </header>
          <Button
            variant="outline"
            size="sm"
            icon={RefreshCw}
            disabled={loadingSessions}
            onClick={() => void loadSessions()}
            style={{ marginTop: 4, flexShrink: 0 }}
          >
            {t("appSettings.dshRefresh")}
          </Button>
        </div>

        {error && (
          <p className="dsh-toolbar-error" role="alert">
            {error}
          </p>
        )}

        {/* Session selector */}
        <div style={{ marginTop: 16 }}>
          <select
            value={selectedSessionId}
            onChange={(e) => {
              setSelectedSessionId(e.target.value);
              setGoals([]);
            }}
            disabled={loadingSessions || sessions.length === 0}
            style={{
              width: "100%",
              maxWidth: 420,
              height: 32,
              padding: "0 10px",
              border: "1px solid var(--border-medium)",
              borderRadius: "var(--radius-sm)",
              background: "var(--bg-input)",
              color: sessions.length === 0 ? "var(--text-hint)" : "var(--text-primary)",
              fontSize: 12,
              fontFamily: "inherit",
              outline: "none",
            }}
          >
            {sessions.length === 0 ? (
              <option value="">{t("appSettings.dshNoSessions")}</option>
            ) : (
              <>
                <option value="">{t("appSettings.dshSelectSession")}</option>
                {sessions.map((s) => (
                  <option key={s.sessionId} value={s.sessionId}>
                    {s.sessionId.slice(0, 12)}
                    {s.running ? " ●" : ""}
                    {s.cwd ? `  ${s.cwd}` : ""}
                  </option>
                ))}
              </>
            )}
          </select>
        </div>

        {selectedSessionId && (
          <>
            {/* Create goal */}
            <div style={{ marginTop: 16, display: "flex", gap: 7, alignItems: "center" }}>
              <input
                type="text"
                placeholder={t("appSettings.dshNewGoalPlaceholder")}
                value={newGoalTitle}
                onChange={(e) => setNewGoalTitle(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void handleCreate();
                }}
                style={{
                  flex: 1,
                  height: 32,
                  padding: "0 10px",
                  border: "1px solid var(--border-medium)",
                  borderRadius: "var(--radius-sm)",
                  background: "var(--bg-input)",
                  color: "var(--text-primary)",
                  fontSize: 12,
                  fontFamily: "inherit",
                  outline: "none",
                }}
              />
              <Button
                variant="default"
                size="sm"
                icon={Plus}
                disabled={creating || !newGoalTitle.trim()}
                onClick={() => void handleCreate()}
              >
                {t("appSettings.dshCreateGoal")}
              </Button>
            </div>

            {/* Goals list */}
            <div style={{ marginTop: 16 }}>
              {goals.length === 0 ? (
                <div className="dsh-empty-state">{t("appSettings.dshNoGoals")}</div>
              ) : (
                <>
                  <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                    {goals.map((goal) => (
                      <GoalRow
                        key={goal.goalId}
                        goal={goal}
                        sessionId={selectedSessionId}
                        t={t}
                        onUpdate={handleUpdate}
                        onCleared={() => handleRemoveLocal(goal.goalId)}
                      />
                    ))}
                  </div>
                  <div style={{ marginTop: 14, display: "flex", justifyContent: "flex-end" }}>
                    <Button
                      variant="outline"
                      size="sm"
                      icon={Trash2}
                      disabled={clearing}
                      style={{
                        color: "var(--danger)",
                        borderColor: "color-mix(in srgb, var(--danger) 30%, var(--border-medium))",
                      }}
                      onClick={() => void handleClearAll()}
                    >
                      {t("appSettings.dshGoalClear")}
                    </Button>
                  </div>
                </>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
