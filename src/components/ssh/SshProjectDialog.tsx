import { useMemo, useState } from "react";
import * as Select from "@radix-ui/react-select";
import { Check, ChevronDown, FolderOpen, Plus, X } from "lucide-react";
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
  onConnectionsChange: (connections: SshConnection[]) => void;
  onClose: () => void;
  onOpen: (input: SshProjectInput) => void;
}

function deriveRemoteProjectName(remotePath: string): string {
  const trimmed = remotePath.trim().replace(/\/+$/, "");
  if (!trimmed) return "";
  const parts = trimmed.split("/");
  return parts[parts.length - 1] || trimmed;
}

export function SshProjectDialog({ connections, onConnectionsChange, onClose, onOpen }: Props) {
  const { t } = useI18n();
  const firstConnection = connections[0];
  const [connectionDialogOpen, setConnectionDialogOpen] = useState(false);
  const [connectionId, setConnectionId] = useState(firstConnection?.id ?? "");
  const selectedConnection = useMemo(
    () => connections.find((connection) => connection.id === connectionId) ?? firstConnection,
    [connectionId, connections, firstConnection],
  );
  const [remotePath, setRemotePath] = useState(selectedConnection?.remotePath ?? "");
  const [name, setName] = useState(() => deriveRemoteProjectName(firstConnection?.remotePath ?? ""));
  const normalizedRemotePath = remotePath.trim();
  const normalizedName = name.trim() || deriveRemoteProjectName(normalizedRemotePath);
  const canSubmit = Boolean(selectedConnection && normalizedRemotePath && normalizedName);

  function handleConnectionChange(nextId: string) {
    const nextConnection = connections.find((connection) => connection.id === nextId);
    setConnectionId(nextId);
    if (nextConnection?.remotePath) {
      setRemotePath(nextConnection.remotePath);
      setName((current) => current || deriveRemoteProjectName(nextConnection.remotePath ?? ""));
    }
  }

  function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    if (!canSubmit || !selectedConnection) return;
    onOpen({
      connectionId: selectedConnection.id,
      remotePath: normalizedRemotePath,
      name: normalizedName,
    });
  }

  return (
    <div style={s.sshDialogOverlay} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form style={s.sshDialog} onSubmit={handleSubmit}>
        <div style={s.sshDialogHeader}>
          <div style={s.sshDialogTitle}>{t("sshProject.title")}</div>
          <button type="button" style={s.modalCloseBtn} onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div style={s.sshDialogBody}>
          {connections.length === 0 ? (
            <div style={s.sshSecretNote}>{t("sshProject.noConnections")}</div>
          ) : (
            <>
              <label style={s.sshField}>
                <span style={s.sshLabel}>{t("sshProject.connection")}</span>
                <Select.Root value={connectionId} onValueChange={handleConnectionChange}>
                  <Select.Trigger aria-label={t("sshProject.connection")} style={s.settingsSelectTrigger}>
                    <Select.Value>
                      {selectedConnection
                        ? `${selectedConnection.name} (${selectedConnection.username}@${selectedConnection.host})`
                        : t("sshProject.connection")}
                    </Select.Value>
                    <Select.Icon>
                      <ChevronDown size={13} strokeWidth={2.2} color="var(--text-hint)" />
                    </Select.Icon>
                  </Select.Trigger>
                  <Select.Portal>
                    <Select.Content position="popper" sideOffset={4} style={s.settingsSelectContent}>
                      <Select.Viewport style={s.settingsSelectViewport}>
                        {connections.map((connection) => {
                          const selected = connection.id === selectedConnection?.id;
                          return (
                            <Select.Item
                              key={connection.id}
                              value={connection.id}
                              className="radix-select-item"
                              style={selected ? s.settingsSelectOptionSelected : s.settingsSelectOption}
                            >
                              <Select.ItemText>
                                {connection.name} ({connection.username}@{connection.host})
                              </Select.ItemText>
                              <Select.ItemIndicator style={s.settingsSelectIndicator}>
                                <Check size={13} style={s.settingsSelectCheck} />
                              </Select.ItemIndicator>
                            </Select.Item>
                          );
                        })}
                      </Select.Viewport>
                    </Select.Content>
                  </Select.Portal>
                </Select.Root>
              </label>
              <label style={s.sshField}>
                <span style={s.sshLabel}>{t("ssh.field.remotePath")}</span>
                <input
                  value={remotePath}
                  onChange={(event) => {
                    const nextPath = event.target.value;
                    setRemotePath(nextPath);
                    setName((current) => current || deriveRemoteProjectName(nextPath));
                  }}
                  placeholder="/srv/app"
                  style={s.sshInput}
                  autoFocus
                />
              </label>
              <label style={s.sshField}>
                <span style={s.sshLabel}>{t("sshProject.projectName")}</span>
                <input
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  placeholder={deriveRemoteProjectName(remotePath) || "app"}
                  style={s.sshInput}
                />
              </label>
            </>
          )}
        </div>

        <div style={s.sshDialogFooter}>
          <button
            type="button"
            style={s.sshSecondaryButton}
            onClick={() => setConnectionDialogOpen(true)}
          >
            <Plus size={14} />
            {t("ssh.newConnection")}
          </button>
          <div style={{ flex: 1 }} />
          <button type="button" style={s.sshSecondaryButton} onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button
            type="submit"
            style={canSubmit ? s.sshPrimaryButton : s.sshPrimaryButtonDisabled}
            disabled={!canSubmit}
          >
            <FolderOpen size={14} />
            {t("sshProject.open")}
          </button>
        </div>
      </form>
      {connectionDialogOpen && (
        <SshConnectionDialog
          onClose={() => setConnectionDialogOpen(false)}
          onSave={(connection) => {
            const nextConnections = [connection, ...connections];
            onConnectionsChange(nextConnections);
            setConnectionId(connection.id);
            if (connection.remotePath) {
              setRemotePath(connection.remotePath);
              setName((current) => current || deriveRemoteProjectName(connection.remotePath ?? ""));
            }
            setConnectionDialogOpen(false);
          }}
        />
      )}
    </div>
  );
}
