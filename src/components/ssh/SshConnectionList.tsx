import { Copy, Edit3, Plus, Server, Trash2 } from "lucide-react";
import type { SshConnection } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";

interface Props {
  connections: SshConnection[];
  selectedId: string | null;
  onSelect: (connection: SshConnection) => void;
  onCreate: () => void;
  onEdit: (connection: SshConnection) => void;
  onDelete: (connectionId: string) => void;
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
}: Props) {
  const { t } = useI18n();
  const groupedConnections = connections.reduce<Array<[string, SshConnection[]]>>(
    (groups, connection) => {
      const groupName = connection.group?.trim() || t("ssh.defaultGroup");
      const existing = groups.find(([name]) => name === groupName);
      if (existing) {
        existing[1].push(connection);
      } else {
        groups.push([groupName, [connection]]);
      }
      return groups;
    },
    [],
  );

  return (
    <div style={s.sshListWrap}>
      <div style={s.sshListHeader}>
        <div>
          <div style={s.sshPanelTitle}>{t("ssh.connections")}</div>
          <div style={s.sshPanelSubtitle}>{t("ssh.connectionCount", { count: connections.length })}</div>
        </div>
        <button type="button" style={s.sshIconButton} title={t("ssh.newConnection")} onClick={onCreate}>
          <Plus size={15} />
        </button>
      </div>

      {connections.length === 0 ? (
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
          {groupedConnections.map(([group, groupConnections]) => (
            <div key={group} style={s.sshConnectionGroup}>
              <div style={s.sshConnectionGroupTitle}>{group}</div>
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
                  >
                    <Server size={16} color={selected ? "var(--control-active-fg)" : "var(--text-hint)"} />
                    <span style={s.sshConnectionText}>
                      <span style={s.sshConnectionName}>{connection.name}</span>
                      <span style={s.sshConnectionMeta}>{connectionSubtitle(connection)}</span>
                    </span>
                    <span style={s.sshConnectionActions}>
                      <span
                        role="button"
                        tabIndex={canCopyPassword ? 0 : -1}
                        aria-disabled={!canCopyPassword}
                        aria-label={t("ssh.copyPassword")}
                        style={{
                          ...s.sshRowAction,
                          opacity: canCopyPassword ? 1 : 0.35,
                          cursor: canCopyPassword ? "pointer" : "not-allowed",
                        }}
                        title={canCopyPassword ? t("ssh.copyPassword") : t("ssh.noPasswordHint")}
                        onClick={(event) => {
                          event.stopPropagation();
                          if (!canCopyPassword) return;
                          void navigator.clipboard?.writeText(password);
                        }}
                        onKeyDown={(event) => {
                          if (event.key !== "Enter" && event.key !== " ") return;
                          event.preventDefault();
                          event.stopPropagation();
                          if (!canCopyPassword) return;
                          void navigator.clipboard?.writeText(password);
                        }}
                      >
                        <Copy size={13} />
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
    </div>
  );
}
