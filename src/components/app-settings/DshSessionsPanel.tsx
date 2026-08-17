/**
 * DSH Sessions & Workspaces panel — settings nav entry.
 * Shows sessions grouped by workspace, with search, rename, fork, archive.
 * Secondary tab exposes workspace CRUD.
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Archive,
  Check,
  FolderOpen,
  GitFork,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import type { DshSessionSummary, DshWorkspace, DshWorkspaceList } from "../../types";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { AnimatedSelectionGroup } from "../ui/AnimatedSelection";
import "./DshPluginsPanel.css";

type TabKey = "sessions" | "workspaces";

function errorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  return String(error || "Unknown error");
}

function timeAgo(ts: number, lang: string): string {
  const diff = (Date.now() - ts) / 1000;
  if (diff < 60) return lang === "zh" ? "刚刚" : "just now";
  if (diff < 3600) {
    const m = Math.floor(diff / 60);
    return lang === "zh" ? `${m}分钟前` : `${m}m ago`;
  }
  if (diff < 86400) {
    const h = Math.floor(diff / 3600);
    return lang === "zh" ? `${h}小时前` : `${h}h ago`;
  }
  const d = Math.floor(diff / 86400);
  return lang === "zh" ? `${d}天前` : `${d}d ago`;
}

export function DshSessionsPanel() {
  const { t, language } = useI18n();
  const [tab, setTab] = useState<TabKey>("sessions");

  return (
    <div className="dsh-settings-panel">
      <AnimatedSelectionGroup
        value={tab}
        onChange={setTab}
        ariaLabel={t("appSettings.dshSessionsPrimaryViews")}
        role="tablist"
        equalWidth
        className="dsh-primary-tabs"
        options={[
          { value: "sessions", label: t("appSettings.dshSessionsTab") },
          { value: "workspaces", label: t("appSettings.dshWorkspacesTab") },
        ]}
      />
      {tab === "sessions" ? <SessionsView lang={language} t={t} /> : <WorkspacesView t={t} />}
    </div>
  );
}

// ── Sessions view ─────────────────────────────────────────────────────────────

function SessionsView({
  lang,
  t,
}: {
  lang: string;
  t: (key: string, vars?: Record<string, string | number>) => string;
}) {
  const [workspaceData, setWorkspaceData] = useState<DshWorkspaceList | null>(null);
  const [sessions, setSessions] = useState<DshSessionSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameTitle, setRenameTitle] = useState("");
  const [renameSaving, setRenameSaving] = useState(false);

  useEffect(() => {
    void load();
  }, []);

  useEffect(() => {
    const refresh = () => void load();
    window.addEventListener("dsh-host-refresh", refresh);
    return () => window.removeEventListener("dsh-host-refresh", refresh);
  }, []);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [wl, sl] = await Promise.all([
        invoke<DshWorkspaceList>("list_dsh_workspaces"),
        invoke<DshSessionSummary[]>("list_dsh_sessions"),
      ]);
      setWorkspaceData(wl);
      setSessions(sl);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleSearch(query: string) {
    setSearchQuery(query);
    if (!query.trim()) {
      void load();
      return;
    }
    try {
      const result = await invoke<{ items: DshSessionSummary[]; hasMore: boolean }>(
        "search_dsh_sessions",
        { query },
      );
      setSessions(result.items);
    } catch {
      // keep existing list on search error
    }
  }

  async function handleRename(sessionId: string) {
    if (!renameTitle.trim() || renameSaving) return;
    setRenameSaving(true);
    try {
      await invoke("rename_dsh_session", { sessionId, title: renameTitle.trim() });
      setRenamingId(null);
      void load();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRenameSaving(false);
    }
  }

  async function handleFork(sessionId: string) {
    try {
      await invoke("fork_dsh_session", { sessionId });
      void load();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function handleArchive(sessionId: string) {
    try {
      await invoke("archive_dsh_session", { sessionId });
      void load();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  // Group sessions by workspace
  const workspaceMap = new Map<string, string>(); // sessionId → workspaceTitle
  if (workspaceData) {
    for (const ws of workspaceData.items) {
      for (const sid of ws.sessionIds) {
        workspaceMap.set(sid, ws.title || ws.path);
      }
    }
  }

  const filteredSessions = searchQuery.trim()
    ? sessions
    : sessions.filter((s) => !workspaceData?.archivedSessionIds.includes(s.sessionId));

  // Group by workspace
  const grouped: Map<string, DshSessionSummary[]> = new Map();
  for (const s of filteredSessions) {
    const ws = workspaceMap.get(s.sessionId) ?? t("appSettings.dshNoWorkspace");
    if (!grouped.has(ws)) grouped.set(ws, []);
    grouped.get(ws)!.push(s);
  }

  return (
    <section className="dsh-page" aria-label={t("appSettings.dshSessionsTitle")}>
      <header className="dsh-section-heading">
        <h2>{t("appSettings.dshSessionsTitle")}</h2>
        <p>{t("appSettings.dshSessionsIntro")}</p>
      </header>

      {error && (
        <p className="dsh-toolbar-error" role="alert">
          {error}
        </p>
      )}

      <div style={{ display: "flex", gap: 8, alignItems: "center", margin: "16px 0 12px" }}>
        <div style={{ flex: 1, position: "relative", display: "flex", alignItems: "center" }}>
          <Search
            size={14}
            style={{
              position: "absolute",
              left: 10,
              color: "var(--text-hint)",
              pointerEvents: "none",
            }}
          />
          <input
            type="text"
            placeholder={t("appSettings.dshSessionsSearch")}
            value={searchQuery}
            onChange={(e) => handleSearch(e.target.value)}
            style={{
              width: "100%",
              height: 30,
              padding: "0 10px 0 30px",
              border: "1px solid var(--border-medium)",
              borderRadius: "var(--radius-sm)",
              background: "var(--bg-input)",
              color: "var(--text-primary)",
              fontSize: 12,
              fontFamily: "inherit",
              outline: "none",
            }}
          />
        </div>
        <Button
          variant="outline"
          size="sm"
          icon={RefreshCw}
          disabled={loading}
          onClick={() => void load()}
        >
          {t("appSettings.dshRefresh")}
        </Button>
      </div>

      {loading ? (
        <div style={{ padding: 20, color: "var(--text-hint)", fontSize: 12 }}>
          {t("appSettings.dshLoading")}
        </div>
      ) : filteredSessions.length === 0 ? (
        <div style={{ padding: 20, color: "var(--text-hint)", fontSize: 12 }}>
          {t("appSettings.dshNoSessions")}
        </div>
      ) : (
        Array.from(grouped.entries()).map(([wsName, wsSessions]) => (
          <div key={wsName} style={{ marginBottom: 20 }}>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                marginBottom: 8,
                color: "var(--text-secondary)",
                fontSize: 11,
                fontWeight: 700,
                textTransform: "uppercase",
                letterSpacing: "0.5px",
              }}
            >
              <FolderOpen size={12} />
              {wsName}
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {wsSessions.map((session) => (
                <div
                  key={session.sessionId}
                  className="dsh-config-card"
                  style={{ padding: "10px 14px" }}
                >
                  {renamingId === session.sessionId ? (
                    <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                      <input
                        autoFocus
                        type="text"
                        value={renameTitle}
                        onChange={(e) => setRenameTitle(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") void handleRename(session.sessionId);
                          if (e.key === "Escape") setRenamingId(null);
                        }}
                        style={{
                          flex: 1,
                          height: 26,
                          padding: "0 8px",
                          border: "1px solid var(--border-focus)",
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
                        size="xs"
                        icon={Check}
                        disabled={renameSaving}
                        onClick={() => void handleRename(session.sessionId)}
                      />
                      <Button
                        variant="ghost"
                        size="xs"
                        icon={X}
                        onClick={() => setRenamingId(null)}
                      />
                    </div>
                  ) : (
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div
                          style={{ fontSize: 12.5, fontWeight: 600, color: "var(--text-primary)" }}
                        >
                          {session.sessionId.slice(0, 8)}
                          {session.running && (
                            <span
                              style={{
                                marginLeft: 8,
                                padding: "1px 6px",
                                background:
                                  "color-mix(in srgb, var(--success, #22c55e) 15%, transparent)",
                                color: "var(--success, #22c55e)",
                                borderRadius: "var(--radius-sm)",
                                fontSize: 10,
                                fontWeight: 700,
                              }}
                            >
                              {t("appSettings.dshSessionRunning")}
                            </span>
                          )}
                        </div>
                        <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 2 }}>
                          {timeAgo(session.updatedAt, lang)}
                          {session.cwd && (
                            <span
                              style={{
                                marginLeft: 8,
                                fontFamily: "var(--font-mono)",
                                opacity: 0.7,
                              }}
                            >
                              {session.cwd}
                            </span>
                          )}
                        </div>
                      </div>
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        icon={Pencil}
                        title={t("appSettings.dshRenameSession")}
                        onClick={() => {
                          setRenamingId(session.sessionId);
                          setRenameTitle("");
                        }}
                      />
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        icon={GitFork}
                        title={t("appSettings.dshForkSession")}
                        onClick={() => void handleFork(session.sessionId)}
                      />
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        icon={Archive}
                        title={t("appSettings.dshArchiveSession")}
                        onClick={() => void handleArchive(session.sessionId)}
                      />
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        ))
      )}
    </section>
  );
}

// ── Workspaces view ───────────────────────────────────────────────────────────

function WorkspacesView({ t }: { t: (key: string) => string }) {
  const [workspaces, setWorkspaces] = useState<DshWorkspace[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creatingPath, setCreatingPath] = useState("");
  const [creating, setCreating] = useState(false);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameTitle, setRenameTitle] = useState("");
  const [renameSaving, setRenameSaving] = useState(false);

  useEffect(() => {
    void load();
  }, []);

  useEffect(() => {
    const refresh = () => void load();
    window.addEventListener("dsh-host-refresh", refresh);
    return () => window.removeEventListener("dsh-host-refresh", refresh);
  }, []);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const data = await invoke<DshWorkspaceList>("list_dsh_workspaces");
      setWorkspaces(data.items);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleCreate() {
    if (!creatingPath.trim() || creating) return;
    setCreating(true);
    setError(null);
    try {
      await invoke("create_dsh_workspace", { path: creatingPath.trim() });
      setCreatingPath("");
      void load();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setCreating(false);
    }
  }

  async function handleRename(workspaceId: string) {
    if (!renameTitle.trim() || renameSaving) return;
    setRenameSaving(true);
    try {
      await invoke("rename_dsh_workspace", { workspaceId, title: renameTitle.trim() });
      setRenamingId(null);
      void load();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRenameSaving(false);
    }
  }

  async function handleDelete(workspaceId: string) {
    try {
      await invoke("delete_dsh_workspace", { workspaceId });
      void load();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <section className="dsh-page" aria-label={t("appSettings.dshWorkspacesTitle")}>
      <header className="dsh-section-heading">
        <h2>{t("appSettings.dshWorkspacesTitle")}</h2>
        <p>{t("appSettings.dshWorkspacesIntro")}</p>
      </header>

      {error && (
        <p className="dsh-toolbar-error" role="alert">
          {error}
        </p>
      )}

      <div style={{ display: "flex", gap: 8, alignItems: "center", margin: "16px 0 14px" }}>
        <input
          type="text"
          placeholder={t("appSettings.dshWorkspacePath")}
          value={creatingPath}
          onChange={(e) => setCreatingPath(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void handleCreate();
          }}
          style={{
            flex: 1,
            height: 30,
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
          disabled={creating || !creatingPath.trim()}
          onClick={() => void handleCreate()}
        >
          {t("appSettings.dshCreateWorkspace")}
        </Button>
      </div>

      {loading ? (
        <div style={{ padding: 20, color: "var(--text-hint)", fontSize: 12 }}>
          {t("appSettings.dshLoading")}
        </div>
      ) : workspaces.length === 0 ? (
        <div style={{ padding: 20, color: "var(--text-hint)", fontSize: 12 }}>
          {t("appSettings.dshNoWorkspaces")}
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {workspaces.map((ws) => (
            <div key={ws.workspaceId} className="dsh-config-card" style={{ padding: "12px 14px" }}>
              {renamingId === ws.workspaceId ? (
                <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                  <input
                    autoFocus
                    type="text"
                    value={renameTitle}
                    onChange={(e) => setRenameTitle(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void handleRename(ws.workspaceId);
                      if (e.key === "Escape") setRenamingId(null);
                    }}
                    style={{
                      flex: 1,
                      height: 26,
                      padding: "0 8px",
                      border: "1px solid var(--border-focus)",
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
                    size="xs"
                    icon={Check}
                    disabled={renameSaving}
                    onClick={() => void handleRename(ws.workspaceId)}
                  />
                  <Button variant="ghost" size="xs" icon={X} onClick={() => setRenamingId(null)} />
                </div>
              ) : (
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <FolderOpen size={16} color="var(--text-secondary)" style={{ flexShrink: 0 }} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                      {ws.title || ws.path}
                    </div>
                    <div
                      style={{
                        fontSize: 11,
                        color: "var(--text-secondary)",
                        fontFamily: "var(--font-mono)",
                        marginTop: 2,
                      }}
                    >
                      {ws.path}
                    </div>
                    <div style={{ fontSize: 11, color: "var(--text-hint)", marginTop: 2 }}>
                      {ws.sessionIds.length} {t("appSettings.dshSessionsCount")}
                    </div>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    icon={Pencil}
                    title={t("appSettings.dshRenameWorkspace")}
                    onClick={() => {
                      setRenamingId(ws.workspaceId);
                      setRenameTitle(ws.title || "");
                    }}
                  />
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    icon={Trash2}
                    title={t("appSettings.dshDeleteWorkspace")}
                    style={{ color: "var(--danger)" }}
                    onClick={() => void handleDelete(ws.workspaceId)}
                  />
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
