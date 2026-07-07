import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as Select from "@radix-ui/react-select";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeftRight,
  ArrowRight,
  ArrowUp,
  Archive,
  Check,
  ChevronRight,
  ChevronDown,
  Code2,
  Copy,
  Cpu,
  Database,
  File,
  FileImage,
  FileJson,
  FileText,
  Folder,
  FolderPlus,
  HardDrive,
  MoveRight,
  Pencil,
  Package,
  RefreshCw,
  Search,
  Server,
  Trash2,
  Video,
  X,
} from "lucide-react";
import type { SshConnection, ThemeVariant } from "../../types";
import { useI18n } from "../../i18n";
import { useToast } from "../Toast";
import { writeClipboardText } from "../file-explorer/clipboard";
import {
  createSftpDirectory,
  deleteSftpPaths,
  readSftpDir,
  renameSftpPath,
  transferSftpPaths,
} from "./sftpOperations";
import { SftpPreview } from "./SftpPreview";
import {
  defaultSftpPathForEndpoint,
  DEFAULT_SFTP_SORT_PREFERENCE,
  filterSftpTreeEntriesByName,
  formatSftpTransferPercent,
  flattenSftpTreeEntries,
  groupSftpSshConnections,
  normalizeSftpSortPreference,
  pruneExpandedPathsForFolderSelection,
  sftpBreadcrumbSegments,
  sftpClickAction,
  sftpFileIconKind,
  sftpFileName,
  sftpKeyAction,
  sftpParentPath,
  sftpProgressRingBackground,
  shouldPromptForUnknownSftpConflict,
  sortSftpEntries,
  type SftpConflictStrategy,
  type SftpEndpoint,
  type SftpEntry,
  type SftpOperation,
  type SftpSortDirection,
  type SftpSortField,
  type SftpSortPreference,
} from "./sftpTypes";
import s from "../../styles";

type PaneSide = "left" | "right";

type PaneState = {
  endpoint: SftpEndpoint;
  entries: SftpEntry[];
  loading: boolean;
  error: string | null;
  selectedPath: string | null;
  pathInput: string;
  editingPath: boolean;
  configured: boolean;
  expandedPaths: Set<string>;
  childrenByPath: Map<string, SftpEntry[]>;
  loadingChildren: Set<string>;
  childErrors: Map<string, string>;
  sortField: SftpSortField;
  sortDirection: SftpSortDirection;
};

type SftpProjectConfigContext =
  | { kind: "local"; projectPath: string }
  | { kind: "ssh"; connection: SshConnection; projectPath: string };

type DragPayload = {
  side: PaneSide;
  endpoint: SftpEndpoint;
  paths: string[];
};

type ClipboardPayload = {
  endpoint: SftpEndpoint;
  paths: string[];
  sourceSide: PaneSide;
};

type PreviewTarget = {
  endpoint: SftpEndpoint;
  path: string;
  isDir: boolean;
} | null;

type TransferConflict = {
  operation: SftpOperation;
  sourceEndpoint: SftpEndpoint;
  sourceSide: PaneSide;
  targetSide: PaneSide;
  paths: string[];
  targetEndpoint: SftpEndpoint;
} | null;

type TransferTask = {
  id: string;
  operation: SftpOperation;
  names: string[];
  status: "running" | "completed" | "failed";
  completed: number;
  total: number;
  progress: number;
};

function endpointLabel(endpoint: SftpEndpoint): string {
  return endpoint.kind === "local" ? "Local" : endpoint.connectionName;
}

function endpointStorageValue(endpoint: SftpEndpoint): string {
  return endpoint.kind === "local" ? "local" : `ssh:${endpoint.connectionId}`;
}

function endpointFromStorageValue(
  value: string,
  currentPath: string,
  connections: SshConnection[],
  localDefaultPath: string,
): SftpEndpoint {
  if (value === "local") {
    void currentPath;
    return { kind: "local", path: localDefaultPath };
  }
  const connectionId = value.replace(/^ssh:/, "");
  const connection = connections.find((item) => item.id === connectionId) ?? connections[0];
  if (!connection) return { kind: "local", path: localDefaultPath };
  return {
    kind: "ssh",
    connectionId: connection.id,
    connectionName: connection.name,
    path: defaultSftpPathForEndpoint("ssh", connection, localDefaultPath),
  };
}

function makeInitialPane(
  endpoint: SftpEndpoint,
  sortPreference: SftpSortPreference = DEFAULT_SFTP_SORT_PREFERENCE,
): PaneState {
  return {
    endpoint,
    entries: [],
    loading: false,
    error: null,
    selectedPath: null,
    pathInput: endpoint.path,
    editingPath: false,
    configured: false,
    expandedPaths: new Set(),
    childrenByPath: new Map(),
    loadingChildren: new Set(),
    childErrors: new Map(),
    sortField: sortPreference.field,
    sortDirection: sortPreference.direction,
  };
}

function applyPaneSortPreference(pane: PaneState, preference: SftpSortPreference): PaneState {
  return {
    ...pane,
    sortField: preference.field,
    sortDirection: preference.direction,
    entries: sortSftpEntries(pane.entries, preference.field, preference.direction),
    childrenByPath: new Map(
      Array.from(pane.childrenByPath.entries()).map(([path, entries]) => [
        path,
        sortSftpEntries(entries, preference.field, preference.direction),
      ]),
    ),
  };
}

