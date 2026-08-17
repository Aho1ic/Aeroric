/**
 * DSH Subagents panel — list and interrupt subagents for a selected session.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { History, RefreshCw, Send, Square } from "lucide-react";
import type { DshSessionHistory, DshSessionSummary, DshSubagentSummary } from "../../types";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import "./DshPluginsPanel.css";

function errorMessage(e: unknown): string {
  if (typeof e === "string" && e.trim()) return e;
  if (e instanceof Error) return e.message;
  return String(e || "Unknown error");
}

export function DshSubagentsPanel() {
  const { t } = useI18n();
  const [sessions, setSessions] = useState<DshSessionSummary[]>([]);
  const [selectedSessionId, setSelectedSessionId] = useState<string>("");
  const [subagents, setSubagents] = useState<DshSubagentSummary[]>([]);
  const [loadingSessions, setLoadingSessions] = useState(false);
  const [loadingSubagents, setLoadingSubagents] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [interruptingId, setInterruptingId] = useState<string | null>(null);
  const [promptingId, setPromptingId] = useState<string | null>(null);
  const [promptText, setPromptText] = useState<Record<string, string>>({});
  const [historyFor, setHistoryFor] = useState<string | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyData, setHistoryData] = useState<DshSessionHistory | null>(null);

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

  const loadSubagents = useCallback(async (sessionId: string) => {
    setLoadingSubagents(true);
    setError(null);
    try {
      const list = await invoke<DshSubagentSummary[]>("list_dsh_subagents", { sessionId });
      setSubagents(list);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoadingSubagents(false);
    }
  }, []);

  useEffect(() => {
    void loadSessions();
  }, [loadSessions]);

  useEffect(() => {
    if (selectedSessionId) void loadSubagents(selectedSessionId);
    else setSubagents([]);
  }, [loadSubagents, selectedSessionId]);

  async function handleInterrupt(sessionId: string) {
    setInterruptingId(sessionId);
    try {
      const child = subagents.find((item) => item.sessionId === sessionId);
      await invoke("interrupt_dsh_subagent", {
        sessionId,
        parentSessionId: child?.parentSessionId ?? selectedSessionId,
        mode: child?.mode ?? "continuable",
      });
      if (selectedSessionId) void loadSubagents(selectedSessionId);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setInterruptingId(null);
    }
  }

  async function handlePromptSubagent(sessionId: string) {
    const text = (promptText[sessionId] ?? "").trim();
    if (!text || promptingId) return;
    setPromptingId(sessionId);
    setError(null);
    try {
      const child = subagents.find((item) => item.sessionId === sessionId);
      await invoke("prompt_dsh_subagent", {
        sessionId,
        parentSessionId: child?.parentSessionId ?? selectedSessionId,
        mode: child?.mode ?? "continuable",
        content: text,
        clientTimeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      });
      setPromptText((prev) => ({ ...prev, [sessionId]: "" }));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setPromptingId(null);
    }
  }

  async function handleShowHistory(sessionId: string) {
    if (historyFor === sessionId) {
      setHistoryFor(null);
      setHistoryData(null);
      return;
    }
    setHistoryFor(sessionId);
    setHistoryLoading(true);
    setHistoryData(null);
    try {
      const child = subagents.find((item) => item.sessionId === sessionId);
      const data = await invoke<DshSessionHistory>("get_dsh_subagent_history", {
        sessionId,
        parentSessionId: child?.parentSessionId ?? selectedSessionId,
        mode: child?.mode ?? "continuable",
      });
      setHistoryData(data);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setHistoryLoading(false);
    }
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
            <h2>{t("appSettings.dshSubagentsTitle")}</h2>
            <p>{t("appSettings.dshSubagentsIntro")}</p>
          </header>
          <Button
            variant="outline"
            size="sm"
            icon={RefreshCw}
            disabled={loadingSubagents || loadingSessions}
            onClick={() => {
              void loadSessions();
              if (selectedSessionId) void loadSubagents(selectedSessionId);
            }}
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
            onChange={(e) => setSelectedSessionId(e.target.value)}
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

        {/* Subagent list */}
        <div style={{ marginTop: 18 }}>
          {loadingSubagents ? (
            <div className="dsh-empty-state">{t("appSettings.dshLoading")}</div>
          ) : !selectedSessionId ? null : subagents.length === 0 ? (
            <div className="dsh-empty-state">{t("appSettings.dshNoSubagents")}</div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {subagents.map((sa) => (
                <div
                  key={sa.sessionId}
                  className="dsh-config-card"
                  style={{ padding: "11px 14px" }}
                >
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
                        <code
                          style={{
                            fontSize: 12,
                            fontFamily: "var(--font-mono)",
                            color: "var(--text-primary)",
                          }}
                        >
                          {sa.sessionId.slice(0, 12)}
                        </code>
                        <span
                          style={{
                            padding: "2px 6px",
                            borderRadius: "var(--radius-sm)",
                            fontSize: 10,
                            fontWeight: 600,
                            background: sa.running
                              ? "color-mix(in srgb, var(--success, #22c55e) 15%, transparent)"
                              : "var(--bg-subtle)",
                            color: sa.running ? "var(--success, #22c55e)" : "var(--text-hint)",
                          }}
                        >
                          {sa.running
                            ? t("appSettings.dshSubagentRunning")
                            : t("appSettings.dshSubagentStopped")}
                        </span>
                      </div>
                      {sa.cwd && (
                        <div
                          style={{
                            marginTop: 3,
                            fontSize: 11,
                            color: "var(--text-hint)",
                            fontFamily: "var(--font-mono)",
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {sa.cwd}
                        </div>
                      )}
                    </div>
                    {sa.running && (
                      <Button
                        variant="outline"
                        size="xs"
                        icon={Square}
                        disabled={interruptingId === sa.sessionId}
                        onClick={() => void handleInterrupt(sa.sessionId)}
                      >
                        {interruptingId === sa.sessionId
                          ? t("appSettings.dshInterrupting")
                          : t("appSettings.dshInterruptSubagent")}
                      </Button>
                    )}
                    <Button
                      variant="outline"
                      size="xs"
                      icon={History}
                      disabled={historyFor === sa.sessionId && historyLoading}
                      onClick={() => void handleShowHistory(sa.sessionId)}
                    >
                      {historyFor === sa.sessionId && historyLoading
                        ? t("appSettings.dshLoading")
                        : t("appSettings.dshSubagentHistory")}
                    </Button>
                  </div>
                  {/* Only continuable children accept follow-up prompts. */}
                  {sa.mode !== "one-shot" && (
                    <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
                      <input
                        type="text"
                        value={promptText[sa.sessionId] ?? ""}
                        onChange={(e) =>
                          setPromptText((prev) => ({ ...prev, [sa.sessionId]: e.target.value }))
                        }
                        placeholder={t("appSettings.dshSubagentPromptPlaceholder")}
                        style={{
                          flex: 1,
                          minWidth: 0,
                          height: 28,
                          padding: "0 8px",
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
                        variant="outline"
                        size="xs"
                        icon={Send}
                        disabled={
                          promptingId === sa.sessionId || !(promptText[sa.sessionId] ?? "").trim()
                        }
                        onClick={() => void handlePromptSubagent(sa.sessionId)}
                      >
                        {promptingId === sa.sessionId
                          ? t("appSettings.dshSending")
                          : t("appSettings.dshSend")}
                      </Button>
                    </div>
                  )}
                  {/* Inline history viewer */}
                  {historyFor === sa.sessionId && (
                    <div
                      style={{
                        marginTop: 6,
                        padding: 8,
                        borderRadius: "var(--radius-sm)",
                        background: "var(--bg-subtle)",
                        border: "1px solid var(--border-dim)",
                        maxHeight: 200,
                        overflowY: "auto",
                        fontSize: 11,
                        fontFamily: "var(--font-mono)",
                        color: "var(--text-secondary)",
                        whiteSpace: "pre-wrap",
                      }}
                    >
                      {historyLoading
                        ? t("appSettings.dshLoading")
                        : historyData && historyData.events.length > 0
                          ? JSON.stringify(historyData.events, null, 2)
                          : t("appSettings.dshNoSubagentHistory")}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
