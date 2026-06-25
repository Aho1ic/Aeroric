import { useMemo, useState } from "react";
import { Check, Copy, Edit3, FolderOpen, Plus, Server, Users } from "lucide-react";
import type { SshConnection } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { SshConnectionDialog } from "./SshConnectionDialog";

export interface SshProjectInput {
  connectionId: string;
  remotePath: string;
  name: string;
}

interface Props {
  connections: SshConnection[];
  groups?: string[];
  onConnectionsChange: (connections: SshConnection[]) => void;
  onClose: () => void;
  onOpen: (input: SshProjectInput) => void;
}

export function deriveRemoteProjectName(remotePath: string, fallback: string): string {
  const trimmed = remotePath.trim().replace(/\/+$/, "");
  if (!trimmed) return fallback.trim() || "remote";
  const parts = trimmed.split("/");
  return parts[parts.length - 1] || fallback.trim() || "remote";
}

export function sshProjectInputForConnection(connection: SshConnection): SshProjectInput | null {
  const remotePath = connection.remotePath?.trim() ?? "";
  if (!remotePath) return null;
  return {
    connectionId: connection.id,
    remotePath,
    name: connection.name.trim() || deriveRemoteProjectName(remotePath, connection.name),
  };
}

function connectionTarget(connection: SshConnection): string {
  return `${connection.username}@${connection.host}:${connection.port}`;
}

async function copyConnectionPassword(connection: SshConnection) {
  const password = connection.password ?? "";
  if (!password || !navigator.clipboard?.writeText) return;
  await navigator.clipboard.writeText(password);
}

function groupConnections(connections: SshConnection[], fallbackGroup: string) {
  const map = new Map<string, SshConnection[]>();
  for (const connection of connections) {
    const group = connection.group?.trim() || fallbackGroup;
    map.set(group, [...(map.get(group) ?? []), connection]);
  }
  return Array.from(map.entries());
}

function GroupNameDialog({
  onClose,
  onSubmit,
}: {
  onClose: () => void;
  onSubmit: (name: string) => void;
}) {
  const { t } = useI18n();
  const [name, setName] = useState("");
  const normalized = name.trim();
  return (
    <div
      style={s.sshDialogOverlay}
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <form
        style={s.sshDialog}
        onSubmit={(event) => {
          event.preventDefault();
          if (!normalized) return;
          onSubmit(normalized);
        }}
      >
        <div style={s.sshDialogHeader}>
          <div style={s.sshDialogTitle}>{t("sshProject.newGroup")}</div>
        </div>
        <div style={s.sshDialogBody}>
          <label style={s.sshField}>
            <span style={s.sshLabel}>{t("ssh.field.group")}</span>
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t("ssh.defaultGroup")}
              style={s.sshInput}
              autoFocus
            />
          </label>
          <div style={s.sshSecretNote}>{t("sshProject.newGroupHint")}</div>
        </div>
        <div style={s.sshDialogFooter}>
          <button type="button" style={s.sshSecondaryButton} onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button
            type="submit"
            style={normalized ? s.sshPrimaryButton : s.sshPrimaryButtonDisabled}
            disabled={!normalized}
          >
            <Plus size={14} />
            {t("sshProject.createGroup")}
          </button>
        </div>
      </form>
    </div>
  );
}

