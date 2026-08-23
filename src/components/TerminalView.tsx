import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
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
  fitTerminalAtBottom,
  createSmartWriter,
  attachMacWebKitTerminalGuard,
  attachCursorLineHighlight,
  applyTerminalFontSize,
  applyTerminalFontFamily,
  attachTerminalWheelScroll,
} from "./terminalShared";
import type { TerminalResizeFn, TerminalWriteFn } from "../hooks/useTerminalManager";
import {
  createTerminalRingBuffer,
  joinTerminalBuffer,
  joinTerminalBufferFrom,
  pushTerminalChunk,
  terminalBufferAbsLength,
  type TerminalRingBuffer,
} from "../terminalRingBuffer";
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
  dshVariant?: boolean;
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
  dshVariant = false,
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
  // 主题重建 / 快照恢复要重放原始输出，所以这里保留一份镜像；用环形缓冲是因为长跑
  // 任务（尤其是 input_required 期间一直挂载着的终端）会无休止地往里追加。
  const rawBufferRef = useRef<TerminalRingBuffer | null>(null);

  if (rawBufferRef.current === null) {
    const buffer = createTerminalRingBuffer();
    const raw = rawReplayData ?? initialData ?? "";
    if (raw) pushTerminalChunk(buffer, raw);
    rawBufferRef.current = buffer;
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
      const buffer = rawBufferRef.current;
      if (buffer) pushTerminalChunk(buffer, data);
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
    const setTerminalRestoring = (restoring: boolean) => {
      if (restoring) {
        container.setAttribute("data-terminal-restoring", "true");
      } else {
        container.removeAttribute("data-terminal-restoring");
      }
    };
    setTerminalRestoring(Boolean(initialSnapshot || initialData));

    const createRuntime = (theme: ThemeVariant): TerminalRuntime => {
      const { term, fitAddon } = initTerminal(theme, 1000, terminalFontSize, monoFontFamily);
      // 只有 agent 终端需要这条兜底（shell 面板要留着 xterm 的 alternate scroll）。
      const wheelScroll = attachTerminalWheelScroll(term);
      const serializeAddon = new SerializeAddon();
      term.loadAddon(serializeAddon);
      term.open(container);
      applyTerminalTextareaInputAttributes(term);

      const writer = createSmartWriter(term, () => theme, {
        themeAwareAnsiRemap: true,
        resumeOnAnyOutput: true,
      });
      const disposeInputFix = attachMacWebKitShiftInputFix(term);
      const disposeWindowsImeFix = attachWindowsIMEPositionFix(term);
      const disposeMacWebKitGuard = attachMacWebKitTerminalGuard({ term, container, writer });
      const disposeCursorLineHighlight = highlightCursorLine
        ? attachCursorLineHighlight(term, container)
        : () => {};
      const sendInput = (data: string) => {
        // 滚轮上报不算"用户输入":暂停输出会把 agent 的滚动重绘往后推,而滚动期间每帧
        // 有好几条上报,累积起来就是可见的抖动。键入才需要这条让路逻辑。
        if (!wheelScroll.isReplayingWheel()) writer.pauseForUserInput();
        onInputRef.current(data);
      };
      const disposeSmartCopy = attachSmartCopy(term, {
        matchesNewline: (event) => matchesTerminalNewline(event, shiftEnterNewlineRef.current),
        onNewline: () => sendInput(TERMINAL_NEWLINE_SEQUENCE),
        onPaste: (text) => sendInput(text),
      });
      const linuxIme = attachLinuxIMEFix(term, sendInput, {
        onCompositionStateChange: writer.setCompositionPaused,
      });

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
          // 先停掉待发的滚轮帧:rAF 回调若打在已 dispose 的 term 上会抛。
          wheelScroll.dispose();
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
      const size = fitTerminalAtBottom(runtime.fitAddon, runtime.term, container);
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
      // renderedLength 是**绝对**偏移（含已被环形缓冲裁掉的部分），所以这里比较和
      // 续写都必须走绝对长度，不能用当前留存内容的 length。
      const buffer = rawBufferRef.current;
      const absLength = buffer ? terminalBufferAbsLength(buffer) : 0;
      if (buffer && absLength > renderedLength) {
        // 恢复期间新到的输出属于"补齐历史"的一部分，同样不要走帧预算。
        runtime.writer.writeImmediate(joinTerminalBufferFrom(buffer, renderedLength), () =>
          finishQueuedWrites(runtime, absLength, viewportY, wasAtBottom, hadFocus, onComplete),
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
      try {
        runtime.term.refresh(0, Math.max(0, runtime.term.rows - 1));
      } catch {
        // The renderer can disappear while a theme rebuild is being replaced.
      }

      const complete = () => {
        if (disposed || runtimeRef.current !== runtime) return;
        if (runtime.theme !== desiredThemeRef.current) {
          runtime.dispose();
          runtimeRef.current = null;
          rebuildingRef.current = false;
          rebuildRef.current?.(desiredThemeRef.current, onComplete);
          return;
        }
        setTerminalRestoring(false);
        if (hadFocus) runtime.focus();
        const callbacks = pendingWriteCallbacks.splice(0);
        callbacks.forEach((callback) => callback?.());
        onComplete?.();
      };

      if (container.hasAttribute("data-terminal-restoring")) {
        // xterm 的 write 回调表示解析已完成，但画布刷新仍可能排在下一帧。
        // 先定位到底部，再等一帧显示最终画面，避免暴露历史回放的中间滚动过程。
        window.requestAnimationFrame(complete);
      } else {
        complete();
      }
    };

    rebuildRef.current = (theme, onComplete) => {
      if (disposed || rebuildingRef.current) return;
      const previous = runtimeRef.current;
      const buffer = rawBufferRef.current;
      const raw = buffer ? joinTerminalBuffer(buffer) : "";
      // 与 raw 同一时刻取绝对长度：写入回调是异步的，届时缓冲可能已经又追加了内容。
      const rawAbsLength = buffer ? terminalBufferAbsLength(buffer) : 0;
      rebuildingRef.current = true;
      setTerminalRestoring(Boolean(raw));
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
        // 切换主题会重建 xterm 实例并重放整个缓冲，同样必须一次性灌入，
        // 否则换个主题就要再看一遍滚动动画。
        runtime.writer.writeImmediate(raw, () =>
          finishQueuedWrites(runtime, rawAbsLength, viewportY, wasAtBottom, hadFocus, onComplete),
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
    const restoredRawLength = rawBufferRef.current
      ? terminalBufferAbsLength(rawBufferRef.current)
      : 0;

    const completeRestore = () => {
      finishQueuedWrites(runtime, restoredRawLength, 0, true, true, () => {
        onReadyRef.current?.(terminalGeneration);
      });
    };
    window.requestAnimationFrame(() => {
      fitRuntime(runtime);
      // 历史恢复走 writeImmediate：逐帧写入会让用户看到一遍从中间滚到底部的回放动画
      // （没有 snapshot 的终端要补的是整个 8 MB 缓冲）。这里只要最终画面。
      if (initialSnapshot) {
        runtime.term.write(initialSnapshot, () => {
          if (initialData) {
            runtime.writer.writeImmediate(initialData, completeRestore);
            return;
          }
          completeRestore();
        });
        return;
      }
      if (initialData) {
        runtime.writer.writeImmediate(initialData, completeRestore);
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

    let resizeFrame: number | null = null;
    let lastObservedContainerSize: { width: number; height: number } | null = null;
    const resizeObserver = new ResizeObserver((entries) => {
      if (!isActiveRef.current) return;
      const rect = entries[0]?.contentRect;
      if (rect && Number.isFinite(rect.width) && Number.isFinite(rect.height)) {
        if (
          lastObservedContainerSize?.width === rect.width &&
          lastObservedContainerSize?.height === rect.height
        ) {
          return;
        }
        lastObservedContainerSize = { width: rect.width, height: rect.height };
      }
      container.setAttribute("data-terminal-resizing", "true");
      if (resizeFrame !== null) return;
      resizeFrame = window.requestAnimationFrame(() => {
        resizeFrame = null;
        const current = runtimeRef.current;
        if (current) fitRuntime(current);
      });
    });
    resizeObserver.observe(container);

    return () => {
      disposed = true;
      const runtime = runtimeRef.current;
      // xterm 的 write callback 之前，serialize 只能拿到旧画面或半个解析批次；若仍把
      // 当前 raw buffer 末尾记作 snapshot 偏移，下次打开就会跳过未渲染的 TUI 帧，
      // 后续相对光标更新只能叠在旧画面上，表现为文字散落/重叠。忙时保留上一个干净
      // snapshot（没有则回放 raw），不要制造一个与 buffer 偏移不一致的新 snapshot。
      if (!rebuildingRef.current && runtime?.writer.isIdle()) {
        try {
          const snapshot = runtime.serializeAddon.serialize();
          if (snapshot) onSnapshotRef.current?.(snapshot);
        } catch {
          // Terminal teardown must remain best-effort.
        }
      }
      onRegisterRef.current(null);
      rebuildRef.current = null;
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
      container.removeAttribute("data-terminal-resizing");
      resizeObserver.disconnect();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      runtimeRef.current?.dispose();
      runtimeRef.current = null;
      rebuildingRef.current = true;
      setTerminalRestoring(false);
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

  useLayoutEffect(() => {
    const runtime = runtimeRef.current;
    if (!runtime || !isActive) return;
    const container = containerRef.current;
    if (!container) return;

    // React can reveal an already-mounted terminal before its activation effect runs.
    // Close that one-frame gap so the first painted frame is already bottom-anchored.
    container.setAttribute("data-terminal-activating", "true");
    let revealFrame: number | null = null;
    const fitFrame = window.requestAnimationFrame(() => {
      const current = runtimeRef.current;
      if (!current || !isActiveRef.current) {
        container.removeAttribute("data-terminal-activating");
        return;
      }
      const size = fitTerminalAtBottom(current.fitAddon, current.term, container);
      if (size) notifyResize(size.cols, size.rows);
      revealFrame = window.requestAnimationFrame(() => {
        container.removeAttribute("data-terminal-activating");
        runtimeRef.current?.focus();
      });
    });

    return () => {
      window.cancelAnimationFrame(fitFrame);
      if (revealFrame !== null) window.cancelAnimationFrame(revealFrame);
      container.removeAttribute("data-terminal-activating");
    };
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
      data-terminal-theme={themeVariant}
      data-terminal-variant={dshVariant ? "dsh" : "native"}
      className={dshVariant ? "dsh-terminal-surface" : undefined}
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
