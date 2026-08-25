import { useState } from "react";
import { Check, Copy, Edit3, FolderPlus, Plus, Server, Trash2, X } from "lucide-react";
import type { SshConnection } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { SshConnectionContextMenu, type SshConnectionProtocol } from "./SshConnectionContextMenu";
import { useCopyFeedback } from "./useCopyFeedback";
import { SshGroupContextMenu } from "./SshGroupContextMenu";
import { mergeSshGroupNames, normalizeSshGroupName } from "./sshGroups";

interface Props {
  connections: SshConnection[];
  selectedId: string | null;
  onSelect: (connection: SshConnection) => void;
  onCreate: () => void;
  onEdit: (connection: SshConnection) => void;
  onDelete: (connectionId: string) => void;
  onConnect?: (connection: SshConnection, protocol: SshConnectionProtocol) => void;
  /** 已配置的分组名单,含还没有连接的空分组。不传则退回从连接派生。 */
  groupNames?: string[];
  onCreateGroup?: (groupName: string) => void;
  onDeleteGroup?: (groupName: string) => void;
  onRenameGroup?: (groupName: string, nextName: string) => void;
  /** 在指定分组下新建连接,让分组标题上的 + 直接把分组带进对话框。 */
  onCreateInGroup?: (groupName: string) => void;
}

function connectionSubtitle(connection: SshConnection): string {
  const target = `${connection.username}@${connection.host}:${connection.port}`;
  return connection.remotePath ? `${target} · ${connection.remotePath}` : target;
}

