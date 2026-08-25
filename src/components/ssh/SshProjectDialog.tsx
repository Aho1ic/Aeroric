import { useMemo, useState } from "react";
import { Check, Copy, Edit3, FolderOpen, Plus, Server, Users } from "lucide-react";
import type { SshConnection } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { SshConnectionDialog } from "./SshConnectionDialog";
import { SshConnectionContextMenu, type SshConnectionProtocol } from "./SshConnectionContextMenu";
import { sshProjectInputForConnection, type SshProjectInput } from "./sshProject";
import { useCopyFeedback } from "./useCopyFeedback";
import { SshGroupContextMenu } from "./SshGroupContextMenu";
import { useSshGroups } from "./useSshGroups";

export {
  deriveRemoteProjectName,
  sshProjectInputForConnection,
  type SshProjectInput,
} from "./sshProject";

interface Props {
  connections: SshConnection[];
  groups?: string[];
  onConnectionsChange: (connections: SshConnection[]) => void;
  onDeleteConnection?: (connectionId: string) => void | Promise<void>;
  onClose: () => void;
  onOpen: (input: SshProjectInput) => void;
  onOpenSftp?: (connection: SshConnection) => void;
}

function connectionTarget(connection: SshConnection): string {
  return `${connection.username}@${connection.host}:${connection.port}`;
}

async function copyConnectionPassword(connection: SshConnection) {
  const password = connection.password ?? "";
  if (!password || !navigator.clipboard?.writeText) return;
  await navigator.clipboard.writeText(password);
}

/**
 * 分组 → 连接。命名分组按名单顺序在前(空分组也占位),未分组的落到末尾。
 * 第三项标记是否为真实分组:默认分组只是未分组连接的容器,不能改名或删除。
 */
