import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { IS_MAC_WEBKIT } from "../platform";
import {
  applyTerminalTextareaInputAttributes,
  beginTerminalTextareaInternalFocusReset,
  endTerminalTextareaInternalFocusReset,
} from "./terminalInputFix";
import type { ThemeVariant } from "../types";

// ── Theme ────────────────────────────────────────────────────────────────────

export const DARK_THEME = {
  background: "rgba(6, 8, 10, 0.94)",
  foreground: "#d6dce8",
  cursor: "#528bff",
  selectionBackground: "#1f4662",
  black: "#1b1d23",
  red: "#e06c75",
  green: "#98c379",
  yellow: "#e5c07b",
  blue: "#61afef",
  magenta: "#c678dd",
  cyan: "#56b6c2",
  white: "#b8c0ce",
  brightBlack: "#7b8494",
  brightRed: "#e88388",
  brightGreen: "#b0d48c",
  brightYellow: "#f0cf8c",
  brightBlue: "#79c0ff",
  brightMagenta: "#d19aee",
  brightCyan: "#6fc5d0",
  brightWhite: "#eef1f7",
};

export const LIGHT_THEME = {
  background: "#ffffff",
  foreground: "#24292f",
  cursor: "#24292f",
  selectionBackground: "#b3d7ff",
  black: "#24292f",
  red: "#cf222e",
  green: "#116329",
  yellow: "#9a6700",
  blue: "#0550ae",
  magenta: "#8250df",
  cyan: "#1b7c83",
  white: "#3f3f46",
  brightBlack: "#57606a",
  brightRed: "#a40e26",
  brightGreen: "#1a7f37",
  brightYellow: "#633c01",
  brightBlue: "#0969da",
  brightMagenta: "#6639ba",
  brightCyan: "#3192aa",
  brightWhite: "#24292f",
};

// Solarized Light–inspired warm palette to match the eyecare CSS tokens.
export const EYECARE_THEME = {
  background: "rgba(253, 246, 227, 0.9)",
  foreground: "#586e75",
  cursor: "#586e75",
  selectionBackground: "#eee8d5",
  black: "#073642",
  red: "#dc322f",
  green: "#859900",
  yellow: "#b58900",
  blue: "#268bd2",
  magenta: "#d33682",
  cyan: "#2aa198",
  white: "#657b83",
  brightBlack: "#657b83",
  brightRed: "#cb4b16",
  brightGreen: "#586e75",
  brightYellow: "#657b83",
  brightBlue: "#839496",
  brightMagenta: "#6c71c4",
  brightCyan: "#93a1a1",
  brightWhite: "#3f3724",
};

export function themeFor(variant: ThemeVariant) {
  if (variant === "dark") return DARK_THEME;
  if (variant === "eyecare") return EYECARE_THEME;
  return LIGHT_THEME;
}

export function terminalMinimumContrastRatioForTheme(variant: ThemeVariant): number {
  return variant === "light" ? 4.5 : 1;
}

export function applyTerminalTheme(term: Terminal, variant: ThemeVariant): void {
  term.options.theme = themeFor(variant);
  term.options.minimumContrastRatio = terminalMinimumContrastRatioForTheme(variant);
}

const TERMINAL_INPUT_BACKGROUND_RGB: Record<ThemeVariant, readonly [number, number, number]> = {
  light: [241, 243, 245],
  dark: [17, 21, 26],
  eyecare: [238, 232, 213],
};

export function terminalInputBackgroundForTheme(variant: ThemeVariant): string {
  if (variant === "dark") return "#11151a";
  if (variant === "eyecare") return "#eee8d5";
  return "#f1f3f5";
}

// ── Watermark flow control ───────────────────────────────────────────────────

const HIGH_WATER = 96 * 1024; // 96 KB：超过时停止写入
const LOW_WATER = 16 * 1024; // 16 KB：恢复写入
export const TERMINAL_WRITE_CHUNK_SIZE = 16 * 1024;
export const TERMINAL_FRAME_WRITE_BUDGET = 32 * 1024;
export const TERMINAL_USER_INPUT_PAUSE_MS = 48;
/**
 * 本地 viewport 滚动的插值时长。
 *
 * 100ms 左右是"看得出是连续位移、又不会觉得画面追不上手"的区间；再长滚动会显得拖沓，
 * 再短就退化回逐行跳。只影响 xterm 自己滚 scrollback 的场景，见 `initTerminal`。
 */
export const TERMINAL_SMOOTH_SCROLL_MS = 100;
const ANSI_FG_RESET = "\x1b[39m";
const TERMINAL_HIGHLIGHT_PATTERN =
  /\b(error|exception|traceback|failed|fail|warning|warn|success|passed|pass|running|done)\b|\b\d+(?:\.\d+)?(?:%|ms|s|MB|GB|KB)?\b/gi;

function hasTerminalControlSequence(data: string): boolean {
  for (let index = 0; index < data.length; index += 1) {
    const code = data.charCodeAt(index);
    if (code === 0x1b || code === 0x9b || code === 0x07 || code === 0x08) return true;
  }
  return false;
}

export function colorizePlainTerminalOutput(data: string): string {
  if (!data || hasTerminalControlSequence(data)) return data;
  return data.replace(TERMINAL_HIGHLIGHT_PATTERN, (match) => {
    const lower = match.toLowerCase();
    if (
      lower === "error" ||
      lower === "exception" ||
      lower === "traceback" ||
      lower === "failed" ||
      lower === "fail"
    ) {
      return `\x1b[31m${match}${ANSI_FG_RESET}`;
    }
    if (lower === "warning" || lower === "warn") {
      return `\x1b[33m${match}${ANSI_FG_RESET}`;
    }
    if (lower === "success" || lower === "passed" || lower === "pass" || lower === "done") {
      return `\x1b[32m${match}${ANSI_FG_RESET}`;
    }
    if (lower === "running") {
      return `\x1b[35m${match}${ANSI_FG_RESET}`;
    }
    return `\x1b[36m${match}${ANSI_FG_RESET}`;
  });
}

function isSgrBody(body: string): boolean {
  for (let index = 0; index < body.length; index += 1) {
    const code = body.charCodeAt(index);
    if (code !== 0x3b && (code < 0x30 || code > 0x39)) return false;
  }
  return true;
}

function pushTerminalInputBackground(
  next: string[],
  variant: ThemeVariant,
  explicitInputBackground: boolean,
): void {
  if (!explicitInputBackground) {
    next.push("49");
    return;
  }
  const [red, green, blue] = TERMINAL_INPUT_BACKGROUND_RGB[variant];
  next.push("48", "2", String(red), String(green), String(blue));
}

