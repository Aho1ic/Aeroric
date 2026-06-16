import { useMemo, useState } from "react";
import { Columns2, Edit3, Maximize2, Plus, Server } from "lucide-react";
import type { FontFamily, SshConnection, TerminalFontSize, ThemeVariant } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { SshConnectionDialog } from "./SshConnectionDialog";
import { SshTerminalPanel } from "./SshTerminalPanel";

type SshWorkspaceLayout = "split" | "full";

function connectionTarget(connection: SshConnection): string {
  return `${connection.username}@${connection.host}:${connection.port}`;
}

function groupConnections(connections: SshConnection[], fallbackGroup: string) {
  const map = new Map<string, SshConnection[]>();
  for (const connection of connections) {
    const group = connection.group?.trim() || fallbackGroup;
    map.set(group, [...(map.get(group) ?? []), connection]);
  }
  return Array.from(map.entries());
}

function connectionGroups(connections: SshConnection[]) {
  return Array.from(
    new Set(
      connections
        .map((connection) => connection.group?.trim())
        .filter((group): group is string => Boolean(group)),
    ),
  );
}

function SshCardPicker({
  connections,
  selectedId,
  onOpen,
  onEdit,
}: {
  connections: SshConnection[];
  selectedId: string | null;
  onOpen: (connection: SshConnection) => void;
  onEdit: (connection: SshConnection) => void;
}) {
  const { t } = useI18n();
  const grouped = useMemo(
    () => groupConnections(connections, t("ssh.defaultGroup")),
    [connections, t],
  );

  if (connections.length === 0) {
    return (
      <div style={s.sshEmptyState}>
        <div style={s.sshEmptyTitle}>{t("ssh.emptyTitle")}</div>
        <div style={s.sshSecretNote}>{t("sshProject.noConnections")}</div>
      </div>
    );
  }

  return (
    <div style={s.sshProjectConnectionPicker}>
      {grouped.map(([group, items]) => (
        <section key={group} style={s.sshProjectGroupSection}>
          <div style={s.sshProjectGroupTitle}>{group}</div>
          <div style={s.sshProjectCardGrid}>
            {items.map((connection) => {
              const selected = selectedId === connection.id;
              return (
                <div key={connection.id} style={selected ? s.sshProjectCardSelected : s.sshProjectCard}>
                  <button
                    type="button"
                    style={s.sshProjectCardSelect}
                    onClick={() => onOpen(connection)}
                    onDoubleClick={() => onOpen(connection)}
                  >
                    <span style={s.sshProjectCardIcon}>
                      <Server size={17} />
                    </span>
                    <span style={s.sshProjectCardText}>
                      <span style={s.sshProjectCardName}>{connection.name}</span>
                      <span style={s.sshProjectCardMeta}>{connectionTarget(connection)}</span>
                      {connection.remotePath && (
                        <span style={s.sshProjectCardMeta}>{connection.remotePath}</span>
                      )}
                    </span>
                  </button>
                  <button
                    type="button"
                    style={s.sshProjectCardEdit}
                    title={t("common.edit")}
                    onClick={(event) => {
                      event.stopPropagation();
                      onEdit(connection);
                    }}
                  >
                    <Edit3 size={14} />
                  </button>
                </div>
              );
            })}
          </div>
        </section>
      ))}
    </div>
  );
}

function SshWorkspaceHeader({
  layout,
  localProject,
  showingCards,
  onToggleLayout,
  onShowCards,
  onNewConnection,
}: {
  layout: SshWorkspaceLayout;
  localProject: boolean;
  showingCards: boolean;
  onToggleLayout: () => void;
  onShowCards: () => void;
  onNewConnection: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="ssh-workspace-header">
      <div className="ssh-workspace-title">
        <Server size={15} />
        {t("ssh.title")}
      </div>
      <div className="ssh-workspace-actions">
        <button type="button" className="ssh-workspace-icon-btn" title={t("ssh.newConnection")} onClick={onNewConnection}>
          <Plus size={15} />
        </button>
        <button
          type="button"
          className={`ssh-workspace-icon-btn${showingCards ? " active" : ""}`}
          title={t("ssh.showConnections")}
          onClick={onShowCards}
        >
          <Server size={15} />
        </button>
        {localProject && (
          <button
            type="button"
            className="ssh-workspace-icon-btn"
            title={layout === "full" ? t("ssh.splitView") : t("ssh.fullView")}
            onClick={onToggleLayout}
          >
            {layout === "full" ? <Columns2 size={15} /> : <Maximize2 size={15} />}
          </button>
        )}
      </div>
    </div>
  );
}

