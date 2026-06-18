import type React from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  ArrowLeft,
  Box,
  Container,
  Play,
  RefreshCw,
  RotateCw,
  ScrollText,
  Square,
  Tag,
  Trash2,
} from "lucide-react";
import type {
  DockerContainerSummary,
  DockerImageSummary,
  DockerResources,
  SshConnection,
} from "../../types";
import { useI18n } from "../../i18n";

type DockerTab = "images" | "containers";
type ContainerAction = "start" | "restart" | "stop" | "delete";

function dockerRemoteKey(remote?: SshConnection): string {
  if (!remote) return "local";
  return [
    remote.id,
    remote.name,
    remote.host,
    remote.port,
    remote.username,
    remote.remotePath,
    remote.identityFile,
    remote.password,
  ]
    .map((value) => value ?? "")
    .join("\u001f");
}

export function isIgnorableDockerRefreshError(error: unknown): boolean {
  const normalized = String(error).toLowerCase();
  return (
    normalized.includes("authorized users only") ||
    (normalized.includes("connection to ") && normalized.includes(" closed"))
  );
}

function isSameDockerRemote(a: SshConnection | null | undefined, b: SshConnection | null | undefined): boolean {
  return dockerRemoteKey(a ?? undefined) === dockerRemoteKey(b ?? undefined);
}

const pageStyle: React.CSSProperties = {
  flex: 1,
  display: "flex",
  flexDirection: "column",
  minWidth: 0,
  minHeight: 0,
  background: "var(--bg-panel)",
};

const headerStyle: React.CSSProperties = {
  height: 58,
  flexShrink: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 12,
  padding: "0 22px",
  borderBottom: "1px solid var(--border-dim)",
  background: "var(--bg-panel)",
};

const titleStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  minWidth: 0,
  fontSize: 15,
  fontWeight: 720,
  color: "var(--text-primary)",
};

const tabRowStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
};

function tabStyle(active: boolean): React.CSSProperties {
  return {
    height: 30,
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    padding: "0 10px",
    border: `1px solid ${active ? "var(--border-focus)" : "var(--border-dim)"}`,
    borderRadius: 7,
    background: active ? "var(--control-active-bg)" : "var(--bg-card)",
    color: active ? "var(--control-active-fg)" : "var(--text-secondary)",
    fontSize: 12,
    fontWeight: 650,
    cursor: "pointer",
    whiteSpace: "nowrap",
  };
}

const refreshButtonStyle: React.CSSProperties = {
  height: 30,
  display: "inline-flex",
  alignItems: "center",
  gap: 7,
  padding: "0 11px",
  border: "1px solid var(--border-dim)",
  borderRadius: 7,
  background: "var(--bg-card)",
  color: "var(--text-secondary)",
  fontSize: 12,
  fontWeight: 650,
  cursor: "pointer",
};

export const dockerBodyStyle: React.CSSProperties = {
  flex: 1,
  minWidth: 0,
  minHeight: 0,
  overflow: "auto",
  padding: "0 22px 22px",
  paddingTop: 0,
};

const tableStyle: React.CSSProperties = {
  width: "100%",
  borderCollapse: "separate",
  borderSpacing: 0,
  fontSize: 12,
  color: "var(--text-secondary)",
  userSelect: "text",
  WebkitUserSelect: "text",
};

const thStyle: React.CSSProperties = {
  position: "sticky",
  top: 0,
  zIndex: 1,
  padding: "9px 10px",
  background: "var(--bg-subtle)",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 11,
  fontWeight: 750,
  textAlign: "left",
  whiteSpace: "nowrap",
};

const tdStyle: React.CSSProperties = {
  padding: "10px",
  borderBottom: "1px solid var(--border-dim)",
  verticalAlign: "middle",
  maxWidth: 260,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  userSelect: "text",
  WebkitUserSelect: "text",
};

const monoCellStyle: React.CSSProperties = {
  ...tdStyle,
  fontFamily: "var(--font-mono)",
  fontSize: 11.5,
};

