import { create } from "zustand";
import type { ThemeMode, ThemeVariant, FontFamily } from "../../types";

type AppearanceState = {
  themeMode: ThemeMode;
  themeVariant: ThemeVariant;
  terminalFontSize: number;
  uiFontFamily: FontFamily;
  monoFontFamily: FontFamily;
  attentionBadge: boolean;
  taskDisplayWindow: 3 | 7 | 15 | 30 | "all";
  setThemeMode: (mode: ThemeMode) => void;
  setThemeVariant: (variant: ThemeVariant) => void;
  setTerminalFontSize: (size: number) => void;
  setUiFontFamily: (family: FontFamily) => void;
  setMonoFontFamily: (family: FontFamily) => void;
  setAttentionBadge: (value: boolean) => void;
  setTaskDisplayWindow: (value: 3 | 7 | 15 | 30 | "all") => void;
  hydrate: (partial: Partial<Omit<AppearanceState, "hydrate" | keyof AppearanceActions>>) => void;
};

type AppearanceActions = Pick<
  AppearanceState,
  | "setThemeMode"
  | "setThemeVariant"
  | "setTerminalFontSize"
  | "setUiFontFamily"
  | "setMonoFontFamily"
  | "setAttentionBadge"
  | "setTaskDisplayWindow"
>;

export const useAppearanceStore = create<AppearanceState>()((set) => ({
  themeMode: "system",
  themeVariant: "light",
  terminalFontSize: 11,
  uiFontFamily: "system",
  monoFontFamily: "system",
  attentionBadge: true,
  taskDisplayWindow: 7,
  setThemeMode: (themeMode) => set({ themeMode }),
  setThemeVariant: (themeVariant) => set({ themeVariant }),
  setTerminalFontSize: (terminalFontSize) => set({ terminalFontSize }),
  setUiFontFamily: (uiFontFamily) => set({ uiFontFamily }),
  setMonoFontFamily: (monoFontFamily) => set({ monoFontFamily }),
  setAttentionBadge: (attentionBadge) => set({ attentionBadge }),
  setTaskDisplayWindow: (taskDisplayWindow) => set({ taskDisplayWindow }),
  hydrate: (partial) => set(partial),
}));
