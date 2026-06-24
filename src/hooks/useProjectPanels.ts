import { useState, useCallback, useRef } from "react";
import {
  MAIN_EDITOR_GROUP_ID,
  createDefaultEditorGroupsState,
  openFileInEditorGroup,
  splitEditorGroupRight,
  type EditorGroup,
  type EditorGroupId,
  type EditorGroupsState,
  type OpenFileSelection,
  type OpenFileTab,
  type OpenFilesState,
} from "./projectPanelsState";

type RightPanel =
  | "files"
  | "git-changes"
  | "git-history"
  | "search"
  | "problems"
  | "tests"
  | "run"
  | "ssh"
  | "sftp"
  | "database"
  | "docker"
  | "notes"
  | null;

type OpenDiff =
  | { kind: "file"; filePath: string; staged: boolean; label: string }
  | { kind: "commit"; hash: string; message: string }
  | { kind: "commit-file"; hash: string; filePath: string; label: string };

export function useProjectPanels() {
  const [rightPanel, setRightPanel] = useState<RightPanel>(null);
  const [editorGroupsState, setEditorGroupsState] = useState<EditorGroupsState>(() =>
    createDefaultEditorGroupsState(),
  );
  const [openDiff, setOpenDiff] = useState<OpenDiff | null>(null);
  const [rightPanelWidth, setRightPanelWidth] = useState(280);
  const [terminalHeight, setTerminalHeight] = useState(240);
  const rightPanelWidthRef = useRef(rightPanelWidth);
  rightPanelWidthRef.current = rightPanelWidth;
  const terminalHeightRef = useRef(terminalHeight);
  terminalHeightRef.current = terminalHeight;

  const handleTogglePanel = useCallback((panel: Exclude<RightPanel, null>) => {
    setRightPanel((prev) => (prev === panel ? null : panel));
  }, []);

  const openRightPanel = useCallback((panel: Exclude<RightPanel, null>) => {
    setRightPanel(panel);
  }, []);

  const closeRightPanel = useCallback(() => {
    setRightPanel(null);
  }, []);

  const handleFileSelect = useCallback(
    (path: string, name: string, selection?: OpenFileSelection) => {
      setOpenDiff(null);
      setEditorGroupsState((prev) =>
        openFileInEditorGroup(prev, { path, name, selection }, prev.activeGroupId),
      );
    },
    [],
  );

  const resolveEditorGroupId = useCallback(
    (state: EditorGroupsState, groupId?: EditorGroupId): EditorGroupId => {
      if (groupId && state.groups.some((group) => group.id === groupId)) return groupId;
      if (state.groups.some((group) => group.id === state.activeGroupId)) {
        return state.activeGroupId;
      }
      return MAIN_EDITOR_GROUP_ID;
    },
    [],
  );

  const updateEditorGroup = useCallback(
    (groupId: EditorGroupId | undefined, updater: (group: OpenFilesState) => OpenFilesState) => {
      setEditorGroupsState((prev) => {
        const targetGroupId = resolveEditorGroupId(prev, groupId);
        const groups = prev.groups.map((group) => {
          if (group.id !== targetGroupId) return group;
          return { ...updater(group), id: group.id };
        });
        const nextActiveGroupId =
          groups.find((group) => group.id === targetGroupId && group.tabs.length > 0)?.id ??
          groups.find((group) => group.tabs.length > 0)?.id ??
          MAIN_EDITOR_GROUP_ID;
        return {
          activeGroupId: nextActiveGroupId,
          groups,
        };
      });
    },
    [resolveEditorGroupId],
  );

  const handleEditorGroupFocus = useCallback((groupId: EditorGroupId) => {
    setEditorGroupsState((prev) => {
      if (!prev.groups.some((group) => group.id === groupId)) return prev;
      return { ...prev, activeGroupId: groupId };
    });
  }, []);

  const handleSplitEditorGroupRight = useCallback(() => {
    setEditorGroupsState((prev) => splitEditorGroupRight(prev));
  }, []);

  const handleFileTabSelect = useCallback(
    (path: string, groupId?: EditorGroupId) => {
      updateEditorGroup(groupId, (group) => ({
        tabs: group.tabs,
        activePath: group.tabs.some((tab) => tab.path === path) ? path : group.activePath,
      }));
    },
    [updateEditorGroup],
  );

  const handleFileTabClose = useCallback(
    (path: string, groupId?: EditorGroupId) => {
      updateEditorGroup(groupId, (group) => {
        const closingIndex = group.tabs.findIndex((tab) => tab.path === path);
        if (closingIndex === -1) return group;

        const nextTabs = group.tabs.filter((tab) => tab.path !== path);
        const nextActivePath =
          group.activePath !== path
            ? group.activePath
            : (nextTabs[Math.min(closingIndex, nextTabs.length - 1)]?.path ?? null);

        return {
          tabs: nextTabs,
          activePath: nextActivePath,
        };
      });
    },
    [updateEditorGroup],
  );

  const handleCloseOtherFileTabs = useCallback(
    (path: string, groupId?: EditorGroupId) => {
      updateEditorGroup(groupId, (group) => {
        const activeTab = group.tabs.find((tab) => tab.path === path);
        if (!activeTab) return group;
        return {
          tabs: [activeTab],
          activePath: activeTab.path,
        };
      });
    },
    [updateEditorGroup],
  );

  const handleCloseTabsToRight = useCallback(
    (path: string, groupId?: EditorGroupId) => {
      updateEditorGroup(groupId, (group) => {
        const activeIndex = group.tabs.findIndex((tab) => tab.path === path);
        if (activeIndex === -1) return group;

        const nextTabs = group.tabs.slice(0, activeIndex + 1);
        return {
          tabs: nextTabs,
          activePath: nextTabs.some((tab) => tab.path === group.activePath)
            ? group.activePath
            : path,
        };
      });
    },
    [updateEditorGroup],
  );

  const handleCloseAllFileTabs = useCallback(
    (groupId?: EditorGroupId) => {
      updateEditorGroup(groupId, () => ({
        tabs: [],
        activePath: null,
      }));
    },
    [updateEditorGroup],
  );

  const activeEditorGroup =
    editorGroupsState.groups.find((group) => group.id === editorGroupsState.activeGroupId) ??
    editorGroupsState.groups[0] ??
    null;

  const openFiles = activeEditorGroup?.tabs ?? [];
  const activeFilePath = activeEditorGroup?.activePath ?? null;

  const openEditorGroups = editorGroupsState.groups.filter((group) => group.tabs.length > 0);

  const handleLegacyFileTabSelect = useCallback((path: string) => {
    setEditorGroupsState((prev) => ({
      ...prev,
      groups: prev.groups.map((group) => {
        if (group.id !== prev.activeGroupId) return group;
        return {
          ...group,
          activePath: group.tabs.some((tab) => tab.path === path) ? path : group.activePath,
        };
      }),
    }));
  }, []);

  const handleDiffFileSelect = useCallback((filePath: string, staged: boolean, label: string) => {
    setOpenDiff({ kind: "file", filePath, staged, label });
  }, []);

  const handleCommitSelect = useCallback((hash: string, message: string) => {
    setOpenDiff({ kind: "commit", hash, message });
  }, []);

  const handleCommitFileClick = useCallback((hash: string, filePath: string, label: string) => {
    setOpenDiff({ kind: "commit-file", hash, filePath, label });
  }, []);

  const clearFileAndDiff = useCallback(() => {
    setEditorGroupsState(createDefaultEditorGroupsState());
    setOpenDiff(null);
  }, []);

  const handleRightResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = rightPanelWidthRef.current;
    const onMouseMove = (ev: MouseEvent) => {
      const newWidth = Math.max(180, Math.min(600, startWidth + (startX - ev.clientX)));
      setRightPanelWidth(newWidth);
    };
    const onMouseUp = () => {
      document.removeEventListener("mousemove", onMouseMove);
      document.removeEventListener("mouseup", onMouseUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
  }, []);

  const handleTerminalResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startHeight = terminalHeightRef.current;
    const onMouseMove = (ev: MouseEvent) => {
      const newHeight = Math.max(100, Math.min(600, startHeight + (startY - ev.clientY)));
      setTerminalHeight(newHeight);
    };
    const onMouseUp = () => {
      document.removeEventListener("mousemove", onMouseMove);
      document.removeEventListener("mouseup", onMouseUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";
    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
  }, []);

  return {
    rightPanel,
    editorGroups: openEditorGroups,
    activeEditorGroupId: editorGroupsState.activeGroupId,
    openFiles,
    activeFilePath,
    openDiff,
    rightPanelWidth,
    terminalHeight,
    setOpenDiff,
    openRightPanel,
    closeRightPanel,
    handleTogglePanel,
    handleFileSelect,
    handleEditorGroupFocus,
    handleSplitEditorGroupRight,
    handleFileTabSelect,
    handleLegacyFileTabSelect,
    handleFileTabClose,
    handleCloseOtherFileTabs,
    handleCloseTabsToRight,
    handleCloseAllFileTabs,
    handleDiffFileSelect,
    handleCommitSelect,
    handleCommitFileClick,
    clearFileAndDiff,
    handleRightResizeStart,
    handleTerminalResizeStart,
  };
}

export type { RightPanel, OpenDiff, OpenFileTab, OpenFileSelection, EditorGroup, EditorGroupId };
