/**
 * App 的外观/显示偏好簇:主题(明暗/护眼 + 跟随系统)、终端字号、任务展示窗口、
 * attention 角标、SFTP 本地默认路径、UI/等宽字体、dsh 网页搜索开关 —— 九个
 * useState 加它们各自的持久化 effect、原生窗口主题同步、以及一次性的系统深色
 * 偏好监听。原来内联在 App.tsx,簇内每个 effect 只依赖本簇状态,与任务/项目
 * 状态互不耦合,适合整体搬出。
 *
 * 返回值沿用原先的命名,App 侧的 props 传递与 onToggleTheme 快捷键无需改动。
 */
import { useCallback, useEffect, useState } from "react";
import { setTheme as setAppTheme } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  getInitialAttentionBadge,
  getInitialDshWebSearchEnabled,
  getInitialFontFamily,
  getInitialTaskDisplayWindow,
  getInitialTerminalFontSize,
  getInitialThemeMode,
  getSystemPrefersDark,
  nativeThemeForVariant,
  nativeWindowBackgroundForVariant,
  resolveThemeVariant,
} from "../appThemeState";
import { FONT_PLATFORM, getFontStorageKey, getTerminalFontSizeStorageKey } from "../platform";
import { composeFontStack } from "../utils/fonts";
import {
  getInitialSftpLocalDefaultPath,
  normalizeSftpLocalDefaultPath,
  SFTP_LOCAL_PATH_STORAGE_KEY,
} from "../settings";
import type {
  FontFamily,
  TaskDisplayWindow,
  TerminalFontSize,
  ThemeMode,
  ThemeVariant,
} from "../types";
import { DEFAULT_MONO_FONT_BY_PLATFORM, DEFAULT_UI_FONT_BY_PLATFORM } from "../types";

