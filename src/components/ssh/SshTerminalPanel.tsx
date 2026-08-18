import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { Plug, Power, Server } from "lucide-react";
import type { FontFamily, SshConnection, TerminalFontSize, ThemeVariant } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { themeFor } from "../terminalShared";
import { createTerminalRuntime, type TerminalRuntime } from "../terminalRuntime";
import { SshConnectionDialog } from "./SshConnectionDialog";
import { SshConnectionList } from "./SshConnectionList";
import type { SshConnectionProtocol } from "./SshConnectionContextMenu";
import { createSshShellId, shouldAttemptSshAutoConnect } from "./session";
import "@xterm/xterm/css/xterm.css";

interface ActiveSshSession {
  shellId: string;
  connection: SshConnection;
}

interface Props {
  connections: SshConnection[];
  onConnectionsChange: (connections: SshConnection[]) => void;
  onDeleteConnection?: (connectionId: string) => void | Promise<void>;
  active: boolean;
  width: number | string;
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  monoFontFamily: FontFamily;
  initialConnectionId?: string;
  autoConnect?: boolean;
  hideConnectionList?: boolean;
  onReady?: () => void;
  onConnectSftp?: (connection: SshConnection) => void;
}

export interface SshTerminalPanelHandle {
  sendCommand: (cmd: string) => void;
}

