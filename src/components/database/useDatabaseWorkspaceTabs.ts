import { useCallback, type Dispatch, type SetStateAction } from "react";
import { useDatabaseWorkspaceStore } from "./DatabaseWorkspaceContext";
import type { DbWorkspaceMode, WorkspaceTabContextMenuAction } from "./databaseViewModel";
import type { WorkspaceTab } from "./databaseWorkspaceStore";

/**
 * Owns the coordinated tab/store transitions for the database workspace.
 * Content loading stays in DatabaseView; tab ordering and activation no longer
 * depend on unrelated connection or editor state in that component.
 */
export function useDatabaseWorkspaceTabs(
  setWorkspaceMode: Dispatch<SetStateAction<DbWorkspaceMode>>,
) {
  const workspaceTabs = useDatabaseWorkspaceStore((state) => state.workspace.tabs);
  const setWorkspaceTabs = useDatabaseWorkspaceStore((state) => state.setWorkspaceTabs);
  const activeTabId = useDatabaseWorkspaceStore((state) => state.workspace.activeTabId);
  const setActiveTabId = useDatabaseWorkspaceStore((state) => state.setActiveTabId);
  const shortWorkspaceTabIds = useDatabaseWorkspaceStore((state) => state.workspace.shortTabIds);
  const setShortWorkspaceTabIds = useDatabaseWorkspaceStore((state) => state.setShortTabIds);
  const contextMenu = useDatabaseWorkspaceStore((state) => state.menus.contextMenu);
  const setContextMenu = useDatabaseWorkspaceStore((state) => state.setContextMenu);

  const activateWorkspaceTab = useCallback(
    (tab: WorkspaceTab | undefined) => {
      if (!tab) {
        setActiveTabId("");
        return;
      }
      setActiveTabId(tab.id);
      setWorkspaceMode(tab.mode);
    },
    [setActiveTabId, setWorkspaceMode],
  );

  const closeWorkspaceTab = useCallback(
    (tabId: string) => {
      setWorkspaceTabs((current) => {
        const next = current.filter((tab) => tab.id !== tabId);
        if (activeTabId === tabId) activateWorkspaceTab(next[next.length - 1]);
        return next;
      });
      setShortWorkspaceTabIds((current) => {
        const next = new Set(current);
        next.delete(tabId);
        return next;
      });
    },
    [activeTabId, activateWorkspaceTab, setShortWorkspaceTabIds, setWorkspaceTabs],
  );

  const closeWorkspaceTabs = useCallback(
    (tabIds: Set<string>) => {
      setWorkspaceTabs((current) => {
        const next = current.filter((tab) => !tabIds.has(tab.id));
        if (tabIds.has(activeTabId)) activateWorkspaceTab(next[next.length - 1]);
        return next;
      });
      setShortWorkspaceTabIds((current) => {
        const next = new Set(current);
        tabIds.forEach((tabId) => next.delete(tabId));
        return next;
      });
    },
    [activeTabId, activateWorkspaceTab, setShortWorkspaceTabIds, setWorkspaceTabs],
  );

  const runWorkspaceTabContextMenuAction = useCallback(
    (action: WorkspaceTabContextMenuAction) => {
      const menu = contextMenu?.kind === "workspace-tab" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      if (action === "toggleShortTitle") {
        setShortWorkspaceTabIds((current) => {
          const next = new Set(current);
          if (next.has(menu.tabId)) next.delete(menu.tabId);
          else next.add(menu.tabId);
          return next;
        });
        return;
      }
      if (action === "pinTab") {
        setWorkspaceTabs((current) => {
          const tab = current.find((item) => item.id === menu.tabId);
          return tab ? [tab, ...current.filter((item) => item.id !== menu.tabId)] : current;
        });
        return;
      }
      if (action === "closeTab") {
        closeWorkspaceTab(menu.tabId);
        return;
      }
      if (action === "closeOtherTabs") {
        closeWorkspaceTabs(
          new Set(workspaceTabs.filter((tab) => tab.id !== menu.tabId).map((tab) => tab.id)),
        );
        return;
      }
      if (action === "closeAllTabs") {
        closeWorkspaceTabs(new Set(workspaceTabs.map((tab) => tab.id)));
      }
    },
    [
      closeWorkspaceTab,
      closeWorkspaceTabs,
      contextMenu,
      setContextMenu,
      setShortWorkspaceTabIds,
      setWorkspaceTabs,
      workspaceTabs,
    ],
  );

  return {
    workspaceTabs,
    setWorkspaceTabs,
    activeTabId,
    setActiveTabId,
    shortWorkspaceTabIds,
    contextMenu,
    setContextMenu,
    activateWorkspaceTab,
    closeWorkspaceTab,
    closeWorkspaceTabs,
    runWorkspaceTabContextMenuAction,
  };
}
