import type {
  FontFamily,
  TaskDisplayWindow,
  TerminalFontSize,
  ThemeMode,
  ThemeVariant,
} from "./types";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  DEFAULT_TASK_DISPLAY_WINDOW,
  clampTerminalFontSize,
  normalizeTaskDisplayWindow,
} from "./types";

export function getSystemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function getInitialThemeMode(): ThemeMode {
  const stored = localStorage.getItem("aeroric:theme");
  return stored === "dark" || stored === "light" || stored === "system" || stored === "eyecare"
    ? stored
    : "light";
}

export function resolveThemeVariant(mode: ThemeMode, systemPrefersDark: boolean): ThemeVariant {
  if (mode === "system") return systemPrefersDark ? "dark" : "light";
  return mode;
}

export function getInitialTerminalFontSize(): TerminalFontSize {
  const stored = localStorage.getItem("aeroric:terminalFontSize");
  if (stored == null) return DEFAULT_TERMINAL_FONT_SIZE;
  const parsed = Number(stored);
  return Number.isFinite(parsed) ? clampTerminalFontSize(parsed) : DEFAULT_TERMINAL_FONT_SIZE;
}

export function getInitialTaskDisplayWindow(): TaskDisplayWindow {
  const stored = localStorage.getItem("aeroric:taskDisplayWindow");
  return stored == null ? DEFAULT_TASK_DISPLAY_WINDOW : normalizeTaskDisplayWindow(stored);
}

export function getInitialAttentionBadge(): boolean {
  // 默认开启:项目栏显示待确认任务数量角标;关闭后回退为黄色小圆点
  return localStorage.getItem("aeroric:attentionBadge") !== "0";
}

export function getInitialFontFamily(
  key: string,
  fallback: FontFamily,
  legacyDefaults: readonly FontFamily[] = [],
): FontFamily {
  const stored = localStorage.getItem(key);
  if (!stored) return fallback;
  // 老用户 localStorage 里可能存着历史默认字体链（缺 CJK 字形）。若命中旧默认值，
  // 说明用户从未主动改过字体，自动迁移到当前默认值以修复终端中文乱码/错位。
  if (legacyDefaults.includes(stored.trim())) return fallback;
  return stored;
}

export function disableTextInputAutoFeatures(target: EventTarget | null): void {
  if (!(target instanceof HTMLElement)) return;
  const input =
    target instanceof HTMLTextAreaElement
      ? target
      : target instanceof HTMLInputElement
        ? target
        : null;
  if (!input) return;
  if (input instanceof HTMLInputElement) {
    const type = input.type.toLowerCase();
    if (!["", "text", "search", "password", "email", "url", "tel"].includes(type)) return;
  }
  input.setAttribute("autocomplete", "off");
  input.setAttribute("autocorrect", "off");
  input.setAttribute("autocapitalize", "off");
  input.setAttribute("spellcheck", "false");
}