function groupConnections(
  connections: SshConnection[],
  namedGroups: string[],
  fallbackGroup: string,
): Array<[string, SshConnection[], boolean]> {
  const buckets = new Map<string, SshConnection[]>(namedGroups.map((name) => [name, []]));
  const ungrouped: SshConnection[] = [];
  for (const connection of connections) {
    const group = connection.group?.trim();
    const bucket = group ? buckets.get(group) : undefined;
    if (bucket) bucket.push(connection);
    else if (group) buckets.set(group, [connection]);
    else ungrouped.push(connection);
  }
  const grouped: Array<[string, SshConnection[], boolean]> = Array.from(
    buckets,
    ([name, items]) => [name, items, true],
  );
  if (ungrouped.length > 0) grouped.push([fallbackGroup, ungrouped, false]);
  return grouped;
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
  onDeleteConnection,
  onClose,
  onOpen,
  onOpenSftp,
}: Props) {
  const { t } = useI18n();
  const firstOpenable = connections.find((connection) => connection.remotePath?.trim());
  const [selectedId, setSelectedId] = useState(firstOpenable?.id ?? connections[0]?.id ?? "");
  const [editingConnection, setEditingConnection] = useState<SshConnection | null>(null);
  const [creatingConnection, setCreatingConnection] = useState(false);
  const [groupDialogOpen, setGroupDialogOpen] = useState(false);
  const [initialGroup, setInitialGroup] = useState("");
  const { copiedId: copiedConnectionId, markCopied } = useCopyFeedback();
  const [contextMenu, setContextMenu] = useState<{
    connection: SshConnection;
    x: number;
    y: number;
  } | null>(null);
  const [groupMenu, setGroupMenu] = useState<{ group: string; x: number; y: number } | null>(null);
  // 与右侧栏 SSH 列表同源(store 订阅):那里建的空分组在这里也要看得到、删得掉。
  const {
    groups: storedGroups,
    createGroup,
    deleteGroup,
    renameGroup,
  } = useSshGroups(connections, onConnectionsChange);
  const selectedConnection = useMemo(
    () => connections.find((connection) => connection.id === selectedId) ?? connections[0] ?? null,
    [connections, selectedId],
  );
  const groupedConnections = useMemo(
    () => groupConnections(connections, storedGroups, t("ssh.defaultGroup")),
    [connections, storedGroups, t],
  );
  const knownGroups = useMemo(
    () =>
      Array.from(
        new Set(
          [
            ...groups,
            ...storedGroups,
            ...connections
              .map((connection) => connection.group?.trim())
              .filter((group): group is string => Boolean(group)),
            initialGroup,
          ].filter(Boolean),
        ),
      ),
    [connections, groups, storedGroups, initialGroup],
  );
  const selectedRemotePath = selectedConnection?.remotePath?.trim() ?? "";
  const canOpen = Boolean(selectedConnection && selectedRemotePath);

  function saveConnection(connection: SshConnection) {
    const exists = connections.some((item) => item.id === connection.id);
    const nextConnections = exists
      ? connections.map((item) => (item.id === connection.id ? connection : item))
      : [connection, ...connections];
    onConnectionsChange(nextConnections);
    // 对话框里手输的新分组也要进名单,否则移走最后一条连接它就消失了。
    createGroup(connection.group ?? "");
    setSelectedId(connection.id);
    setEditingConnection(null);
    setCreatingConnection(false);
    setInitialGroup("");
  }

  function openConnection(connection: SshConnection) {
    const input = sshProjectInputForConnection(connection);
    if (input) onOpen(input);
  }

  function deleteConnection(connectionId: string) {
    const nextConnections = connections.filter((connection) => connection.id !== connectionId);
    if (onDeleteConnection) {
      void onDeleteConnection(connectionId);
    } else {
      onConnectionsChange(nextConnections);
    }
    if (selectedId === connectionId) {
      setSelectedId(nextConnections[0]?.id ?? "");
    }
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
        {groupedConnections.length === 0 ? (
          <div style={s.sshEmptyState}>
            <Server size={28} />
            <div style={s.sshEmptyTitle}>{t("ssh.emptyTitle")}</div>
            <div style={s.sshSecretNote}>{t("sshProject.noConnections")}</div>
          </div>
        ) : (
          <div style={s.sshProjectConnectionPicker}>
            {groupedConnections.map(([group, grouped, isNamed]) => (
              <section key={group} style={s.sshProjectGroupSection}>
                <div
                  style={s.sshProjectGroupTitle}
                  onContextMenu={(event) => {
                    // 默认分组不是真实分组,重命名/删除都无从落地。
                    if (!isNamed) return;
                    event.preventDefault();
                    event.stopPropagation();
                    setGroupMenu({ group, x: event.clientX, y: event.clientY });
                  }}
                >
                  {group}
                </div>
                {grouped.length === 0 && <div style={s.sshSecretNote}>{t("ssh.groupEmpty")}</div>}
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
                        onContextMenu={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          setSelectedId(connection.id);
                          setContextMenu({
                            connection,
                            x: event.clientX,
                            y: event.clientY,
                          });
                        }}
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
                          className="ssh-copy-action"
                          title={canCopyPassword ? t("ssh.copyPassword") : t("ssh.noPasswordHint")}
                          aria-label={t("ssh.copyPassword")}
                          style={{
                            ...s.sshProjectCardEdit,
                            opacity: canCopyPassword ? 1 : 0.35,
                            cursor: canCopyPassword ? "pointer" : "not-allowed",
                          }}
                          data-copied={copied ? "true" : undefined}
                          disabled={!canCopyPassword}
                          onClick={() => {
                            if (!canCopyPassword) return;
                            void copyConnectionPassword(connection).then(() => {
                              markCopied(connection.id);
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
            // 先把分组登记进名单,这样即便用户放弃新建连接,空分组也留得住。
            createGroup(group);
            setInitialGroup(group);
            setCreatingConnection(true);
            setGroupDialogOpen(false);
          }}
        />
      )}
      {groupMenu && (
        <SshGroupContextMenu
          group={groupMenu.group}
          x={groupMenu.x}
          y={groupMenu.y}
          takenNames={storedGroups.filter((name) => name !== groupMenu.group)}
          onClose={() => setGroupMenu(null)}
          onRename={renameGroup}
          onDelete={deleteGroup}
          onCreateConnection={(group) => {
            setInitialGroup(group);
            setCreatingConnection(true);
          }}
        />
      )}
      {contextMenu && (
        <SshConnectionContextMenu
          connection={contextMenu.connection}
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
          onConnect={(connection, protocol: SshConnectionProtocol) => {
            if (protocol === "sftp") {
              onOpenSftp?.(connection);
              return;
            }
            openConnection(connection);
          }}
          onDelete={(connection) => deleteConnection(connection.id)}
        />
      )}
    </div>
  );
}

export const SshProjectDialog = SshProjectPage;