function remapAnsiSgrBody(
  body: string,
  variant: ThemeVariant,
  explicitInputBackground: boolean,
): string | null {
  const parts = body ? body.split(";") : [];
  const next: string[] = [];
  let changed = false;

  for (let index = 0; index < parts.length; index += 1) {
    const code = parts[index];
    if (code === "40" || code === "100") {
      pushTerminalInputBackground(next, variant, explicitInputBackground);
      changed = true;
      continue;
    }
    if (variant !== "dark" && (code === "41" || code === "101")) {
      next.push("48", "2", "255", "235", "233");
      changed = true;
      continue;
    }
    if (variant !== "dark" && (code === "42" || code === "102")) {
      next.push("48", "2", "218", "251", "225");
      changed = true;
      continue;
    }

    if (code === "37" || code === "97") {
      next.push("39");
      changed = true;
      continue;
    }

    if (code === "38" && parts[index + 1] === "2") {
      const red = Number(parts[index + 2]);
      const green = Number(parts[index + 3]);
      const blue = Number(parts[index + 4]);
      const neutral = Math.max(red, green, blue) - Math.min(red, green, blue) <= 18;
      if (
        (red >= 235 && green >= 235 && blue >= 235) ||
        (neutral && red >= 150 && green >= 150 && blue >= 150)
      ) {
        next.push("39");
        index += 4;
        changed = true;
        continue;
      }
      if (index + 4 < parts.length) {
        next.push(...parts.slice(index, index + 5));
        index += 4;
        continue;
      }
    }

    if (code === "38" && parts[index + 1] === "5") {
      const color = Number(parts[index + 2]);
      if (color === 15 || color >= 231) {
        next.push("39");
        index += 2;
        changed = true;
        continue;
      }
      if (index + 2 < parts.length) {
        next.push(...parts.slice(index, index + 3));
        index += 2;
        continue;
      }
    }

    if (code === "48" && parts[index + 1] === "2") {
      const red = Number(parts[index + 2]);
      const green = Number(parts[index + 3]);
      const blue = Number(parts[index + 4]);
      const max = Math.max(red, green, blue);
      const min = Math.min(red, green, blue);
      if (max - min <= 18) {
        pushTerminalInputBackground(next, variant, explicitInputBackground);
        index += 4;
        changed = true;
        continue;
      }
      if (variant !== "dark" && max <= 110) {
        const color =
          red > green * 1.18 && red > blue * 1.18
            ? ["255", "235", "233"]
            : green > red * 1.18 && green > blue * 1.08
              ? ["218", "251", "225"]
              : null;
        if (!color) {
          next.push(...parts.slice(index, index + 5));
          index += 4;
          continue;
        }
        next.push("48", "2", ...color);
        index += 4;
        changed = true;
        continue;
      }
      if (index + 4 < parts.length) {
        next.push(...parts.slice(index, index + 5));
        index += 4;
        continue;
      }
    }

    if (code === "48" && parts[index + 1] === "5") {
      const color = Number(parts[index + 2]);
      const replacement =
        variant === "dark"
          ? null
          : [1, 9, 52, 88, 124].includes(color)
            ? ["255", "235", "233"]
            : [2, 10, 22, 28, 34].includes(color)
              ? ["218", "251", "225"]
              : null;
      if (replacement) {
        next.push("48", "2", ...replacement);
        index += 2;
        changed = true;
        continue;
      }
      if ([0, 7, 8, 15, 16].includes(color) || color >= 231) {
        pushTerminalInputBackground(next, variant, explicitInputBackground);
        index += 2;
        changed = true;
        continue;
      }
      if (index + 2 < parts.length) {
        next.push(...parts.slice(index, index + 3));
        index += 2;
        continue;
      }
    }

    if (code === "58" && parts[index + 1] === "2" && index + 4 < parts.length) {
      next.push(...parts.slice(index, index + 5));
      index += 4;
      continue;
    }

    if (code === "58" && parts[index + 1] === "5" && index + 2 < parts.length) {
      next.push(...parts.slice(index, index + 3));
      index += 2;
      continue;
    }

    next.push(code);
  }

  return changed ? next.join(";") : null;
}

function remapAnsi(data: string, variant: ThemeVariant, explicitInputBackground: boolean): string {
  if (!data || !data.includes("\x1b[")) return data;
  let result = "";
  let last = 0;

  for (let index = 0; index < data.length; index += 1) {
    if (data.charCodeAt(index) !== 0x1b || data[index + 1] !== "[") continue;
    let end = index + 2;
    while (end < data.length && data[end] !== "m") end += 1;
    if (end >= data.length) break;

    const body = data.slice(index + 2, end);
    if (!isSgrBody(body)) continue;

    const remapped = remapAnsiSgrBody(body, variant, explicitInputBackground);
    if (remapped === null) continue;

    result += data.slice(last, index);
    result += `\x1b[${remapped}m`;
    last = end + 1;
    index = end;
  }

  return last === 0 ? data : result + data.slice(last);
}

export function remapAnsiForTheme(data: string, variant: ThemeVariant): string {
  return remapAnsi(data, variant, true);
}

export function remapLightAnsiForeground(data: string, variant: ThemeVariant): string {
  if (variant === "dark") return data;
  return remapAnsi(data, variant, false);
}

export interface SmartWriter {
  write: (data: string, callback?: () => void) => void;
  /**
   * 一次性灌入历史回放，绕过每帧字节预算。
   *
   * 逐帧写入是为了给实时输出留出渲染时间，但用在恢复历史上会把"补齐 N MB 缓冲"
   * 变成用户肉眼可见的、从中间一路滚到底部的动画（8 MB / 32 KB 每帧 ≈ 250 帧）。
   * 恢复历史时用户要的只是最终画面，直接交给 xterm 自己的写队列。
   */
  writeImmediate: (data: string, callback?: () => void) => void;
  drainPending: () => void;
  setSelectionPaused: (paused: boolean) => void;
  setCompositionPaused: (paused: boolean) => void;
  pauseForUserInput: (durationMs?: number) => void;
  isIdle: () => boolean;
}

interface SmartWriterOptions {
  resumeOnAnyOutput?: boolean;
  themeAwareAnsiRemap?: boolean;
  highlightPlainOutput?: boolean;
}

interface TerminalSelectionGuardOptions {
  term: Terminal;
  container: HTMLElement;
  writer?: Pick<SmartWriter, "setSelectionPaused">;
}

function setMacWebKitTextareaAttrs(term: Terminal): void {
  applyTerminalTextareaInputAttributes(term);
}

