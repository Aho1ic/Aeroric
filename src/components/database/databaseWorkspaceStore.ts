import type { SetStateAction } from "react";
import { createStore } from "zustand/vanilla";
import type { DatabaseContextMenuState, DbWorkspaceMode } from "./databaseViewModel";

export interface WorkspaceTab {
  id: string;
  mode: DbWorkspaceMode;
  label: string;
  closable: boolean;
}

export interface DatabaseWorkspaceState {
  navigation: {
    activeConnectionId: string | null;
    activeDbxConnectionId: string | null;
  };
  workspace: {
    tabs: WorkspaceTab[];
    activeTabId: string;
    shortTabIds: Set<string>;
  };
  dialogs: {
    connectionOpen: boolean;
    editingConnectionId: string | null;
  };
  menus: {
    contextMenu: DatabaseContextMenuState;
  };
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setWorkspaceTabs: (value: SetStateAction<WorkspaceTab[]>) => void;
  setActiveTabId: (id: string) => void;
  setShortTabIds: (value: SetStateAction<Set<string>>) => void;
  setConnectionDialog: (open: boolean, editingId?: string | null) => void;
  setContextMenu: (menu: DatabaseContextMenuState) => void;
  reset: () => void;
}

const initialState = {
  navigation: {
    activeConnectionId: null,
    activeDbxConnectionId: null,
  },
  workspace: {
    tabs: [] as WorkspaceTab[],
    activeTabId: "",
    shortTabIds: new Set<string>(),
  },
  dialogs: {
    connectionOpen: false,
    editingConnectionId: null as string | null,
  },
  menus: {
    contextMenu: null as DatabaseContextMenuState,
  },
};

export function createDatabaseWorkspaceStore() {
  return createStore<DatabaseWorkspaceState>((set) => ({
    ...initialState,
    workspace: { ...initialState.workspace, shortTabIds: new Set() },
    setActiveConnectionId: (id) =>
      set((state) => ({ navigation: { ...state.navigation, activeConnectionId: id } })),
    setActiveDbxConnectionId: (id) =>
      set((state) => ({ navigation: { ...state.navigation, activeDbxConnectionId: id } })),
    setWorkspaceTabs: (value) =>
      set((state) => ({
        workspace: {
          ...state.workspace,
          tabs: typeof value === "function" ? value(state.workspace.tabs) : value,
        },
      })),
    setActiveTabId: (activeTabId) =>
      set((state) => ({ workspace: { ...state.workspace, activeTabId } })),
    setShortTabIds: (value) =>
      set((state) => ({
        workspace: {
          ...state.workspace,
          shortTabIds: typeof value === "function" ? value(state.workspace.shortTabIds) : value,
        },
      })),
    setConnectionDialog: (connectionOpen, editingConnectionId = null) =>
      set({ dialogs: { connectionOpen, editingConnectionId } }),
    setContextMenu: (contextMenu) => set({ menus: { contextMenu } }),
    reset: () =>
      set({
        ...initialState,
        navigation: { ...initialState.navigation },
        workspace: { ...initialState.workspace, shortTabIds: new Set() },
        dialogs: { ...initialState.dialogs },
        menus: { ...initialState.menus },
      }),
  }));
}