export function SshWorkspace({
  connections,
  onConnectionsChange,
  active,
  themeVariant,
  terminalFontSize,
  monoFontFamily,
  remoteConnection,
}: {
  connections: SshConnection[];
  onConnectionsChange: (connections: SshConnection[]) => void;
  active: boolean;
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  monoFontFamily: FontFamily;
  remoteConnection?: SshConnection;
}) {
  const localProject = !remoteConnection;
  const [layout, setLayout] = useState<SshWorkspaceLayout>("full");
  const [selectedConnectionId, setSelectedConnectionId] = useState<string | null>(null);
  const [showCards, setShowCards] = useState(true);
  const [editingConnection, setEditingConnection] = useState<SshConnection | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const groups = useMemo(() => connectionGroups(connections), [connections]);
  const selectedConnection = selectedConnectionId
    ? connections.find((connection) => connection.id === selectedConnectionId)
    : null;
  const rightShowsCards = showCards || !selectedConnection;

  const saveConnection = (connection: SshConnection) => {
    const exists = connections.some((item) => item.id === connection.id);
    const next = exists
      ? connections.map((item) => (item.id === connection.id ? connection : item))
      : [connection, ...connections];
    onConnectionsChange(next);
    setSelectedConnectionId(connection.id);
    setShowCards(false);
    setEditingConnection(null);
    setDialogOpen(false);
  };

  const renderChooserOrTerminal = (fill = true) => (
    <div className={fill ? "ssh-workspace-pane fill" : "ssh-workspace-pane"}>
      <SshWorkspaceHeader
        layout={layout}
        localProject={localProject}
        showingCards={rightShowsCards}
        onToggleLayout={() => setLayout((prev) => (prev === "full" ? "split" : "full"))}
        onShowCards={() => setShowCards(true)}
        onNewConnection={() => {
          setEditingConnection(null);
          setDialogOpen(true);
        }}
      />
      {rightShowsCards ? (
        <div className="ssh-workspace-card-scroll">
          <SshCardPicker
            connections={connections}
            selectedId={selectedConnectionId}
            onOpen={(connection) => {
              setSelectedConnectionId(connection.id);
              setShowCards(false);
            }}
            onEdit={(connection) => {
              setEditingConnection(connection);
              setDialogOpen(true);
            }}
          />
        </div>
      ) : (
        <SshTerminalPanel
          key={selectedConnection!.id}
          connections={connections}
          onConnectionsChange={onConnectionsChange}
          active={active}
          width="100%"
          themeVariant={themeVariant}
          terminalFontSize={terminalFontSize}
          monoFontFamily={monoFontFamily}
          initialConnectionId={selectedConnection!.id}
          autoConnect
          hideConnectionList
        />
      )}
    </div>
  );

  return (
    <div className="ssh-workspace">
      {remoteConnection ? (
        <div className="ssh-workspace-grid">
          <div className="ssh-workspace-pane ssh-workspace-remote-slot" />
          {renderChooserOrTerminal(false)}
        </div>
      ) : layout === "split" ? (
        <div className="ssh-workspace-grid">{renderChooserOrTerminal(false)}</div>
      ) : (
        renderChooserOrTerminal()
      )}

      {dialogOpen && (
        <SshConnectionDialog
          connection={editingConnection}
          groups={groups}
          onClose={() => {
            setDialogOpen(false);
            setEditingConnection(null);
          }}
          onSave={saveConnection}
        />
      )}
    </div>
  );
}