// macOS WKWebView 在 xterm 选区拖动期间会被 NSTextInputClient 持续查询
// characterIndexForPoint，触发 LocalFrame::rangeForPoint → ICU 簇分析，
// 主线程被打满。
//
// 修复：拖动期间把 textarea 设 disabled——NSTextInputContext 没有可接收 focus
// 的 text input 就不查询，hit-test 风暴断在源头。松手 enable 后 refocus，
// 普通字符 / IME 输入照常。社区先例：xterm.js Discussion #5227。
//
// 历史：
// - 曾经基于 inert 把终端外的 sibling 子树标为不可命中（试图阻断 NSTextInput
//   hit-test 遍历）。2026-05-25 sample 实证 inert 只改变交互语义，不改变
//   RenderText 在 layout tree 的存在，hit-test 照样遍历，已删。
// - 曾用 textarea.blur()。2026-05-27 用户 A/B 实测拼音卡 / 英文不卡，印证 IME
//   路径是真因；blur 后 textarea 仍 focusable（可能被 RAF / 内部回调夺回焦点），
//   改为 disabled 是硬性禁用，更彻底。
// - 曾叠加 user-select:none 抑制 + window.getSelection().removeAllRanges() +
//   TERMINAL_SELECTION_ACTIVE_EVENT 广播给 RunningView/useUsageSnapshot 暂停
//   IPC 轮询。2026-05-27 disabled 升级实测拼音不卡，旁支防御全部移除。
export function attachMacWebKitTerminalGuard({
  term,
  container,
  writer,
}: TerminalSelectionGuardOptions): () => void {
  if (!IS_MAC_WEBKIT) return () => {};

  setMacWebKitTextareaAttrs(term);

  let pointerSelecting = false;
  let terminalHasSelection = term.hasSelection();
  let textareaDisabledBySelectionGuard = false;
  let internalFocusResetActive = false;

  const beginInternalFocusReset = () => {
    if (!term.textarea || internalFocusResetActive) return;
    internalFocusResetActive = true;
    beginTerminalTextareaInternalFocusReset(term.textarea);
  };

  const endInternalFocusReset = () => {
    if (!term.textarea || !internalFocusResetActive) return;
    internalFocusResetActive = false;
    endTerminalTextareaInternalFocusReset(term.textarea);
  };

  // 拖选期间用 disabled 切断 IME host：
  // - blur: textarea 仍 focusable，后续 RAF / 内部回调可能把焦点夺回，IME 又能查
  // - disabled: 硬性禁用接收 focus / input，IME 100% 无法发起 NSTextInputClient 查询
  // 参考：xterm.js Discussion #5227（社区实战验证）。
  const disableTextarea = () => {
    if (term.textarea && !term.textarea.disabled) {
      beginInternalFocusReset();
      textareaDisabledBySelectionGuard = true;
      term.textarea.disabled = true;
    }
  };

  const enableTextarea = () => {
    if (term.textarea && textareaDisabledBySelectionGuard) {
      term.textarea.disabled = false;
      textareaDisabledBySelectionGuard = false;
    }
  };

  const refocusTextarea = () => {
    try {
      if (term.textarea && !term.textarea.disabled) {
        term.textarea.focus({ preventScroll: true });
      }
    } finally {
      endInternalFocusReset();
    }
  };

  const syncSelectionGuard = () => {
    if (pointerSelecting) disableTextarea();
    else enableTextarea();
  };

  const handlePointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    pointerSelecting = true;
    writer?.setSelectionPaused(true);
    syncSelectionGuard();
  };

  const handlePointerUp = (e: PointerEvent) => {
    if (e.button !== 0) return;
    // document 级监听：必须先确认是终端发起的拖选流程，否则会把别处输入框的焦点抢走。
    if (!pointerSelecting) return;
    pointerSelecting = false;
    writer?.setSelectionPaused(false);
    terminalHasSelection = term.hasSelection();
    syncSelectionGuard();
    refocusTextarea();
  };

  const handlePointerCancel = () => {
    if (!pointerSelecting) return;
    pointerSelecting = false;
    writer?.setSelectionPaused(false);
    terminalHasSelection = term.hasSelection();
    syncSelectionGuard();
    refocusTextarea();
  };

  const handleDocumentPointerDown = (e: PointerEvent) => {
    const target = e.target;
    if (!terminalHasSelection || (target instanceof Node && container.contains(target))) return;
    pointerSelecting = false;
    terminalHasSelection = false;
    writer?.setSelectionPaused(false);
    term.clearSelection();
    syncSelectionGuard();
    // 用户点了终端外部，焦点本来就该去那里，不强抢回 textarea。
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || !terminalHasSelection) return;
    pointerSelecting = false;
    terminalHasSelection = false;
    writer?.setSelectionPaused(false);
    term.clearSelection();
    syncSelectionGuard();
    refocusTextarea();
  };

  const selectionDisposable = term.onSelectionChange(() => {
    terminalHasSelection = term.hasSelection();
    syncSelectionGuard();
  });

  container.addEventListener("pointerdown", handlePointerDown);
  document.addEventListener("pointerup", handlePointerUp);
  document.addEventListener("pointercancel", handlePointerCancel);
  document.addEventListener("pointerdown", handleDocumentPointerDown, true);
  document.addEventListener("keydown", handleKeyDown, true);

  return () => {
    selectionDisposable.dispose();
    container.removeEventListener("pointerdown", handlePointerDown);
    document.removeEventListener("pointerup", handlePointerUp);
    document.removeEventListener("pointercancel", handlePointerCancel);
    document.removeEventListener("pointerdown", handleDocumentPointerDown, true);
    document.removeEventListener("keydown", handleKeyDown, true);
    // 兜底：若卸载时仍处于选区拖动状态，恢复 textarea，避免下次输入丢失。
    enableTextarea();
    endInternalFocusReset();
    writer?.setSelectionPaused(false);
  };
}

// Agent TUI 会把光标停在自己的输入框行。保留一个透明的行定位层，
// 让光标位置跟随 xterm 的光标 Y，但不遮挡输入文字。
export function attachCursorLineHighlight(term: Terminal, container: HTMLElement): () => void {
  let overlay: HTMLDivElement | null = null;
  let rafId: ReturnType<typeof globalThis.setTimeout> | number | null = null;

  const getScreen = () => container.querySelector<HTMLElement>(".xterm-screen");
  const getThemeVariant = (): ThemeVariant => {
    const theme = container.dataset.terminalTheme;
    return theme === "dark" || theme === "eyecare" ? theme : "light";
  };

  const ensureOverlay = (): HTMLDivElement | null => {
    if (overlay && overlay.isConnected) return overlay;
    const screen = getScreen();
    if (!screen) return null;
    overlay = document.createElement("div");
    overlay.className = "aeroric-cursor-line";
    // 作为 screen 的第一个子节点插入；CSS 的 z-index:2 会把它提升到渲染 canvas
    // 之上，同时保持在 z-index:5 的 IME helpers 之下。
    screen.insertBefore(overlay, screen.firstChild);
    return overlay;
  };

  const syncTheme = (el: HTMLDivElement) => {
    const theme = getThemeVariant();
    el.dataset.terminalTheme = theme;
    el.style.background = "transparent";
  };

  const render = () => {
    const el = ensureOverlay();
    const screen = getScreen();
    if (!el || !screen) return;
    syncTheme(el);
    const rows = Math.max(1, term.rows);
    const rowHeight = screen.clientHeight / rows;
    if (!(rowHeight > 0)) {
      el.style.display = "none";
      return;
    }
    const cursorY = Math.max(0, Math.min(rows - 1, term.buffer.active.cursorY));
    el.style.display = "block";
    el.style.height = `${rowHeight}px`;
    el.style.transform = `translateY(${cursorY * rowHeight}px)`;
  };

  const cancelScheduledRender = () => {
    if (rafId === null) return;
    if (typeof window !== "undefined" && typeof window.cancelAnimationFrame === "function") {
      window.cancelAnimationFrame(rafId as number);
    } else {
      globalThis.clearTimeout(rafId as ReturnType<typeof globalThis.setTimeout>);
    }
    rafId = null;
  };

  // onRender 频率高，用 rAF 合并；光标移动 / 尺寸变化频率低，直接渲染，保证响应。
  const scheduleRender = () => {
    if (rafId !== null) return;
    if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
      rafId = window.requestAnimationFrame(() => {
        rafId = null;
        render();
      });
    } else {
      rafId = globalThis.setTimeout(() => {
        rafId = null;
        render();
      }, 16);
    }
  };

  const cursorDisposable = term.onCursorMove(render);
  const renderDisposable = term.onRender(scheduleRender);
  const resizeDisposable = term.onResize(render);
  const themeObserver =
    typeof MutationObserver === "undefined"
      ? null
      : new MutationObserver(() => {
          if (overlay) syncTheme(overlay);
        });
  themeObserver?.observe(container, {
    attributes: true,
    attributeFilter: ["data-terminal-theme"],
  });
  render();

  return () => {
    cancelScheduledRender();
    cursorDisposable.dispose();
    renderDisposable.dispose();
    resizeDisposable.dispose();
    themeObserver?.disconnect();
    overlay?.remove();
    overlay = null;
  };
}

