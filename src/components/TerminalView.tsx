import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { attachSmartCopy } from "./terminalCopyHelper";
import {
  DEFAULT_SHIFT_ENTER_NEWLINE,
  matchesTerminalNewline,
  normalizeShiftEnterNewline,
  TERMINAL_NEWLINE_SEQUENCE,
} from "../shortcuts";
import type { TerminalFontSize, FontFamily, ThemeVariant } from "../types";
import {
  themeFor,
  initTerminal,
  safeFit,
  createSmartWriter,
  attachMacWebKitTerminalGuard,
  attachCursorLineHighlight,
  applyTerminalFontSize,
  applyTerminalFontFamily,
} from "./terminalShared";
import type { TerminalResizeFn, TerminalWriteFn } from "../hooks/useTerminalManager";
import {
  applyTerminalTextareaInputAttributes,
  attachLinuxIMEFix,
  attachMacWebKitShiftInputFix,
  attachWindowsIMEPositionFix,
} from "./terminalInputFix";
import "@xterm/xterm/css/xterm.css";

interface TerminalViewProps {
  onInput: (data: string) => void;
  onResize: (cols: number, rows: number) => void;
  onRegisterTerminal: (writeFn: TerminalWriteFn | null, resizeFn?: TerminalResizeFn) => number;
  onReady?: (generation: number) => void;
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  monoFontFamily: FontFamily;
  isActive?: boolean;
  initialData?: string;
  initialSnapshot?: string;
  rawReplayData?: string;
  onSnapshot?: (snapshot: string) => void;
  highlightCursorLine?: boolean;
}

interface TerminalRuntime {
  term: Terminal;
  fitAddon: FitAddon;
  serializeAddon: SerializeAddon;
  writer: ReturnType<typeof createSmartWriter>;
  theme: ThemeVariant;
  focus: () => void;
  dispose: () => void;
}

