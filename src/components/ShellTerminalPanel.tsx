import type React from "react";
import { useCallback, useEffect, useRef, useState, forwardRef, useImperativeHandle } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { attachSmartCopy } from "./terminalCopyHelper";
import type { TerminalFontSize, FontFamily, ThemeVariant } from "../types";
import {
  themeFor,
  initTerminal,
  loadWebglAddon,
  safeFit,
  createSmartWriter,
  attachMacWebKitTerminalGuard,
  applyTerminalFontSize,
  applyTerminalFontFamily,
  terminalFontFamilyCss,
} from "./terminalShared";
import { attachLinuxIMEFix, attachMacWebKitShiftInputFix } from "./terminalInputFix";
import { Minus, Plus, Terminal as TerminalIcon, Trash2, X } from "lucide-react";
import { useI18n } from "../i18n";
import { shellTerminalPanelRootStyle } from "./project-page/viewMode";
import "@xterm/xterm/css/xterm.css";

interface ShellOutputEvent {
  shell_id: string;
  data: string;
}

export interface ShellTerminalPanelHandle {
  sendCommand: (cmd: string) => void;
}

interface ShellTerminalInstanceHandle {
  sendCommand: (cmd: string) => void;
}

interface ShellSession {
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
  const terminalRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const themeVariantRef = useRef(themeVariant);
  const isActiveRef = useRef(isActive);
  const terminalFontSizeRef = useRef(terminalFontSize);
  const monoFontFamilyRef = useRef(monoFontFamily);
  const onReadyRef = useRef(onReady);
  const lastSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  themeVariantRef.current = themeVariant;
  isActiveRef.current = isActive;
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
    const term = terminalRef.current;
    if (!term) return;
    if (term.textarea?.disabled) {
      term.textarea.disabled = false;
    }
    term.focus();
    term.textarea?.focus({ preventScroll: true });
  }, []);

  useEffect(() => {
    if (!containerRef.current) return;
    const container = containerRef.current;
    let cleaned = false;
    let initTimeoutId: number | null = null;
    let readyTimeoutId: number | null = null;

    const { term, fitAddon } = initTerminal(
      themeVariantRef.current,
      5000,
      terminalFontSizeRef.current,
      monoFontFamilyRef.current,
    );
    terminalRef.current = term;
    fitAddonRef.current = fitAddon;
    term.open(container);
    const disposeInputFix = attachMacWebKitShiftInputFix(term);
    loadWebglAddon(term);
    const writer = createSmartWriter(term, () => themeVariantRef.current);
    const disposeMacWebKitGuard = attachMacWebKitTerminalGuard({ term, container, writer });

    const fit = () => {
      if (cleaned) return;
      const s = safeFit(fitAddon, term, container);
      if (!s) return;
      const last = lastSizeRef.current;
      if (last && last.cols === s.cols && last.rows === s.rows) return;
      lastSizeRef.current = { cols: s.cols, rows: s.rows };
      invoke("resize_pty", { taskId: shellId, cols: s.cols, rows: s.rows }).catch(() => {});
    };

    initTimeoutId = window.setTimeout(() => {
      if (cleaned) return;
      fit();
      invoke<void>("open_shell", {
        shellId,
        projectPath,
        cols: term.cols,
        rows: term.rows,
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

    const disposeSmartCopy = attachSmartCopy(term, {
      onPaste: (text) => {
        invoke("send_input", { taskId: shellId, data: text }).catch(() => {});
      },
    });
    const linuxIME = attachLinuxIMEFix(term, (data) => {
      invoke("send_input", { taskId: shellId, data }).catch(() => {});
    });
    const disposeOnData = { dispose: () => linuxIME.dispose() };

    const resizeObserver = new ResizeObserver(() => {
      setTimeout(() => {
        if (isActiveRef.current) {
          fit();
        }
      }, 50);
    });
    resizeObserver.observe(container);

    const handleVisibilityChange = () => {
      if (document.visibilityState !== "visible" || !terminalRef.current || !isActiveRef.current)
        return;
      window.requestAnimationFrame(() => {
        fit();
        const t = terminalRef.current;
        if (t) {
          focusTerminal();
        }
      });
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);

    let unlisten: (() => void) | null = null;
    listen<ShellOutputEvent>("shell-output", (event) => {
      if (event.payload.shell_id === shellId && terminalRef.current) {
        writer.write(event.payload.data);
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
      disposeSmartCopy();
      disposeOnData.dispose();
      resizeObserver.disconnect();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      terminalRef.current = null;
      fitAddonRef.current = null;
      disposeMacWebKitGuard();
      disposeInputFix();
      term.dispose();
    };
  }, [focusTerminal, shellId, projectPath]);

  useEffect(() => {
    if (!isActive) return;
    window.requestAnimationFrame(() => {
      if (!fitAddonRef.current || !terminalRef.current || !containerRef.current) return;
      const s = safeFit(fitAddonRef.current, terminalRef.current, containerRef.current);
      if (s) {
        const last = lastSizeRef.current;
        if (!last || last.cols !== s.cols || last.rows !== s.rows) {
          lastSizeRef.current = { cols: s.cols, rows: s.rows };
          invoke("resize_pty", { taskId: shellId, cols: s.cols, rows: s.rows }).catch(() => {});
        }
      }
      focusTerminal();
    });
  }, [focusTerminal, isActive, shellId]);

  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.options.theme = themeFor(themeVariant);
    }
  }, [themeVariant]);

  useEffect(() => {
    if (!terminalRef.current || !fitAddonRef.current || !containerRef.current) return;
    const size = applyTerminalFontSize(
      terminalRef.current,
      fitAddonRef.current,
      terminalFontSize,
      containerRef.current,
    );
    if (!size) return;
    const last = lastSizeRef.current;
    if (last && last.cols === size.cols && last.rows === size.rows) return;
    lastSizeRef.current = { cols: size.cols, rows: size.rows };
    invoke("resize_pty", { taskId: shellId, cols: size.cols, rows: size.rows }).catch(() => {});
  }, [terminalFontSize, shellId]);

  useEffect(() => {
    if (!terminalRef.current || !fitAddonRef.current || !containerRef.current) return;
    const size = applyTerminalFontFamily(
      terminalRef.current,
      fitAddonRef.current,
      monoFontFamily,
      containerRef.current,
    );
    if (!size) return;
    const last = lastSizeRef.current;
    if (last && last.cols === size.cols && last.rows === size.rows) return;
    lastSizeRef.current = { cols: size.cols, rows: size.rows };
    invoke("resize_pty", { taskId: shellId, cols: size.cols, rows: size.rows }).catch(() => {});
  }, [monoFontFamily, shellId]);

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
        fontFamily: terminalFontFamilyCss(monoFontFamily),
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

    useImperativeHandle(
      ref,
      () => ({
        sendCommand: (cmd: string) => {
          const currentShellId = activeShellIdRef.current;
          if (!currentShellId) return;
          shellRefs.current[currentShellId]?.sendCommand(cmd);
        },
      }),
      [],
    );

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
        invoke("kill_shell", { shellId }).catch(() => {});
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

    const handleCloseAll = useCallback(() => {
      for (const shell of shells) {
        invoke("kill_shell", { shellId: shell.id }).catch(() => {});
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
        <div
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
              <button
                key={shell.id}
                type="button"
                onClick={() => setActiveShellId(shell.id)}
                title={shell.title}
                style={{
                  height: 22,
                  minWidth: 0,
                  maxWidth: 106,
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 5,
                  padding: "0 5px 0 7px",
                  border: `1px solid ${selected ? "var(--border-strong)" : "var(--border-dim)"}`,
                  borderRadius: 999,
                  background: selected ? "var(--control-active-bg)" : "transparent",
                  color: selected ? "var(--control-active-fg)" : "var(--text-muted)",
                  cursor: "pointer",
                  flexShrink: 0,
                  fontSize: 11,
                  fontWeight: selected ? 650 : 560,
                }}
              >
                <TerminalIcon size={11.5} />
                <span
                  style={{
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  zsh {index + 1}
                </span>
                <span
                  role="button"
                  tabIndex={0}
                  title={t("terminal.closeShell", { title: shell.title })}
                  onClick={(event) => {
                    event.stopPropagation();
                    handleCloseShell(shell.id);
                  }}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter" && event.key !== " ") return;
                    event.preventDefault();
                    event.stopPropagation();
                    handleCloseShell(shell.id);
                  }}
                  style={{
                    width: 14,
                    height: 14,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    borderRadius: 999,
                    color: "var(--text-hint)",
                    flexShrink: 0,
                  }}
                >
                  <Trash2 size={9.5} />
                </span>
              </button>
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
        </div>
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
