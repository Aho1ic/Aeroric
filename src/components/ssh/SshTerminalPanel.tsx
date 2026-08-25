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
import type {
  FontFamily,
  SshConnection,
  SshHostKey,
  SshHostKeyStatus,
  TerminalFontSize,
  ThemeVariant,
} from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { themeFor } from "../terminalShared";
import { createTerminalRuntime, type TerminalRuntime } from "../terminalRuntime";
import { SshConnectionDialog } from "./SshConnectionDialog";
import { SshConnectionList } from "./SshConnectionList";
import { SshHostKeyDialog } from "./SshHostKeyDialog";
import type { SshConnectionProtocol } from "./SshConnectionContextMenu";
import { createSshShellId, shouldAttemptSshAutoConnect, sshHostKeyGate } from "./session";
import { useSshGroups } from "./useSshGroups";
import "@xterm/xterm/css/xterm.css";

interface ActiveSshSession {
  shellId: string;
  connection: SshConnection;
}

/** 等待用户确认 host key 的连接。确认后才真正开会话。 */
interface PendingHostKey {
  connection: SshConnection;
  target: string;
  keys: SshHostKey[];
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
  // 新建连接时预填的分组,来自分组标题上的 +。
  const [dialogInitialGroup, setDialogInitialGroup] = useState("");
  const [activeSession, setActiveSession] = useState<ActiveSshSession | null>(null);
  const [pendingHostKey, setPendingHostKey] = useState<PendingHostKey | null>(null);
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

  // 对话框下拉、列表、欢迎页视图共用同一份名单(store 订阅),否则一侧建的空分组
  // 在另一侧看不到。
  const {
    groups: connectionGroups,
    createGroup: handleCreateGroup,
    deleteGroup: handleDeleteGroup,
    renameGroup: handleRenameGroup,
  } = useSshGroups(connections, saveConnections);

  const handleSaveConnection = useCallback(
    (connection: SshConnection) => {
      const exists = connections.some((item) => item.id === connection.id);
      const next = exists
        ? connections.map((item) => (item.id === connection.id ? connection : item))
        : [connection, ...connections];
      saveConnections(next);
      // 在对话框里手输的新分组也要进名单,否则它只在"有连接引用它"时才可见,
      // 一旦把最后一条连接移走就消失了。
      handleCreateGroup(connection.group ?? "");
      setSelectedId(connection.id);
      setDialogOpen(false);
      setDialogInitialGroup("");
      setEditingConnection(null);
    },
    [connections, handleCreateGroup, saveConnections],
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

  const startSession = useCallback(
    (connection: SshConnection) => {
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
    [connections, saveConnections],
  );

  const connectConnection = useCallback(
    (connection: SshConnection) => {
      if (!connection) return;
      if (activeSession) {
        invoke("kill_ssh_shell", { shellId: activeSession.shellId }).catch(console.error);
      }
      setError(null);
      // 后端对每次 ssh 调用都强制 StrictHostKeyChecking=yes,未登记的主机必然
      // 连不上。先问一次:需要确认指纹就弹窗,让用户在 App 里完成命令行 ssh
      // 那个 "Are you sure you want to continue connecting?" 步骤。
      invoke<SshHostKeyStatus>("check_ssh_host_key", { connection })
        .then((status) => {
          const gate = sshHostKeyGate(status);
          if (gate.action === "prompt") {
            setPendingHostKey({ connection, target: gate.target, keys: gate.keys });
            return;
          }
          startSession(connection);
        })
        .catch((e: unknown) => {
          // 查不出来就照常连,由 ssh 给出真实结果 —— 这一步只为改善措辞,
          // 不该变成新的失败点。
          console.warn("check_ssh_host_key failed", e);
          startSession(connection);
        });
    },
    [activeSession, startSession],
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
            setDialogInitialGroup("");
            setDialogOpen(true);
          }}
          onEdit={(connection) => {
            setEditingConnection(connection);
            setDialogInitialGroup("");
            setDialogOpen(true);
          }}
          onDelete={handleDeleteConnection}
          onConnect={handleConnectionMenuAction}
          groupNames={connectionGroups}
          onCreateGroup={handleCreateGroup}
          onDeleteGroup={handleDeleteGroup}
          onRenameGroup={handleRenameGroup}
          onCreateInGroup={(group) => {
            setEditingConnection(null);
            setDialogInitialGroup(group);
            setDialogOpen(true);
          }}
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
          initialGroup={dialogInitialGroup}
          onClose={() => {
            setDialogOpen(false);
            setDialogInitialGroup("");
            setEditingConnection(null);
          }}
          onSave={handleSaveConnection}
        />
      )}

      {/* 与连接列表无关:嵌入模式下也要能确认 host key,否则那条路径永远连不上新主机。 */}
      {pendingHostKey && (
        <SshHostKeyDialog
          connection={pendingHostKey.connection}
          target={pendingHostKey.target}
          keys={pendingHostKey.keys}
          onTrusted={() => {
            const { connection } = pendingHostKey;
            setPendingHostKey(null);
            startSession(connection);
          }}
          onCancel={() => setPendingHostKey(null)}
        />
      )}
    </div>
  );
});