type TerminalSequenceState = "ground" | "esc" | "csi" | "string" | "stringEsc";

function isCsiFinal(code: number): boolean {
  return code >= 0x40 && code <= 0x7e;
}

function isTerminalStringTerminator(code: number): boolean {
  return code === 0x07 || code === 0x9c;
}

function avoidTrailingHighSurrogate(data: string, start: number, end: number): number {
  if (end <= start || end >= data.length) return end;
  const prevCode = data.charCodeAt(end - 1);
  const nextCode = data.charCodeAt(end);
  if (prevCode >= 0xd800 && prevCode <= 0xdbff && nextCode >= 0xdc00 && nextCode <= 0xdfff) {
    return end - 1;
  }
  return end;
}

function findSafeTerminalChunkEnd(data: string, start: number, preferredEnd: number): number {
  let state: TerminalSequenceState = "ground";
  let sequenceStart = -1;

  const scanUntil = (limit: number): boolean => {
    for (let index = start; index < limit; index += 1) {
      const code = data.charCodeAt(index);
      const char = data[index];

      if (state === "ground") {
        if (code === 0x1b) {
          state = "esc";
          sequenceStart = index;
          continue;
        }
        if (code === 0x9b) {
          state = "csi";
          sequenceStart = index;
          continue;
        }
        if (code === 0x9d || code === 0x90 || code === 0x9e || code === 0x9f) {
          state = "string";
          sequenceStart = index;
          continue;
        }
        continue;
      }

      if (state === "esc") {
        if (char === "[") {
          state = "csi";
          continue;
        }
        if (char === "]" || char === "P" || char === "^" || char === "_") {
          state = "string";
          continue;
        }
        state = "ground";
        sequenceStart = -1;
        continue;
      }

      if (state === "csi") {
        if (isCsiFinal(code)) {
          state = "ground";
          sequenceStart = -1;
        }
        continue;
      }

      if (state === "string") {
        if (isTerminalStringTerminator(code)) {
          state = "ground";
          sequenceStart = -1;
          continue;
        }
        if (code === 0x1b) {
          state = "stringEsc";
        }
        continue;
      }

      if (state === "stringEsc") {
        if (char === "\\") {
          state = "ground";
          sequenceStart = -1;
          continue;
        }
        state = "string";
      }
    }
    return state === "ground";
  };

  if (scanUntil(preferredEnd)) {
    return avoidTrailingHighSurrogate(data, start, preferredEnd);
  }

  if (sequenceStart > start) {
    return avoidTrailingHighSurrogate(data, start, sequenceStart);
  }

  let extendState = state as TerminalSequenceState;
  for (let index = preferredEnd; index < data.length; index += 1) {
    const code = data.charCodeAt(index);
    const char = data[index];
    if (extendState === "esc") {
      if (char === "[") extendState = "csi";
      else if (char === "]" || char === "P" || char === "^" || char === "_") extendState = "string";
      else return index + 1;
      continue;
    }
    if (extendState === "csi") {
      if (isCsiFinal(code)) return index + 1;
      continue;
    }
    if (extendState === "string") {
      if (isTerminalStringTerminator(code)) return index + 1;
      if (code === 0x1b) extendState = "stringEsc";
      continue;
    }
    if (extendState === "stringEsc") {
      if (char === "\\") return index + 1;
      extendState = "string";
    }
  }

  return data.length;
}

export function splitTerminalWriteChunk(
  data: string,
  maxChunkSize = TERMINAL_WRITE_CHUNK_SIZE,
): string[] {
  if (data.length <= maxChunkSize) return [data];
  const chunks: string[] = [];
  let start = 0;
  while (start < data.length) {
    const preferredEnd = Math.min(start + maxChunkSize, data.length);
    let end = findSafeTerminalChunkEnd(data, start, preferredEnd);
    if (end <= start) end = preferredEnd;
    chunks.push(data.slice(start, end));
    start = end;
  }
  return chunks;
}

function nowMs(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

function scheduleFrame(callback: FrameRequestCallback): void {
  if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
    window.requestAnimationFrame(callback);
    return;
  }
  globalThis.setTimeout(() => callback(nowMs()), 16);
}

function refreshTerminalCursorLine(term: Terminal): void {
  try {
    const cursorY = term.buffer.active.cursorY;
    term.refresh(cursorY, cursorY);
  } catch {
    /* xterm refresh is a best-effort repaint hint. */
  }
}

/**
 * 创建基于水位线的流控写入器。
 *
 * - 当 xterm write queue 积累超过 HIGH_WATER 时暂停写入
 * - 低于 LOW_WATER 时恢复
 * - selectionPaused 在鼠标选择期间暂停写入（可选使用）
 */
