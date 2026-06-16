import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Plug, Power, Server } from "lucide-react";
import type { FontFamily, SshConnection, TerminalFontSize, ThemeVariant } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { attachLinuxIMEFix, attachMacWebKitShiftInputFix } from "../terminalInputFix";
import { attachSmartCopy } from "../terminalCopyHelper";
import {
  applyTerminalFontFamily,
  applyTerminalFontSize,
  attachMacWebKitTerminalGuard,
  createSmartWriter,
  initTerminal,
  loadWebglAddon,
  safeFit,
  themeFor,
} from "../terminalShared";
import { SshConnectionDialog } from "./SshConnectionDialog";
import { SshConnectionList } from "./SshConnectionList";
import { createSshShellId, shouldAttemptSshAutoConnect } from "./session";
import "@xterm/xterm/css/xterm.css";

interface ShellOutputEvent {
  shell_id: string;
  data: string;
}

interface ActiveSshSession {
  shellId: string;
  connection: SshConnection;
}

interface Props {
  connections: SshConnection[];
  onConnectionsChange: (connections: SshConnection[]) => void;
  active: boolean;
  width: number | string;
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  monoFontFamily: FontFamily;
  initialConnectionId?: string;
  autoConnect?: boolean;
  hideConnectionList?: boolean;
}

export function SshTerminalPanel({
  connections,
  onConnectionsChange,
  active,
  width,
  themeVariant,
  terminalFontSize,
  monoFontFamily,
  initialConnectionId,
  autoConnect = false,
  hideConnectionList = false,
}: Props) {
  const { t } = useI18n();
  const terminalContainerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const lastSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const activeRef = useRef(active);
  const [selectedId, setSelectedId] = useState<string | null>(
    () => initialConnectionId ?? connections[0]?.id ?? null,
  );
  const [editingConnection, setEditingConnection] = useState<SshConnection | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [activeSession, setActiveSession] = useState<ActiveSshSession | null>(null);
  const [error, setError] = useState<string | null>(null);
  const autoConnectStartedRef = useRef<string | null>(null);
  activeRef.current = active;

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
      saveConnections(next);
      if (selectedId === connectionId) {
        setSelectedId(next[0]?.id ?? null);
      }
    },
    [activeSession, connections, saveConnections, selectedId],
  );

  const handleConnect = useCallback(() => {
    if (!selectedConnection) return;
    if (activeSession) {
      invoke("kill_ssh_shell", { shellId: activeSession.shellId }).catch(console.error);
    }
    const now = Date.now();
    const connection = { ...selectedConnection, lastConnectedAt: now };
    saveConnections(
      connections.map((item) => (item.id === connection.id ? connection : item)),
    );
    setError(null);
    setActiveSession({
      shellId: createSshShellId(connection.id, now),
      connection,
    });
  }, [activeSession, connections, saveConnections, selectedConnection]);

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
    let cleaned = false;
    let unlisten: (() => void) | null = null;
    let initTimeoutId: number | null = null;

    const { term, fitAddon } = initTerminal(themeVariant, 5000, terminalFontSize, monoFontFamily);
    terminalRef.current = term;
    fitAddonRef.current = fitAddon;
    term.open(container);
    const disposeInputFix = attachMacWebKitShiftInputFix(term);
    loadWebglAddon(term);
    const writer = createSmartWriter(term);
    const disposeMacWebKitGuard = attachMacWebKitTerminalGuard({ term, container, writer });
    const disposeSmartCopy = attachSmartCopy(term, {
      onPaste: (text) => {
        invoke("send_input", { taskId: activeSession.shellId, data: text }).catch(console.error);
      },
    });
    const input = attachLinuxIMEFix(term, (data) => {
      invoke("send_input", { taskId: activeSession.shellId, data }).catch(console.error);
    });

    const fit = () => {
      if (cleaned) return;
      const size = safeFit(fitAddon, term, container);
      if (!size) return;
      const last = lastSizeRef.current;
      if (last && last.cols === size.cols && last.rows === size.rows) return;
      lastSizeRef.current = size;
      invoke("resize_pty", {
        taskId: activeSession.shellId,
        cols: size.cols,
        rows: size.rows,
      }).catch(() => {});
    };

    initTimeoutId = window.setTimeout(() => {
      if (cleaned) return;
      fit();
      invoke<void>("open_ssh_shell", {
        shellId: activeSession.shellId,
        connection: activeSession.connection,
        cols: term.cols,
        rows: term.rows,
      })
        .then(() => {
          if (activeRef.current) term.focus();
        })
        .catch((e: unknown) => {
          const message = String(e);
          setError(message);
          term.writeln(`\r\nError: ${message}`);
        });
    }, 50);

    const resizeObserver = new ResizeObserver(() => {
      window.setTimeout(() => {
        if (activeRef.current) fit();
      }, 50);
    });
    resizeObserver.observe(container);

    listen<ShellOutputEvent>("shell-output", (event) => {
      if (event.payload.shell_id === activeSession.shellId) {
        writer.write(event.payload.data);
      }
    }).then((fn) => {
      if (cleaned) fn();
      else unlisten = fn;
    });

    return () => {
      cleaned = true;
      if (initTimeoutId !== null) window.clearTimeout(initTimeoutId);
      unlisten?.();
      resizeObserver.disconnect();
      disposeSmartCopy();
      input.dispose();
      disposeMacWebKitGuard();
      disposeInputFix();
      terminalRef.current = null;
      fitAddonRef.current = null;
      lastSizeRef.current = null;
      term.dispose();
    };
  }, [activeSession, monoFontFamily, terminalFontSize, themeVariant]);

  useEffect(() => {
    if (!active || !terminalRef.current || !fitAddonRef.current || !terminalContainerRef.current || !activeSession) {
      return;
    }
    window.requestAnimationFrame(() => {
      if (!terminalRef.current || !fitAddonRef.current || !terminalContainerRef.current) return;
      const size = safeFit(fitAddonRef.current, terminalRef.current, terminalContainerRef.current);
      if (size) {
        const last = lastSizeRef.current;
        if (!last || last.cols !== size.cols || last.rows !== size.rows) {
          lastSizeRef.current = size;
          invoke("resize_pty", { taskId: activeSession.shellId, cols: size.cols, rows: size.rows }).catch(
            () => {},
          );
        }
      }
      terminalRef.current.focus();
    });
  }, [active, activeSession]);

  useEffect(() => {
    if (!terminalRef.current) return;
    terminalRef.current.options.theme = themeFor(themeVariant);
  }, [themeVariant]);

  useEffect(() => {
    if (!terminalRef.current || !fitAddonRef.current || !terminalContainerRef.current || !activeSession) {
      return;
    }
    const size = applyTerminalFontSize(
      terminalRef.current,
      fitAddonRef.current,
      terminalFontSize,
      terminalContainerRef.current,
    );
    if (!size) return;
    invoke("resize_pty", { taskId: activeSession.shellId, cols: size.cols, rows: size.rows }).catch(
      () => {},
    );
  }, [activeSession, terminalFontSize]);

  useEffect(() => {
    if (!terminalRef.current || !fitAddonRef.current || !terminalContainerRef.current || !activeSession) {
      return;
    }
    const size = applyTerminalFontFamily(
      terminalRef.current,
      fitAddonRef.current,
      monoFontFamily,
      terminalContainerRef.current,
    );
    if (!size) return;
    invoke("resize_pty", { taskId: activeSession.shellId, cols: size.cols, rows: size.rows }).catch(
      () => {},
    );
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
}