export function SshProjectPage({
  connections,
  groups = [],
  onConnectionsChange,
  onClose,
  onOpen,
}: Props) {
  const { t } = useI18n();
  const firstOpenable = connections.find((connection) => connection.remotePath?.trim());
  const [selectedId, setSelectedId] = useState(firstOpenable?.id ?? connections[0]?.id ?? "");
  const [editingConnection, setEditingConnection] = useState<SshConnection | null>(null);
  const [creatingConnection, setCreatingConnection] = useState(false);
  const [groupDialogOpen, setGroupDialogOpen] = useState(false);
  const [initialGroup, setInitialGroup] = useState("");
  const [copiedConnectionId, setCopiedConnectionId] = useState<string | null>(null);
  const selectedConnection = useMemo(
    () => connections.find((connection) => connection.id === selectedId) ?? connections[0] ?? null,
    [connections, selectedId],
  );
  const groupedConnections = useMemo(
    () => groupConnections(connections, t("ssh.defaultGroup")),
    [connections, t],
  );
  const knownGroups = useMemo(
    () =>
      Array.from(
        new Set(
          [
            ...groups,
            ...connections
              .map((connection) => connection.group?.trim())
              .filter((group): group is string => Boolean(group)),
            initialGroup,
          ].filter(Boolean),
        ),
      ),
    [connections, groups, initialGroup],
  );
  const selectedRemotePath = selectedConnection?.remotePath?.trim() ?? "";
  const canOpen = Boolean(selectedConnection && selectedRemotePath);

  function saveConnection(connection: SshConnection) {
    const exists = connections.some((item) => item.id === connection.id);
    const nextConnections = exists
      ? connections.map((item) => (item.id === connection.id ? connection : item))
      : [connection, ...connections];
    onConnectionsChange(nextConnections);
    setSelectedId(connection.id);
    setEditingConnection(null);
    setCreatingConnection(false);
    setInitialGroup("");
  }

  function openConnection(connection: SshConnection) {
    const input = sshProjectInputForConnection(connection);
    if (input) onOpen(input);
  }

  function handleOpen() {
    if (!selectedConnection || !selectedRemotePath) return;
    openConnection(selectedConnection);
  }

  return (
    <div style={s.sshProjectPage}>
      <div style={s.sshProjectPageHeader}>
        <div>
          <div style={s.sshProjectPageTitle}>{t("sshProject.title")}</div>
          <div style={s.sshProjectPageSubtitle}>{t("sshProject.subtitle")}</div>
        </div>
        <button type="button" style={s.sshSecondaryButton} onClick={onClose}>
          {t("project.backHome")}
        </button>
      </div>

      <div style={s.sshProjectPageBody}>
        {connections.length === 0 ? (
          <div style={s.sshEmptyState}>
            <Server size={28} />
            <div style={s.sshEmptyTitle}>{t("ssh.emptyTitle")}</div>
            <div style={s.sshSecretNote}>{t("sshProject.noConnections")}</div>
          </div>
        ) : (
          <div style={s.sshProjectConnectionPicker}>
            {groupedConnections.map(([group, grouped]) => (
              <section key={group} style={s.sshProjectGroupSection}>
                <div style={s.sshProjectGroupTitle}>{group}</div>
                <div style={s.sshProjectCardGrid}>
                  {grouped.map((connection) => {
                    const selected = connection.id === selectedConnection?.id;
                    const hasRemotePath = Boolean(connection.remotePath?.trim());
                    const canCopyPassword = Boolean(connection.password?.trim());
                    const copied = copiedConnectionId === connection.id;
                    return (
                      <div
                        key={connection.id}
                        style={selected ? s.sshProjectCardSelected : s.sshProjectCard}
                      >
                        <button
                          type="button"
                          style={s.sshProjectCardSelect}
                          onClick={() => setSelectedId(connection.id)}
                          onDoubleClick={(event) => {
                            event.preventDefault();
                            openConnection(connection);
                          }}
                        >
                          <span style={s.sshProjectCardIcon}>
                            <Server size={18} strokeWidth={2} />
                          </span>
                          <span style={s.sshProjectCardText}>
                            <span style={s.sshProjectCardName}>{connection.name}</span>
                            <span style={s.sshProjectCardMeta}>{connectionTarget(connection)}</span>
                            <span
                              style={{
                                ...s.sshProjectCardMeta,
                                color: hasRemotePath ? "var(--text-muted)" : "var(--warning)",
                              }}
                            >
                              {hasRemotePath
                                ? connection.remotePath
                                : t("sshProject.remotePathMissing")}
                            </span>
                          </span>
                        </button>
                        <button
                          type="button"
                          title={t("common.edit")}
                          aria-label={t("common.edit")}
                          style={s.sshProjectCardEdit}
                          onClick={() => setEditingConnection(connection)}
                        >
                          <Edit3 size={14} />
                        </button>
                        <button
                          type="button"
                          title={canCopyPassword ? t("ssh.copyPassword") : t("ssh.noPasswordHint")}
                          aria-label={t("ssh.copyPassword")}
                          style={{
                            ...s.sshProjectCardEdit,
                            opacity: canCopyPassword ? 1 : 0.35,
                            cursor: canCopyPassword ? "pointer" : "not-allowed",
                            transform: copied ? "scale(1.12)" : "scale(1)",
                            color: copied ? "var(--success)" : s.sshProjectCardEdit.color,
                            border: copied
                              ? "1px solid var(--success)"
                              : s.sshProjectCardEdit.border,
                            transition:
                              "transform 0.16s ease, color 0.16s ease, border-color 0.16s ease",
                          }}
                          data-copied={copied ? "true" : undefined}
                          disabled={!canCopyPassword}
                          onClick={() => {
                            if (!canCopyPassword) return;
                            void copyConnectionPassword(connection).then(() => {
                              setCopiedConnectionId(connection.id);
                              window.setTimeout(() => {
                                setCopiedConnectionId((current) =>
                                  current === connection.id ? null : current,
                                );
                              }, 900);
                            });
                          }}
                        >
                          {copied ? <Check size={14} /> : <Copy size={14} />}
                        </button>
                      </div>
                    );
                  })}
                </div>
              </section>
            ))}
          </div>
        )}
      </div>

      <div style={s.sshProjectPageFooter}>
        <button
          type="button"
          style={s.sshSecondaryButton}
          onClick={() => {
            setInitialGroup("");
            setCreatingConnection(true);
          }}
        >
          <Plus size={14} />
          {t("ssh.newConnection")}
        </button>
        <button type="button" style={s.sshSecondaryButton} onClick={() => setGroupDialogOpen(true)}>
          <Users size={14} />
          {t("sshProject.newGroup")}
        </button>
        <div style={{ flex: 1 }} />
        <button
          type="button"
          style={canOpen ? s.sshPrimaryButton : s.sshPrimaryButtonDisabled}
          disabled={!canOpen}
          onClick={handleOpen}
          title={canOpen ? t("sshProject.open") : t("sshProject.remotePathMissing")}
        >
          <FolderOpen size={14} />
          {t("sshProject.open")}
        </button>
      </div>

      {(creatingConnection || editingConnection) && (
        <SshConnectionDialog
          connection={editingConnection}
          groups={knownGroups}
          initialGroup={initialGroup}
          onClose={() => {
            setCreatingConnection(false);
            setEditingConnection(null);
            setInitialGroup("");
          }}
          onSave={saveConnection}
        />
      )}
      {groupDialogOpen && (
        <GroupNameDialog
          onClose={() => setGroupDialogOpen(false)}
          onSubmit={(group) => {
            setInitialGroup(group);
            setCreatingConnection(true);
            setGroupDialogOpen(false);
          }}
        />
      )}
    </div>
  );
}

export const SshProjectDialog = SshProjectPage;
