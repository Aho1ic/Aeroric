/**
 * DSH Skills panel — browse skills available to a selected session's agent.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BookOpen, RefreshCw } from "lucide-react";
import type { DshSessionSummary, DshSkillEntry } from "../../types";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import "./DshPluginsPanel.css";

function errorMessage(e: unknown): string {
  if (typeof e === "string" && e.trim()) return e;
  if (e instanceof Error) return e.message;
  return String(e || "Unknown error");
}

export function DshSkillsPanel() {
  const { t } = useI18n();
  const [sessions, setSessions] = useState<DshSessionSummary[]>([]);
  const [selectedSessionId, setSelectedSessionId] = useState<string>("");
  const [skills, setSkills] = useState<DshSkillEntry[]>([]);
  const [loadingSessions, setLoadingSessions] = useState(false);
  const [loadingSkills, setLoadingSkills] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadSessions = useCallback(async () => {
    setLoadingSessions(true);
    setError(null);
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

  const loadSkills = useCallback(async (sessionId: string) => {
    setLoadingSkills(true);
    setError(null);
    try {
      const list = await invoke<DshSkillEntry[]>("list_dsh_skills", { sessionId });
      setSkills(list);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoadingSkills(false);
    }
  }, []);

  useEffect(() => {
    void loadSessions();
  }, [loadSessions]);

  useEffect(() => {
    if (selectedSessionId) void loadSkills(selectedSessionId);
    else setSkills([]);
  }, [loadSkills, selectedSessionId]);

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
            <h2>{t("appSettings.dshSkillsTitle")}</h2>
            <p>{t("appSettings.dshSkillsIntro")}</p>
          </header>
          <Button
            variant="outline"
            size="sm"
            icon={RefreshCw}
            disabled={loadingSkills || loadingSessions}
            onClick={() => {
              void loadSessions();
              if (selectedSessionId) void loadSkills(selectedSessionId);
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

        {/* Skills list */}
        <div style={{ marginTop: 18 }}>
          {loadingSkills ? (
            <div className="dsh-empty-state">{t("appSettings.dshLoading")}</div>
          ) : !selectedSessionId ? null : skills.length === 0 ? (
            <div className="dsh-empty-state">{t("appSettings.dshNoSkills")}</div>
          ) : (
            <div className="dsh-plugin-grid">
              {skills.map((skill) => (
                <div key={skill.id} className="dsh-plugin-card">
                  <div className="dsh-plugin-card__main" style={{ cursor: "default" }}>
                    <div
                      className="dsh-config-card__icon"
                      style={{ flexShrink: 0, color: "var(--accent)" }}
                    >
                      <BookOpen size={14} />
                    </div>
                    <div style={{ flex: 1, minWidth: 0, display: "grid", gap: 2 }}>
                      <strong
                        style={{
                          fontSize: 12,
                          fontWeight: 600,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {skill.name ?? skill.id}
                      </strong>
                      {skill.name && (
                        <span
                          style={{
                            fontSize: 10,
                            color: "var(--text-hint)",
                            fontFamily: "var(--font-mono)",
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {skill.id}
                        </span>
                      )}
                    </div>
                  </div>
                  {skill.description && (
                    <div className="dsh-plugin-card__details">
                      <p
                        style={{
                          margin: 0,
                          fontSize: 11.5,
                          color: "var(--text-secondary)",
                          lineHeight: 1.5,
                        }}
                      >
                        {skill.description}
                      </p>
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