export function createSmartWriter(
  term: Terminal,
  getThemeVariant: () => ThemeVariant = () => "dark",
  options: SmartWriterOptions = {},
): SmartWriter {
  const shouldHighlightPlainOutput = options.highlightPlainOutput ?? !options.themeAwareAnsiRemap;
  const state = {
    pendingChunks: [] as Array<{ data: string; callback?: () => void }>,
    watermark: 0,
    immediateWrites: 0,
    paused: false,
    selectionPaused: false,
    compositionPaused: false,
    inputPausedUntil: 0,
    drainScheduled: false,
    sawTerminalControl: false,
  };

  function transformOutput(data: string): string {
    // PTY read / IPC batching boundaries are arbitrary and can split one CSI
    // sequence into "\x1b[" and "12;40H". Once a stream has shown terminal
    // control, keep later control-free chunks raw: inserting highlight SGR into
    // the continuation would abort cursor addressing and scatter TUI fragments.
    if (hasTerminalControlSequence(data) || data.includes("\r")) {
      state.sawTerminalControl = true;
    }
    const highlighted =
      shouldHighlightPlainOutput && !state.sawTerminalControl
        ? colorizePlainTerminalOutput(data)
        : data;
    const remapOutput = options.themeAwareAnsiRemap ? remapAnsiForTheme : remapLightAnsiForeground;
    return remapOutput(highlighted, getThemeVariant());
  }

  function flushOne(data: string, callback?: () => void) {
    state.watermark += data.length;
    term.write(data, () => {
      state.watermark -= data.length;
      callback?.();
      if (state.paused && state.watermark < LOW_WATER) {
        state.paused = false;
        scheduleDrain();
      }
    });
  }

  function scheduleDrain(delayMs = 0) {
    if (state.drainScheduled) return;
    state.drainScheduled = true;
    const run = () => {
      state.drainScheduled = false;
      drainPending();
    };
    if (delayMs > 0) {
      globalThis.setTimeout(run, delayMs);
      return;
    }
    scheduleFrame(run);
  }

  function drainPending() {
    if (state.selectionPaused || state.compositionPaused) return;
    const scheduling = (
      globalThis.navigator as Navigator & {
        scheduling?: { isInputPending?: () => boolean };
      }
    )?.scheduling;
    if (scheduling?.isInputPending?.()) {
      scheduleDrain();
      return;
    }
    const inputPauseRemaining = state.inputPausedUntil - nowMs();
    if (inputPauseRemaining > 0) {
      scheduleDrain(inputPauseRemaining);
      return;
    }

    let bytesThisFrame = 0;
    while (
      state.pendingChunks.length > 0 &&
      !state.paused &&
      !state.selectionPaused &&
      !state.compositionPaused
    ) {
      const next = state.pendingChunks.shift()!;
      if (state.watermark >= HIGH_WATER) {
        state.pendingChunks.unshift(next);
        state.paused = true;
        break;
      }
      flushOne(next.data, next.callback);
      bytesThisFrame += next.data.length;
      if (bytesThisFrame >= TERMINAL_FRAME_WRITE_BUDGET) {
        break;
      }
    }
    if (
      state.pendingChunks.length > 0 &&
      !state.paused &&
      !state.selectionPaused &&
      !state.compositionPaused
    ) {
      scheduleDrain();
    }
  }

  function write(data: string, callback?: () => void) {
    const hasInteractiveControl =
      data.includes("\u001b") || data.includes("\r") || data.includes("\b");
    if (state.inputPausedUntil > nowMs() && (hasInteractiveControl || options.resumeOnAnyOutput)) {
      state.inputPausedUntil = 0;
    }
    const output = transformOutput(data);
    const chunks = splitTerminalWriteChunk(output);
    for (let index = 0; index < chunks.length; index += 1) {
      state.pendingChunks.push({
        data: chunks[index],
        callback: index === chunks.length - 1 ? callback : undefined,
      });
    }
    if (state.watermark >= HIGH_WATER) state.paused = true;
    drainPending();
  }

  function writeImmediate(data: string, callback?: () => void) {
    if (!data) {
      callback?.();
      return;
    }
    const output = transformOutput(data);
    // 仍按 ANSI 边界切块（xterm 对超大单块写入不友好），但不受帧预算限制，
    // 由 xterm 自己的写队列在同一批里消化完。
    const chunks = splitTerminalWriteChunk(output);
    state.immediateWrites += 1;
    for (let index = 0; index < chunks.length; index += 1) {
      const isLast = index === chunks.length - 1;
      term.write(
        chunks[index],
        isLast
          ? () => {
              state.immediateWrites -= 1;
              callback?.();
            }
          : undefined,
      );
    }
  }

  function setSelectionPaused(paused: boolean) {
    state.selectionPaused = paused;
    if (!paused) scheduleDrain();
  }

  function setCompositionPaused(paused: boolean) {
    state.compositionPaused = paused;
    if (!paused) scheduleDrain();
  }

  function pauseForUserInput(durationMs = TERMINAL_USER_INPUT_PAUSE_MS) {
    state.inputPausedUntil = Math.max(state.inputPausedUntil, nowMs() + durationMs);
    refreshTerminalCursorLine(term);
    scheduleFrame(() => refreshTerminalCursorLine(term));
    if (state.pendingChunks.length > 0) scheduleDrain(durationMs);
  }

  function isIdle() {
    return state.pendingChunks.length === 0 && state.watermark === 0 && state.immediateWrites === 0;
  }

  return {
    write,
    writeImmediate,
    drainPending,
    setSelectionPaused,
    setCompositionPaused,
    pauseForUserInput,
    isIdle,
  };
}

// ── Wheel policy ─────────────────────────────────────────────────────────────

// 只有这三种协议会真的向程序上报滚轮（CoreMouseService 的 DEFAULT_PROTOCOLS）。
// x10 不算：它的 events 只有 DOWN，restrict 直接拒掉 WHEEL。
const APP_WHEEL_MOUSE_MODES: ReadonlySet<string> = new Set(["vt200", "drag", "any"]);

/**
 * 一次 DOM 滚轮事件最多放大成几行上报，按屏数算。
 *
 * macOS 的惯性甩动能在一个事件里给出上千 px 的 deltaY，不设上限就会一次往 pty 写
 * 几百条鼠标序列。三屏足够覆盖正常的大幅翻页，又不会把管道灌爆。
 */
const MAX_WHEEL_LINES_PER_EVENT_SCREENS = 3;

/**
 * 每帧最多向程序发几条滚轮上报。
 *
 * 为什么要限：一条上报就是 agent 的一次整屏重绘。原来一次手势把全部行数**同步**灌进
 * pty（实测一个普通档位 7 条、一次惯性甩动 72 条），agent 只能逐条重绘、逐屏回吐,
 * 前端拿到的是一长串已经过时的中间态 —— 症状就是"手停了画面还在追"。
 *
 * 改成按帧分摊后，每帧只发这么多条，agent 的回吐与我们的发送交错进行，画面每帧都在动。
 * 4 条 × 60fps = 240 行/秒，比任何真实手势都快，同时把管道深度压到 agent 追得上的量级。
 */
const MAX_WHEEL_REPORTS_PER_FRAME = 4;

/**
 * 未发送的滚轮行数上限（屏数）。
 *
 * 超出就丢掉最老的部分：惯性甩动能攒出上千行，全部发完要几十帧，那时用户早就松手了，
 * 补完这些行只会让画面继续"自己滚"。留三屏足够覆盖一次正常的大幅翻页。
 */
const MAX_PENDING_WHEEL_SCREENS = 3;

/**
 * 每帧预算 = 待发行数 ÷ 这个除数(再夹进 1..MAX)。
 *
 * 为什么不用固定条数:固定 4 条/帧意味着每帧恰好跳 4 行(约 64px)然后停住,匀速直线,
 * 台阶感就是"卡顿"的来源。按剩余量取商后,大甩动开头满速、尾部自然收窄到 1 行/帧 ——
 * 指数衰减,也就是浏览器惯性滚动的 ease-out 手感。
 *
 * 取 3 而不是更大:一个普通档位约 7 行,3 → 3/2/1/1 四帧收敛,既有减速段又不显迟滞。
 */
const WHEEL_EASE_DIVISOR = 3;

/**
 * 等 agent 重绘的宽限时间,超过就无条件发下一批。
 *
 * 闭环必须有超时兜底:agent 已经滚到顶/底时一个字节都不会回吐,只等信号会把队列锁死,
 * 症状是"滚到顶之后再往回滚要卡一下"。70ms 约四帧,足够覆盖一次全屏 TUI 重绘,
 * 又不会在真的没有回吐时让手感明显发粘。
 */
