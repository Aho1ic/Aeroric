import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type { FontFamily, TerminalFontSize, ThemeVariant } from "../../types";
import { createTerminalRuntime, type TerminalRuntime } from "../terminalRuntime";
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
  const runtimeRef = useRef<TerminalRuntime | null>(null);
  const shellIdRef = useRef(createWslShellId(projectId, distribution));
  const activeRef = useRef(active);
  const themeVariantRef = useRef(themeVariant);
  const terminalFontSizeRef = useRef(terminalFontSize);
  const monoFontFamilyRef = useRef(monoFontFamily);
  const onReadyRef = useRef(onReady);
  const [error, setError] = useState<string | null>(null);
  activeRef.current = active;
  themeVariantRef.current = themeVariant;
  terminalFontSizeRef.current = terminalFontSize;
  monoFontFamilyRef.current = monoFontFamily;
  onReadyRef.current = onReady;

  useImperativeHandle(
    ref,
    () => ({
      sendCommand: (command) => {
        invoke("send_input", { taskId: shellIdRef.current, data: command }).catch(console.error);
      },
    }),
    [],
  );

  const focus = useCallback(() => runtimeRef.current?.focus(), []);

  useEffect(() => {
    if (!containerRef.current) return;
    const container = containerRef.current;
    const shellId = shellIdRef.current;
    let cleaned = false;
    let startTimer: number | null = null;
    const runtime = createTerminalRuntime({
      container,
      themeVariant: themeVariantRef.current,
      terminalFontSize: terminalFontSizeRef.current,
      monoFontFamily: monoFontFamilyRef.current,
      isActive: () => activeRef.current,
      onInput: (data) => {
        invoke("send_input", { taskId: shellId, data }).catch(console.error);
      },
      onResize: ({ cols, rows }) => {
        invoke("resize_pty", { taskId: shellId, cols, rows }).catch(console.error);
      },
    });
    runtimeRef.current = runtime;
    const output = new Channel<string>();
    output.onmessage = (data) => {
      if (!cleaned) runtime.writer.write(data);
    };
    startTimer = window.setTimeout(() => {
      runtime.fit();
      invoke("open_wsl_shell", {
        shellId,
        distribution,
        linuxProjectPath,
        cols: runtime.term.cols,
        rows: runtime.term.rows,
        onOutput: output,
      })
        .then(() => {
          if (!cleaned) {
            setError(null);
            onReadyRef.current?.();
            if (activeRef.current) runtime.focus();
          }
        })
        .catch((nextError) => {
          if (!cleaned) {
            setError(String(nextError));
            runtime.term.writeln(`\r\nError: ${String(nextError)}`);
          }
        });
    }, 50);
    return () => {
      cleaned = true;
      if (startTimer !== null) window.clearTimeout(startTimer);
      invoke("kill_wsl_shell", { shellId }).catch(console.error);
      runtime.dispose();
      runtimeRef.current = null;
    };
  }, [distribution, linuxProjectPath]);

  useEffect(() => {
    if (!active || !runtimeRef.current) return;
    window.requestAnimationFrame(() => {
      if (!runtimeRef.current) return;
      runtimeRef.current.fit();
      focus();
    });
  }, [active, focus]);

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
