import type { SetStateAction } from "react";
import { createStore } from "zustand/vanilla";

export type NotebookFormat = "markdown" | "richtext";
export interface NotebookNote {
  id: string;
  title: string;
  body: string;
  format: NotebookFormat;
  updatedAt: number;
}

export interface NotebookPanelState {
  notes: NotebookNote[];
  activeId: string | null;
  setNotes: (value: SetStateAction<NotebookNote[]>) => void;
  setActiveId: (id: string | null) => void;
  hydrate: (notes: NotebookNote[]) => void;
  reset: () => void;
}

export function createNotebookStore(initialNotes: NotebookNote[] = []) {
  return createStore<NotebookPanelState>((set) => ({
    notes: initialNotes,
    activeId: initialNotes[0]?.id ?? null,
    setNotes: (value) =>
      set((state) => ({ notes: typeof value === "function" ? value(state.notes) : value })),
    setActiveId: (activeId) => set({ activeId }),
    hydrate: (notes) => set({ notes, activeId: notes[0]?.id ?? null }),
    reset: () => set({ notes: [], activeId: null }),
  }));
}