function cloneSet<T>(value: Set<T>): Set<T> {
  return new Set(value);
}

function cloneMap<K, V>(value: Map<K, V>): Map<K, V> {
  return new Map(value);
}

function EntryIcon({ entry }: { entry: SftpEntry }) {
  const kind = sftpFileIconKind(entry);
  const iconProps = { size: 14, strokeWidth: 1.8 };
  return (
    <span className={`sftp-entry-icon ${kind}`}>
      {kind === "folder" && <Folder {...iconProps} />}
      {kind === "database" && <Database {...iconProps} />}
      {kind === "model" && <Cpu {...iconProps} />}
      {kind === "video" && <Video {...iconProps} />}
      {kind === "package" && <Package {...iconProps} />}
      {kind === "image" && <FileImage {...iconProps} />}
      {kind === "markdown" && <FileText {...iconProps} />}
      {kind === "json" && <FileJson {...iconProps} />}
      {kind === "archive" && <Archive {...iconProps} />}
      {kind === "code" && <Code2 {...iconProps} />}
      {kind === "text" && <FileText {...iconProps} />}
      {kind === "file" && <File {...iconProps} />}
    </span>
  );
}

function formatSize(size?: number | null): string {
  if (!size) return "";
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

export function SftpPanel({
  sshConnections,
  localDefaultPath,
  active: _active,
  width,
  themeVariant,
  currentSshConnectionId,
  onClose,
  projectConfig,
}: {
  sshConnections: SshConnection[];
  localDefaultPath: string;
  active: boolean;
  width?: number | string;
  themeVariant: ThemeVariant;
  currentSshConnectionId?: string;
  onClose?: () => void;
  projectConfig?: SftpProjectConfigContext;
}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [defaultSort, setDefaultSort] = useState<SftpSortPreference>(DEFAULT_SFTP_SORT_PREFERENCE);
  const [left, setLeft] = useState(() =>
    makeInitialPane({ kind: "local", path: localDefaultPath }, defaultSort),
  );
  const [right, setRight] = useState(() => {
    const first =
      sshConnections.find((connection) => connection.id === currentSshConnectionId) ??
      sshConnections[0];
    if (!first) return makeInitialPane({ kind: "local", path: localDefaultPath }, defaultSort);
    return makeInitialPane(
      {
        kind: "ssh",
        connectionId: first.id,
        connectionName: first.name,
        path: defaultSftpPathForEndpoint("ssh", first, localDefaultPath),
      },
      defaultSort,
    );
  });
  const [searchQuery, setSearchQuery] = useState("");
  const [dragPayload, setDragPayload] = useState<DragPayload | null>(null);
  const [clipboardPayload, setClipboardPayload] = useState<ClipboardPayload | null>(null);
  const [focusedSide, setFocusedSide] = useState<PaneSide>("left");
  const [previewTarget, setPreviewTarget] = useState<PreviewTarget>(null);
  const [transferConflict, setTransferConflict] = useState<TransferConflict>(null);
  const [transferTasks, setTransferTasks] = useState<TransferTask[]>([]);
  const sshConnectionGroups = useMemo(
    () => groupSftpSshConnections(sshConnections, t("ssh.defaultGroup")),
    [sshConnections, t],
  );

  const beginTransferTask = useCallback((operation: SftpOperation, paths: string[]) => {
    const total = Math.max(paths.length, 1);
    const task: TransferTask = {
      id: `sftp-transfer-${Date.now()}-${Math.random().toString(36).slice(2)}`,
      operation,
      names: paths.map(sftpFileName),
      status: "running",
      completed: 0,
      total,
      progress: 0,
    };
    setTransferTasks((current) => [task, ...current].slice(0, 8));
    return task.id;
  }, []);

  const finishTransferTask = useCallback((id: string, status: TransferTask["status"]) => {
    setTransferTasks((current) =>
      current.map((task) =>
        task.id === id
          ? {
              ...task,
              status,
              completed: status === "completed" ? task.total : task.completed,
              progress: status === "completed" ? 100 : task.progress,
            }
          : task,
      ),
    );
  }, []);

  const advanceTransferTask = useCallback((id: string, completed: number) => {
    setTransferTasks((current) =>
      current.map((task) =>
        task.id === id
          ? {
              ...task,
              completed: Math.max(0, Math.min(task.total, completed)),
              progress: formatSftpTransferPercent(completed, task.total),
            }
          : task,
      ),
    );
  }, []);

  const transferPathsWithProgress = useCallback(
    async (
      taskId: string,
      operation: SftpOperation,
      sourceEndpoint: SftpEndpoint,
      selectedPaths: string[],
      targetEndpoint: SftpEndpoint,
      conflictStrategy: SftpConflictStrategy,
    ) => {
      for (let index = 0; index < selectedPaths.length; index += 1) {
        await transferSftpPaths(
          operation,
          sourceEndpoint,
          [selectedPaths[index]],
          targetEndpoint,
          sshConnections,
          conflictStrategy,
        );
        advanceTransferTask(taskId, index + 1);
      }
    },
    [advanceTransferTask, sshConnections],
  );

  const updatePane = useCallback((side: PaneSide, updater: (pane: PaneState) => PaneState) => {
    if (side === "left") setLeft(updater);
    else setRight(updater);
  }, []);

  useEffect(() => {
    if (!projectConfig) return;
    let cancelled = false;
    const command =
      projectConfig.kind === "ssh" ? "remote_read_project_config" : "read_project_config";
    const args =
      projectConfig.kind === "ssh"
        ? { connection: projectConfig.connection, remoteProjectPath: projectConfig.projectPath }
        : { projectPath: projectConfig.projectPath };

    invoke<{ editor?: { sftp_sort?: unknown } }>(command, args)
      .then((config) => {
        if (cancelled) return;
        const preference = normalizeSftpSortPreference(config.editor?.sftp_sort);
        setDefaultSort(preference);
        setLeft((pane) => applyPaneSortPreference(pane, preference));
        setRight((pane) => applyPaneSortPreference(pane, preference));
      })
      .catch((error) => {
        console.warn("Failed to read SFTP sort preference", error);
      });

    return () => {
      cancelled = true;
    };
  }, [projectConfig]);

  const refreshPane = useCallback(
    async (side: PaneSide, pane: PaneState) => {
      updatePane(side, (prev) => ({ ...prev, loading: true, error: null }));
      try {
        const entries = await readSftpDir(pane.endpoint, sshConnections);
        updatePane(side, (prev) => ({
          ...prev,
          entries: sortSftpEntries(entries, prev.sortField, prev.sortDirection),
          loading: false,
          error: null,
          selectedPath: entries.some((entry) => entry.path === prev.selectedPath)
            ? prev.selectedPath
            : null,
          configured: true,
          expandedPaths: new Set(),
          childrenByPath: new Map(),
          loadingChildren: new Set(),
          childErrors: new Map(),
        }));
      } catch (error) {
        updatePane(side, (prev) => ({ ...prev, loading: false, error: String(error) }));
      }
    },
    [sshConnections, updatePane],
  );

  const loadChildren = useCallback(
    async (side: PaneSide, endpoint: SftpEndpoint, path: string) => {
      updatePane(side, (prev) => {
        const loadingChildren = cloneSet(prev.loadingChildren);
        const childErrors = cloneMap(prev.childErrors);
        loadingChildren.add(path);
        childErrors.delete(path);
        return { ...prev, loadingChildren, childErrors };
      });
      try {
        const entries = await readSftpDir({ ...endpoint, path }, sshConnections);
        updatePane(side, (prev) => {
          const childrenByPath = cloneMap(prev.childrenByPath);
          const loadingChildren = cloneSet(prev.loadingChildren);
          const childErrors = cloneMap(prev.childErrors);
          childrenByPath.set(path, sortSftpEntries(entries, prev.sortField, prev.sortDirection));
          loadingChildren.delete(path);
          childErrors.delete(path);
          return { ...prev, childrenByPath, loadingChildren, childErrors };
        });
      } catch (error) {
        updatePane(side, (prev) => {
          const loadingChildren = cloneSet(prev.loadingChildren);
          const childErrors = cloneMap(prev.childErrors);
          loadingChildren.delete(path);
          childErrors.set(path, String(error));
          return { ...prev, loadingChildren, childErrors };
        });
      }
    },
    [sshConnections, updatePane],
  );

  const panes = useMemo(() => ({ left, right }), [left, right]);

  const findEntryInPane = useCallback((pane: PaneState, path: string | null) => {
    if (!path) return undefined;
    return flattenSftpTreeEntries(pane.entries, pane.expandedPaths, pane.childrenByPath).find(
      (row) => row.entry.path === path,
    )?.entry;
  }, []);

  const selectedDirectoryEndpoint = useCallback(
    (pane: PaneState): SftpEndpoint => {
      const selected = findEntryInPane(pane, pane.selectedPath);
      if (!selected?.isDir) return pane.endpoint;
      return { ...pane.endpoint, path: selected.path };
    },
    [findEntryInPane],
  );

  const setEndpoint = useCallback(
    (side: PaneSide, endpoint: SftpEndpoint) => {
      updatePane(side, () => makeInitialPane(endpoint, defaultSort));
      setTimeout(() => {
        const nextPane = makeInitialPane(endpoint, defaultSort);
        void refreshPane(side, nextPane);
      }, 0);
    },
    [defaultSort, refreshPane, updatePane],
  );

  const goToPath = useCallback(
    (side: PaneSide) => {
      const pane = panes[side];
      const path = pane.pathInput.trim() || "/";
      const endpoint = { ...pane.endpoint, path };
      setEndpoint(side, endpoint);
    },
    [panes, setEndpoint],
  );

  const startPane = useCallback(
    (side: PaneSide) => {
      const pane = panes[side];
      const path = pane.pathInput.trim() || "/";
      const endpoint = { ...pane.endpoint, path };
      updatePane(side, () => ({ ...makeInitialPane(endpoint, defaultSort), configured: true }));
      setTimeout(() => {
        const nextPane = { ...makeInitialPane(endpoint, defaultSort), configured: true };
        void refreshPane(side, nextPane);
      }, 0);
    },
    [defaultSort, panes, refreshPane, updatePane],
  );

  const selectEntry = useCallback(
    (side: PaneSide, entry: SftpEntry) => {
      updatePane(side, (pane) => ({
        ...pane,
        selectedPath: entry.path,
        expandedPaths: entry.isDir
          ? pruneExpandedPathsForFolderSelection(pane.expandedPaths, entry.path)
          : pane.expandedPaths,
      }));
    },
    [updatePane],
  );

  const toggleExpanded = useCallback(
    (side: PaneSide, entry: SftpEntry) => {
      if (!entry.isDir) return;
      let shouldLoad = false;
      updatePane(side, (pane) => {
        const expandedPaths = cloneSet(pane.expandedPaths);
        if (expandedPaths.has(entry.path)) {
          expandedPaths.delete(entry.path);
        } else {
          expandedPaths.add(entry.path);
          shouldLoad = !pane.childrenByPath.has(entry.path);
        }
        return { ...pane, expandedPaths };
      });
      if (shouldLoad) {
        void loadChildren(side, panes[side].endpoint, entry.path);
      }
    },
    [loadChildren, panes, updatePane],
  );

  const handleEntryClick = useCallback(
    (side: PaneSide, entry: SftpEntry) => {
      const pane = panes[side];
      const action = sftpClickAction({
        isDir: entry.isDir,
        isSelected: pane.selectedPath === entry.path,
      });
      if (action === "toggle") {
        toggleExpanded(side, entry);
        return;
      }
      selectEntry(side, entry);
    },
    [panes, selectEntry, toggleExpanded],
  );

  const openEntry = useCallback(
    (side: PaneSide, entry: SftpEntry) => {
      if (entry.isDir) {
        setEndpoint(side, { ...panes[side].endpoint, path: entry.path });
      } else {
        selectEntry(side, entry);
      }
    },
    [panes, selectEntry, setEndpoint],
  );

  const previewSelected = useCallback(
    (side: PaneSide) => {
      const pane = panes[side];
      if (!pane.selectedPath) return;
      const selected = findEntryInPane(pane, pane.selectedPath);
      if (!selected) return;
      setPreviewTarget({ endpoint: pane.endpoint, path: selected.path, isDir: selected.isDir });
    },
    [findEntryInPane, panes],
  );

  const runTransfer = useCallback(
    async (
      operation: SftpOperation,
      sourceSide: PaneSide,
      targetSide: PaneSide,
      paths?: string[],
      targetOverride?: SftpEndpoint,
      conflictStrategy: SftpConflictStrategy = "fail",
      sourceOverride?: SftpEndpoint,
    ) => {
      const source = panes[sourceSide];
      const target = panes[targetSide];
      const selectedPaths = paths ?? (source.selectedPath ? [source.selectedPath] : []);
      if (selectedPaths.length === 0) return;
      const sourceEndpoint = sourceOverride ?? source.endpoint;
      const targetEndpoint = targetOverride ?? target.endpoint;
      const targetEntries =
        targetEndpoint.path === target.endpoint.path
          ? target.entries
          : target.childrenByPath.get(targetEndpoint.path);
      if (
        conflictStrategy === "fail" &&
        shouldPromptForUnknownSftpConflict(selectedPaths, targetEntries)
      ) {
        setTransferConflict({
          operation,
          sourceEndpoint,
          sourceSide,
          targetSide,
          paths: selectedPaths,
          targetEndpoint,
        });
        return;
      }
      const taskId = beginTransferTask(operation, selectedPaths);
      try {
        await transferPathsWithProgress(
          taskId,
          operation,
          sourceEndpoint,
          selectedPaths,
          targetEndpoint,
          conflictStrategy,
        );
        finishTransferTask(taskId, "completed");
        showToast(t(operation === "copy" ? "sftp.copyDone" : "sftp.moveDone"));
        await refreshPane(sourceSide, source);
        await refreshPane(targetSide, target);
      } catch (error) {
        finishTransferTask(taskId, "failed");
        showToast(t("sftp.operationFailed", { error: String(error) }));
      }
    },
    [
      beginTransferTask,
      finishTransferTask,
      panes,
      refreshPane,
      showToast,
      sshConnections,
      t,
      transferPathsWithProgress,
    ],
  );

  const resolveTransferConflict = useCallback(
    (strategy: "cancel" | "merge" | "replace") => {
      const pending = transferConflict;
      setTransferConflict(null);
      if (!pending || strategy === "cancel") return;
      void runTransfer(
        pending.operation,
        pending.sourceSide,
        pending.targetSide,
        pending.paths,
        pending.targetEndpoint,
        strategy,
        pending.sourceEndpoint,
      );
    },
    [runTransfer, transferConflict],
  );

  const copySelected = useCallback(
    (side: PaneSide) => {
      const pane = panes[side];
      if (!pane.selectedPath) return;
      setClipboardPayload({
        endpoint: pane.endpoint,
        paths: [pane.selectedPath],
        sourceSide: side,
      });
      showToast(t("sftp.copiedToClipboard"));
    },
    [panes, showToast, t],
  );

  const copySelectedPath = useCallback(
    async (side: PaneSide) => {
      const pane = panes[side];
      if (!pane.selectedPath) return;
      try {
        await writeClipboardText(pane.selectedPath);
        showToast(t("file.pathCopied"));
      } catch (error) {
        showToast(t("sftp.operationFailed", { error: String(error) }));
      }
    },
    [panes, showToast, t],
  );

  const pasteIntoPane = useCallback(
    async (side: PaneSide) => {
      if (!clipboardPayload) return;
      const target = panes[side];
      const targetEndpoint = selectedDirectoryEndpoint(target);
      const targetEntries =
        targetEndpoint.path === target.endpoint.path
          ? target.entries
          : target.childrenByPath.get(targetEndpoint.path);
      if (shouldPromptForUnknownSftpConflict(clipboardPayload.paths, targetEntries)) {
        setTransferConflict({
          operation: "copy",
          sourceEndpoint: clipboardPayload.endpoint,
          sourceSide: clipboardPayload.sourceSide,
          targetSide: side,
          paths: clipboardPayload.paths,
          targetEndpoint,
        });
        return;
      }
      const taskId = beginTransferTask("copy", clipboardPayload.paths);
      try {
        await transferPathsWithProgress(
          taskId,
          "copy",
          clipboardPayload.endpoint,
          clipboardPayload.paths,
          targetEndpoint,
          "fail",
        );
        finishTransferTask(taskId, "completed");
        showToast(t("sftp.copyDone"));
        await refreshPane(side, target);
      } catch (error) {
        finishTransferTask(taskId, "failed");
        showToast(t("sftp.operationFailed", { error: String(error) }));
      }
    },
    [
      beginTransferTask,
      clipboardPayload,
      finishTransferTask,
      panes,
      refreshPane,
      selectedDirectoryEndpoint,
      showToast,
      sshConnections,
      t,
      transferPathsWithProgress,
    ],
  );

  const deleteSelected = useCallback(
    async (side: PaneSide) => {
      const pane = panes[side];
      if (!pane.selectedPath) return;
      const ok = await confirm(t("sftp.confirmDelete", { name: sftpFileName(pane.selectedPath) }), {
        title: t("sftp.delete"),
        kind: "warning",
      });
      if (!ok) return;
      try {
        await deleteSftpPaths(pane.endpoint, sshConnections, [pane.selectedPath]);
        showToast(t("sftp.deleteDone"));
        await refreshPane(side, pane);
      } catch (error) {
        showToast(t("sftp.operationFailed", { error: String(error) }));
      }
    },
    [panes, refreshPane, showToast, sshConnections, t],
  );

  const createFolder = useCallback(
    async (side: PaneSide) => {
      const pane = panes[side];
      const name = window.prompt(t("sftp.newFolderName"));
      if (!name) return;
      try {
        await createSftpDirectory(pane.endpoint, sshConnections, name);
        await refreshPane(side, pane);
      } catch (error) {
        showToast(t("sftp.operationFailed", { error: String(error) }));
      }
    },
    [panes, refreshPane, showToast, sshConnections, t],
  );

  const renameSelected = useCallback(
    async (side: PaneSide) => {
      const pane = panes[side];
      if (!pane.selectedPath) return;
      const name = window.prompt(t("sftp.renameTo"), sftpFileName(pane.selectedPath));
      if (!name) return;
      try {
        await renameSftpPath(pane.endpoint, sshConnections, pane.selectedPath, name);
        await refreshPane(side, pane);
      } catch (error) {
        showToast(t("sftp.operationFailed", { error: String(error) }));
      }
    },
    [panes, refreshPane, showToast, sshConnections, t],
  );

  const renderPane = (side: PaneSide) => {
    const pane = panes[side];
    const opposite: PaneSide = side === "left" ? "right" : "left";
    const selectedName = pane.selectedPath ? sftpFileName(pane.selectedPath) : null;
    const filteredTree = filterSftpTreeEntriesByName(
      pane.entries,
      pane.childrenByPath,
      searchQuery,
    );
    const expandedPaths = searchQuery.trim()
      ? new Set([...pane.expandedPaths, ...filteredTree.childrenByPath.keys()])
      : pane.expandedPaths;
    const flattenedEntries = flattenSftpTreeEntries(
      filteredTree.entries,
      expandedPaths,
      filteredTree.childrenByPath,
      pane.sortField,
      pane.sortDirection,
    );
    return (
      <div className={`sftp-pane${focusedSide === side ? " focused" : ""}`}>
        <div className="sftp-pane-status">
          <span className="sftp-endpoint-badge">
            {pane.endpoint.kind === "local" ? <HardDrive size={13} /> : <Server size={13} />}
            {endpointLabel(pane.endpoint)}
          </span>
          <span className="sftp-selection-label">{selectedName ?? t("sftp.noSelection")}</span>
        </div>
        <div className="sftp-pane-toolbar">
          <Select.Root
            value={endpointStorageValue(pane.endpoint)}
            onValueChange={(value) => {
              const nextEndpoint = endpointFromStorageValue(
                value,
                pane.endpoint.path,
                sshConnections,
                localDefaultPath,
              );
              updatePane(side, () => makeInitialPane(nextEndpoint));
            }}
          >
            <Select.Trigger aria-label={t("sftp.location")} className="sftp-select-trigger">
              <Select.Value>{endpointLabel(pane.endpoint)}</Select.Value>
              <Select.Icon asChild>
                <ChevronDown size={13} />
              </Select.Icon>
            </Select.Trigger>
            <Select.Portal>
              <Select.Content position="popper" sideOffset={4} style={s.settingsSelectContent}>
                <Select.Viewport style={s.settingsSelectViewport}>
                  <Select.Item value="local" style={s.fileSearchTypeItem}>
                    <Select.ItemText>{t("sftp.local")}</Select.ItemText>
                    <Select.ItemIndicator style={s.settingsSelectIndicator}>
                      <Check size={13} />
                    </Select.ItemIndicator>
                  </Select.Item>
                  {sshConnectionGroups.map((group) => (
                    <Select.Group key={group.label}>
                      <Select.Label className="radix-select-label">{group.label}</Select.Label>
                      {group.connections.map((connection) => (
                        <Select.Item
                          key={connection.id}
                          value={`ssh:${connection.id}`}
                          style={s.fileSearchTypeItem}
                        >
                          <Select.ItemText>{connection.name}</Select.ItemText>
                          <Select.ItemIndicator style={s.settingsSelectIndicator}>
                            <Check size={13} />
                          </Select.ItemIndicator>
                        </Select.Item>
                      ))}
                    </Select.Group>
                  ))}
                </Select.Viewport>
              </Select.Content>
            </Select.Portal>
          </Select.Root>
          <div
            className={`sftp-path-shell${pane.editingPath || !pane.configured ? " editing" : ""}`}
            onClick={(event) => {
              if (event.target === event.currentTarget) {
                updatePane(side, (prev) => ({ ...prev, editingPath: true }));
              }
            }}
          >
            {pane.editingPath || !pane.configured ? (
              <input
                className="sftp-path-input"
                value={pane.pathInput}
                onChange={(event) =>
                  updatePane(side, (prev) => ({ ...prev, pathInput: event.target.value }))
                }
                onBlur={() => updatePane(side, (prev) => ({ ...prev, editingPath: false }))}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    updatePane(side, (prev) => ({ ...prev, editingPath: false }));
                    if (pane.configured) goToPath(side);
                    else startPane(side);
                  }
                }}
                autoFocus={!pane.configured}
              />
            ) : (
              <>
                <div className="sftp-breadcrumbs">
                  {sftpBreadcrumbSegments(pane.endpoint.path).map((segment, index) => (
                    <button
                      key={segment.path}
                      type="button"
                      className="sftp-breadcrumb-btn"
                      onClick={() => setEndpoint(side, { ...pane.endpoint, path: segment.path })}
                    >
                      {index > 0 && <span className="sftp-breadcrumb-sep">/</span>}
                      <span>{segment.label}</span>
                    </button>
                  ))}
                </div>
                <button
                  type="button"
                  className="sftp-path-edit-zone"
                  onClick={() => updatePane(side, (prev) => ({ ...prev, editingPath: true }))}
                  aria-label={t("sftp.editPath")}
                />
              </>
            )}
          </div>
          {!pane.configured && (
            <button
              className="sftp-icon-btn"
              title={t("sftp.openPane")}
              aria-label={t("sftp.openPane")}
              onClick={() => startPane(side)}
            >
              <ArrowRight size={14} />
            </button>
          )}
          <button
            className="sftp-icon-btn"
            title={t("common.refresh")}
            onClick={() => refreshPane(side, pane)}
          >
            <RefreshCw size={14} />
          </button>
        </div>
        {!pane.configured && (
          <div className="sftp-start-panel">
            <div className="sftp-start-title">{t("sftp.chooseStart")}</div>
            <div className="sftp-start-caption">{t("sftp.chooseStartHint")}</div>
          </div>
        )}
        {pane.configured && (
          <>
            <div className="sftp-actions">
              <button
                className={`sftp-action-btn${pane.sortField === "name" ? " active" : ""}`}
                onClick={() =>
                  updatePane(side, (prev) => ({
                    ...prev,
                    sortField: "name",
                    sortDirection: "asc",
                    entries: sortSftpEntries(prev.entries, "name", "asc"),
                    childrenByPath: new Map(
                      Array.from(prev.childrenByPath.entries()).map(([path, entries]) => [
                        path,
                        sortSftpEntries(entries, "name", "asc"),
                      ]),
                    ),
                  }))
                }
              >
                {t("file.sortByName")}
              </button>
              <button
                className={`sftp-action-btn${pane.sortField === "modified" ? " active" : ""}`}
                onClick={() =>
                  updatePane(side, (prev) => ({
                    ...prev,
                    sortField: "modified",
                    sortDirection: "desc",
                    entries: sortSftpEntries(prev.entries, "modified", "desc"),
                    childrenByPath: new Map(
                      Array.from(prev.childrenByPath.entries()).map(([path, entries]) => [
                        path,
                        sortSftpEntries(entries, "modified", "desc"),
                      ]),
                    ),
                  }))
                }
              >
                {t("file.sortByModified")}
              </button>
              <button
                className="sftp-action-btn"
                onClick={() =>
                  updatePane(side, (prev) => {
                    const sortDirection = prev.sortDirection === "asc" ? "desc" : "asc";
                    return {
                      ...prev,
                      sortDirection,
                      entries: sortSftpEntries(prev.entries, prev.sortField, sortDirection),
                      childrenByPath: new Map(
                        Array.from(prev.childrenByPath.entries()).map(([path, entries]) => [
                          path,
                          sortSftpEntries(entries, prev.sortField, sortDirection),
                        ]),
                      ),
                    };
                  })
                }
              >
                {pane.sortDirection === "asc" ? t("file.sortAsc") : t("file.sortDesc")}
              </button>
              <button
                className="sftp-action-btn"
                onClick={() =>
                  setEndpoint(side, { ...pane.endpoint, path: sftpParentPath(pane.endpoint.path) })
                }
              >
                <ArrowUp size={13} />
                {t("sftp.up")}
              </button>
              <button className="sftp-action-btn" onClick={() => createFolder(side)}>
                <FolderPlus size={13} />
                {t("sftp.newFolder")}
              </button>
              <button
                className="sftp-action-btn"
                disabled={!pane.selectedPath}
                onClick={() => renameSelected(side)}
              >
                <Pencil size={13} />
                {t("sftp.rename")}
              </button>
              <button
                className="sftp-action-btn"
                disabled={!pane.selectedPath}
                onClick={() => runTransfer("copy", side, opposite)}
              >
                <Copy size={13} />
                {t("sftp.copyToOther")}
              </button>
              <button
                className="sftp-action-btn"
                disabled={!pane.selectedPath}
                onClick={() => runTransfer("move", side, opposite)}
              >
                <MoveRight size={13} />
                {t("sftp.moveToOther")}
              </button>
              <button
                className="sftp-action-btn danger"
                disabled={!pane.selectedPath}
                onClick={() => deleteSelected(side)}
              >
                <Trash2 size={13} />
              </button>
            </div>
            <div
              tabIndex={0}
              className="sftp-list"
              onFocus={() => setFocusedSide(side)}
              onMouseDown={(event) => {
                event.currentTarget.focus({ preventScroll: true });
              }}
              onKeyDown={(event) => {
                const action = sftpKeyAction(event);
                if (!action) return;
                event.preventDefault();
                if (action === "copy") copySelected(side);
                if (action === "copyPath") void copySelectedPath(side);
                if (action === "paste") void pasteIntoPane(side);
                if (action === "delete") void deleteSelected(side);
                if (action === "preview") previewSelected(side);
              }}
              onDragOver={(event) => {
                if (dragPayload) event.preventDefault();
              }}
              onDrop={(event) => {
                event.preventDefault();
                if (!dragPayload) return;
                void runTransfer("move", dragPayload.side, side, dragPayload.paths);
                setDragPayload(null);
              }}
            >
              <div className="sftp-list-head">
                <span>{t("sftp.name")}</span>
                <span>{t("sftp.size")}</span>
              </div>
              {pane.loading && <div className="sftp-empty">{t("common.loading")}</div>}
              {!pane.loading && pane.error && <div className="sftp-empty error">{pane.error}</div>}
              {!pane.loading && !pane.error && flattenedEntries.length === 0 && (
                <div className="sftp-empty">
                  {searchQuery.trim() ? t("file.searchNoResults") : t("file.emptyDirectory")}
                </div>
              )}
              {!pane.loading &&
                !pane.error &&
                flattenedEntries.map(({ entry, depth }) => (
                  <div
                    key={entry.path}
                    className={`sftp-row${pane.selectedPath === entry.path ? " selected" : ""}`}
                    draggable
                    onClick={(event) => {
                      if (event.detail > 1) return;
                      handleEntryClick(side, entry);
                    }}
                    onDoubleClick={() => openEntry(side, entry)}
                    onDragStart={() =>
                      setDragPayload({ side, endpoint: pane.endpoint, paths: [entry.path] })
                    }
                    onDragEnd={() => setDragPayload(null)}
                    onDragOver={(event) => {
                      if (entry.isDir && dragPayload) event.preventDefault();
                    }}
                    onDrop={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      if (!dragPayload || !entry.isDir) return;
                      const targetEndpoint = { ...pane.endpoint, path: entry.path };
                      void runTransfer(
                        "move",
                        dragPayload.side,
                        side,
                        dragPayload.paths,
                        targetEndpoint,
                      );
                      setDragPayload(null);
                    }}
                  >
                    <span className="sftp-row-name" style={{ paddingLeft: depth * 16 }}>
                      <button
                        type="button"
                        className={`sftp-expand-btn${entry.isDir ? "" : " hidden"}`}
                        disabled={!entry.isDir}
                        onClick={(event) => {
                          event.stopPropagation();
                          toggleExpanded(side, entry);
                        }}
                      >
                        {pane.expandedPaths.has(entry.path) ? (
                          <ChevronDown size={12} />
                        ) : (
                          <ChevronRight size={12} />
                        )}
                      </button>
                      <EntryIcon entry={entry} />
                      {entry.name}
                      {pane.loadingChildren.has(entry.path) && (
                        <span className="sftp-row-hint">{t("common.loading")}</span>
                      )}
                      {pane.childErrors.has(entry.path) && (
                        <span className="sftp-row-error">{pane.childErrors.get(entry.path)}</span>
                      )}
                    </span>
                    <span className="sftp-row-size">
                      {entry.isDir ? "" : formatSize(entry.size)}
                    </span>
                  </div>
                ))}
            </div>
          </>
        )}
      </div>
    );
  };

  const runningTransferCount = transferTasks.filter((task) => task.status === "running").length;
  const lastTransfer = transferTasks[0] ?? null;
  const progressState = runningTransferCount > 0 ? "running" : (lastTransfer?.status ?? "idle");
  const activeProgressPercent =
    progressState === "running"
      ? Math.max(
          0,
          ...transferTasks
            .filter((task) => task.status === "running")
            .map((task) => task.progress),
        )
      : lastTransfer
        ? lastTransfer.progress
        : 0;
  const transferProgressLabel =
    progressState === "running"
      ? t("sftp.transferProgressRunning", { count: runningTransferCount })
      : progressState === "completed"
        ? t("sftp.transferProgressCompleted")
        : progressState === "failed"
          ? t("sftp.transferProgressFailed")
          : t("sftp.transferProgress");
  const transferTaskLine = (task: TransferTask) => {
    const name = task.names.join(", ");
    const percent = `${formatSftpTransferPercent(task.completed, task.total)}%`;
    if (task.status === "running") {
      return `${t(task.operation === "copy" ? "sftp.transferCopying" : "sftp.transferMoving", {
        name,
      })} - ${percent}`;
    }
    if (task.status === "completed") {
      return `${t(task.operation === "copy" ? "sftp.transferCopied" : "sftp.transferMoved", {
        name,
      })} - ${percent}`;
    }
    return `${t("sftp.transferFailed", { name })} - ${percent}`;
  };

  return (
    <div className="sftp-panel" style={{ width }}>
      <div className="sftp-header">
        <div className="sftp-title">
          <Server size={15} />
          {t("sftp.title")}
        </div>
        <div className="sftp-header-actions">
          {transferTasks.length > 0 && (
            <div className="sftp-transfer-progress">
              <button
                type="button"
                className={`sftp-transfer-progress-btn ${progressState}`}
                aria-label={transferProgressLabel}
                aria-busy={runningTransferCount > 0}
                title={transferProgressLabel}
              >
                <span
                  className="sftp-progress-ring"
                  style={{
                    background: sftpProgressRingBackground(
                      activeProgressPercent,
                      progressState === "failed" ? "var(--danger)" : "var(--accent)",
                    ),
                  }}
                  aria-hidden="true"
                />
                <span className="sftp-progress-count">{activeProgressPercent}%</span>
              </button>
              <div className="sftp-transfer-popover" role="status">
                <div className="sftp-transfer-popover-title">{t("sftp.transferTasks")}</div>
                {transferTasks.map((task) => (
                  <div key={task.id} className={`sftp-transfer-task ${task.status}`}>
                    <span className="sftp-transfer-task-dot" aria-hidden="true" />
                    <span>{transferTaskLine(task)}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
          <div className="sftp-transfer-hint">
            <ArrowLeftRight size={13} />
            {focusedSide === "left" ? t("sftp.leftActive") : t("sftp.rightActive")}
          </div>
          {onClose && (
            <button
              className="sftp-icon-btn"
              type="button"
              title={t("common.close")}
              onClick={onClose}
            >
              <X size={14} />
            </button>
          )}
        </div>
      </div>
      <div className="sftp-search-bar">
        <Search size={14} />
        <input
          value={searchQuery}
          onChange={(event) => setSearchQuery(event.target.value)}
          placeholder={t("sftp.searchPlaceholder")}
          aria-label={t("sftp.searchPlaceholder")}
          className="sftp-search-input"
        />
        {searchQuery && (
          <button
            type="button"
            className="sftp-search-clear"
            aria-label={t("common.close")}
            onClick={() => setSearchQuery("")}
          >
            <X size={13} />
          </button>
        )}
      </div>
      <div className="sftp-panes">
        {renderPane("left")}
        {renderPane("right")}
      </div>
      {previewTarget && (
        <div
          className="sftp-preview-overlay"
          role="dialog"
          aria-modal="true"
          aria-label={sftpFileName(previewTarget.path)}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setPreviewTarget(null);
          }}
        >
          <div className={`sftp-preview-dialog${previewTarget.isDir ? " compact" : ""}`}>
            <SftpPreview
              endpoint={previewTarget.endpoint}
              filePath={previewTarget.path}
              isDirectory={previewTarget.isDir}
              connections={sshConnections}
              themeVariant={themeVariant}
              onClose={() => setPreviewTarget(null)}
            />
          </div>
        </div>
      )}
      {transferConflict && (
        <div
          className="sftp-preview-overlay"
          role="dialog"
          aria-modal="true"
          aria-label={t("sftp.conflictTitle")}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) resolveTransferConflict("cancel");
          }}
        >
          <div className="sftp-conflict-dialog">
            <div className="sftp-conflict-title">{t("sftp.conflictTitle")}</div>
            <div className="sftp-conflict-message">{t("sftp.conflictMessage")}</div>
            <div className="sftp-conflict-actions">
              <button
                type="button"
                className="sftp-action-btn"
                onClick={() => resolveTransferConflict("cancel")}
              >
                {t("common.cancel")}
              </button>
              <button
                type="button"
                className="sftp-action-btn"
                onClick={() => resolveTransferConflict("merge")}
              >
                {t("sftp.merge")}
              </button>
              <button
                type="button"
                className="sftp-action-btn danger"
                onClick={() => resolveTransferConflict("replace")}
              >
                {t("sftp.replace")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