export const WHEEL_REPAINT_GRACE_MS = 70;

/** 行内没滚满一行的余量,按终端实例累计,方向反转时清零。 */
interface WheelCarry {
  lines: number;
}

/**
 * 这次滚轮手势应该滚多少行（带符号，负数向上）。
 *
 * 不复用 xterm 的 `consumeWheelEvent`：它对 |deltaY| < 50 的事件乘 0.3 当"疑似触控板"
 * 阻尼，而我们要的恰恰是"终端滚动行程 == 手上的行程"，那个阻尼是反向的。
 *
 * 三种 deltaMode 都要认：Firefox 给 LINE，部分环境给 PAGE，Chromium / WebKit 给 PIXEL。
 * 只有 PIXEL 需要 cell 高度换算，所以另外两种在拿不到行高时也能正常工作。
 */
function wheelLinesForEvent(
  event: WheelEvent,
  rows: number,
  cellHeight: number | null,
  carry: WheelCarry,
): number {
  let lines: number;
  if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) {
    lines = event.deltaY;
  } else if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) {
    lines = event.deltaY * Math.max(rows, 1);
  } else {
    // PIXEL：物理像素 ÷ 行高 = 行数,这就是"行程一致"的定义。
    if (cellHeight === null || cellHeight <= 0) return 0;
    lines = event.deltaY / cellHeight;
  }

  // 方向一反就丢掉旧余量,否则上一段的残留会把回滚的第一下吃掉或多送一行。
  if (carry.lines !== 0 && Math.sign(carry.lines) !== Math.sign(lines)) {
    carry.lines = 0;
  }
  const total = carry.lines + lines;
  // 触控板的一次微小位移不足一行,先攒着,别当成 0 丢掉——丢了就是"滚了没反应"。
  const whole = Math.trunc(total);
  carry.lines = total - whole;

  const cap = Math.max(rows, 1) * MAX_WHEEL_LINES_PER_EVENT_SCREENS;
  return Math.max(-cap, Math.min(cap, whole));
}

/** 从 DOM 量出一行的 CSS 像素高。xterm 没有公开 cell 尺寸,只能这么取。 */
function measureCellHeight(term: Terminal): number | null {
  const screen = term.element?.querySelector<HTMLElement>(".xterm-screen");
  if (!screen) return null;
  const rows = Math.max(term.rows, 1);
  const height = screen.getBoundingClientRect().height;
  if (!(height > 0)) return null;
  return height / rows;
}

/**
 * agent 终端的滚轮兜底：滚轮只是"翻看内容"的本地手势，绝不合成按键。
 *
 * xterm 在没有本地 scrollback 的 buffer（也就是 alt screen）里有一条兜底：把滚轮
 * 翻译成 ESC[A / ESC[B 当方向键发给程序（CoreBrowserTerminal 的 always-on wheel
 * listener）。对 agent 来说这等于"滚一下滚轮 = 按一下上下键"——Claude Code 的 Chat
 * 键位是 up:"history:previous"，输入框立刻翻出上一条历史，把正在写的内容顶掉。
 *
 * 正常情况下 agent 自己会开鼠标上报（Claude Code 的 Scroll 键位把 wheelup/wheeldown
 * 绑到 scroll:lineUp/lineDown），滚轮走下面第一条分支交给程序自己滚——pty.rs 曾用
 * CLAUDE_CODE_DISABLE_MOUSE 关掉这个能力，已移除，理由见那边的注释。这里保留兜底是
 * 为了用户自己关掉上报、或某个 agent 主动不开的情况：alt screen 本来就没有可翻的历史，
 * 什么都不做远好过改写用户的输入框。另外两种情况保持 xterm 原样：
 * - 程序开了 vt200/drag/any（agent、vim、tmux、less --mouse）→ 上报给程序，由它自己滚
 * - normal buffer → ScrollableElement 已经滚过本地 scrollback 了（只有滚到顶/底
 *   没滚动时事件才会冒泡到这里，此时 hasScrollback 为真，xterm 也不会合成按键）
 *
 * 不区分修饰键：ctrl / meta + 滚轮走同一条兜底，同样会变成方向键。
 *
 * 只给 agent 终端装（TerminalView）。shell / SSH / WSL 面板不装：那边跑的 less、man、
 * git log、vim、htop 正是靠这条 alternate-scroll 方向键滚动的，吞掉等于它们也滚不动，
 * 而 shell 里没有会被方向键顶掉的输入框。
 */
export function attachTerminalWheelScroll(term: Terminal): TerminalWheelScroll {
  const carry: WheelCarry = { lines: 0 };
  // 我们自己补发的那些 LINE 事件要原样交给 xterm,不能再进放大逻辑(否则递归)。
  let replaying = false;
  const pacer = createWheelReportPacer(term, () => {
    replaying = true;
    return () => {
      replaying = false;
    };
  });

  term.attachCustomWheelEventHandler((event) => {
    if (event.deltaY === 0) return true;
    if (APP_WHEEL_MOUSE_MODES.has(term.modes.mouseTrackingMode)) {
      if (replaying) return true;
      return queueAppWheelReports(term, event, carry, pacer);
    }
    if (term.buffer.active.type !== "alternate") return true;
    // listener 是 { passive: false } 注册的，可以拦掉祖先容器的默认滚动。
    event.preventDefault();
    return false;
  });

  return {
    dispose: () => pacer.cancel(),
    // 合成事件是同步 dispatch 的,xterm 的 onData 就发生在 beginReplay/endReplay 之间,
    // 所以这个标志天然精确 —— 比去正则匹配 \x1b[<…M 那串鼠标序列稳得多。
    isReplayingWheel: () => replaying,
  };
}

/** [`attachTerminalWheelScroll`] 的句柄。 */
export interface TerminalWheelScroll {
  /** 卸载时必须调:否则待发的 rAF 回调会打到已 dispose 的 term 上。 */
  dispose: () => void;
  /**
   * 当前是否正在派发我们自己合成的滚轮事件。
   *
   * 调用方用它把"滚轮上报"和"用户敲键"分开:上报不该触发 `pauseForUserInput` ——
   * 那会为每条上报挂起输出 48ms 并重绘两次光标行,而滚动期间每帧有好几条上报,
   * 等于把 agent 的重绘反复往后推,正是我们要消除的抖动来源。
   */
  isReplayingWheel: () => boolean;
}

/** 按帧分摊发送滚轮上报的队列。 */
interface WheelReportPacer {
  /** 累计待发行数（带符号）并确保有一帧已排期。 */
  enqueue: (lines: number, clientX: number, clientY: number) => void;
  /** 取消尚未发出的帧。终端卸载时必须调,否则 rAF 回调会打到已 dispose 的 term 上。 */
  cancel: () => void;
}

