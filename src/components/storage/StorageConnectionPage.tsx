import { useCallback, useMemo, useState } from "react";
import {
  AlertTriangle,
  Cloud,
  Edit3,
  FolderOpen,
  HardDrive,
  Network,
  Plus,
  Trash2,
  Unplug,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { storageApi } from "../../lib/storageApi";
import { useStorageConnections } from "../../hooks/useStorageConnections";
import type { StorageConnection, StorageProtocol } from "../../types/storage";
import s from "../../styles";
import { StorageConnectionDialog } from "./StorageConnectionDialog";
import { storageConnectionSummary, storageProtocolGroup } from "./storageProtocolForm";

interface Props {
  onClose: () => void;
  /** 在 SFTP 面板里打开该连接。 */
  onOpen?: (connection: StorageConnection) => void;
}

function ProtocolIcon({ protocol }: { protocol: StorageProtocol }) {
  const group = storageProtocolGroup(protocol);
  if (group === "cloudDrive") return <Cloud size={18} strokeWidth={2} />;
  if (group === "fileShare") return <Network size={18} strokeWidth={2} />;
  return <HardDrive size={18} strokeWidth={2} />;
}

export function StorageConnectionPage({ onClose, onOpen }: Props) {
  const { t } = useI18n();
  const {
    connections,
    descriptors,
    descriptorByProtocol,
    secretKeys,
    error,
    refresh,
    saveConnection,
    deleteConnection,
  } = useStorageConnections();
  const [selectedId, setSelectedId] = useState<string>("");
  const [dialogConnection, setDialogConnection] = useState<StorageConnection | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [status, setStatus] = useState<{ kind: "ok" | "error"; message: string } | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const selected = useMemo(
    () => connections.find((item) => item.id === selectedId) ?? connections[0] ?? null,
    [connections, selectedId],
  );
  const groups = useMemo(
    () =>
      Array.from(
        new Set(
          connections
            .map((connection) => connection.group?.trim())
            .filter((group): group is string => Boolean(group)),
        ),
      ),
    [connections],
  );
  const grouped = useMemo(() => {
    const map = new Map<string, StorageConnection[]>();
    for (const connection of connections) {
      const label = connection.group?.trim() || t("ssh.defaultGroup");
      map.set(label, [...(map.get(label) ?? []), connection]);
    }
    return Array.from(map.entries());
  }, [connections, t]);

  const testConnection = useCallback(
    async (connection: StorageConnection) => {
      setBusyId(connection.id);
      setStatus(null);
      try {
        await storageApi.testConnection(connection.id);
        await storageApi.touchConnection(connection.id);
        await refresh();
        setStatus({ kind: "ok", message: t("storage.testPassed", { name: connection.name }) });
      } catch (cause) {
        setStatus({ kind: "error", message: String(cause) });
      } finally {
        setBusyId(null);
      }
    },
    [refresh, t],
  );

  const unmount = useCallback(
    async (connection: StorageConnection) => {
      setBusyId(connection.id);
      setStatus(null);
      try {
        await storageApi.unmountConnection(connection.id);
        setStatus({ kind: "ok", message: t("storage.unmounted", { name: connection.name }) });
      } catch (cause) {
        setStatus({ kind: "error", message: String(cause) });
      } finally {
        setBusyId(null);
      }
    },
    [t],
  );

  return (
    <div style={s.sshProjectPage}>
      <div style={s.sshProjectPageHeader}>
        <div>
          <div style={s.sshProjectPageTitle}>{t("storage.title")}</div>
          <div style={s.sshProjectPageSubtitle}>{t("storage.subtitle")}</div>
        </div>
        <button type="button" style={s.sshSecondaryButton} onClick={onClose}>
          {t("project.backHome")}
        </button>
      </div>

      <div style={s.sshProjectPageBody}>
        {(error || status) && (
          <div
            role="status"
            style={{
              ...s.sshSecretNote,
              marginBottom: 12,
              color: status?.kind === "ok" ? "var(--success)" : "var(--danger)",
            }}
          >
            {status?.message ?? error}
          </div>
        )}

        {connections.length === 0 ? (
          <div style={s.sshEmptyState}>
            <Cloud size={28} />
            <div style={s.sshEmptyTitle}>{t("storage.emptyTitle")}</div>
            <div style={s.sshSecretNote}>{t("storage.emptyHint")}</div>
          </div>
        ) : (
          <div style={s.sshProjectConnectionPicker}>
            {grouped.map(([group, items]) => (
              <section key={group} style={s.sshProjectGroupSection}>
                <div style={s.sshProjectGroupTitle}>{group}</div>
                <div style={s.sshProjectCardGrid}>
                  {items.map((connection) => {
                    const descriptor = descriptorByProtocol.get(connection.protocol);
                    const isSelected = connection.id === selected?.id;
                    const busy = busyId === connection.id;
                    return (
                      <div
                        key={connection.id}
                        style={isSelected ? s.sshProjectCardSelected : s.sshProjectCard}
                      >
                        <button
                          type="button"
                          style={s.sshProjectCardSelect}
                          onClick={() => setSelectedId(connection.id)}
                          onDoubleClick={() => onOpen?.(connection)}
                        >
                          <span style={s.sshProjectCardIcon}>
                            <ProtocolIcon protocol={connection.protocol} />
                          </span>
                          <span style={s.sshProjectCardText}>
                            <span style={s.sshProjectCardName}>{connection.name}</span>
                            <span style={s.sshProjectCardMeta}>
                              {t(`storage.protocol.${connection.protocol}`)}
                            </span>
                            <span style={s.sshProjectCardMeta}>
                              {storageConnectionSummary(connection)}
                            </span>
                            {descriptor?.deprecated && (
                              <span
                                style={{ ...s.sshProjectCardMeta, color: "var(--warning)" }}
                                title={t(`storage.deprecated.${connection.protocol}`)}
                              >
                                <AlertTriangle size={11} /> {t("storage.deprecatedBadge")}
                              </span>
                            )}
                          </span>
                        </button>
                        <button
                          type="button"
                          style={s.sshProjectCardEdit}
                          title={t("common.edit")}
                          aria-label={t("common.edit")}
                          onClick={() => {
                            setDialogConnection(connection);
                            setDialogOpen(true);
                          }}
                        >
                          <Edit3 size={14} />
                        </button>
                        {descriptor?.systemMount && (
                          <button
                            type="button"
                            style={s.sshProjectCardEdit}
                            title={t("storage.unmount")}
                            aria-label={t("storage.unmount")}
                            disabled={busy}
                            onClick={() => void unmount(connection)}
                          >
                            <Unplug size={14} />
                          </button>
                        )}
                        <button
                          type="button"
                          style={s.sshProjectCardEdit}
                          title={t("common.delete")}
                          aria-label={t("common.delete")}
                          onClick={() => {
                            void deleteConnection(connection.id).catch((cause) =>
                              setStatus({ kind: "error", message: String(cause) }),
                            );
                          }}
                        >
                          <Trash2 size={14} />
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
            setDialogConnection(null);
            setDialogOpen(true);
          }}
        >
          <Plus size={14} />
          {t("storage.newConnection")}
        </button>
        <button
          type="button"
          style={selected ? s.sshSecondaryButton : s.sshPrimaryButtonDisabled}
          disabled={!selected || busyId !== null}
          onClick={() => selected && void testConnection(selected)}
        >
          {t("storage.test")}
        </button>
        <div style={{ flex: 1 }} />
        <button
          type="button"
          style={selected ? s.sshPrimaryButton : s.sshPrimaryButtonDisabled}
          disabled={!selected}
          onClick={() => selected && onOpen?.(selected)}
        >
          <FolderOpen size={14} />
          {t("storage.browse")}
        </button>
      </div>

      {dialogOpen && (
        <StorageConnectionDialog
          connection={dialogConnection}
          descriptors={descriptors}
          groups={groups}
          savedSecretKeys={dialogConnection ? (secretKeys[dialogConnection.id] ?? []) : []}
          onClose={() => {
            setDialogOpen(false);
            setDialogConnection(null);
          }}
          onSave={async (connection) => {
            await saveConnection(connection);
            setSelectedId(connection.id);
            setDialogOpen(false);
            setDialogConnection(null);
          }}
        />
      )}
    </div>
  );
}
