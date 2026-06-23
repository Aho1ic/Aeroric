import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { useCancellableInvoke } from "../hooks/useCancellableInvoke";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { ArrowDown, ArrowUp, RotateCcw } from "lucide-react";
import s from "../styles";
import { useToast } from "./Toast";
import { useI18n } from "../i18n";
import { writeClipboardText } from "./file-explorer/clipboard";
import { FileExplorerContextMenu } from "./file-explorer/ContextMenu";
import { CreateInputRow } from "./file-explorer/CreateInputRow";
import { RenameInputRow } from "./file-explorer/RenameInputRow";
import { TreeItem } from "./file-explorer/TreeItem";
import {
  fileExplorerClickAction,
  fileExplorerKeyAction,
  fileExplorerPreviewEndpoint,
  pasteTargetDirectory,
} from "./file-explorer/keyboard";
import { SftpPreview } from "./sftp/SftpPreview";
import type { SftpEndpoint } from "./sftp/sftpTypes";
import {
  type FileSortDirection,
  type FileSortField,
  sortFileEntries,
} from "./file-explorer/fileEntryUtils";
import {
  AUTO_REFRESH_MS,
  ROW_HEIGHT,
  type ContextMenuState,
  type CreateKind,
  type FsEntry,
  type TreeNode,
} from "./file-explorer/types";
import {
  findNode,
  flattenVisible,
  joinPath,
  loadTreeNodes,
  parentPathOf,
  pathSeparator,
  updateNode,
} from "./file-explorer/treeUtils";
import type { SshConnection, ThemeVariant } from "../types";

type RemoteFileContext = {
  connection: SshConnection;
  projectPath: string;
};

type FilePreviewRequest = {
  endpoint: SftpEndpoint;
  filePath: string;
  isDirectory: boolean;
  connections: SshConnection[];
};

function sortTreeNodes(
  nodes: TreeNode[],
  field: FileSortField,
  direction: FileSortDirection,
): TreeNode[] {
  return sortFileEntries(nodes, field, direction).map((node) => {
    if (!node.children) return node;
    const children = sortTreeNodes(node.children, field, direction);
    return children === node.children ? node : { ...node, children };
  });
}