export function SshConnectionList({
  connections,
  selectedId,
  onSelect,
  onCreate,
  onEdit,
  onDelete,
  onConnect,
  groupNames,
  onCreateGroup,
  onDeleteGroup,
  onRenameGroup,
  onCreateInGroup,
}: Props) {
  const { t } = useI18n();
  const { copiedId, markCopied } = useCopyFeedback();
  const [contextMenu, setContextMenu] = useState<{
    connection: SshConnection;
    x: number;
    y: number;
  } | null>(null);
  const [draftGroup, setDraftGroup] = useState<string | null>(null);
  const [groupMenu, setGroupMenu] = useState<{ group: string; x: number; y: number } | null>(null);
  // 只有真的写进剪贴板才给成功反馈，否则用户会以为复制成功了。
  const copyPassword = async (connectionId: string, password: string) => {
    try {
      await navigator.clipboard?.writeText(password);
      markCopied(connectionId);
    } catch (error) {
      console.warn("copy ssh password failed", error);
    }
  };
  const defaultGroupLabel = t("ssh.defaultGroup");
  // 命名分组按名单顺序在前(空分组也要占位),没有分组的连接归到末尾的默认分组。
  const namedGroups = mergeSshGroupNames(connections, groupNames ?? []);
  const buckets = new Map<string, SshConnection[]>(namedGroups.map((name) => [name, []]));
  const ungrouped: SshConnection[] = [];
  for (const connection of connections) {
    const group = normalizeSshGroupName(connection.group);
    const bucket = group ? buckets.get(group) : undefined;
    if (bucket) bucket.push(connection);
    else if (group) buckets.set(group, [connection]);
    else ungrouped.push(connection);
  }
  const groupedConnections: Array<[string, SshConnection[], boolean]> = Array.from(
    buckets,
    ([name, items]) => [name, items, true] as [string, SshConnection[], boolean],
  );
  if (ungrouped.length > 0) {
    groupedConnections.push([defaultGroupLabel, ungrouped, false]);
  }

  const submitDraftGroup = () => {
    const normalized = normalizeSshGroupName(draftGroup);
    if (normalized && !namedGroups.includes(normalized)) onCreateGroup?.(normalized);
    setDraftGroup(null);
  };

  return (
    <div style={s.sshListWrap}>
      <div style={s.sshListHeader}>
        <div>
          <div style={s.sshPanelTitle}>{t("ssh.connections")}</div>
          <div style={s.sshPanelSubtitle}>
            {t("ssh.connectionCount", { count: connections.length })}
          </div>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {onCreateGroup && (
            <button
              type="button"
              style={s.sshIconButton}
              title={t("ssh.newGroup")}
              aria-label={t("ssh.newGroup")}
              onClick={() => setDraftGroup("")}
            >
              <FolderPlus size={15} />
            </button>
          )}
          <button
            type="button"
            style={s.sshIconButton}
            title={t("ssh.newConnection")}
            onClick={onCreate}
          >
            <Plus size={15} />
          </button>
        </div>
      </div>

      {draftGroup !== null && (
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <input
            autoFocus
            aria-label={t("ssh.groupName")}
            placeholder={t("ssh.groupName")}
            value={draftGroup}
            onChange={(event) => setDraftGroup(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                submitDraftGroup();
              }
              if (event.key === "Escape") {
                event.preventDefault();
                setDraftGroup(null);
              }
            }}
            style={{ ...s.sshInput, flex: 1, minWidth: 0 }}
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
          />
          <button
            type="button"
            style={s.sshIconButton}
            title={t("common.save")}
            aria-label={t("common.save")}
            onClick={submitDraftGroup}
          >
            <Check size={14} />
          </button>
          <button
            type="button"
            style={s.sshIconButton}
            title={t("common.cancel")}
            aria-label={t("common.cancel")}
            onClick={() => setDraftGroup(null)}
          >
            <X size={14} />
          </button>
        </div>
      )}

      {connections.length === 0 && groupedConnections.length === 0 ? (
        <div style={s.sshEmptyState}>
          <Server size={24} />
          <div style={s.sshEmptyTitle}>{t("ssh.emptyTitle")}</div>
          <button type="button" style={s.sshPrimaryButton} onClick={onCreate}>
            <Plus size={14} />
            {t("ssh.newConnection")}
          </button>
        </div>
      ) : (
        <div style={s.sshConnectionRows}>
          {groupedConnections.map(([group, groupConnections, isNamed]) => (
            <div key={group} style={s.sshConnectionGroup}>
              <div
                style={{ ...s.sshConnectionGroupTitle, display: "flex", alignItems: "center" }}
                onContextMenu={(event) => {
                  // 默认分组不是真实分组,重命名/删除都无从落地。
                  if (!isNamed || (!onRenameGroup && !onDeleteGroup)) return;
                  event.preventDefault();
                  event.stopPropagation();
                  setGroupMenu({ group, x: event.clientX, y: event.clientY });
                }}
              >
                <span
                  style={{
                    flex: 1,
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={group}
                >
                  {group}
                </span>
                {isNamed && onCreateInGroup && (
                  <span
                    role="button"
                    tabIndex={0}
                    style={s.sshRowAction}
                    title={t("ssh.createInGroup", { group })}
                    aria-label={t("ssh.createInGroup", { group })}
                    onClick={() => onCreateInGroup(group)}
                    onKeyDown={(event) => {
                      if (event.key !== "Enter" && event.key !== " ") return;
                      event.preventDefault();
                      onCreateInGroup(group);
                    }}
                  >
                    <Plus size={12} />
                  </span>
                )}
                {isNamed && onDeleteGroup && (
                  <span
                    role="button"
                    tabIndex={0}
                    style={s.sshRowAction}
                    title={t("ssh.deleteGroupNamed", { group })}
                    aria-label={t("ssh.deleteGroupNamed", { group })}
                    onClick={() => onDeleteGroup(group)}
                    onKeyDown={(event) => {
                      if (event.key !== "Enter" && event.key !== " ") return;
                      event.preventDefault();
                      onDeleteGroup(group);
                    }}
                  >
                    <X size={12} />
                  </span>
                )}
              </div>
              {groupConnections.length === 0 && (
                <div style={{ padding: "6px 8px", fontSize: 11, color: "var(--text-hint)" }}>
                  {t("ssh.groupEmpty")}
                </div>
              )}
              {groupConnections.map((connection) => {
                const selected = connection.id === selectedId;
                const password = connection.password?.trim() ?? "";
                const canCopyPassword = password.length > 0;
                return (
                  <button
                    key={connection.id}
                    type="button"
                    style={selected ? s.sshConnectionRowSelected : s.sshConnectionRow}
                    onClick={() => onSelect(connection)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      onSelect(connection);
                      setContextMenu({
                        connection,
                        x: event.clientX,
                        y: event.clientY,
                      });
                    }}
                  >
                    <Server
                      size={16}
                      color={selected ? "var(--control-active-fg)" : "var(--text-hint)"}
                    />
                    <span style={s.sshConnectionText}>
                      <span style={s.sshConnectionName}>{connection.name}</span>
                      <span style={s.sshConnectionMeta}>{connectionSubtitle(connection)}</span>
                    </span>
                    <span style={s.sshConnectionActions}>
                      <span
                        role="button"
                        className="ssh-copy-action"
                        tabIndex={canCopyPassword ? 0 : -1}
                        aria-disabled={!canCopyPassword}
                        aria-label={t("ssh.copyPassword")}
                        data-copied={copiedId === connection.id ? "true" : undefined}
                        style={{
                          ...s.sshRowAction,
                          opacity: canCopyPassword ? 1 : 0.35,
                          cursor: canCopyPassword ? "pointer" : "not-allowed",
                        }}
                        title={canCopyPassword ? t("ssh.copyPassword") : t("ssh.noPasswordHint")}
                        onClick={(event) => {
                          event.stopPropagation();
                          if (!canCopyPassword) return;
                          void copyPassword(connection.id, password);
                        }}
                        onKeyDown={(event) => {
                          if (event.key !== "Enter" && event.key !== " ") return;
                          event.preventDefault();
                          event.stopPropagation();
                          if (!canCopyPassword) return;
                          void copyPassword(connection.id, password);
                        }}
                      >
                        {copiedId === connection.id ? <Check size={13} /> : <Copy size={13} />}
                      </span>
                      <span
                        role="button"
                        tabIndex={0}
                        style={s.sshRowAction}
                        title={t("common.edit")}
                        onClick={(event) => {
                          event.stopPropagation();
                          onEdit(connection);
                        }}
                        onKeyDown={(event) => {
                          if (event.key === "Enter" || event.key === " ") {
                            event.preventDefault();
                            event.stopPropagation();
                            onEdit(connection);
                          }
                        }}
                      >
                        <Edit3 size={13} />
                      </span>
                      <span
                        role="button"
                        tabIndex={0}
                        style={s.sshRowAction}
                        title={t("common.delete")}
                        onClick={(event) => {
                          event.stopPropagation();
                          onDelete(connection.id);
                        }}
                        onKeyDown={(event) => {
                          if (event.key === "Enter" || event.key === " ") {
                            event.preventDefault();
                            event.stopPropagation();
                            onDelete(connection.id);
                          }
                        }}
                      >
                        <Trash2 size={13} />
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      )}
      {groupMenu && (onRenameGroup || onDeleteGroup) && (
        <SshGroupContextMenu
          group={groupMenu.group}
          x={groupMenu.x}
          y={groupMenu.y}
          takenNames={namedGroups.filter((name) => name !== groupMenu.group)}
          onClose={() => setGroupMenu(null)}
          onRename={(from, to) => onRenameGroup?.(from, to)}
          onDelete={(name) => onDeleteGroup?.(name)}
          onCreateConnection={onCreateInGroup}
        />
      )}
      {contextMenu && (
        <SshConnectionContextMenu
          connection={contextMenu.connection}
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
          onConnect={(connection, protocol) => {
            onConnect?.(connection, protocol);
          }}
          onDelete={(connection) => onDelete(connection.id)}
        />
      )}
    </div>
  );
}
