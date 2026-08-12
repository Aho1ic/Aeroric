import { createContext, useContext, type ReactNode } from "react";
import { useStore } from "zustand";
import type { StoreApi } from "zustand/vanilla";
import type { NotebookPanelState } from "./notebookStore";

const NotebookStoreContext = createContext<StoreApi<NotebookPanelState> | null>(null);

export function NotebookStoreProvider({
  store,
  children,
}: {
  store: StoreApi<NotebookPanelState>;
  children: ReactNode;
}) {
  return <NotebookStoreContext.Provider value={store}>{children}</NotebookStoreContext.Provider>;
}

export function useNotebookStore<Selected>(
  selector: (state: NotebookPanelState) => Selected,
): Selected {
  const store = useContext(NotebookStoreContext);
  if (!store) throw new Error("useNotebookStore must be used within NotebookStoreProvider");
  return useStore(store, selector);
}