export function FileExplorer({
  projectPath,
  projectName,
  onFileSelect,
  active = true,
  width = 240,
  remote,
  themeVariant,
  onPreviewRequest,
}: {
  projectPath: string;
  projectName: string;
  onFileSelect: (path: string, name: string) => void;
  active?: boolean;
  width?: number;
  remote?: RemoteFileContext;
  themeVariant: ThemeVariant;
  onPreviewRequest?: (request: FilePreviewRequest) => void;
}) {
  const { t } = useI18n();
  const [nodes, setNodes] = useState<TreeNode[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(500);
  const scrollRef = useRef<HTMLDivElement>(null);
  const { showToast } = useToast();
  const [ctxMenu, setCtxMenu] = useState<ContextMenuState | null>(null);
  const [creating, setCreating] = useState<{
    parentPath: string;
    kind: CreateKind;
  } | null>(null);
  const [creatingValue, setCreatingValue] = useState("");
  const [renamingPath, setRenamingPath] = useState<string | null>(null);
  const [renamingValue, setRenamingValue] = useState("");
  const [sortField, setSortField] = useState<FileSortField>("name");
  const [sortDirection, setSortDirection] = useState<FileSortDirection>("asc");
  const [previewTarget, setPreviewTarget] = useState<{ path: string; isDir: boolean } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const commitInFlightRef = useRef(false);
  const deleteInFlightRef = useRef(false);
  const renameInFlightRef = useRef(false);
  const pasteInFlightRef = useRef(false);

  const handleContextMenu = useCallback((e: React.MouseEvent, node: TreeNode) => {
    e.preventDefault();
    e.stopPropagation();
    setCtxMenu({
      x: e.clientX,
      y: e.clientY,
      path: node.path,
      isDir: node.is_dir,
      isRoot: false,
    });
  }, []);

  const handleSortFieldClick = useCallback((field: FileSortField) => {
    if (field === sortField) {
      setSortDirection((current) => (current === "asc" ? "desc" : "asc"));
      return;
    }
    setSortField(field);
    setSortDirection("asc");
  }, [sortField]);

  const renderSortArrow = (field: FileSortField) => {
    if (field !== sortField) return null;
    return sortDirection === "asc" ? (
      <ArrowUp size={11} strokeWidth={2} data-testid="sort-arrow-up" aria-hidden="true" />
    ) : (
      <ArrowDown size={11} strokeWidth={2} data-testid="sort-arrow-down" aria-hidden="true" />
    );
  };

  const handleEmptyContextMenu = useCallback(
    (e: React.MouseEvent) => {
      if (e.target !== e.currentTarget) return;
      e.preventDefault();
      e.stopPropagation();
      setCtxMenu({
        x: e.clientX,
        y: e.clientY,
        path: projectPath,
        isDir: true,
        isRoot: true,
      });
    },
    [projectPath],
  );

  const closeCtxMenu = useCallback(() => setCtxMenu(null), []);

  const openInSystemFolder = useCallback(
    async (event: React.MouseEvent, path: string) => {
      event.preventDefault();
      event.stopPropagation();
      setCtxMenu(null);

      try {
        await invoke("open_in_system_file_manager", { path, projectPath });
      } catch (error) {
        console.error("Failed to open file in system folder", error);
        showToast(t("file.failedOpenSystemFolder", { error: String(error) }));
      }
    },
    [projectPath, showToast, t],
  );

  const copyPath = useCallback(async (event: React.MouseEvent, path: string, withAt: boolean) => {
    event.preventDefault();
    event.stopPropagation();

    try {
      await writeClipboardText(withAt ? `@${path}` : path);
    } catch (error) {
      console.error("Failed to copy file path", error);
    } finally {
      setCtxMenu(null);
    }
  }, []);

  const { safeInvoke, isCancelled } = useCancellableInvoke();
  const nodesRef = useRef<TreeNode[]>([]);
  const refreshIdRef = useRef(0);

  useEffect(() => {
    nodesRef.current = nodes;
  }, [nodes]);

  useEffect(() => {
    setNodes((prev) => sortTreeNodes(prev, sortField, sortDirection));
  }, [sortDirection, sortField]);

  const readEntries = useCallback(
    (path: string) =>
      remote
        ? safeInvoke<FsEntry[]>("remote_read_dir_entries", {
            connection: remote.connection,
            remotePath: path,
            remoteProjectPath: remote.projectPath,
          })
        : safeInvoke<FsEntry[]>("read_dir_entries", { path, projectPath }),
    [projectPath, remote, safeInvoke],
  );

  const refresh = useCallback(
    async (showLoading = false) => {
      const refreshId = refreshIdRef.current + 1;
      refreshIdRef.current = refreshId;
      if (showLoading) setLoading(true);

      try {
        const nextNodes = await loadTreeNodes(projectPath, nodesRef.current, async (path) => {
          const entries = await readEntries(path);
          return entries ? sortFileEntries(entries, sortField, sortDirection) : entries;
        });
        if (nextNodes === null || refreshId !== refreshIdRef.current) return;
        if (nextNodes !== nodesRef.current) {
          setNodes(nextNodes);
        }
        setLoading(false);
      } catch {
        if (!isCancelled() && refreshId === refreshIdRef.current) {
          setLoading(false);
        }
      }
    },
    [isCancelled, projectPath, readEntries, sortDirection, sortField],
  );

  useEffect(() => {
    if (!active) return;
    void refresh(true);
  }, [active, projectPath, refresh]);

  useEffect(() => {
    if (!active) return;

    const handleVisibilityRefresh = () => {
      if (document.visibilityState !== "visible") return;
      void refresh();
    };

    const timer = window.setInterval(() => {
      if (document.visibilityState !== "visible") return;
      void refresh();
    }, AUTO_REFRESH_MS);

    window.addEventListener("focus", handleVisibilityRefresh);
    document.addEventListener("visibilitychange", handleVisibilityRefresh);

    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", handleVisibilityRefresh);
      document.removeEventListener("visibilitychange", handleVisibilityRefresh);
    };
  }, [active, refresh]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    setViewportHeight(el.clientHeight);
    const ro = new ResizeObserver(() => setViewportHeight(el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const flat = useMemo(
    () => flattenVisible(nodes, projectPath, creating),
    [nodes, projectPath, creating],
  );
  const selectedNode = useMemo(
    () => (selectedPath ? findNode(nodes, selectedPath) : null),
    [nodes, selectedPath],
  );

  // The create-input row is rendered outside the virtualized slice (see render block) so its
  // DOM node remains mounted even when scrolled out of view — otherwise the input ref would
  // race with focus/scroll on long trees. We still need its index from `flat` to position it.
  const creatingPlacement = useMemo(() => {
    if (!creating) return null;
    const idx = flat.findIndex((r) => r.kind === "input");
    if (idx < 0) return null;
    const row = flat[idx];
    if (row.kind !== "input") return null;
    return { index: idx, depth: row.depth, kind: row.createKind };
  }, [flat, creating]);

  const renamingPlacement = useMemo(() => {
    if (!renamingPath) return null;
    const idx = flat.findIndex((row) => row.kind === "node" && row.node.path === renamingPath);
    if (idx < 0) return null;
    const row = flat[idx];
    if (row.kind !== "node") return null;
    return { index: idx, depth: row.depth, node: row.node };
  }, [flat, renamingPath]);

  const OVERSCAN = 5;
  const startIdx = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const endIdx = Math.min(
    flat.length - 1,
    Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN,
  );

  const handleToggle = useCallback(
    (dirPath: string) => {
      // Invalidate any in-flight auto-refresh: it captured a snapshot before this
      // toggle and would otherwise apply that stale tree, collapsing the folder the
      // user just expanded (issue #194).
      refreshIdRef.current += 1;

      const current = findNode(nodesRef.current, dirPath);
      const shouldExpand = !current?.expanded;

      setNodes((prev) =>
        updateNode(prev, dirPath, (node) => {
          const nextChildren = shouldExpand ? (node.children ?? []) : node.children;
          if (node.expanded === shouldExpand && node.children === nextChildren) {
            return node;
          }
          return { ...node, expanded: shouldExpand, children: nextChildren };
        }),
      );

      if (!shouldExpand) return;

      void (async () => {
        const currentChildren = findNode(nodesRef.current, dirPath)?.children ?? [];
        const nextChildren = await loadTreeNodes(dirPath, currentChildren, async (path) => {
          const entries = await readEntries(path);
          return entries ? sortFileEntries(entries, sortField, sortDirection) : entries;
        });
        if (nextChildren === null) return;
        setNodes((prev) =>
          updateNode(prev, dirPath, (node) =>
            node.children === nextChildren ? node : { ...node, children: nextChildren },
          ),
        );
      })();
    },
    [readEntries, sortDirection, sortField],
  );

  const handleSelect = useCallback(
    (node: TreeNode) => {
      scrollRef.current?.focus({ preventScroll: true });
      const action = fileExplorerClickAction({ isDir: node.is_dir, isSelected: selectedPath === node.path });
      if (action === "toggle") {
        handleToggle(node.path);
        return;
      }
      if (action === "selectAndToggle") {
        setSelectedPath(node.path);
        handleToggle(node.path);
        return;
      }
      setSelectedPath(node.path);
      if (!node.is_dir) {
        onFileSelect(node.path, node.name);
      }
    },
    [handleToggle, onFileSelect, selectedPath],
  );

  const handleOpen = useCallback(
    (node: TreeNode) => {
      scrollRef.current?.focus({ preventScroll: true });
      if (node.is_dir) {
        handleToggle(node.path);
      } else {
        setSelectedPath(node.path);
        onFileSelect(node.path, node.name);
      }
    },
    [handleToggle, onFileSelect],
  );

  const ensureExpanded = useCallback(
    (dirPath: string) => {
      if (dirPath === projectPath) return;
      const current = findNode(nodesRef.current, dirPath);
      if (!current?.expanded) {
        handleToggle(dirPath);
      }
    },
    [handleToggle, projectPath],
  );

  const startCreate = useCallback(
    (kind: CreateKind) => {
      if (!ctxMenu) return;
      let parentPath: string;
      if (ctxMenu.isRoot) {
        parentPath = projectPath;
      } else if (ctxMenu.isDir) {
        parentPath = ctxMenu.path;
        ensureExpanded(parentPath);
      } else {
        parentPath = parentPathOf(ctxMenu.path);
      }
      setCtxMenu(null);
      setCreatingValue("");
      setCreating({ parentPath, kind });
    },
    [ctxMenu, ensureExpanded, projectPath],
  );

  const cancelCreate = useCallback(() => {
    setCreating(null);
    setCreatingValue("");
  }, []);

  const cancelRename = useCallback(() => {
    setRenamingPath(null);
    setRenamingValue("");
  }, []);

  const commitCreate = useCallback(async () => {
    if (!creating) return;
    if (commitInFlightRef.current) return;
    const name = creatingValue.trim();
    if (!name) {
      cancelCreate();
      return;
    }
    if (name.includes("/") || name.includes("\\")) {
      showToast(t("file.createFailed", { error: "Invalid file name" }));
      return;
    }
    commitInFlightRef.current = true;
    const fullPath = joinPath(creating.parentPath, name);
    const kind = creating.kind;
    const parentPath = creating.parentPath;
    try {
      if (kind === "file") {
        if (remote) {
          await safeInvoke("remote_create_file", {
            connection: remote.connection,
            remotePath: fullPath,
            remoteProjectPath: remote.projectPath,
          });
        } else {
          await safeInvoke("create_file", { path: fullPath, projectPath });
        }
      } else {
        if (remote) {
          await safeInvoke("remote_create_directory", {
            connection: remote.connection,
            remotePath: fullPath,
            remoteProjectPath: remote.projectPath,
          });
        } else {
          await safeInvoke("create_directory", { path: fullPath, projectPath });
        }
      }
      if (isCancelled()) return;
      setCreating(null);
      setCreatingValue("");
      if (parentPath !== projectPath) {
        ensureExpanded(parentPath);
      }
      await refresh();
      if (isCancelled()) return;
      setSelectedPath(fullPath);
      if (kind === "file") {
        onFileSelect(fullPath, name);
      }
    } catch (error) {
      if (!isCancelled()) {
        showToast(t("file.createFailed", { error: String(error) }));
      }
    } finally {
      commitInFlightRef.current = false;
    }
  }, [
    cancelCreate,
    creating,
    creatingValue,
    ensureExpanded,
    isCancelled,
    onFileSelect,
    projectPath,
    refresh,
    remote,
    safeInvoke,
    showToast,
    t,
  ]);

  useEffect(() => {
    if (!creating || !creatingPlacement) return;
    const el = scrollRef.current;
    if (!el) return;
    const rowTop = creatingPlacement.index * ROW_HEIGHT;
    const rowBottom = rowTop + ROW_HEIGHT;
    if (rowTop < el.scrollTop || rowBottom > el.scrollTop + el.clientHeight) {
      const targetTop = Math.max(0, rowTop - el.clientHeight / 2 + ROW_HEIGHT);
      el.scrollTo({ top: targetTop, behavior: "auto" });
    }
  }, [creating, creatingPlacement]);

  useEffect(() => {
    if (!renamingPath || !renamingPlacement) return;
    const el = scrollRef.current;
    if (!el) return;
    const rowTop = renamingPlacement.index * ROW_HEIGHT;
    const rowBottom = rowTop + ROW_HEIGHT;
    if (rowTop < el.scrollTop || rowBottom > el.scrollTop + el.clientHeight) {
      const targetTop = Math.max(0, rowTop - el.clientHeight / 2 + ROW_HEIGHT);
      el.scrollTo({ top: targetTop, behavior: "auto" });
    }
  }, [renamingPath, renamingPlacement]);

  useEffect(() => {
    if (creating && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [creating]);

  useEffect(() => {
    if (renamingPath && renameInputRef.current) {
      renameInputRef.current.focus();
      renameInputRef.current.select();
    }
  }, [renamingPath]);

  const startRenameSelected = useCallback(() => {
    if (!selectedNode) return;
    setCreating(null);
    setCreatingValue("");
    setRenamingPath(selectedNode.path);
    setRenamingValue(selectedNode.name);
  }, [selectedNode]);

  const commitRename = useCallback(async () => {
    if (!renamingPath || !renamingPlacement) return;
    if (renameInFlightRef.current) return;
    const name = renamingValue.trim();
    if (!name || name === renamingPlacement.node.name) {
      cancelRename();
      return;
    }
    if (name.includes("/") || name.includes("\\")) {
      showToast(t("file.renameFailed", { error: "Invalid file name" }));
      return;
    }
    renameInFlightRef.current = true;
    const oldPath = renamingPath;
    const parentPath = parentPathOf(oldPath);
    const nextPath = joinPath(parentPath, name);
    try {
      if (remote) {
        await safeInvoke("remote_rename_path", {
          connection: remote.connection,
          remotePath: oldPath,
          newName: name,
          remoteProjectPath: remote.projectPath,
        });
      } else {
        await safeInvoke("rename_path", { path: oldPath, newName: name, projectPath });
      }
      if (isCancelled()) return;
      cancelRename();
      await refresh();
      if (isCancelled()) return;
      setSelectedPath(nextPath);
      if (!renamingPlacement.node.is_dir) {
        onFileSelect(nextPath, name);
      }
    } catch (error) {
      if (!isCancelled()) {
        showToast(t("file.renameFailed", { error: String(error) }));
      }
    } finally {
      renameInFlightRef.current = false;
    }
  }, [
    cancelRename,
    isCancelled,
    onFileSelect,
    projectPath,
    refresh,
    remote,
    renamingPath,
    renamingPlacement,
    renamingValue,
    safeInvoke,
    showToast,
    t,
  ]);

  const pasteSourcePaths = useCallback(
    async (sourcePaths: string[]) => {
      if (sourcePaths.length === 0) {
        showToast(t("file.pasteNoFiles"), "warning");
        return;
      }
      if (pasteInFlightRef.current) return;
      const targetDirectory = pasteTargetDirectory({
        selectedPath,
        selectedIsDir: selectedNode?.is_dir ?? false,
        rootPath: projectPath,
      });
      pasteInFlightRef.current = true;
      try {
        if (remote) {
          await safeInvoke("remote_upload_local_paths_to_directory", {
            connection: remote.connection,
            localSourcePaths: sourcePaths,
            targetDirectory,
            remoteProjectPath: remote.projectPath,
          });
        } else {
          await safeInvoke("copy_paths_to_directory", {
            sourcePaths,
            targetDirectory,
            projectPath,
          });
        }
        if (isCancelled()) return;
        ensureExpanded(targetDirectory);
        await refresh();
      } catch (error) {
        if (!isCancelled()) {
          showToast(t("file.pasteFailed", { error: String(error) }));
        }
      } finally {
        pasteInFlightRef.current = false;
      }
    },
    [
      ensureExpanded,
      isCancelled,
      projectPath,
      refresh,
      remote,
      safeInvoke,
      selectedNode,
      selectedPath,
      showToast,
      t,
    ],
  );

  const pasteFiles = useCallback(
    async (files: FileList | null) => {
      const sourcePaths = Array.from(files ?? [])
        .map((file) => ("path" in file ? String((file as File & { path?: string }).path ?? "") : ""))
        .filter(Boolean);
      await pasteSourcePaths(sourcePaths);
    },
    [pasteSourcePaths],
  );

  const pasteFromSystemClipboard = useCallback(async () => {
    try {
      const sourcePaths = await safeInvoke<string[]>("read_clipboard_file_paths");
      if (isCancelled() || !sourcePaths) return;
      await pasteSourcePaths(sourcePaths);
    } catch (error) {
      if (!isCancelled()) {
        showToast(t("file.pasteFailed", { error: String(error) }));
      }
    }
  }, [isCancelled, pasteSourcePaths, safeInvoke, showToast, t]);

  const deletePath = useCallback(
    async (targetPath: string, isDir: boolean) => {
      if (deleteInFlightRef.current) return;
      const idx = Math.max(targetPath.lastIndexOf("/"), targetPath.lastIndexOf("\\"));
      const name = idx >= 0 ? targetPath.slice(idx + 1) : targetPath;

      const ok = await confirm(
        t(isDir ? "file.confirmDeleteFolder" : "file.confirmDeleteFile", { name }),
        {
          title: t("file.confirmDeleteTitle", { name }),
          kind: "warning",
          okLabel: t("file.delete"),
        },
      );
      if (!ok) return;

      deleteInFlightRef.current = true;
      try {
        if (remote) {
          await safeInvoke("remote_delete_path", {
            connection: remote.connection,
            remotePath: targetPath,
            remoteProjectPath: remote.projectPath,
          });
        } else {
          await safeInvoke("delete_path", { path: targetPath, projectPath });
        }
        if (isCancelled()) return;
        const sep = pathSeparator(targetPath);
        const descendantPrefix = targetPath + sep;
        setSelectedPath((prev) => {
          if (!prev) return prev;
          if (prev === targetPath) return null;
          if (prev.startsWith(descendantPrefix)) return null;
          return prev;
        });
        await refresh();
      } catch (error) {
        if (!isCancelled()) {
          showToast(t("file.deleteFailed", { error: String(error) }));
        }
      } finally {
        deleteInFlightRef.current = false;
      }
    },
    [isCancelled, projectPath, refresh, remote, safeInvoke, showToast, t],
  );

  const handleTreeKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const action = fileExplorerKeyAction(event);
      if (!action) return;
      if ((event.target as HTMLElement).tagName === "INPUT") return;
      event.preventDefault();
      event.stopPropagation();
      if (action === "copyPath") {
        if (!selectedPath) return;
        void writeClipboardText(selectedPath)
          .then(() => showToast(t("file.pathCopied")))
          .catch((error) => {
            console.error("Failed to copy file path", error);
            showToast(t("file.copyPathFailed", { error: String(error) }));
          });
      } else if (action === "rename") {
        startRenameSelected();
      } else if (action === "paste") {
        void pasteFromSystemClipboard();
      } else if (action === "delete") {
        if (selectedNode) {
          void deletePath(selectedNode.path, selectedNode.is_dir);
        }
      } else if (action === "preview") {
        if (selectedNode) {
          if (onPreviewRequest) {
            const endpoint = fileExplorerPreviewEndpoint({ selectedPath: selectedNode.path, remote });
            if (!endpoint) return;
            const baseEndpoint: SftpEndpoint =
              endpoint.kind === "local"
                ? { kind: "local", path: projectPath }
                : {
                    kind: "ssh",
                    connectionId: endpoint.connection.id,
                    connectionName: endpoint.connection.name,
                    path: remote?.projectPath ?? endpoint.path,
                  };
            onPreviewRequest({
              endpoint: baseEndpoint,
              filePath: selectedNode.path,
              isDirectory: selectedNode.is_dir,
              connections: remote ? [remote.connection] : [],
            });
          } else {
            setPreviewTarget({ path: selectedNode.path, isDir: selectedNode.is_dir });
          }
        }
      }
    },
    [
      deletePath,
      pasteFromSystemClipboard,
      selectedNode,
      selectedPath,
      showToast,
      startRenameSelected,
      t,
      onPreviewRequest,
      projectPath,
      remote,
    ],
  );

  const previewEndpoint = useMemo(
    () => fileExplorerPreviewEndpoint({ selectedPath: previewTarget?.path ?? null, remote }),
    [previewTarget, remote],
  );

  const previewBaseEndpoint = useMemo<SftpEndpoint | null>(() => {
    if (!previewEndpoint) return null;
    if (previewEndpoint.kind === "local") return { kind: "local", path: projectPath };
    return {
      kind: "ssh",
      connectionId: previewEndpoint.connection.id,
      connectionName: previewEndpoint.connection.name,
      path: remote?.projectPath ?? previewEndpoint.path,
    };
  }, [previewEndpoint, projectPath, remote]);

  const handleTreePaste = useCallback(
    (event: React.ClipboardEvent<HTMLDivElement>) => {
      if ((event.target as HTMLElement).tagName === "INPUT") return;
      if (!event.clipboardData.files.length) return;
      event.preventDefault();
      void pasteFiles(event.clipboardData.files);
    },
    [pasteFiles],
  );

  const handleDelete = useCallback(async () => {
    if (!ctxMenu || ctxMenu.isRoot) return;
    const targetPath = ctxMenu.path;
    const isDir = ctxMenu.isDir;
    setCtxMenu(null);
    await deletePath(targetPath, isDir);
  }, [ctxMenu, deletePath]);

  return (
    <div style={{ ...s.fileExplorerRoot, width }}>
      {!onPreviewRequest && previewTarget && previewBaseEndpoint && (
        <div
          className="sftp-preview-overlay"
          role="dialog"
          aria-modal="true"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setPreviewTarget(null);
          }}
        >
          <div className={`sftp-preview-dialog${previewTarget.isDir ? " compact" : ""}`}>
            <SftpPreview
              endpoint={previewBaseEndpoint}
              filePath={previewTarget.path}
              isDirectory={previewTarget.isDir}
              connections={remote ? [remote.connection] : []}
              themeVariant={themeVariant}
              onClose={() => setPreviewTarget(null)}
            />
          </div>
        </div>
      )}
      {ctxMenu && (
        <FileExplorerContextMenu
          ctxMenu={ctxMenu}
          onClose={closeCtxMenu}
          onNewFile={() => startCreate("file")}
          onNewFolder={() => startCreate("folder")}
          onDelete={() => void handleDelete()}
          onOpenInSystem={(event, path) => void openInSystemFolder(event, path)}
          onCopyPath={(event, path, withAt) => void copyPath(event, path, withAt)}
          showOpenInSystem={!remote}
        />
      )}
      {/* Header */}
      <div style={s.fileExplorerHeader}>
        <span style={s.fileExplorerHeaderTitle}>{t("file.files")}</span>
        <div style={s.fileExplorerSortControls}>
          <button
            type="button"
            aria-label={`${t("file.sortByName")} ${
              sortField === "name"
                ? sortDirection === "asc"
                  ? "ascending"
                  : "descending"
                : ""
            }`.trim()}
            style={{
              ...s.fileExplorerSortBtn,
              ...(sortField === "name" ? s.fileExplorerSortBtnActive : undefined),
            }}
            onClick={() => handleSortFieldClick("name")}
          >
            {t("file.sortByName")}
            {renderSortArrow("name")}
          </button>
          <button
            type="button"
            aria-label={`${t("file.sortByModified")} ${
              sortField === "modified"
                ? sortDirection === "asc"
                  ? "ascending"
                  : "descending"
                : ""
            }`.trim()}
            style={{
              ...s.fileExplorerSortBtn,
              ...(sortField === "modified" ? s.fileExplorerSortBtnActive : undefined),
            }}
            onClick={() => handleSortFieldClick("modified")}
          >
            {t("file.sortByModified")}
            {renderSortArrow("modified")}
          </button>
        </div>
        <button
          onClick={() => void refresh()}
          title={t("common.refresh")}
          style={s.fileExplorerRefreshBtn}
          onMouseEnter={(e) => {
            (e.currentTarget as HTMLElement).style.color = "var(--text-primary)";
            (e.currentTarget as HTMLElement).style.background = "var(--bg-hover)";
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLElement).style.color = "var(--text-hint)";
            (e.currentTarget as HTMLElement).style.background = "none";
          }}
        >
          <RotateCcw size={13} />
        </button>
      </div>
      {/* Project root label */}
      <div style={s.fileExplorerRootLabel}>
        <span style={s.fileExplorerRootIcon} />
        {projectName}
      </div>
      {/* Tree */}
      <div
        ref={scrollRef}
        tabIndex={0}
        onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
        onContextMenu={handleEmptyContextMenu}
        onKeyDown={handleTreeKeyDown}
        onPaste={handleTreePaste}
        style={s.fileExplorerTreeScroll}
      >
        {loading ? (
          <div onContextMenu={handleEmptyContextMenu} style={s.fileExplorerEmpty}>
            {t("common.loading")}
          </div>
        ) : flat.length === 0 ? (
          <div onContextMenu={handleEmptyContextMenu} style={s.fileExplorerEmpty}>
            {t("file.emptyDirectory")}
          </div>
        ) : (
          <div
            style={{ position: "relative", height: flat.length * ROW_HEIGHT + 12 }}
            onContextMenu={handleEmptyContextMenu}
          >
            {flat.slice(startIdx, endIdx + 1).map((row, i) => {
              if (row.kind === "input") return null;
              if (row.node.path === renamingPath && renamingPlacement) return null;
              const top = (startIdx + i) * ROW_HEIGHT + 2;
              return (
                <div key={row.node.path} style={{ ...s.fileExplorerVirtualRow, top }}>
                  <TreeItem
                    node={row.node}
                    depth={row.depth}
                    selectedPath={selectedPath}
                    contextPath={ctxMenu?.path ?? null}
                    onSelect={handleSelect}
                    onToggle={handleToggle}
                    onOpen={handleOpen}
                    onContextMenu={handleContextMenu}
                  />
                </div>
              );
            })}
            {creating && creatingPlacement && (
              <div
                key="__create_row__"
                style={{
                  ...s.fileExplorerVirtualRow,
                  top: creatingPlacement.index * ROW_HEIGHT + 2,
                }}
              >
                <CreateInputRow
                  depth={creatingPlacement.depth}
                  kind={creatingPlacement.kind}
                  value={creatingValue}
                  onChange={setCreatingValue}
                  onCommit={() => {
                    void commitCreate();
                  }}
                  onCancel={cancelCreate}
                  inputRef={inputRef}
                />
              </div>
            )}
            {renamingPath && renamingPlacement && (
              <div
                key="__rename_row__"
                style={{
                  ...s.fileExplorerVirtualRow,
                  top: renamingPlacement.index * ROW_HEIGHT + 2,
                }}
              >
                <RenameInputRow
                  node={renamingPlacement.node}
                  depth={renamingPlacement.depth}
                  value={renamingValue}
                  onChange={setRenamingValue}
                  onCommit={() => {
                    void commitRename();
                  }}
                  onCancel={cancelRename}
                  inputRef={renameInputRef}
                />
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