/**
 * 把待发行数按帧分摊成鼠标上报,并与 agent 的重绘闭环。
 *
 * 两条独立的节流:
 *
 * 1. **闭环**:发出一批后等 `onWriteParsed`(每帧最多触发一次、解析完成后)再发下一批,
 *    超过 [`WHEEL_REPAINT_GRACE_MS`] 无条件推进。原来是开环 —— 固定每帧灌 4 条,不管
 *    agent 画完没有。一次全屏 TUI 重绘通常超过一帧,于是我们持续跑在 agent 前面:上报
 *    堆在 pty 里,画面以"憋一下、跳一段"的方式回来,帧间距不均,这就是卡顿的主因。
 *    闭环之后每次重绘对应一批上报,节奏由 agent 的实际能力决定,画面匀速。
 *
 * 2. **缓动**:每帧条数按 [`WHEEL_EASE_DIVISOR`] 取剩余量的商,尾部自然收窄成 1 行/帧。
 *
 * 方向反转时丢掉反向的余量 —— 用户已经改了主意,把旧方向补完只会让画面先往回跳一段。
 * 总量仍被 [`MAX_PENDING_WHEEL_SCREENS`] 截断:惯性甩动能攒出上千行,补完只会让画面
 * 在用户松手后继续自己滚。
 */
function createWheelReportPacer(term: Terminal, beginReplay: () => () => void): WheelReportPacer {
  let pendingLines = 0;
  let coords = { clientX: 0, clientY: 0 };
  let frameScheduled = false;
  let cancelled = false;
  // 等重绘的截止时刻;0 表示没在等。
  let repaintDeadline = 0;

  // 没有 onWriteParsed 的实现(测试替身、老版本)退回纯 rAF 节奏,不要因此把滚轮弄死。
  const repaintSignal = typeof term.onWriteParsed === "function" ? term.onWriteParsed : null;
  const disposeRepaint = repaintSignal?.call(term, () => {
    if (repaintDeadline === 0) return;
    // agent 这批画完了,立刻放行下一批,不必等到宽限期结束。
    repaintDeadline = 0;
    if (pendingLines !== 0) schedule();
  });

  const flush = () => {
    frameScheduled = false;
    if (cancelled || pendingLines === 0) return;
    const element = term.element;
    if (!element) {
      pendingLines = 0;
      return;
    }
    // 还在等上一批的重绘:重排一帧继续等,别加深管道深度。
    if (repaintDeadline !== 0) {
      if (nowMs() < repaintDeadline) {
        schedule();
        return;
      }
      repaintDeadline = 0;
    }

    const step = pendingLines < 0 ? -1 : 1;
    const budget = Math.max(
      1,
      Math.min(MAX_WHEEL_REPORTS_PER_FRAME, Math.ceil(Math.abs(pendingLines) / WHEEL_EASE_DIVISOR)),
    );
    const thisFrame = Math.min(Math.abs(pendingLines), budget);
    pendingLines -= step * thisFrame;

    const endReplay = beginReplay();
    try {
      for (let index = 0; index < thisFrame; index += 1) {
        element.dispatchEvent(
          new WheelEvent("wheel", {
            deltaY: step,
            deltaMode: WheelEvent.DOM_DELTA_LINE,
            clientX: coords.clientX,
            clientY: coords.clientY,
            // 不冒泡:listener 就挂在 element 上,冒上去只会让祖先容器跟着滚。
            bubbles: false,
            cancelable: true,
          }),
        );
      }
    } finally {
      endReplay();
    }

    // 只有能收到重绘信号时才闭环;收不到就退回纯 rAF,否则每批都要白等一个宽限期。
    if (repaintSignal) repaintDeadline = nowMs() + WHEEL_REPAINT_GRACE_MS;
    if (pendingLines !== 0) schedule();
  };

  const schedule = () => {
    if (frameScheduled || cancelled) return;
    frameScheduled = true;
    scheduleFrame(flush);
  };

  return {
    enqueue: (lines, clientX, clientY) => {
      if (cancelled) return;
      // 反向时丢掉旧方向的余量,否则画面会先往回跳一段再跟手。
      if (pendingLines !== 0 && Math.sign(pendingLines) !== Math.sign(lines)) {
        pendingLines = 0;
      }
      // 坐标用最新一次事件的:上报的格子位置该跟着当前指针。
      coords = { clientX, clientY };
      const cap = Math.max(term.rows, 1) * MAX_PENDING_WHEEL_SCREENS;
      pendingLines = Math.max(-cap, Math.min(cap, pendingLines + lines));
      schedule();
    },
    cancel: () => {
      cancelled = true;
      pendingLines = 0;
      repaintDeadline = 0;
      disposeRepaint?.dispose();
    },
  };
}

/**
 * 把一次滚轮事件折算成 N 行待发上报,N = 这段行程真正跨过的行数,交给 pacer 按帧发出。
 *
 * 为什么需要这层：xterm 算出了行数却只发一条上报
 * （`CoreBrowserTerminal.bindMouse` 的 wheel 分支，注释原文 "has been simplified to
 * simply send a single up or down sequence"）。而 agent 那边一条上报就是一行
 * （Claude Code 的 Scroll 键位 wheelup → `scroll:lineUp`），于是滚轮转多远都只滚一行 ——
 * 这就是"滚了很长行程、终端几乎不动"的成因。`scrollSensitivity` 治不了它：倍数加在
 * 被丢弃的那个行数上，下游只看正负号。
 *
 * 补发的手段是往 xterm 自己的 listener 上派发 `deltaMode = LINE`、`deltaY = ±1` 的合成
 * 事件：走 LINE 分支时 `consumeWheelEvent` 既不碰 cell 高度也不碰触控板阻尼,一个事件
 * 恰好等于一行上报。这样编码、协议门禁（vt200 / SGR 1006 …）、坐标换算全都还是 xterm
 * 自己那套,我们不重复实现鼠标序列。
 *
 * 合成事件必须带上原事件的 clientX/clientY：`getMouseReportCoords` 用它算格子坐标,
 * 缺了会拿不到 pos 而整条上报被丢掉。修饰键则故意不带 —— `_applyScrollModifier` 会对
 * 带修饰键的事件乘 `fastScrollSensitivity`,那样每条合成事件就不再是一行了；快滚的
 * 倍数已经体现在原事件的 deltaY 里。
 */
function queueAppWheelReports(
  term: Terminal,
  event: WheelEvent,
  carry: WheelCarry,
  pacer: WheelReportPacer,
): boolean {
  // 还没 open() 或拿不到行高：交回 xterm,至少还能滚一行,别把滚轮弄成完全没反应。
  if (!term.element) return true;
  const lines = wheelLinesForEvent(event, term.rows, measureCellHeight(term), carry);
  // lines === 0 时不足一行的余量已经攒进 carry。这里仍要把事件吃掉,否则 xterm 会照旧
  // 发一条上报,等于每个微小位移都滚一行,触控板会变得过于灵敏。
  if (lines !== 0) pacer.enqueue(lines, event.clientX, event.clientY);
  event.preventDefault();
  return false;
}

// ── xterm initialization ─────────────────────────────────────────────────────

export interface InitTerminalResult {
  term: Terminal;
  fitAddon: FitAddon;
}

/**
 * 创建 xterm Terminal 实例并加载通用 addon（FitAddon, Unicode11, WebGL）。
 * 调用方负责 term.open(container)。
 */