const nameCellStyle: React.CSSProperties = {
  ...tdStyle,
  fontWeight: 720,
  color: "var(--text-primary)",
};

const emptyStyle: React.CSSProperties = {
  minHeight: 220,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  gap: 10,
  border: "1px dashed var(--border-medium)",
  borderRadius: 10,
  color: "var(--text-muted)",
  background: "var(--bg-subtle)",
};

const actionGroupStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 4,
};

export function dockerActionButtonStyle(tone: "neutral" | "start" | "restart" | "stop" | "logs" | "danger" = "neutral"): React.CSSProperties {
  const colors = {
    neutral: "var(--text-secondary)",
    start: "#4ade80",
    restart: "var(--warning)",
    stop: "#a855f7",
    logs: "#3b82f6",
    danger: "var(--danger)",
  } as const;
  const color = colors[tone];
  return {
    width: 24,
    height: 24,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    border: `1px solid color-mix(in srgb, ${color} 42%, var(--border-dim))`,
    borderRadius: 6,
    background: "transparent",
    color,
    cursor: "pointer",
    userSelect: "none",
    WebkitUserSelect: "none",
  };
}

function stateBadgeTone(state: string): { bg: string; fg: string; border: string } {
  switch (state.toLowerCase()) {
    case "running":
      return { bg: "color-mix(in srgb, var(--success) 14%, transparent)", fg: "var(--success)", border: "var(--success)" };
    case "exited":
      return { bg: "color-mix(in srgb, var(--text-muted) 14%, transparent)", fg: "var(--text-muted)", border: "var(--text-muted)" };
    case "paused":
      return { bg: "color-mix(in srgb, var(--warning) 16%, transparent)", fg: "var(--warning)", border: "var(--warning)" };
    case "restarting":
      return { bg: "color-mix(in srgb, var(--accent) 16%, transparent)", fg: "var(--accent)", border: "var(--accent)" };
    case "created":
    case "configured":
      return { bg: "color-mix(in srgb, #0891b2 14%, transparent)", fg: "#0891b2", border: "#0891b2" };
    case "dead":
    case "removing":
      return { bg: "color-mix(in srgb, var(--danger) 14%, transparent)", fg: "var(--danger)", border: "var(--danger)" };
    default:
      return { bg: "var(--bg-hover)", fg: "var(--text-secondary)", border: "var(--border-medium)" };
  }
}

function rowAccentColor(seed: string): string {
  const palette = ["#1f9d55", "#2563eb", "#d97706", "#7c3aed", "#db2777", "#0891b2", "#dc2626"];
  let hash = 0;
  for (const ch of seed) hash = (hash * 31 + ch.charCodeAt(0)) % 9973;
  return palette[hash % palette.length];
}

function StateBadge({ state }: { state: string }) {
  const tone = stateBadgeTone(state);
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        height: 20,
        padding: "0 7px",
        borderRadius: 999,
        border: `1px solid ${tone.border}`,
        background: tone.bg,
        color: tone.fg,
        fontSize: 11,
        fontWeight: 700,
      }}
    >
      {state}
    </span>
  );
}

function imageReference(image: DockerImageSummary): string {
  if (
    image.repository !== "-" &&
    image.repository !== "<none>" &&
    image.tag !== "-" &&
    image.tag !== "<none>"
  ) {
    return `${image.repository}:${image.tag}`;
  }
  return image.id;
}

function removeDeletedImage(resources: DockerResources | null, ref: string): DockerResources | null {
  if (!resources) return resources;
  return {
    ...resources,
    images: resources.images.filter((image) => imageReference(image) !== ref && image.id !== ref),
  };
}

