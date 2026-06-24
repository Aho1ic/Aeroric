export type OpenFileSelection = {
  line: number;
  column?: number;
};

export type OpenFileTab = {
  path: string;
  name: string;
  selection?: OpenFileSelection;
};

export type OpenFilesState = {
  tabs: OpenFileTab[];
  activePath: string | null;
};

export const MAIN_EDITOR_GROUP_ID = "main";
export const SIDE_EDITOR_GROUP_ID = "side";

export type EditorGroupId = typeof MAIN_EDITOR_GROUP_ID | typeof SIDE_EDITOR_GROUP_ID;

export type EditorGroup = OpenFilesState & {
  id: EditorGroupId;
};

export type EditorGroupsState = {
  groups: EditorGroup[];
  activeGroupId: EditorGroupId;
};

function normalizeOpenFileTab(tab: OpenFileTab): OpenFileTab {
  if (!tab.selection) {
    return { path: tab.path, name: tab.name };
  }
  return tab;
}

export function openFileTab(previous: OpenFilesState, nextTab: OpenFileTab): OpenFilesState {
  const normalizedNextTab = normalizeOpenFileTab(nextTab);
  const existing = previous.tabs.some((tab) => tab.path === nextTab.path);
  const tabs = existing
    ? previous.tabs.map((tab) => (tab.path === nextTab.path ? normalizedNextTab : tab))
    : [...previous.tabs, normalizedNextTab];

  return {
    tabs,
    activePath: nextTab.path,
  };
}

export function createDefaultEditorGroupsState(): EditorGroupsState {
  return {
    activeGroupId: MAIN_EDITOR_GROUP_ID,
    groups: [{ id: MAIN_EDITOR_GROUP_ID, tabs: [], activePath: null }],
  };
}

function emptyEditorGroup(id: EditorGroupId): EditorGroup {
  return { id, tabs: [], activePath: null };
}

function sortEditorGroups(groups: EditorGroup[]): EditorGroup[] {
  return [...groups].sort((a, b) => {
    if (a.id === b.id) return 0;
    return a.id === MAIN_EDITOR_GROUP_ID ? -1 : 1;
  });
}

function ensureEditorGroup(groups: EditorGroup[], groupId: EditorGroupId): EditorGroup[] {
  const withMain = groups.some((group) => group.id === MAIN_EDITOR_GROUP_ID)
    ? groups
    : [emptyEditorGroup(MAIN_EDITOR_GROUP_ID), ...groups];
  if (withMain.some((group) => group.id === groupId)) return sortEditorGroups(withMain);
  return sortEditorGroups([...withMain, emptyEditorGroup(groupId)]);
}

export function openFileInEditorGroup(
  previous: EditorGroupsState,
  nextTab: OpenFileTab,
  groupId: EditorGroupId = previous.activeGroupId,
): EditorGroupsState {
  const groups = ensureEditorGroup(previous.groups, groupId).map((group) => {
    if (group.id !== groupId) return group;
    return { ...openFileTab(group, nextTab), id: group.id };
  });

  return {
    groups,
    activeGroupId: groupId,
  };
}

export function splitEditorGroupRight(previous: EditorGroupsState): EditorGroupsState {
  const activeGroup =
    previous.groups.find((group) => group.id === previous.activeGroupId) ?? previous.groups[0];
  if (!activeGroup || activeGroup.id === SIDE_EDITOR_GROUP_ID) return previous;

  const activeTab =
    activeGroup.tabs.find((tab) => tab.path === activeGroup.activePath) ??
    activeGroup.tabs[activeGroup.tabs.length - 1];
  if (!activeTab) return previous;

  return openFileInEditorGroup(
    {
      activeGroupId: previous.activeGroupId,
      groups: ensureEditorGroup(previous.groups, SIDE_EDITOR_GROUP_ID),
    },
    activeTab,
    SIDE_EDITOR_GROUP_ID,
  );
}