export function initTerminal(
  variant: ThemeVariant,
  scrollback = 1000,
  fontSize = 12,
  fontFamily = "monospace",
): InitTerminalResult {
  const term = new Terminal({
    convertEol: false,
    scrollback,
    cursorBlink: true,
    cursorStyle: "bar",
    cursorWidth: 1,
    fontFamily,
    fontSize,
    theme: themeFor(variant),
    minimumContrastRatio: terminalMinimumContrastRatioForTheme(variant),
    allowTransparency: true,
    allowProposedApi: true,
    // 本地 scrollback 滚动做成浏览器那样的连续位移（xterm 自己按 rAF 插值到目标行）。
    // 只对 xterm 亲自滚 viewport 的场景生效：shell / SSH / WSL 面板，以及 agent 终端
    // 不在 alt screen 的时候。开了鼠标上报的 alt screen 由 agent 重绘，这里管不到，
    // 那条路径的手感由 attachTerminalWheelScroll 的闭环节流负责。
    // 不影响 less / vim 依赖的 alternate-scroll 方向键路径（那条不走 viewport 滚动）。
    smoothScrollDuration: TERMINAL_SMOOTH_SCROLL_MS,
    // 当运行中的 TUI（Claude Code / Codex）开启鼠标上报时，xterm 默认把拖动当作
    // 鼠标事件转发给程序并取消本地选区，导致 macOS 用户"运行时无法框选"。开启此项后
    // 按住 ⌥ Option 拖动可强制本地选区（iTerm2 / Terminal.app 的标准约定）。
    macOptionClickForcesSelection: true,
  });

  const fitAddon = new FitAddon();
  const unicode11Addon = new Unicode11Addon();
  term.loadAddon(fitAddon);
  term.loadAddon(unicode11Addon);
  term.unicode.activeVersion = "11";

  return { term, fitAddon };
}

/**
 * 尝试加载 WebGL addon，失败时静默降级。
 * 必须在 term.open() 之后调用。
 *
 * 关于"要不要关掉 WebGL"的实测结论（recording8/9/10 对照）：
 * - WebGL 的代价：拖大段选区时偶发 100–400 ms composite 爆点（GPU 几何上传）
 * - DOM renderer 的代价：高频 mousemove（鼠标在终端区域移动）+ 高速文本输出时
 *   持续中等卡顿（每次 mousemove 触发多个 row DOM 节点的 reflow/composite，
 *   rec10 实测 1233 mousemove/2.7s 下出现 511ms 单帧）
 * - Aeroric 日常以"鼠标在终端区域活动"为主，长拖选区相对罕见，因此 WebGL 的
 *   "偶发爆点"比 DOM 的"持续小卡顿"更可接受。
 *
 * 不要为了"避免偶发卡顿"再把这里关掉——见 timeline rec10。
 */
export function loadWebglAddon(term: Terminal): void {
  try {
    const webglAddon = new WebglAddon();
    webglAddon.onContextLoss(() => {
      console.warn("[terminal] WebGL context lost; falling back to xterm DOM renderer");
      webglAddon.dispose();
    });
    term.loadAddon(webglAddon);
  } catch (err) {
    console.warn("[terminal] WebGL addon unavailable; using xterm DOM renderer", err);
    /* 不支持 WebGL 时降级，不影响功能 */
  }
}

/**
 * 安全地执行 fitAddon.fit() 并返回 { cols, rows }，失败/容器不可见时返回 null。
 *
 * container 传了的话会做两道防御（xterm.js issue #3029 / #4338 / #4841 的已知坑）：
 * 1. rect 宽高任一为 0 → 容器在 display:none 子树里，跳过。多项目挂载时这是
 *    日常状态（非激活 ProjectPage display:none）。
 * 2. proposeDimensions 返回非有限值或 cols/rows < 2 → 退化场景，跳过。
 *
 * 为什么必须拦：FitAddon 在 0 尺寸容器上不返回 NaN，而是退化到 `Math.max(
 * MINIMUM_COLS, Math.floor(0 / cell))` = MINIMUM_COLS (2)；若放过 → 调用方
 * notifyResize → resize_pty → SIGWINCH → Claude Code / Codex 这类 TUI 按
 * cols=2 重排，buffer 永久打散成一字一行。VS Code 的同等防线在 _resize()
 * 里是 `if (isNaN(cols) || isNaN(rows)) return`，但 xterm.js 这条 NaN 路径
 * 不存在，必须在 rect 层先拦。
 */
export function safeFit(
  fitAddon: FitAddon,
  term: Terminal,
  container?: HTMLElement,
): { cols: number; rows: number } | null {
  if (container) {
    const rect = container.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return null;
  }
  try {
    const dims = fitAddon.proposeDimensions();
    if (!dims || !Number.isFinite(dims.cols) || !Number.isFinite(dims.rows)) return null;
    if (dims.cols < 2 || dims.rows < 2) return null;
    fitAddon.fit();
    return { cols: term.cols, rows: term.rows };
  } catch {
    return null;
  }
}

const terminalRevealFrames = new WeakMap<HTMLElement, number>();

/**
 * Reflow xterm without exposing its intermediate viewport. Width changes can make
 * xterm rewrap thousands of buffer lines; keeping the renderer hidden until the
 * next paint makes the first visible frame the final, bottom-anchored frame.
 */
export function fitTerminalAtBottom(
  fitAddon: FitAddon,
  term: Terminal,
  container: HTMLElement,
): { cols: number; rows: number } | null {
  container.setAttribute("data-terminal-resizing", "true");
  const size = safeFit(fitAddon, term, container);
  if (!size) {
    container.removeAttribute("data-terminal-resizing");
    return null;
  }
  term.scrollToBottom();
  try {
    term.refresh(0, Math.max(0, term.rows - 1));
  } catch {
    // A renderer can disappear during teardown; revealing the container is enough.
  }

  const pendingFrame = terminalRevealFrames.get(container);
  if (pendingFrame !== undefined) window.cancelAnimationFrame(pendingFrame);
  const frame = window.requestAnimationFrame(() => {
    if (terminalRevealFrames.get(container) !== frame) return;
    terminalRevealFrames.delete(container);
    container.removeAttribute("data-terminal-resizing");
  });
  terminalRevealFrames.set(container, frame);
  return size;
}

/**
 * 更新终端字体大小并重新 fit，返回新的 { cols, rows } 或 null。
 */
export function applyTerminalFontSize(
  term: Terminal,
  fitAddon: FitAddon,
  fontSize: number,
  container?: HTMLElement,
): { cols: number; rows: number } | null {
  if (term.options.fontSize === fontSize) return null;
  term.options.fontSize = fontSize;
  return container
    ? fitTerminalAtBottom(fitAddon, term, container)
    : safeFit(fitAddon, term, container);
}

export function applyTerminalFontFamily(
  term: Terminal,
  fitAddon: FitAddon,
  fontFamily: string,
  container?: HTMLElement,
): { cols: number; rows: number } | null {
  if (term.options.fontFamily === fontFamily) return null;
  term.options.fontFamily = fontFamily;
  return container
    ? fitTerminalAtBottom(fitAddon, term, container)
    : safeFit(fitAddon, term, container);
}
