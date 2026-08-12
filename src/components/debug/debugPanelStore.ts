import type { SetStateAction } from "react";
import { createStore } from "zustand/vanilla";
import type { DebugSessionSnapshot, DebugVariable } from "../../types";

export interface DebugPanelUiState {
  sessions: DebugSessionSnapshot[];
  activeDebugId: string | null;
  expandedVariables: Record<string, DebugVariable[]>;
  expandingVariables: Record<string, boolean>;
  watchDraft: string;
  consoleInput: string;
  setSessions: (value: SetStateAction<DebugSessionSnapshot[]>) => void;
  setActiveDebugId: (id: string | null) => void;
  setExpandedVariables: (value: SetStateAction<Record<string, DebugVariable[]>>) => void;
  setExpandingVariables: (value: SetStateAction<Record<string, boolean>>) => void;
  setWatchDraft: (value: string) => void;
  setConsoleInput: (value: string) => void;
  reset: () => void;
}

export function createDebugPanelStore() {
  return createStore<DebugPanelUiState>((set) => ({
    sessions: [],
    activeDebugId: null,
    expandedVariables: {},
    expandingVariables: {},
    watchDraft: "",
    consoleInput: "",
    setSessions: (value) =>
      set((state) => ({ sessions: typeof value === "function" ? value(state.sessions) : value })),
    setActiveDebugId: (activeDebugId) => set({ activeDebugId }),
    setExpandedVariables: (value) =>
      set((state) => ({
        expandedVariables: typeof value === "function" ? value(state.expandedVariables) : value,
      })),
    setExpandingVariables: (value) =>
      set((state) => ({
        expandingVariables: typeof value === "function" ? value(state.expandingVariables) : value,
      })),
    setWatchDraft: (watchDraft) => set({ watchDraft }),
    setConsoleInput: (consoleInput) => set({ consoleInput }),
    reset: () =>
      set({
        sessions: [],
        activeDebugId: null,
        expandedVariables: {},
        expandingVariables: {},
        watchDraft: "",
        consoleInput: "",
      }),
  }));
}
