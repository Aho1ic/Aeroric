import { useCallback, useMemo, useState } from "react";
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
  File,
  FileImage,
  FileJson,
  FileText,
  Folder,
  FolderPlus,
  HardDrive,
  MoveRight,
  Pencil,
  RefreshCw,
  Server,
  Trash2,
  X,
} from "lucide-react";
import type { SshConnection, ThemeVariant } from "../../types";
import { useI18n } from "../../i18n";
import { useToast } from "../Toast";
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
  flattenSftpTreeEntries,
  pruneExpandedPathsForFolderSelection,
  sftpBreadcrumbSegments,
  sftpClickAction,
  sftpFileIconKind,
  sftpFileName,
  sftpKeyAction,
  sftpParentPath,
  type SftpEndpoint,
  type SftpEntry,
  type SftpOperation,
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
};

type DragPayload = {
  side: PaneSide;
  endpoint: SftpEndpoint;
  paths: string[];
};

type ClipboardPayload = {
  endpoint: SftpEndpoint;
  paths: string[];
};

type PreviewTarget = {
  endpoint: SftpEndpoint;
  path: string;
  isDir: boolean;
} | null;

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
  onClose,
}: {
  sshConnections: SshConnection[];
  localDefaultPath: string;
  active: boolean;
  width?: number | string;
  themeVariant: ThemeVariant;
  onClose?: () => void;
}) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [left, setLeft] = useState(() => makeInitialPane({ kind: "local", path: localDefaultPath }));
  const [right, setRight] = useState(() => {
    const first = sshConnections[0];
    if (!first) return makeInitialPane({ kind: "local", path: localDefaultPath });
    return makeInitialPane({
      kind: "ssh",
      connectionId: first.id,
      connectionName: first.name,
      path: defaultSftpPathForEndpoint("ssh", first, localDefaultPath),
    });
  });
  const [dragPayload, setDragPayload] = useState<DragPayload | null>(null);
  const [clipboardPayload, setClipboardPayload] = useState<ClipboardPayload | null>(null);
  const [focusedSide, setFocusedSide] = useState<PaneSide>("left");
  const [previewTarget, setPreviewTarget] = useState<PreviewTarget>(null);

  const updatePane = useCallback((side: PaneSide, updater: (pane: PaneState) => PaneState) => {
    if (side === "left") setLeft(updater);
    else setRight(updater);
  }, []);

  const refreshPane = useCallback(
    async (side: PaneSide, pane: PaneState) => {
      updatePane(side, (prev) => ({ ...prev, loading: true, error: null }));
      try {
        const entries = await readSftpDir(pane.endpoint, sshConnections);
        updatePane(side, (prev) => ({
          ...prev,
          entries,
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
          childrenByPath.set(path, entries);
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

  const panes = useMemo(
    () => ({ left, right }),
    [left, right],
  );

  const findEntryInPane = useCallback((pane: PaneState, path: string | null) => {
    if (!path) return undefined;
    return flattenSftpTreeEntries(pane.entries, pane.expandedPaths, pane.childrenByPath)
      .find((row) => row.entry.path === path)?.entry;
  }, []);

  const selectedDirectoryEndpoint = useCallback((pane: PaneState): SftpEndpoint => {
    const selected = findEntryInPane(pane, pane.selectedPath);
    if (!selected?.isDir) return pane.endpoint;
    return { ...pane.endpoint, path: selected.path };
  }, [findEntryInPane]);

  const setEndpoint = useCallback(
    (side: PaneSide, endpoint: SftpEndpoint) => {
      updatePane(side, () => makeInitialPane(endpoint));
      setTimeout(() => {
        const nextPane = makeInitialPane(endpoint);
        void refreshPane(side, nextPane);
      }, 0);
    },
    [refreshPane, updatePane],
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
      updatePane(side, () => ({ ...makeInitialPane(endpoint), configured: true }));
      setTimeout(() => {
        const nextPane = { ...makeInitialPane(endpoint), configured: true };
        void refreshPane(side, nextPane);
      }, 0);
    },
    [panes, refreshPane, updatePane],
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
    async (operation: SftpOperation, sourceSide: PaneSide, targetSide: PaneSide, paths?: string[]) => {
      const source = panes[sourceSide];
      const target = panes[targetSide];
      const selectedPaths = paths ?? (source.selectedPath ? [source.selectedPath] : []);
      if (selectedPaths.length === 0) return;
      try {
        await transferSftpPaths(operation, source.endpoint, selectedPaths, target.endpoint, sshConnections);
        showToast(t(operation === "copy" ? "sftp.copyDone" : "sftp.moveDone"));
        await refreshPane(sourceSide, source);
        await refreshPane(targetSide, target);
      } catch (error) {
        showToast(t("sftp.operationFailed", { error: String(error) }));
      }
    },
    [panes, refreshPane, showToast, sshConnections, t],
  );

  const copySelected = useCallback(
    (side: PaneSide) => {
      const pane = panes[side];
      if (!pane.selectedPath) return;
      setClipboardPayload({ endpoint: pane.endpoint, paths: [pane.selectedPath] });
      showToast(t("sftp.copiedToClipboard"));
    },
    [panes, showToast, t],
  );

  const pasteIntoPane = useCallback(
    async (side: PaneSide) => {
      if (!clipboardPayload) return;
      const target = panes[side];
      const targetEndpoint = selectedDirectoryEndpoint(target);
      try {
        await transferSftpPaths(
          "copy",
          clipboardPayload.endpoint,
          clipboardPayload.paths,
          targetEndpoint,
          sshConnections,
        );
        showToast(t("sftp.copyDone"));
        await refreshPane(side, target);
      } catch (error) {
        showToast(t("sftp.operationFailed", { error: String(error) }));
      }
    },
    [clipboardPayload, panes, refreshPane, selectedDirectoryEndpoint, showToast, sshConnections, t],
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
    const flattenedEntries = flattenSftpTreeEntries(
      pane.entries,
      pane.expandedPaths,
      pane.childrenByPath,
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
              const nextEndpoint = endpointFromStorageValue(value, pane.endpoint.path, sshConnections, localDefaultPath);
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
                  {sshConnections.map((connection) => (
                    <Select.Item key={connection.id} value={`ssh:${connection.id}`} style={s.fileSearchTypeItem}>
                      <Select.ItemText>{connection.name}</Select.ItemText>
                      <Select.ItemIndicator style={s.settingsSelectIndicator}>
                        <Check size={13} />
                      </Select.ItemIndicator>
                    </Select.Item>
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
          <button className="sftp-icon-btn" title={t("common.refresh")} onClick={() => refreshPane(side, pane)}>
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
                className="sftp-action-btn"
                onClick={() => setEndpoint(side, { ...pane.endpoint, path: sftpParentPath(pane.endpoint.path) })}
              >
                <ArrowUp size={13} />
                {t("sftp.up")}
              </button>
              <button className="sftp-action-btn" onClick={() => createFolder(side)}>
                <FolderPlus size={13} />
                {t("sftp.newFolder")}
              </button>
              <button className="sftp-action-btn" disabled={!pane.selectedPath} onClick={() => renameSelected(side)}>
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
              {!pane.loading && !pane.error && pane.entries.length === 0 && (
                <div className="sftp-empty">{t("file.emptyDirectory")}</div>
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
                      void transferSftpPaths(
                        "move",
                        dragPayload.endpoint,
                        dragPayload.paths,
                        targetEndpoint,
                        sshConnections,
                      )
                        .then(() => refreshPane(side, pane))
                        .then(() => refreshPane(dragPayload.side, panes[dragPayload.side]))
                        .catch((error) => showToast(t("sftp.operationFailed", { error: String(error) })));
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
                    <span className="sftp-row-size">{entry.isDir ? "" : formatSize(entry.size)}</span>
                  </div>
                ))}
            </div>
          </>
        )}
      </div>
    );
  };

  return (
    <div className="sftp-panel" style={{ width }}>
      <div className="sftp-header">
        <div className="sftp-title">
          <Server size={15} />
          {t("sftp.title")}
        </div>
        <div className="sftp-header-actions">
          <div className="sftp-transfer-hint">
            <ArrowLeftRight size={13} />
            {focusedSide === "left" ? t("sftp.leftActive") : t("sftp.rightActive")}
          </div>
          {onClose && (
            <button className="sftp-icon-btn" type="button" title={t("common.close")} onClick={onClose}>
              <X size={14} />
            </button>
          )}
        </div>
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
    </div>
  );
}
