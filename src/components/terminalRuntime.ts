import type { FitAddon } from "@xterm/addon-fit";
import type { Terminal as XTerm } from "@xterm/xterm";
import {
  applyTerminalFontFamily,
  applyTerminalFontSize,
  applyTerminalTheme,
  attachMacWebKitTerminalGuard,
  createSmartWriter,
  fitTerminalAtBottom,
  initTerminal,
  loadWebglAddon,
  type InitTerminalResult,
} from "./terminalShared";
import {
  applyTerminalTextareaInputAttributes,
  attachLinuxIMEFix,
  attachMacWebKitShiftInputFix,
  attachWindowsIMEPositionFix,
} from "./terminalInputFix";
import { attachSmartCopy } from "./terminalCopyHelper";
import type { FontFamily, TerminalFontSize, ThemeVariant } from "../types";

export interface TerminalRuntimeSize {
  cols: number;
  rows: number;
}

export interface TerminalRuntimeOptions {
  container: HTMLElement;
  themeVariant: ThemeVariant;
  terminalFontSize: TerminalFontSize;
  monoFontFamily: FontFamily;
  isActive: () => boolean;
  onInput: (data: string) => void;
  onResize?: (size: TerminalRuntimeSize) => void;
  scrollback?: number;
}

export interface TerminalRuntime {
  term: XTerm;
  fitAddon: FitAddon;
  writer: ReturnType<typeof createSmartWriter>;
  fit: () => TerminalRuntimeSize | null;
  focus: () => void;
  updateTheme: (variant: ThemeVariant) => void;
  updateFontSize: (fontSize: TerminalFontSize) => TerminalRuntimeSize | null;
  updateFontFamily: (fontFamily: FontFamily) => TerminalRuntimeSize | null;
  dispose: () => void;
}

/**
 * Owns the browser-facing xterm lifecycle shared by local, SSH, and WSL panels.
 * Transport commands stay in the panel that created the runtime.
 */
export function createTerminalRuntime(options: TerminalRuntimeOptions): TerminalRuntime {
  const { container, isActive, onInput, onResize, scrollback = 5000 } = options;
  let currentThemeVariant = options.themeVariant;
  let disposed = false;
  let resizeFrame: number | null = null;
  let visibilityFrame: number | null = null;
  let lastSize: TerminalRuntimeSize | null = null;
  let lastObservedContainerSize: { width: number; height: number } | null = null;

  const terminalResult: InitTerminalResult = initTerminal(
    currentThemeVariant,
    scrollback,
    options.terminalFontSize,
    options.monoFontFamily,
  );
  const { term, fitAddon } = terminalResult;
  term.open(container);
  applyTerminalTextareaInputAttributes(term);
  const disposeInputFix = attachMacWebKitShiftInputFix(term);
  const disposeWindowsImeFix = attachWindowsIMEPositionFix(term);
  loadWebglAddon(term);
  const writer = createSmartWriter(term, () => currentThemeVariant, {
    resumeOnAnyOutput: true,
  });
  const disposeMacWebKitGuard = attachMacWebKitTerminalGuard({ term, container, writer });

  const sendInput = (data: string) => {
    if (disposed) return;
    writer.pauseForUserInput();
    onInput(data);
  };
  const disposeSmartCopy = attachSmartCopy(term, { onPaste: sendInput });
  const linuxIME = attachLinuxIMEFix(term, sendInput, {
    onCompositionStateChange: writer.setCompositionPaused,
  });

  const focus = () => {
    if (disposed) return;
    if (term.textarea?.disabled) term.textarea.disabled = false;
    term.focus();
    term.textarea?.focus({ preventScroll: true });
  };

  const recordSize = (size: TerminalRuntimeSize | null): TerminalRuntimeSize | null => {
    if (!size) return null;
    if (lastSize?.cols === size.cols && lastSize.rows === size.rows) return size;
    lastSize = size;
    onResize?.(size);
    return size;
  };

  const fit = (): TerminalRuntimeSize | null => {
    if (disposed) return null;
    return recordSize(fitTerminalAtBottom(fitAddon, term, container));
  };

  const scheduleFit = (entries?: ResizeObserverEntry[]) => {
    if (disposed || !isActive()) return;
    const rect = entries?.[0]?.contentRect;
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
      if (isActive()) {
        fit();
      } else {
        container.removeAttribute("data-terminal-resizing");
      }
    });
  };
  const resizeObserver = new ResizeObserver(scheduleFit);
  resizeObserver.observe(container);

  const handleVisibilityChange = () => {
    if (document.visibilityState !== "visible" || disposed || !isActive()) return;
    if (visibilityFrame !== null) window.cancelAnimationFrame(visibilityFrame);
    visibilityFrame = window.requestAnimationFrame(() => {
      visibilityFrame = null;
      if (disposed || !isActive()) return;
      fit();
      focus();
    });
  };
  document.addEventListener("visibilitychange", handleVisibilityChange);

  return {
    term,
    fitAddon,
    writer,
    fit,
    focus,
    updateTheme: (variant) => {
      if (disposed) return;
      currentThemeVariant = variant;
      applyTerminalTheme(term, variant);
    },
    updateFontSize: (fontSize) => {
      if (disposed) return null;
      const size = applyTerminalFontSize(term, fitAddon, fontSize, container);
      return recordSize(size);
    },
    updateFontFamily: (fontFamily) => {
      if (disposed) return null;
      const size = applyTerminalFontFamily(term, fitAddon, fontFamily, container);
      return recordSize(size);
    },
    dispose: () => {
      if (disposed) return;
      disposed = true;
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
      container.removeAttribute("data-terminal-resizing");
      if (visibilityFrame !== null) window.cancelAnimationFrame(visibilityFrame);
      resizeObserver.disconnect();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      disposeSmartCopy();
      linuxIME.dispose();
      disposeMacWebKitGuard();
      disposeInputFix();
      disposeWindowsImeFix();
      term.dispose();
    },
  };
}
