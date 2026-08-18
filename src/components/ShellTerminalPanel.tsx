import type React from "react";
import { useCallback, useEffect, useRef, useState, forwardRef, useImperativeHandle } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { TerminalFontSize, FontFamily, ThemeVariant } from "../types";
import { themeFor } from "./terminalShared";
import { createTerminalRuntime, type TerminalRuntime } from "./terminalRuntime";
import { Minus, Plus, Terminal as TerminalIcon, Trash2, X } from "lucide-react";
import { useI18n } from "../i18n";
import { shellTerminalPanelRootStyle } from "./project-page/viewMode";
import { compactTerminalLabel, formatTerminalTabLabel } from "./terminalTabLabel";
import { AnimatedSelectionTrack } from "./ui/AnimatedSelection";
import "@xterm/xterm/css/xterm.css";

interface ShellOutputEvent {
  shell_id: string;
  data: string;
}

export interface ShellTerminalPanelHandle {
  sendCommand: (cmd: string) => void;
  activateShell: (shellId: string) => void;
  addShell: () => void;
  closeShell: (shellId: string) => void;
}

interface ShellTerminalInstanceHandle {
  sendCommand: (cmd: string) => void;
}

export interface ShellSession {
  id: string;
  title: string;
}

interface Props {
  projectPath: string;
  projectId: string;
  isActive?: boolean;
  onClose: () => void;
  onMinimize?: () => void;
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  monoFontFamily: FontFamily;
  onReady?: () => void;
  height?: number | string;
  visible?: boolean;
  onResizeStart?: (e: React.MouseEvent) => void;
  showSessionTabs?: boolean;
  onSessionsChange?: (sessions: ShellSession[], activeShellId: string | null) => void;
  shellLabel?: string;
}

export const SHELL_TERMINAL_MAX_SESSIONS = 10;

export function deriveShellTerminalFontSize(size: TerminalFontSize): TerminalFontSize {
  return Math.max(10, size - 1);
}

function createShellSession(projectId: string, index: number): ShellSession {
  return {
    id: `shell:${projectId}:${index}:${Date.now()}`,
    title: `Terminal ${index}`,
  };
}

const ShellTerminalInstance = forwardRef<
  ShellTerminalInstanceHandle,
  {
    shellId: string;
    projectPath: string;
    isActive: boolean;
    themeVariant: ThemeVariant;
    terminalFontSize: TerminalFontSize;
    monoFontFamily: FontFamily;
    onReady?: () => void;
  }