export function TerminalView({
  onInput,
  onResize,
  onRegisterTerminal,
  onReady,
  themeVariant,
  terminalFontSize,
  monoFontFamily,
  isActive = true,
  initialData,
  initialSnapshot,
  rawReplayData,
  onSnapshot,
  highlightCursorLine = false,
}: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<TerminalRuntime | null>(null);
  const onInputRef = useRef(onInput);
  const onResizeRef = useRef(onResize);
  const onRegisterRef = useRef(onRegisterTerminal);
  const onReadyRef = useRef(onReady);
  const onSnapshotRef = useRef(onSnapshot);
  const isActiveRef = useRef(isActive);
  const lastSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const shiftEnterNewlineRef = useRef<boolean>(DEFAULT_SHIFT_ENTER_NEWLINE);
  const desiredThemeRef = useRef(themeVariant);
  const rebuildRef = useRef<((theme: ThemeVariant, onComplete?: () => void) => void) | null>(null);
  const rebuildingRef = useRef(true);
  const pendingWriteCallbacksRef = useRef<Array<(() => void) | undefined>>([]);
  const rawChunksRef = useRef<string[] | null>(null);

  if (rawChunksRef.current === null) {
    const raw = rawReplayData ?? initialData ?? "";
    rawChunksRef.current = raw ? [raw] : [];
  }

  onInputRef.current = onInput;
  onResizeRef.current = onResize;
  onRegisterRef.current = onRegisterTerminal;
  onReadyRef.current = onReady;
  onSnapshotRef.current = onSnapshot;
  isActiveRef.current = isActive;
  desiredThemeRef.current = themeVariant;

  const stableWriteRef = useRef<TerminalWriteFn | null>(null);
  if (stableWriteRef.current === null) {
    stableWriteRef.current = (data, callback) => {
      rawChunksRef.current?.push(data);
      const runtime = runtimeRef.current;
      if (rebuildingRef.current || !runtime) {
        pendingWriteCallbacksRef.current.push(callback);
        return;
      }
      runtime.writer.write(data, callback);
    };
  }

  const notifyResize = useCallback((cols: number, rows: number) => {
    const last = lastSizeRef.current;
    if (last && last.cols === cols && last.rows === rows) return;
    lastSizeRef.current = { cols, rows };
    onResizeRef.current(cols, rows);
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let disposed = false;
    const pendingWriteCallbacks = pendingWriteCallbacksRef.current;

    const createRuntime = (theme: ThemeVariant): TerminalRuntime => {
      const { term, fitAddon } = initTerminal(theme, 1000, terminalFontSize, monoFontFamily);
      const serializeAddon = new SerializeAddon();
      term.loadAddon(serializeAddon);
      term.open(container);
      applyTerminalTextareaInputAttributes(term);

      const writer = createSmartWriter(term, () => theme);
      const disposeInputFix = attachMacWebKitShiftInputFix(term);
      const disposeWindowsImeFix = attachWindowsIMEPositionFix(term);
      const disposeMacWebKitGuard = attachMacWebKitTerminalGuard({ term, container, writer });
      const disposeCursorLineHighlight = highlightCursorLine
        ? attachCursorLineHighlight(term, container)
        : () => {};
      const sendInput = (data: string) => {
        writer.pauseForUserInput();
        onInputRef.current(data);
      };
      const disposeSmartCopy = attachSmartCopy(term, {
        matchesNewline: (event) => matchesTerminalNewline(event, shiftEnterNewlineRef.current),
        onNewline: () => sendInput(TERMINAL_NEWLINE_SEQUENCE),
        onPaste: (text) => sendInput(text),
      });
      const linuxIme = attachLinuxIMEFix(term, sendInput);

      const focus = () => {
        if (!isActiveRef.current) return;
        window.requestAnimationFrame(() => {
          if (term.textarea?.disabled) term.textarea.disabled = false;
          term.focus();
          term.textarea?.focus({ preventScroll: true });
        });
      };

      const handlePointerDown = (event: PointerEvent) => {
        if (event.button === 0) focus();
      };
      container.addEventListener("pointerdown", handlePointerDown as EventListener);

      return {
        term,
        fitAddon,
        serializeAddon,
        writer,
        theme,
        focus,
        dispose: () => {
          container.removeEventListener("pointerdown", handlePointerDown as EventListener);
          disposeMacWebKitGuard();
          disposeCursorLineHighlight();
          disposeInputFix();
          disposeWindowsImeFix();
          disposeSmartCopy();
          linuxIme.dispose();
          term.dispose();
          container.replaceChildren();
        },
      };
    };

    const fitRuntime = (runtime: TerminalRuntime) => {
      const size = safeFit(runtime.fitAddon, runtime.term, container);
      if (size) notifyResize(size.cols, size.rows);
    };

    const finishQueuedWrites = (
      runtime: TerminalRuntime,
      renderedLength: number,
      viewportY: number,
      wasAtBottom: boolean,
      hadFocus: boolean,
      onComplete?: () => void,
    ) => {
      if (disposed || runtimeRef.current !== runtime) return;
      const raw = rawChunksRef.current?.join("") ?? "";
      if (raw.length > renderedLength) {
        runtime.writer.write(raw.slice(renderedLength), () =>
          finishQueuedWrites(runtime, raw.length, viewportY, wasAtBottom, hadFocus, onComplete),
        );
        return;
      }
      if (runtime.theme !== desiredThemeRef.current) {
        runtime.dispose();
        runtimeRef.current = null;
        rebuildingRef.current = false;
        rebuildRef.current?.(desiredThemeRef.current, onComplete);
        return;
      }

      rebuildingRef.current = false;
      if (wasAtBottom) {
        runtime.term.scrollToBottom();
      } else {
        runtime.term.scrollToLine(Math.min(viewportY, runtime.term.buffer.active.baseY));
      }
      if (hadFocus) runtime.focus();
      const callbacks = pendingWriteCallbacks.splice(0);
      callbacks.forEach((callback) => callback?.());
      onComplete?.();
    };

    rebuildRef.current = (theme, onComplete) => {
      if (disposed || rebuildingRef.current) return;
      const previous = runtimeRef.current;
      const raw = rawChunksRef.current?.join("") ?? "";
      rebuildingRef.current = true;
      const viewportY = previous?.term.buffer.active.viewportY ?? 0;
      const wasAtBottom = previous
        ? previous.term.buffer.active.viewportY >= previous.term.buffer.active.baseY
        : true;
      const hadFocus = Boolean(previous?.term.textarea === document.activeElement);
      previous?.dispose();

      const runtime = createRuntime(theme);
      runtimeRef.current = runtime;
      fitRuntime(runtime);
      if (raw) {
        runtime.writer.write(raw, () =>
          finishQueuedWrites(runtime, raw.length, viewportY, wasAtBottom, hadFocus, onComplete),
        );
      } else {
        finishQueuedWrites(runtime, 0, viewportY, wasAtBottom, hadFocus, onComplete);
      }
    };

    const runtime = createRuntime(themeVariant);
    runtimeRef.current = runtime;
    fitRuntime(runtime);
    const syncRemoteResize: TerminalResizeFn = (cols, rows) => {
      if (!Number.isInteger(cols) || !Number.isInteger(rows) || cols < 2 || rows < 2) return;
      const current = runtimeRef.current;
      if (!current) return;
      current.term.resize(cols, rows);
      lastSizeRef.current = { cols, rows };
    };
    const terminalGeneration = onRegisterRef.current(stableWriteRef.current, syncRemoteResize);
    const restoredRawLength = rawChunksRef.current?.join("").length ?? 0;

    const completeRestore = () => {
      finishQueuedWrites(runtime, restoredRawLength, 0, true, true, () => {
        onReadyRef.current?.(terminalGeneration);
      });
    };
    window.requestAnimationFrame(() => {
      fitRuntime(runtime);
      if (initialSnapshot) {
        runtime.term.write(initialSnapshot, () => {
          if (initialData) {
            runtime.writer.write(initialData, completeRestore);
            return;
          }
          completeRestore();
        });
        return;
      }
      if (initialData) {
        runtime.writer.write(initialData, completeRestore);
        return;
      }
      completeRestore();
    });

    const handleVisibilityChange = () => {
      if (document.visibilityState !== "visible") return;
      window.requestAnimationFrame(() => {
        const current = runtimeRef.current;
        if (!current) return;
        fitRuntime(current);
        current.focus();
      });
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);

    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    const resizeObserver = new ResizeObserver(() => {
      if (resizeTimer) window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        const current = runtimeRef.current;
        if (current) fitRuntime(current);
      }, 50);
    });
    resizeObserver.observe(container);

    return () => {
      disposed = true;
      try {
        const snapshot = runtimeRef.current?.serializeAddon.serialize();
        if (snapshot) onSnapshotRef.current?.(snapshot);
      } catch {
        // Terminal teardown must remain best-effort.
      }
      onRegisterRef.current(null);
      rebuildRef.current = null;
      if (resizeTimer) window.clearTimeout(resizeTimer);
      resizeObserver.disconnect();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      runtimeRef.current?.dispose();
      runtimeRef.current = null;
      rebuildingRef.current = true;
      const callbacks = pendingWriteCallbacks.splice(0);
      callbacks.forEach((callback) => callback?.());
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    function loadNewlineShortcut() {
      invoke<{ terminal_shift_enter_newline?: unknown }>("load_app_settings")
        .then((settings) => {
          shiftEnterNewlineRef.current = normalizeShiftEnterNewline(
            settings.terminal_shift_enter_newline,
          );
        })
        .catch(() => {
          shiftEnterNewlineRef.current = DEFAULT_SHIFT_ENTER_NEWLINE;
        });
    }
    loadNewlineShortcut();
    window.addEventListener("aeroric:app-settings-changed", loadNewlineShortcut);
    return () => window.removeEventListener("aeroric:app-settings-changed", loadNewlineShortcut);
  }, []);

  useEffect(() => {
    const runtime = runtimeRef.current;
    if (!runtime || !isActive) return;
    window.requestAnimationFrame(() => {
      const current = runtimeRef.current;
      const container = containerRef.current;
      if (!current || !container) return;
      const size = safeFit(current.fitAddon, current.term, container);
      if (size) notifyResize(size.cols, size.rows);
      current.focus();
    });
  }, [isActive, notifyResize]);

  useEffect(() => {
    if (runtimeRef.current) runtimeRef.current.term.options.cursorBlink = isActive;
  }, [isActive]);

  useEffect(() => {
    const runtime = runtimeRef.current;
    if (!runtime || runtime.theme === themeVariant) return;
    rebuildRef.current?.(themeVariant);
  }, [themeVariant]);

  useEffect(() => {
    const runtime = runtimeRef.current;
    const container = containerRef.current;
    if (!runtime || !container) return;
    const size = applyTerminalFontSize(runtime.term, runtime.fitAddon, terminalFontSize, container);
    if (size) notifyResize(size.cols, size.rows);
  }, [terminalFontSize, notifyResize]);

  useEffect(() => {
    const runtime = runtimeRef.current;
    const container = containerRef.current;
    if (!runtime || !container) return;
    const size = applyTerminalFontFamily(runtime.term, runtime.fitAddon, monoFontFamily, container);
    if (size) notifyResize(size.cols, size.rows);
  }, [monoFontFamily, notifyResize]);

  return (
    <div
      ref={containerRef}
      data-testid="agent-terminal"
      style={{
        width: "100%",
        height: "100%",
        minWidth: 0,
        minHeight: 0,
        overflow: "hidden",
        cursor: "text",
        background: themeFor(themeVariant).background,
      }}
    />
  );
}
