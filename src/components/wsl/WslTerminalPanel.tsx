import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { FontFamily, TerminalFontSize, ThemeVariant } from "../../types";
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
import "@xterm/xterm/css/xterm.css";

export interface WslTerminalPanelHandle {
  sendCommand: (command: string) => void;
}

export function createWslShellId(projectId: string, distribution: string): string {
  const safeDistribution = distribution.replace(/[^A-Za-z0-9._-]/g, "_");
  return `wsl:${projectId}:${safeDistribution}:${Date.now()}`;
}

export const WslTerminalPanel = forwardRef<
  WslTerminalPanelHandle,
  {
    projectId: string;
    distribution: string;
    linuxProjectPath: string;
    active: boolean;
    themeVariant: ThemeVariant;
    terminalFontSize: TerminalFontSize;
    monoFontFamily: FontFamily;
    onReady?: () => void;
  }
>(function WslTerminalPanel(
  {
    projectId,
    distribution,
    linuxProjectPath,
    active,
    themeVariant,
    terminalFontSize,
    monoFontFamily,
    onReady,
  },
  ref,
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const shellIdRef = useRef(createWslShellId(projectId, distribution));
  const lastSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const activeRef = useRef(active);
  const [error, setError] = useState<string | null>(null);
  activeRef.current = active;

  useImperativeHandle(
    ref,
    () => ({
      sendCommand: (command) => {
        invoke("send_input", { taskId: shellIdRef.current, data: command }).catch(console.error);
      },
    }),
    [],
  );

  const focus = useCallback(() => terminalRef.current?.focus(), []);

  useEffect(() => {
    if (!containerRef.current) return;
    const container = containerRef.current;
    const shellId = shellIdRef.current;
    let cleaned = false;
    let startTimer: number | null = null;
    const { term, fitAddon } = initTerminal(themeVariant, 5000, terminalFontSize, monoFontFamily);
    terminalRef.current = term;
    fitAddonRef.current = fitAddon;
    term.open(container);
    const disposeInputFix = attachMacWebKitShiftInputFix(term);
    loadWebglAddon(term);
    const writer = createSmartWriter(term, () => themeVariant, { resumeOnAnyOutput: true });
    const disposeGuard = attachMacWebKitTerminalGuard({ term, container, writer });
    const disposeCopy = attachSmartCopy(term, {
      onPaste: (text) => {
        writer.pauseForUserInput();
        invoke("send_input", { taskId: shellId, data: text }).catch(console.error);
      },
    });
    const input = attachLinuxIMEFix(term, (data) => {
      writer.pauseForUserInput();
      invoke("send_input", { taskId: shellId, data }).catch(console.error);
    });
    const output = new Channel<string>();
    output.onmessage = (data) => {
      if (!cleaned) writer.write(data);
    };
    const fit = () => {
      const size = safeFit(fitAddon, term, container);
      if (!size) return;
      if (lastSizeRef.current?.cols === size.cols && lastSizeRef.current.rows === size.rows) return;
      lastSizeRef.current = size;
      invoke("resize_pty", { taskId: shellId, cols: size.cols, rows: size.rows }).catch(() => {});
    };
    startTimer = window.setTimeout(() => {
      fit();
      invoke("open_wsl_shell", {
        shellId,
        distribution,
        linuxProjectPath,
        cols: term.cols,
        rows: term.rows,
        onOutput: output,
      })
        .then(() => {
          if (!cleaned) {
            setError(null);
            onReady?.();
            if (activeRef.current) focus();
          }
        })
        .catch((nextError) => {
          if (!cleaned) {
            setError(String(nextError));
            term.writeln(`\r\nError: ${String(nextError)}`);
          }
        });
    }, 50);
    const observer = new ResizeObserver(() => {
      if (activeRef.current) window.setTimeout(fit, 50);
    });
    observer.observe(container);
    return () => {
      cleaned = true;
      if (startTimer !== null) window.clearTimeout(startTimer);
      observer.disconnect();
      disposeCopy();
      input.dispose();
      disposeGuard();
      disposeInputFix();
      invoke("kill_wsl_shell", { shellId }).catch(() => {});
      terminalRef.current = null;
      fitAddonRef.current = null;
      term.dispose();
    };
  }, [
    distribution,
    focus,
    linuxProjectPath,
    monoFontFamily,
    onReady,
    terminalFontSize,
    themeVariant,
  ]);

  useEffect(() => {
    if (!active || !terminalRef.current || !fitAddonRef.current || !containerRef.current) return;
    window.requestAnimationFrame(() => {
      if (!terminalRef.current || !fitAddonRef.current || !containerRef.current) return;
      safeFit(fitAddonRef.current, terminalRef.current, containerRef.current);
      focus();
    });
  }, [active, focus]);

  useEffect(() => {
    if (terminalRef.current) terminalRef.current.options.theme = themeFor(themeVariant);
  }, [themeVariant]);

  useEffect(() => {
    if (!terminalRef.current || !fitAddonRef.current || !containerRef.current) return;
    applyTerminalFontSize(
      terminalRef.current,
      fitAddonRef.current,
      terminalFontSize,
      containerRef.current,
    );
  }, [terminalFontSize]);

  useEffect(() => {
    if (!terminalRef.current || !fitAddonRef.current || !containerRef.current) return;
    applyTerminalFontFamily(
      terminalRef.current,
      fitAddonRef.current,
      monoFontFamily,
      containerRef.current,
    );
  }, [monoFontFamily]);

  return (
    <div
      style={{ position: "relative", width: "100%", height: "100%", background: "var(--bg-panel)" }}
    >
      {error && (
        <div
          style={{ position: "absolute", top: 8, right: 8, color: "var(--danger)", fontSize: 11 }}
        >
          {error}
        </div>
      )}
      <div ref={containerRef} style={{ position: "absolute", inset: 0, padding: "4px 6px" }} />
    </div>
  );
});