>(function ShellTerminalInstance(
  { shellId, projectPath, isActive, themeVariant, terminalFontSize, monoFontFamily, onReady },
  ref,
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<TerminalRuntime | null>(null);
  const isActiveRef = useRef(isActive);
  const themeVariantRef = useRef(themeVariant);
  const terminalFontSizeRef = useRef(terminalFontSize);
  const monoFontFamilyRef = useRef(monoFontFamily);
  const onReadyRef = useRef(onReady);
  isActiveRef.current = isActive;
  themeVariantRef.current = themeVariant;
  terminalFontSizeRef.current = terminalFontSize;
  monoFontFamilyRef.current = monoFontFamily;
  onReadyRef.current = onReady;

  useImperativeHandle(
    ref,
    () => ({
      sendCommand: (cmd: string) => {
        invoke("send_input", { taskId: shellId, data: cmd }).catch(console.error);
      },
    }),
    [shellId],
  );

  const focusTerminal = useCallback(() => {
    runtimeRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!containerRef.current) return;
    const container = containerRef.current;
    let cleaned = false;
    let initTimeoutId: number | null = null;
    let readyTimeoutId: number | null = null;

    const runtime = createTerminalRuntime({
      container,
      themeVariant: themeVariantRef.current,
      terminalFontSize: terminalFontSizeRef.current,
      monoFontFamily: monoFontFamilyRef.current,
      isActive: () => isActiveRef.current,
      onInput: (data) => {
        invoke("send_input", { taskId: shellId, data }).catch(console.error);
      },
      onResize: ({ cols, rows }) => {
        invoke("resize_pty", { taskId: shellId, cols, rows }).catch(console.error);
      },
    });
    runtimeRef.current = runtime;

    initTimeoutId = window.setTimeout(() => {
      if (cleaned) return;
      runtime.fit();
      invoke<void>("open_shell", {
        shellId,
        projectPath,
        cols: runtime.term.cols,
        rows: runtime.term.rows,
      })
        .then(() => {
          if (cleaned) return;
          readyTimeoutId = window.setTimeout(() => {
            if (!cleaned) {
              onReadyRef.current?.();
            }
          }, 300);
        })
        .catch(console.error);
      if (isActiveRef.current) focusTerminal();
    }, 50);

    let unlisten: (() => void) | null = null;
    listen<ShellOutputEvent>("shell-output", (event) => {
      if (event.payload.shell_id === shellId && runtimeRef.current) {
        runtime.writer.write(event.payload.data);
      }
    }).then((fn) => {
      if (cleaned) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cleaned = true;
      if (initTimeoutId !== null) {
        window.clearTimeout(initTimeoutId);
      }
      if (readyTimeoutId !== null) {
        window.clearTimeout(readyTimeoutId);
      }
      unlisten?.();
      runtime.dispose();
      runtimeRef.current = null;
    };
  }, [focusTerminal, shellId, projectPath]);

  useEffect(() => {
    if (!isActive) return;
    window.requestAnimationFrame(() => {
      runtimeRef.current?.fit();
      focusTerminal();
    });
  }, [focusTerminal, isActive, shellId]);

  useEffect(() => {
    runtimeRef.current?.updateTheme(themeVariant);
  }, [themeVariant]);

  useEffect(() => {
    runtimeRef.current?.updateFontSize(terminalFontSize);
  }, [terminalFontSize]);

  useEffect(() => {
    runtimeRef.current?.updateFontFamily(monoFontFamily);
  }, [monoFontFamily]);

  return (
    <div
      ref={containerRef}
      onMouseDown={() => {
        if (isActive) focusTerminal();
      }}
      style={{
        position: "absolute",
        inset: 0,
        overflow: "hidden",
        padding: "4px 6px",
        cursor: "text",
        visibility: isActive ? "visible" : "hidden",
        pointerEvents: isActive ? "auto" : "none",
      }}
    />
  );
});