export function useAppAppearance() {
  const [themeMode, setThemeMode] = useState<ThemeMode>(getInitialThemeMode);
  const [systemPrefersDark, setSystemPrefersDark] = useState(getSystemPrefersDark);
  const themeVariant: ThemeVariant = resolveThemeVariant(themeMode, systemPrefersDark);
  const [terminalFontSize, setTerminalFontSize] = useState<TerminalFontSize>(
    getInitialTerminalFontSize,
  );
  const [taskDisplayWindow, setTaskDisplayWindow] = useState<TaskDisplayWindow>(
    getInitialTaskDisplayWindow,
  );
  const [attentionBadge, setAttentionBadge] = useState<boolean>(getInitialAttentionBadge);
  const [sftpLocalDefaultPath, setSftpLocalDefaultPath] = useState<string>(
    getInitialSftpLocalDefaultPath,
  );
  const [uiFontFamily, setUiFontFamily] = useState<FontFamily>(() =>
    getInitialFontFamily(
      getFontStorageKey("ui"),
      DEFAULT_UI_FONT_BY_PLATFORM[FONT_PLATFORM],
      [],
      FONT_PLATFORM === "macos" ? "aeroric:uiFontFamily" : undefined,
    ),
  );
  const [monoFontFamily, setMonoFontFamily] = useState<FontFamily>(() =>
    getInitialFontFamily(
      getFontStorageKey("mono"),
      DEFAULT_MONO_FONT_BY_PLATFORM[FONT_PLATFORM],
      [],
      FONT_PLATFORM === "macos" ? "aeroric:monoFontFamily" : undefined,
    ),
  );
  const [dshWebSearchEnabled, setDshWebSearchEnabled] = useState<boolean>(
    getInitialDshWebSearchEnabled,
  );

  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = (event: MediaQueryListEvent) => setSystemPrefersDark(event.matches);

    setSystemPrefersDark(mediaQuery.matches);
    mediaQuery.addEventListener("change", handleChange);

    return () => mediaQuery.removeEventListener("change", handleChange);
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", themeVariant === "dark");
    root.classList.toggle("eyecare", themeVariant === "eyecare");
    localStorage.setItem("aeroric:theme", themeMode);
  }, [themeVariant, themeMode]);

  useEffect(() => {
    if (!isTauri()) return;

    // Keep AppKit/Win32 chrome on the exact variant already resolved for the
    // web UI. In particular, an explicit dark value is more reliable than
    // resetting to `null` for system mode and waiting for a second native
    // appearance propagation. The window background is also the surface shown
    // through macOS's transparent title bar.
    const nativeTheme = nativeThemeForVariant(themeVariant);
    const currentWindow = getCurrentWindow();
    Promise.all([
      setAppTheme(nativeTheme),
      currentWindow.setTheme(nativeTheme),
      currentWindow.setBackgroundColor(nativeWindowBackgroundForVariant(themeVariant)),
    ]).catch(console.error);
  }, [themeVariant]);

  useEffect(() => {
    localStorage.setItem(getTerminalFontSizeStorageKey(), String(terminalFontSize));
  }, [terminalFontSize]);

  useEffect(() => {
    localStorage.setItem("aeroric:taskDisplayWindow", String(taskDisplayWindow));
  }, [taskDisplayWindow]);

  useEffect(() => {
    localStorage.setItem("aeroric:attentionBadge", attentionBadge ? "1" : "0");
  }, [attentionBadge]);

  useEffect(() => {
    localStorage.setItem("aeroric:dshWebSearchEnabled", dshWebSearchEnabled ? "1" : "0");
  }, [dshWebSearchEnabled]);

  useEffect(() => {
    localStorage.setItem(
      SFTP_LOCAL_PATH_STORAGE_KEY,
      normalizeSftpLocalDefaultPath(sftpLocalDefaultPath),
    );
  }, [sftpLocalDefaultPath]);

  useEffect(() => {
    const value = uiFontFamily.trim() || DEFAULT_UI_FONT_BY_PLATFORM[FONT_PLATFORM];
    localStorage.setItem(getFontStorageKey("ui"), value);
    // 用户只选单个族名时补齐当前平台的回退链，避免 Windows / Linux 缺字形。
    document.documentElement.style.setProperty(
      "--font-ui",
      composeFontStack(value, DEFAULT_UI_FONT_BY_PLATFORM[FONT_PLATFORM]),
    );
  }, [uiFontFamily]);

  useEffect(() => {
    const value = monoFontFamily.trim() || DEFAULT_MONO_FONT_BY_PLATFORM[FONT_PLATFORM];
    localStorage.setItem(getFontStorageKey("mono"), value);
    document.documentElement.style.setProperty(
      "--font-mono",
      composeFontStack(value, DEFAULT_MONO_FONT_BY_PLATFORM[FONT_PLATFORM]),
    );
  }, [monoFontFamily]);

  const handleToggleTheme = useCallback(() => {
    setThemeMode((currentMode) => {
      // Toggle only cycles between the two standard variants. Special themes
      // (eyecare and any future opt-in variants) retreat to "light" so the
      // shortcut remains a one-tap escape hatch back to the canonical pair.
      if (currentMode === "dark") return "light";
      if (currentMode === "light") return "dark";
      if (currentMode === "system") return systemPrefersDark ? "light" : "dark";
      return "light";
    });
  }, [systemPrefersDark]);

  return {
    themeMode,
    setThemeMode,
    systemPrefersDark,
    themeVariant,
    terminalFontSize,
    setTerminalFontSize,
    taskDisplayWindow,
    setTaskDisplayWindow,
    attentionBadge,
    setAttentionBadge,
    sftpLocalDefaultPath,
    setSftpLocalDefaultPath,
    uiFontFamily,
    setUiFontFamily,
    monoFontFamily,
    setMonoFontFamily,
    dshWebSearchEnabled,
    setDshWebSearchEnabled,
    handleToggleTheme,
  };
}
