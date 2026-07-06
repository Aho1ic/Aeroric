import { useEffect, useState } from "react";
import type {
  FontFamily,
  TaskDisplayWindow,
  TerminalFontSize,
  ThemeMode,
  ThemeVariant,
} from "../types";
import { AppSettingsDialog } from "./AppSettingsDialog";
import {
  OPEN_APP_SETTINGS_EVENT,
  type NavKey,
  type OpenAppSettingsDetail,
} from "./app-settings/types";

export function AppSettingsEventHost({
  themeVariant,
  themeMode,
  systemPrefersDark,
  onThemeModeChange,
  terminalFontSize,
  onTerminalFontSizeChange,
  taskDisplayWindow,
  onTaskDisplayWindowChange,
  attentionBadge,
  onAttentionBadgeChange,
  sftpLocalDefaultPath,
  onSftpLocalDefaultPathChange,
  uiFontFamily,
  onUiFontFamilyChange,
  monoFontFamily,
  onMonoFontFamilyChange,
}: {
  themeVariant: ThemeVariant;
  themeMode: ThemeMode;
  systemPrefersDark: boolean;
  onThemeModeChange: (mode: ThemeMode) => void;
  terminalFontSize: TerminalFontSize;
  onTerminalFontSizeChange: (size: TerminalFontSize) => void;
  taskDisplayWindow: TaskDisplayWindow;
  onTaskDisplayWindowChange: (window: TaskDisplayWindow) => void;
  attentionBadge: boolean;
  onAttentionBadgeChange: (enabled: boolean) => void;
  sftpLocalDefaultPath: string;
  onSftpLocalDefaultPathChange: (path: string) => void;
  uiFontFamily: FontFamily;
  onUiFontFamilyChange: (family: FontFamily) => void;
  monoFontFamily: FontFamily;
  onMonoFontFamilyChange: (family: FontFamily) => void;
}) {
  const [showAppSettings, setShowAppSettings] = useState(false);
  const [initialSettingsNav, setInitialSettingsNav] = useState<NavKey>("general");

  useEffect(() => {
    const open = (event: Event) => {
      const detail =
        event instanceof CustomEvent ? (event.detail as OpenAppSettingsDetail | undefined) : null;
      setInitialSettingsNav(detail?.initialNav ?? "general");
      setShowAppSettings(true);
    };
    window.addEventListener(OPEN_APP_SETTINGS_EVENT, open);
    return () => window.removeEventListener(OPEN_APP_SETTINGS_EVENT, open);
  }, []);

  if (!showAppSettings) return null;

  return (
    <AppSettingsDialog
      initialNav={initialSettingsNav}
      themeVariant={themeVariant}
      themeMode={themeMode}
      systemPrefersDark={systemPrefersDark}
      onThemeModeChange={onThemeModeChange}
      terminalFontSize={terminalFontSize}
      onTerminalFontSizeChange={onTerminalFontSizeChange}
      taskDisplayWindow={taskDisplayWindow}
      onTaskDisplayWindowChange={onTaskDisplayWindowChange}
      attentionBadge={attentionBadge}
      onAttentionBadgeChange={onAttentionBadgeChange}
      sftpLocalDefaultPath={sftpLocalDefaultPath}
      onSftpLocalDefaultPathChange={onSftpLocalDefaultPathChange}
      uiFontFamily={uiFontFamily}
      onUiFontFamilyChange={onUiFontFamilyChange}
      monoFontFamily={monoFontFamily}
      onMonoFontFamilyChange={onMonoFontFamilyChange}
      onClose={() => setShowAppSettings(false)}
    />
  );
}