export const ShellTerminalPanel = forwardRef<ShellTerminalPanelHandle, Props>(
  function ShellTerminalPanel(
    {
      projectPath,
      projectId,
      isActive = true,
      onClose,
      onMinimize,
      themeVariant,
      terminalFontSize,
      monoFontFamily,
      onReady,
      height = 240,
      visible = true,
      onResizeStart,
      showSessionTabs = true,
      onSessionsChange,
      shellLabel = "Shell",
    },
    ref,
  ) {
    const { t } = useI18n();
    const initialShellRef = useRef<ShellSession | null>(null);
    if (!initialShellRef.current) {
      initialShellRef.current = createShellSession(projectId, 1);
    }

    const nextShellIndexRef = useRef(2);
    const shellRefs = useRef<Record<string, ShellTerminalInstanceHandle | null>>({});
    const [shells, setShells] = useState<ShellSession[]>(() => [initialShellRef.current!]);
    const [activeShellId, setActiveShellId] = useState<string | null>(
      () => initialShellRef.current!.id,
    );
    const activeShellIdRef = useRef(activeShellId);
    activeShellIdRef.current = activeShellId;

    const handleAddShell = useCallback(() => {
      if (shells.length >= SHELL_TERMINAL_MAX_SESSIONS) return;
      const nextShell = createShellSession(projectId, nextShellIndexRef.current++);
      setShells((prev) => [...prev, nextShell]);
      setActiveShellId(nextShell.id);
    }, [projectId, shells.length]);

    const handleCloseShell = useCallback(
      (shellId: string) => {
        const closingIndex = shells.findIndex((shell) => shell.id === shellId);
        if (closingIndex === -1) return;

        const nextShells = shells.filter((shell) => shell.id !== shellId);
        invoke("kill_shell", { shellId }).catch(console.error);
        setShells(nextShells);
        delete shellRefs.current[shellId];

        if (nextShells.length === 0) {
          onClose();
          return;
        }

        if (activeShellId === shellId) {
          setActiveShellId(
            nextShells[closingIndex]?.id ??
              nextShells[closingIndex - 1]?.id ??
              nextShells[0]?.id ??
              null,
          );
        }
      },
      [activeShellId, onClose, shells],
    );

    useImperativeHandle(
      ref,
      () => ({
        sendCommand: (cmd: string) => {
          const currentShellId = activeShellIdRef.current;
          if (!currentShellId) return;
          shellRefs.current[currentShellId]?.sendCommand(cmd);
        },
        activateShell: (shellId: string) => {
          if (shells.some((shell) => shell.id === shellId)) setActiveShellId(shellId);
        },
        addShell: handleAddShell,
        closeShell: handleCloseShell,
      }),
      [handleAddShell, handleCloseShell, shells],
    );

    useEffect(() => {
      onSessionsChange?.(shells, activeShellId);
    }, [activeShellId, onSessionsChange, shells]);

    const handleCloseAll = useCallback(() => {
      for (const shell of shells) {
        invoke("kill_shell", { shellId: shell.id }).catch(console.error);
        delete shellRefs.current[shell.id];
      }
      setShells([]);
      setActiveShellId(null);
      onClose();
    }, [onClose, shells]);

    return (
      <div
        style={{
          ...shellTerminalPanelRootStyle({ visible, height }),
          borderTop: "1px solid var(--border-dim)",
          display: "flex",
          flexDirection: "column",
          background: themeFor(themeVariant).background,
          overflow: "hidden",
          visibility: visible ? "visible" : "hidden",
          pointerEvents: visible ? "auto" : "none",
        }}
      >
        {onResizeStart && (
          <div
            onMouseDown={onResizeStart}
            style={{
              height: 4,
              flexShrink: 0,
              cursor: "row-resize",
              background: "transparent",
            }}
          />
        )}
        <div
          style={{
            height: 32,
            flexShrink: 0,
            display: "flex",
            alignItems: "center",
            padding: "0 8px 0 12px",
            borderBottom: "1px solid var(--border-dim)",
            background: "color-mix(in srgb, var(--bg-sidebar) 92%, transparent)",
            gap: 6,
          }}
        >
          <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-primary)", flex: 1 }}>
            {t("terminal.title")}
          </span>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
            {shells.length}/{SHELL_TERMINAL_MAX_SESSIONS}
          </span>
          {onMinimize && (
            <button
              onClick={onMinimize}
              title={t("terminal.minimize")}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: 3,
                borderRadius: 4,
                display: "flex",
                alignItems: "center",
                color: "var(--text-hint)",
              }}
            >
              <Minus size={14} />
            </button>
          )}
          <button
            onClick={handleCloseAll}
            title={t("terminal.closeTerminals")}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: 3,
              borderRadius: 4,
              display: "flex",
              alignItems: "center",
              color: "var(--text-hint)",
            }}
          >
            <X size={14} />
          </button>
        </div>
        {showSessionTabs && (
          <AnimatedSelectionTrack
            value={activeShellId ?? ""}
            ariaLabel={t("terminal.title")}
            role="tablist"
            className="terminal-session-tabs"
            style={{
              minHeight: 30,
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
              gap: 5,
              padding: "4px 8px",
              borderBottom: "1px solid var(--border-dim)",
              background: "color-mix(in srgb, var(--bg-root) 72%, var(--bg-sidebar))",
              overflowX: "auto",
            }}
          >
            {shells.map((shell, index) => {
              const selected = activeShellId === shell.id;
              return (
                <div
                  key={shell.id}
                  className="terminal-session-tab"
                  data-animated-selection-item
                  data-selection-value={shell.id}
                  data-selected={selected ? "true" : "false"}
                  style={{
                    height: 22,
                    minWidth: 0,
                    minHeight: 22,
                    maxWidth: 168,
                    display: "inline-flex",
                    alignItems: "center",
                    padding: "0 3px 0 0",
                    border: "1px solid var(--border-dim)",
                    borderRadius: 999,
                    background: "transparent",
                    flexShrink: 0,
                  }}
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={selected}
                    tabIndex={selected ? 0 : -1}
                    onClick={() => setActiveShellId(shell.id)}
                    title={formatTerminalTabLabel(shellLabel, index)}
                    style={{
                      minWidth: 0,
                      flex: 1,
                      height: "100%",
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 5,
                      padding: "0 3px 0 7px",
                      border: 0,
                      background: "transparent",
                      overflow: "hidden",
                      whiteSpace: "nowrap",
                      color: selected ? "var(--control-active-fg)" : "var(--text-muted)",
                      cursor: "pointer",
                      fontSize: 11,
                      fontWeight: selected ? 650 : 560,
                    }}
                  >
                    <span className="terminal-session-tab__cursor" aria-hidden="true" />
                    <TerminalIcon size={11.5} />
                    <span className="terminal-session-tab__label">
                      {compactTerminalLabel(shellLabel)}
                    </span>
                    <span className="terminal-session-tab__index" aria-hidden="true">
                      {index + 1}
                    </span>
                  </button>
                  <button
                    type="button"
                    aria-label={t("terminal.closeShell", { title: shell.title })}
                    title={t("terminal.closeShell", { title: shell.title })}
                    onClick={() => handleCloseShell(shell.id)}
                    style={{
                      width: 14,
                      height: 14,
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      borderRadius: 999,
                      border: 0,
                      padding: 0,
                      background: "transparent",
                      color: "var(--text-hint)",
                      cursor: "pointer",
                      flexShrink: 0,
                    }}
                  >
                    <Trash2 size={9.5} />
                  </button>
                </div>
              );
            })}
            <button
              onClick={handleAddShell}
              disabled={shells.length >= SHELL_TERMINAL_MAX_SESSIONS}
              title={
                shells.length >= SHELL_TERMINAL_MAX_SESSIONS
                  ? t("terminal.limitReached")
                  : t("terminal.newTerminal")
              }
              style={{
                width: 22,
                height: 22,
                borderRadius: 999,
                border: "1px solid var(--border-dim)",
                background:
                  shells.length >= SHELL_TERMINAL_MAX_SESSIONS ? "transparent" : "var(--bg-hover)",
                color:
                  shells.length >= SHELL_TERMINAL_MAX_SESSIONS
                    ? "var(--text-hint)"
                    : "var(--text-secondary)",
                cursor: shells.length >= SHELL_TERMINAL_MAX_SESSIONS ? "not-allowed" : "pointer",
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                flexShrink: 0,
              }}
            >
              <Plus size={12} />
            </button>
          </AnimatedSelectionTrack>
        )}
        <div style={{ flex: 1, minWidth: 0, minHeight: 0, position: "relative" }}>
          {shells.map((shell) => (
            <ShellTerminalInstance
              key={shell.id}
              ref={(instance) => {
                shellRefs.current[shell.id] = instance;
              }}
              shellId={shell.id}
              projectPath={projectPath}
              isActive={isActive && activeShellId === shell.id}
              themeVariant={themeVariant}
              terminalFontSize={terminalFontSize}
              monoFontFamily={monoFontFamily}
              onReady={onReady}
            />
          ))}
        </div>
      </div>
    );
  },
);
