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
import { FONT_PLATFORM, getTerminalFontSizeStorageKey } from "./platform";

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

// 原生窗口装饰（macOS titlebar / Win32 chrome）只认 light/dark；eyecare 归入 light
// 家族，让系统按钮与滚动条保持浅色外观。
export function nativeThemeForVariant(variant: ThemeVariant): "light" | "dark" {
  return variant === "dark" ? "dark" : "light";
}

// macOS 的 titlebarAppearsTransparent 会透出窗口背景色，所以标题栏配色由这里决定。
// 取值与 App.css 里 prefers-reduced-transparency 下的实色 token 保持一致。
export function nativeWindowBackgroundForVariant(variant: ThemeVariant): string {
  if (variant === "dark") return "#050607";
  if (variant === "eyecare") return "#f6eddc";
  return "#fbfbfc";
}

export function getInitialTerminalFontSize(): TerminalFontSize {
  // 按平台隔离，老 key 仅作为 macOS 的迁移来源（历史版本只在 mac 上被使用过）。
  const stored =
    localStorage.getItem(getTerminalFontSizeStorageKey()) ??
    (FONT_PLATFORM === "macos" ? localStorage.getItem("aeroric:terminalFontSize") : null);
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

export function getInitialDshWebSearchEnabled(): boolean {
  // 默认开启:DSH 任务允许使用 web_search 工具
  return localStorage.getItem("aeroric:dshWebSearchEnabled") !== "0";
}

export function getInitialFontFamily(
  key: string,
  fallback: FontFamily,
  legacyDefaults: readonly FontFamily[] = [],
  legacyKey?: string,
): FontFamily {
  const stored = localStorage.getItem(key) ?? (legacyKey ? localStorage.getItem(legacyKey) : null);
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
