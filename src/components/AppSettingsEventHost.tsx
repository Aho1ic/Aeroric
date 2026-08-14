import { lazy, Suspense, useEffect, useState } from "react";
import type {
  FontFamily,
  TaskDisplayWindow,
  TerminalFontSize,
  ThemeMode,
  ThemeVariant,
} from "../types";
import {
  OPEN_APP_SETTINGS_EVENT,
  START_DSH_CREATOR_DRAFT_EVENT,
  type NavKey,
  type OpenAppSettingsDetail,
} from "./app-settings/types";

const AppSettingsDialog = lazy(() =>
  import("./AppSettingsDialog").then((module) => ({ default: module.AppSettingsDialog })),
);

export type AppSettingsEventHostProps = {
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
  dshWebSearchEnabled: boolean;
  onDshWebSearchEnabledChange: (enabled: boolean) => void;
};

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
  dshWebSearchEnabled,
  onDshWebSearchEnabledChange,
}: AppSettingsEventHostProps) {
  const [showAppSettings, setShowAppSettings] = useState(false);
  const [initialSettingsNav, setInitialSettingsNav] = useState<NavKey>("general");

  useEffect(() => {
    const open = (event: Event) => {
      const detail =
        event instanceof CustomEvent ? (event.detail as OpenAppSettingsDetail | undefined) : null;
      setInitialSettingsNav(detail?.initialNav ?? "general");
      setShowAppSettings(true);
    };
    const startCreatorDraft = () => setShowAppSettings(false);
    window.addEventListener(OPEN_APP_SETTINGS_EVENT, open);
    window.addEventListener(START_DSH_CREATOR_DRAFT_EVENT, startCreatorDraft);
    return () => {
      window.removeEventListener(OPEN_APP_SETTINGS_EVENT, open);
      window.removeEventListener(START_DSH_CREATOR_DRAFT_EVENT, startCreatorDraft);
    };
  }, []);

  if (!showAppSettings) return null;

  return (
    <Suspense fallback={null}>
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
        dshWebSearchEnabled={dshWebSearchEnabled}
        onDshWebSearchEnabledChange={onDshWebSearchEnabledChange}
        onClose={() => setShowAppSettings(false)}
      />
    </Suspense>
  );
}