function ImageTable({
  images,
  busyKey,
  onDelete,
  onTag,
}: {
  images: DockerImageSummary[];
  busyKey: string | null;
  onDelete: (image: DockerImageSummary) => void;
  onTag: (image: DockerImageSummary) => void;
}) {
  const { t } = useI18n();
  if (images.length === 0) {
    return (
      <div style={emptyStyle}>
        <Box size={28} strokeWidth={1.6} />
        <div>{t("docker.noImages")}</div>
      </div>
    );
  }
  return (
    <table style={tableStyle}>
      <thead>
        <tr>
          <th style={thStyle}>{t("docker.repository")}</th>
          <th style={thStyle}>{t("docker.tag")}</th>
          <th style={thStyle}>{t("docker.imageId")}</th>
          <th style={thStyle}>{t("docker.digest")}</th>
          <th style={thStyle}>{t("docker.created")}</th>
          <th style={thStyle}>{t("docker.size")}</th>
          <th style={thStyle}>{t("docker.actions")}</th>
        </tr>
      </thead>
      <tbody>
        {images.map((image) => {
          const ref = imageReference(image);
          const busy = busyKey === ref;
          const accent = rowAccentColor(ref);
          return (
          <tr key={`${image.id}:${image.repository}:${image.tag}`} style={{ boxShadow: `inset 3px 0 0 ${accent}` }}>
            <td style={nameCellStyle} title={image.repository}>{image.repository}</td>
            <td style={tdStyle} title={image.tag}>{image.tag}</td>
            <td style={monoCellStyle} title={image.id}>{image.id}</td>
            <td style={monoCellStyle} title={image.digest}>{image.digest}</td>
            <td style={tdStyle} title={image.createdSince}>{image.createdSince}</td>
            <td style={tdStyle} title={image.size}>{image.size}</td>
            <td style={tdStyle}>
              <span style={actionGroupStyle}>
                <button type="button" style={dockerActionButtonStyle()} disabled={busy} title={t("docker.tagImage")} onClick={() => onTag(image)}>
                  <Tag size={13} />
                </button>
                <button type="button" style={dockerActionButtonStyle("danger")} disabled={busy} title={t("docker.deleteImage")} onClick={() => onDelete(image)}>
                  <Trash2 size={13} />
                </button>
              </span>
            </td>
          </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function ContainerTable({
  containers,
  busyKey,
  onAction,
  onLogs,
}: {
  containers: DockerContainerSummary[];
  busyKey: string | null;
  onAction: (container: DockerContainerSummary, action: ContainerAction) => void;
  onLogs: (container: DockerContainerSummary) => void;
}) {
  const { t } = useI18n();
  if (containers.length === 0) {
    return (
      <div style={emptyStyle}>
        <Container size={28} strokeWidth={1.6} />
        <div>{t("docker.noContainers")}</div>
      </div>
    );
  }
  return (
    <table style={tableStyle}>
      <thead>
        <tr>
          <th style={thStyle}>{t("docker.name")}</th>
          <th style={thStyle}>{t("docker.image")}</th>
          <th style={thStyle}>{t("docker.containerId")}</th>
          <th style={thStyle}>{t("docker.state")}</th>
          <th style={thStyle}>{t("docker.status")}</th>
          <th style={thStyle}>{t("docker.ports")}</th>
          <th style={thStyle}>{t("docker.created")}</th>
          <th style={thStyle}>{t("docker.actions")}</th>
        </tr>
      </thead>
      <tbody>
        {containers.map((container) => {
          const busy = busyKey === container.id;
          const isRunning = container.state.toLowerCase() === "running";
          const accent = stateBadgeTone(container.state).border;
          return (
          <tr key={container.id} style={{ boxShadow: `inset 3px 0 0 ${accent}` }}>
            <td style={nameCellStyle} title={container.names}>{container.names}</td>
            <td style={nameCellStyle} title={container.image}>{container.image}</td>
            <td style={monoCellStyle} title={container.id}>{container.id}</td>
            <td style={tdStyle} title={container.state}><StateBadge state={container.state} /></td>
            <td style={tdStyle} title={container.status}>{container.status}</td>
            <td style={tdStyle} title={container.ports}>{container.ports}</td>
            <td style={tdStyle} title={container.createdAt}>{container.createdAt}</td>
            <td style={tdStyle}>
              <span style={actionGroupStyle}>
                <button type="button" style={dockerActionButtonStyle("start")} disabled={busy || isRunning} title={t("docker.start")} onClick={() => onAction(container, "start")}>
                  <Play size={13} fill="currentColor" />
                </button>
                <button type="button" style={dockerActionButtonStyle("restart")} disabled={busy} title={t("docker.restart")} onClick={() => onAction(container, "restart")}>
                  <RotateCw size={13} />
                </button>
                <button type="button" style={dockerActionButtonStyle("stop")} disabled={busy || !isRunning} title={t("docker.stop")} onClick={() => onAction(container, "stop")}>
                  <Square size={12} fill="currentColor" />
                </button>
                <button type="button" style={dockerActionButtonStyle("logs")} disabled={busy} title={t("docker.logs")} onClick={() => onLogs(container)}>
                  <ScrollText size={13} />
                </button>
                <button type="button" style={dockerActionButtonStyle("danger")} disabled={busy} title={t("docker.deleteContainer")} onClick={() => onAction(container, "delete")}>
                  <Trash2 size={13} />
                </button>
              </span>
            </td>
          </tr>
          );
        })}
      </tbody>
    </table>
  );
}

export function DockerServiceView({
  remote,
  sourceLabel,
}: {
  remote?: SshConnection;
  sourceLabel?: string;
}) {
  const { t } = useI18n();
  const remoteKey = dockerRemoteKey(remote);
  const actionInProgressRef = useRef(false);
  const pendingRemoteRefreshRef = useRef<{ key: string; target: SshConnection | null } | null>(null);
  const remoteSnapshotRef = useRef<{ key: string; target: SshConnection | null } | null>(null);
  if (!remoteSnapshotRef.current) {
    remoteSnapshotRef.current = { key: remoteKey, target: remote ?? null };
  } else if (remoteSnapshotRef.current.key !== remoteKey) {
    const nextSnapshot = { key: remoteKey, target: remote ?? null };
    if (actionInProgressRef.current) {
      pendingRemoteRefreshRef.current = nextSnapshot;
    } else {
      remoteSnapshotRef.current = nextSnapshot;
    }
  }
  const currentRemote = remoteSnapshotRef.current.target;
  const [tab, setTab] = useState<DockerTab>("containers");
  const [resources, setResources] = useState<DockerResources | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [logView, setLogView] = useState<{ title: string; content: string } | null>(null);
  const [tagDraft, setTagDraft] = useState<{ image: DockerImageSummary; source: string; target: string } | null>(null);

  const load = useCallback(async (options: { preserveOnError?: boolean; remote?: SshConnection | null } = {}) => {
    setLoading(true);
    setError(null);
    const requestRemote = options.remote ?? remoteSnapshotRef.current?.target ?? currentRemote;
    try {
      const nextResources = await invoke<DockerResources>("list_docker_resources", { remote: requestRemote });
      if (!isSameDockerRemote(requestRemote, remoteSnapshotRef.current?.target)) return;
      setResources(nextResources);
    } catch (err) {
      if (!options.preserveOnError) {
        setResources(null);
        setError(String(err));
      } else if (isIgnorableDockerRefreshError(err)) {
        console.warn("Ignoring Docker post-action refresh error", err);
      } else {
        console.warn("Failed to refresh Docker resources after action", err);
      }
    } finally {
      setLoading(false);
    }
  }, [currentRemote]);

  const applyPendingRemoteRefresh = useCallback(() => {
    const pending = pendingRemoteRefreshRef.current;
    if (!pending) return;
    pendingRemoteRefreshRef.current = null;
    remoteSnapshotRef.current = pending;
    void load({ remote: pending.target });
  }, [load]);

  const runContainerAction = useCallback(
    async (container: DockerContainerSummary, action: ContainerAction) => {
      const actionRemote = currentRemote;
      actionInProgressRef.current = true;
      if (action === "delete") {
        const ok = await confirm(t("docker.confirmDeleteContainer", { name: container.names }), {
          title: t("docker.deleteContainer"),
          kind: "warning",
          okLabel: t("file.delete"),
          cancelLabel: t("common.cancel"),
        });
        if (!ok) {
          actionInProgressRef.current = false;
          applyPendingRemoteRefresh();
          return;
        }
      }
      setBusyKey(container.id);
      setError(null);
      try {
        await invoke("docker_container_action", {
          remote: actionRemote,
          action,
          containerId: container.id,
        });
        await load({ preserveOnError: true, remote: actionRemote });
      } catch (err) {
        setError(String(err));
      } finally {
        setBusyKey(null);
        actionInProgressRef.current = false;
        applyPendingRemoteRefresh();
      }
    },
    [applyPendingRemoteRefresh, currentRemote, load, t],
  );

  const loadContainerLogs = useCallback(
    async (container: DockerContainerSummary) => {
      setBusyKey(container.id);
      setError(null);
      try {
        const content = await invoke<string>("docker_container_logs", {
          remote: currentRemote,
          containerId: container.id,
        });
        setLogView({ title: `${container.names} · ${t("docker.logs")}`, content });
      } catch (err) {
        setError(String(err));
      } finally {
        setBusyKey(null);
      }
    },
    [currentRemote, t],
  );

  const deleteImage = useCallback(
    async (image: DockerImageSummary) => {
      const ref = imageReference(image);
      const actionRemote = currentRemote;
      actionInProgressRef.current = true;
      const ok = await confirm(t("docker.confirmDeleteImage", { name: ref }), {
        title: t("docker.deleteImage"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) {
        actionInProgressRef.current = false;
        applyPendingRemoteRefresh();
        return;
      }
      setBusyKey(ref);
      setError(null);
      try {
        await invoke("docker_delete_image", { remote: actionRemote, image: ref });
        setResources((current) => removeDeletedImage(current, ref));
        await load({ preserveOnError: true, remote: actionRemote });
      } catch (err) {
        setError(String(err));
      } finally {
        setBusyKey(null);
        actionInProgressRef.current = false;
        applyPendingRemoteRefresh();
      }
    },
    [applyPendingRemoteRefresh, currentRemote, load, t],
  );

  const tagImage = useCallback(
    (image: DockerImageSummary) => {
      const source = imageReference(image);
      setTagDraft({ image, source, target: source });
    },
    [],
  );

  const submitTagImage = useCallback(
    async (image: DockerImageSummary) => {
      const actionRemote = currentRemote;
      const source = imageReference(image);
      const target = tagDraft?.target.trim();
      if (!target || target === source) {
        setTagDraft(null);
        return;
      }
      setBusyKey(source);
      setError(null);
      try {
        await invoke("docker_tag_image", { remote: actionRemote, source, target });
        setTagDraft(null);
        await load({ preserveOnError: true, remote: actionRemote });
      } catch (err) {
        setError(String(err));
      } finally {
        setBusyKey(null);
      }
    },
    [currentRemote, load, tagDraft?.target],
  );

  useEffect(() => {
    if (actionInProgressRef.current) return;
    void load();
  }, [load]);

  return (
    <div style={pageStyle}>
      <div style={headerStyle}>
        <div style={titleStyle}>
          <Container size={18} strokeWidth={2} color="var(--accent)" />
          <span>{t("docker.title")}</span>
          {sourceLabel && <span style={{ color: "var(--text-muted)", fontSize: 12, fontWeight: 560 }}>{sourceLabel}</span>}
        </div>
        <div style={tabRowStyle}>
          {logView && (
            <button type="button" style={refreshButtonStyle} onClick={() => setLogView(null)}>
              <ArrowLeft size={13} />
              {t("common.back")}
            </button>
          )}
          <button type="button" style={tabStyle(tab === "containers")} onClick={() => setTab("containers")}>
            <Container size={14} />
            {t("docker.containers")}
            {resources ? ` ${resources.containers.length}` : ""}
          </button>
          <button type="button" style={tabStyle(tab === "images")} onClick={() => setTab("images")}>
            <Box size={14} />
            {t("docker.images")}
            {resources ? ` ${resources.images.length}` : ""}
          </button>
          <button type="button" style={refreshButtonStyle} disabled={loading} onClick={() => void load()}>
            <RefreshCw size={13} className={loading ? "spin" : undefined} />
            {t("common.refresh")}
          </button>
        </div>
      </div>
      <div style={dockerBodyStyle}>
        {logView ? (
          <pre
            style={{
              margin: 0,
              minHeight: "100%",
              padding: 14,
              border: "1px solid var(--border-dim)",
              borderRadius: 8,
              background: "var(--bg-subtle)",
              color: "var(--text-primary)",
              fontFamily: "var(--font-mono)",
              fontSize: 11.5,
              lineHeight: 1.55,
              overflow: "auto",
              whiteSpace: "pre-wrap",
            }}
          >
            {logView.content || t("docker.noLogs")}
          </pre>
        ) : error ? (
          <div style={emptyStyle}>
            <AlertCircle size={28} strokeWidth={1.6} color="var(--danger-fg)" />
            <div style={{ color: "var(--text-secondary)", fontWeight: 650 }}>{t("docker.loadFailed")}</div>
            <div style={{ maxWidth: 720, textAlign: "center", fontSize: 12 }}>{error}</div>
          </div>
        ) : loading && !resources ? (
          <div style={emptyStyle}>{t("common.loading")}</div>
        ) : tab === "containers" ? (
          <ContainerTable
            containers={resources?.containers ?? []}
            busyKey={busyKey}
            onAction={runContainerAction}
            onLogs={loadContainerLogs}
          />
        ) : (
          <ImageTable
            images={resources?.images ?? []}
            busyKey={busyKey}
            onDelete={deleteImage}
            onTag={tagImage}
          />
        )}
      </div>
      {tagDraft && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            zIndex: 3000,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "rgba(0, 0, 0, 0.28)",
          }}
          role="dialog"
          aria-modal="true"
          aria-label={t("docker.tagImage")}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setTagDraft(null);
          }}
        >
          <form
            style={{
              width: 380,
              maxWidth: "calc(100vw - 32px)",
              border: "1px solid var(--border-medium)",
              borderRadius: 10,
              background: "var(--bg-panel)",
              boxShadow: "0 18px 48px rgba(0, 0, 0, 0.24)",
              overflow: "hidden",
            }}
            onSubmit={(event) => {
              event.preventDefault();
              void submitTagImage(tagDraft.image);
            }}
          >
            <div style={{ padding: "14px 16px", borderBottom: "1px solid var(--border-dim)", fontWeight: 750, color: "var(--text-primary)" }}>
              {t("docker.tagImage")}
            </div>
            <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 8 }}>
              <label style={{ fontSize: 12, color: "var(--text-muted)" }}>{t("docker.tagPrompt")}</label>
              <input
                value={tagDraft.target}
                onChange={(event) => setTagDraft((current) => current ? { ...current, target: event.target.value } : current)}
                autoFocus
                autoCapitalize="off"
                autoCorrect="off"
                spellCheck={false}
                style={{
                  height: 34,
                  border: "1px solid var(--border-medium)",
                  borderRadius: 7,
                  background: "var(--bg-input)",
                  color: "var(--text-primary)",
                  padding: "0 10px",
                  fontSize: 12.5,
                  outline: "none",
                }}
              />
            </div>
            <div style={{ padding: "12px 16px", display: "flex", justifyContent: "flex-end", gap: 8, borderTop: "1px solid var(--border-dim)" }}>
              <button type="button" style={refreshButtonStyle} onClick={() => setTagDraft(null)}>
                {t("common.cancel")}
              </button>
              <button type="submit" style={refreshButtonStyle} disabled={!tagDraft.target.trim() || tagDraft.target.trim() === tagDraft.source}>
                {t("docker.tag")}
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}