export const SshTerminalPanel = forwardRef<SshTerminalPanelHandle, Props>(function SshTerminalPanel(
  {
    connections,
    onConnectionsChange,
    onDeleteConnection,
    active,
    width,
    themeVariant,
    terminalFontSize,
    monoFontFamily,
    initialConnectionId,
    autoConnect = false,
    hideConnectionList = false,
    onReady,
    onConnectSftp,
  },
  ref,
) {
  const { t } = useI18n();
  const terminalContainerRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<TerminalRuntime | null>(null);
  const activeRef = useRef(active);
  const themeVariantRef = useRef(themeVariant);
  const terminalFontSizeRef = useRef(terminalFontSize);
  const monoFontFamilyRef = useRef(monoFontFamily);
  const onReadyRef = useRef(onReady);
  const [selectedId, setSelectedId] = useState<string | null>(
    () => initialConnectionId ?? connections[0]?.id ?? null,
  );
  const [editingConnection, setEditingConnection] = useState<SshConnection | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [activeSession, setActiveSession] = useState<ActiveSshSession | null>(null);
  const [error, setError] = useState<string | null>(null);
  const autoConnectStartedRef = useRef<string | null>(null);
  activeRef.current = active;
  themeVariantRef.current = themeVariant;
  terminalFontSizeRef.current = terminalFontSize;
  monoFontFamilyRef.current = monoFontFamily;
  onReadyRef.current = onReady;

  useImperativeHandle(
    ref,
    () => ({
      sendCommand: (cmd: string) => {
        const session = activeSession;
        if (!session) return;
        invoke("send_input", { taskId: session.shellId, data: cmd }).catch(console.error);
      },
    }),
    [activeSession],
  );

  const selectedConnection = useMemo(
    () => connections.find((connection) => connection.id === selectedId) ?? connections[0] ?? null,
    [connections, selectedId],
  );
  const terminalTheme = themeFor(themeVariant);
  const connectionGroups = useMemo(
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

  useEffect(() => {
    if (initialConnectionId && initialConnectionId !== selectedId) {
      setSelectedId(initialConnectionId);
      return;
    }
    if (selectedConnection && selectedConnection.id !== selectedId) {
      setSelectedId(selectedConnection.id);
    }
    if (!selectedConnection && selectedId) {
      setSelectedId(null);
    }
  }, [initialConnectionId, selectedConnection, selectedId]);

  const saveConnections = useCallback(
    (nextConnections: SshConnection[]) => {
      onConnectionsChange(nextConnections);
    },
    [onConnectionsChange],
  );

  const handleSaveConnection = useCallback(
    (connection: SshConnection) => {
      const exists = connections.some((item) => item.id === connection.id);
      const next = exists
        ? connections.map((item) => (item.id === connection.id ? connection : item))
        : [connection, ...connections];
      saveConnections(next);
      setSelectedId(connection.id);
      setDialogOpen(false);
      setEditingConnection(null);
    },
    [connections, saveConnections],
  );

  const handleDeleteConnection = useCallback(
    (connectionId: string) => {
      if (activeSession?.connection.id === connectionId) {
        invoke("kill_ssh_shell", { shellId: activeSession.shellId }).catch(console.error);
        setActiveSession(null);
      }
      const next = connections.filter((connection) => connection.id !== connectionId);
      if (onDeleteConnection) {
        void onDeleteConnection(connectionId);
      } else {
        saveConnections(next);
      }
      if (selectedId === connectionId) {
        setSelectedId(next[0]?.id ?? null);
      }
    },
    [activeSession, connections, onDeleteConnection, saveConnections, selectedId],
  );

  const connectConnection = useCallback(
    (connection: SshConnection) => {
      if (!connection) return;
      if (activeSession) {
        invoke("kill_ssh_shell", { shellId: activeSession.shellId }).catch(console.error);
      }
      const now = Date.now();
      const nextConnection = { ...connection, lastConnectedAt: now };
      saveConnections(
        connections.map((item) => (item.id === nextConnection.id ? nextConnection : item)),
      );
      setError(null);
      setActiveSession({
        shellId: createSshShellId(nextConnection.id, now),
        connection: nextConnection,
      });
    },
    [activeSession, connections, saveConnections],
  );

  const handleConnect = useCallback(() => {
    if (!selectedConnection) return;
    connectConnection(selectedConnection);
  }, [connectConnection, selectedConnection]);

  const handleConnectionMenuAction = useCallback(
    (connection: SshConnection, protocol: SshConnectionProtocol) => {
      setSelectedId(connection.id);
      if (protocol === "sftp") {
        onConnectSftp?.(connection);
        return;
      }
      connectConnection(connection);
    },
    [connectConnection, onConnectSftp],
  );

  const handleDisconnect = useCallback(() => {
    if (activeSession) {
      invoke("kill_ssh_shell", { shellId: activeSession.shellId }).catch(console.error);
    }
    setActiveSession(null);
  }, [activeSession]);

  useEffect(() => {
    if (!active) {
      autoConnectStartedRef.current = null;
      return;
    }
    if (
      !shouldAttemptSshAutoConnect({
        autoConnect,
        active,
        hasActiveSession: Boolean(activeSession),
        connectionId: selectedConnection?.id,
        lastStartedConnectionId: autoConnectStartedRef.current,
      })
    ) {
      return;
    }
    autoConnectStartedRef.current = selectedConnection?.id ?? null;
    handleConnect();
  }, [active, activeSession, autoConnect, handleConnect, selectedConnection]);

  useEffect(() => {
    if (!activeSession || !terminalContainerRef.current) return;
    const container = terminalContainerRef.current;
    const session = activeSession;
    let cleaned = false;
    let initTimeoutId: number | null = null;

    const runtime = createTerminalRuntime({
      container,
      themeVariant: themeVariantRef.current,
      terminalFontSize: terminalFontSizeRef.current,
      monoFontFamily: monoFontFamilyRef.current,
      isActive: () => activeRef.current,
      onInput: (data) => {
        invoke("send_input", { taskId: session.shellId, data }).catch(console.error);
      },
      onResize: ({ cols, rows }) => {
        invoke("resize_pty", { taskId: session.shellId, cols, rows }).catch(console.error);
      },
    });
    runtimeRef.current = runtime;
    const outputChannel = new Channel<string>();
    outputChannel.onmessage = (data) => {
      if (!cleaned) runtime.writer.write(data);
    };

    initTimeoutId = window.setTimeout(() => {
      if (cleaned) return;
      runtime.fit();
      invoke<void>("open_ssh_shell", {
        shellId: session.shellId,
        connection: session.connection,
        cols: runtime.term.cols,
        rows: runtime.term.rows,
        onOutput: outputChannel,
      })
        .then(() => {
          onReadyRef.current?.();
          if (activeRef.current) runtime.focus();
        })
        .catch((e: unknown) => {
          const message = String(e);
          setError(message);
          runtime.term.writeln(`\r\nError: ${message}`);
        });
    }, 50);

    return () => {
      cleaned = true;
      if (initTimeoutId !== null) window.clearTimeout(initTimeoutId);
      runtime.dispose();
      runtimeRef.current = null;
    };
  }, [activeSession]);

  useEffect(() => {
    if (!active || !runtimeRef.current || !activeSession) {
      return;
    }
    window.requestAnimationFrame(() => {
      runtimeRef.current?.fit();
      runtimeRef.current?.focus();
    });
  }, [active, activeSession]);

  useEffect(() => {
    runtimeRef.current?.updateTheme(themeVariant);
  }, [themeVariant]);

  useEffect(() => {
    if (!runtimeRef.current || !activeSession) {
      return;
    }
    runtimeRef.current.updateFontSize(terminalFontSize);
  }, [activeSession, terminalFontSize]);

  useEffect(() => {
    if (!runtimeRef.current || !activeSession) {
      return;
    }
    runtimeRef.current.updateFontFamily(monoFontFamily);
  }, [activeSession, monoFontFamily]);

  return (
    <div style={{ ...s.sshPanel, ...(hideConnectionList ? s.sshCenterPanel : null), width }}>
      {!hideConnectionList && (
        <SshConnectionList
          connections={connections}
          selectedId={selectedConnection?.id ?? null}
          onSelect={(connection) => setSelectedId(connection.id)}
          onCreate={() => {
            setEditingConnection(null);
            setDialogOpen(true);
          }}
          onEdit={(connection) => {
            setEditingConnection(connection);
            setDialogOpen(true);
          }}
          onDelete={handleDeleteConnection}
          onConnect={handleConnectionMenuAction}
        />
      )}

      <div style={s.sshTerminalHeader}>
        <div style={s.sshTerminalTitle}>
          <Server size={14} />
          {selectedConnection ? selectedConnection.name : t("ssh.title")}
        </div>
        {activeSession ? (
          <button type="button" style={s.sshSecondaryButton} onClick={handleDisconnect}>
            <Power size={13} />
            {t("ssh.disconnect")}
          </button>
        ) : (
          <button
            type="button"
            style={selectedConnection ? s.sshPrimaryButton : s.sshPrimaryButtonDisabled}
            disabled={!selectedConnection}
            onClick={handleConnect}
          >
            <Plug size={13} />
            {t("ssh.connect")}
          </button>
        )}
      </div>

      {error && <div style={s.sshErrorBanner}>{error}</div>}

      <div
        style={{
          ...s.sshTerminalFrame,
          background: activeSession ? terminalTheme.background : "var(--bg-panel)",
        }}
      >
        {activeSession ? (
          <div
            ref={terminalContainerRef}
            style={{ ...s.sshTerminalCanvas, background: terminalTheme.background }}
          />
        ) : (
          <div style={s.sshTerminalPlaceholder}>{t("ssh.selectAndConnect")}</div>
        )}
      </div>

      {dialogOpen && !hideConnectionList && (
        <SshConnectionDialog
          connection={editingConnection}
          groups={connectionGroups}
          onClose={() => {
            setDialogOpen(false);
            setEditingConnection(null);
          }}
          onSave={handleSaveConnection}
        />
      )}
    </div>
  );
});
