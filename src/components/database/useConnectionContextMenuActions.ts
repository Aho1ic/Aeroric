/**
 * 侧边树里连接节点与连接分组节点的右键动作执行器。
 *
 * 从 `DatabaseView.tsx` 抽出:连接那支要同时照顾 legacy(`connections`)与 dbx
 * (`dbxConnections`)两套连接 —— 原函数先用同一个 id 在两边各找一次,再在每个分支里
 * 分别 `if (legacy)` / `if (dbx)`,这个「两边都试」的写法保持原样。
 *
 * 分支顺序、i18n key 与每一处 setState 都与原来逐字一致。与原文的差别有两处:
 * 一是那 17 员的内联 action 联合抬成了导出的 `ConnectionContextMenuAction`,成员逐字不变;
 * 二是原先直接闭包捕获的 useState setter 现在从 `deps` 进来,`react-hooks/exhaustive-deps`
 * 不再认得它们是稳定引用,于是补进了依赖数组 —— 这些 setter 的身份本来就不变,行为不受影响。
 */

import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback } from "react";

import { useI18n } from "../../i18n";
import { prompt } from "../../lib/appDialog";
import { databaseApi } from "../../lib/databaseApi";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbExecuteResult,
  DbObject,
  DbQueryResult,
  DbSchema,
  DbxDatabaseInfo,
  DbxObjectInfo,
} from "../../types";
import { supportsDbxUserAdmin } from "./DatabaseUserAdminPanel";
import {
  canCreateDatabaseForConnection,
  contextMenuConnectionId,
  dbxConnectionFinalProxyPort,
  dbxConnectionLocalFilePath,
  defaultSqliteBackupFileName,
  sqliteBackupSourcePath,
  type DatabaseContextMenuState,
  type DbWorkspaceMode,
} from "./databaseViewModel";
import type { CreateDatabaseDialogState } from "./useCreateContainerDialogs";
import type { VisibleDatabasesDialogState } from "./useVisibleDatabasesDialog";

/** 连接节点菜单上的全部动作,成员与抽出前的内联联合逐字一致。 */
export type ConnectionContextMenuAction =
  | "open"
  | "close"
  | "newQuery"
  | "queryHistory"
  | "executeSqlFile"
  | "userAdmin"
  | "createDatabase"
  | "copyFinalProxyPort"
  | "selectVisibleDatabases"
  | "edit"
  | "revealDatabaseFile"
  | "backupSqliteDatabase"
  | "togglePin"
  | "moveToGroup"
  | "refresh"
  | "copy"
  | "delete";

export type ConnectionGroupContextMenuAction =
  | "copyName"
  | "newConnection"
  | "newGroup"
  | "renameGroup"
  | "deleteGroup";

export interface ConnectionContextMenuActionsDeps {
  contextMenu: DatabaseContextMenuState | null;
  setContextMenu: (menu: DatabaseContextMenuState | null) => void;
  connections: DbConnectionConfig[];
  dbxConnections: AeroricDbConnectionConfig[];
  /** 关连接时只用来判断被关的是不是当前那条。 */
  activeConnectionId: string | null;
  activeDbxConnectionId: string | null;
  /** 「在系统文件管理器里显示」要带一个 projectPath,拿不到就退回文件自身路径。 */
  projectRoot: string | undefined;
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setActiveDbxDatabase: (database: string | null) => void;
  setActiveDbxObject: (object: DbxObjectInfo | null) => void;
  setActiveObject: (object: DbObject | null) => void;
  setDbxDatabases: (databases: DbxDatabaseInfo[]) => void;
  setDbxObjects: (objects: DbxObjectInfo[]) => void;
  setSchema: (schema: DbSchema | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  copyNodeName: (name: string) => void;
  openQueryHistory: () => void;
  handleNewQuery: () => void;
  handleExecuteSqlFile: () => void;
  handleSelectConnection: (connection: DbConnectionConfig) => void;
  inspect: (connection: DbConnectionConfig) => Promise<void>;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  createDuckDbAttachedDatabaseFile: (connection: AeroricDbConnectionConfig) => Promise<void>;
  openEditDbxConnectionDialog: (connection: AeroricDbConnectionConfig) => void;
  toggleDbxConnectionPinned: (connection: AeroricDbConnectionConfig) => Promise<void>;
  moveDbxConnectionToGroup: (connection: AeroricDbConnectionConfig) => Promise<void>;
  copyLegacyConnection: (connection: DbConnectionConfig) => void;
  copyDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  handleDeleteConnection: (connectionId: string) => void;
  handleDeleteDbxConnection: (connectionId: string) => Promise<void>;
  openNewConnectionDialog: (connectionGroup?: unknown) => void;
  addExtraDbxConnectionGroup: (groupName: string) => void;
  renameDbxConnectionGroup: (groupName: string) => Promise<void>;
  deleteDbxConnectionGroup: (groupName: string) => Promise<void>;
  createDatabase: Pick<CreateDatabaseDialogState, "open">;
  visibleDatabasesDialog: Pick<VisibleDatabasesDialogState, "open">;
}

export interface ConnectionContextMenuActions {
  runContextMenuAction: (action: ConnectionContextMenuAction) => Promise<void>;
  runConnectionGroupContextMenuAction: (action: ConnectionGroupContextMenuAction) => Promise<void>;
}

export function useConnectionContextMenuActions(
  deps: ConnectionContextMenuActionsDeps,
): ConnectionContextMenuActions {
  const { t } = useI18n();
  const {
    contextMenu,
    setContextMenu,
    connections,
    dbxConnections,
    activeConnectionId,
    activeDbxConnectionId,
    projectRoot,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxObject,
    setActiveObject,
    setDbxDatabases,
    setDbxObjects,
    setSchema,
    setWorkspaceMode,
    setSqlResult,
    setQueryResult,
    setLoading,
    setError,
    copyNodeName,
    openQueryHistory,
    handleNewQuery,
    handleExecuteSqlFile,
    handleSelectConnection,
    inspect,
    loadDbxConnection,
    createDuckDbAttachedDatabaseFile,
    openEditDbxConnectionDialog,
    toggleDbxConnectionPinned,
    moveDbxConnectionToGroup,
    copyLegacyConnection,
    copyDbxConnection,
    handleDeleteConnection,
    handleDeleteDbxConnection,
    openNewConnectionDialog,
    addExtraDbxConnectionGroup,
    renameDbxConnectionGroup,
    deleteDbxConnectionGroup,
    createDatabase,
    visibleDatabasesDialog,
  } = deps;

  const runContextMenuAction = useCallback(
    async (action: ConnectionContextMenuAction) => {
      const menu = contextMenu;
      setContextMenu(null);
      if (!menu) return;
      if (menu.kind === "connection-group") return;
      const menuConnectionId = contextMenuConnectionId(menu);
      if (!menuConnectionId) return;
      const legacy = connections.find((connection) => connection.id === menuConnectionId) ?? null;
      const dbx = dbxConnections.find((connection) => connection.id === menuConnectionId) ?? null;

      if (action === "open") {
        if (legacy) await inspect(legacy);
        if (dbx) await loadDbxConnection(dbx);
        return;
      }
      if (action === "newQuery") {
        if (legacy) handleSelectConnection(legacy);
        if (dbx) await loadDbxConnection(dbx);
        handleNewQuery();
        return;
      }
      if (action === "queryHistory") {
        if (legacy) handleSelectConnection(legacy);
        if (dbx) await loadDbxConnection(dbx);
        openQueryHistory();
        return;
      }
      if (action === "executeSqlFile") {
        if (legacy) handleSelectConnection(legacy);
        if (dbx) await loadDbxConnection(dbx);
        handleExecuteSqlFile();
        return;
      }
      if (action === "userAdmin") {
        if (dbx) await loadDbxConnection(dbx);
        setWorkspaceMode("user-admin");
        setError(
          dbx && supportsDbxUserAdmin(dbx.dbType) ? null : t("database.selectUserAdminConnection"),
        );
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "createDatabase") {
        if (dbx?.dbType === "duckdb") {
          await createDuckDbAttachedDatabaseFile(dbx);
        } else if (dbx && canCreateDatabaseForConnection(dbx)) {
          createDatabase.open(dbx);
        }
        return;
      }
      if (action === "copyFinalProxyPort") {
        const port = dbxConnectionFinalProxyPort(dbx);
        if (port != null) copyNodeName(String(port));
        return;
      }
      if (action === "selectVisibleDatabases") {
        if (dbx) await visibleDatabasesDialog.open(dbx);
        return;
      }
      if (action === "edit") {
        if (dbx) openEditDbxConnectionDialog(dbx);
        return;
      }
      if (action === "revealDatabaseFile") {
        const path = dbxConnectionLocalFilePath(dbx);
        if (!path) return;
        try {
          await invoke("open_in_system_file_manager", { path, projectPath: projectRoot ?? path });
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "backupSqliteDatabase") {
        if (!dbx || !sqliteBackupSourcePath(dbx)) return;
        const destinationPath = await saveDialog({
          defaultPath: defaultSqliteBackupFileName(dbx),
          filters: [{ name: "SQLite", extensions: ["db", "sqlite", "sqlite3"] }],
        });
        if (typeof destinationPath !== "string" || !destinationPath.trim()) return;
        setLoading(true);
        setError(null);
        try {
          await databaseApi.dbxBackupSqliteDatabase(dbx.id, destinationPath.trim());
        } catch (err) {
          setError(String(err));
        } finally {
          setLoading(false);
        }
        return;
      }
      if (action === "togglePin") {
        if (dbx) await toggleDbxConnectionPinned(dbx);
        return;
      }
      if (action === "moveToGroup") {
        if (dbx) await moveDbxConnectionToGroup(dbx);
        return;
      }
      if (action === "refresh") {
        if (legacy) await inspect(legacy);
        if (dbx) await loadDbxConnection(dbx);
        return;
      }
      if (action === "close") {
        if (dbx) await databaseApi.dbxDisconnect(dbx.id);
        if (activeDbxConnectionId === menuConnectionId) {
          setActiveDbxConnectionId(null);
          setDbxDatabases([]);
          setDbxObjects([]);
          setActiveDbxDatabase(null);
          setActiveDbxObject(null);
        }
        if (activeConnectionId === menuConnectionId) {
          setActiveConnectionId(null);
          setSchema(null);
          setActiveObject(null);
          setQueryResult(null);
        }
        return;
      }
      if (action === "copy") {
        if (legacy) copyLegacyConnection(legacy);
        if (dbx) await copyDbxConnection(dbx);
        return;
      }
      if (action === "delete") {
        if (legacy) handleDeleteConnection(legacy.id);
        if (dbx) await handleDeleteDbxConnection(dbx.id);
        return;
      }
    },
    [
      contextMenu,
      setContextMenu,
      connections,
      dbxConnections,
      inspect,
      loadDbxConnection,
      handleSelectConnection,
      handleNewQuery,
      openQueryHistory,
      handleExecuteSqlFile,
      t,
      createDuckDbAttachedDatabaseFile,
      createDatabase,
      copyNodeName,
      visibleDatabasesDialog,
      openEditDbxConnectionDialog,
      projectRoot,
      toggleDbxConnectionPinned,
      moveDbxConnectionToGroup,
      activeDbxConnectionId,
      activeConnectionId,
      setActiveDbxConnectionId,
      setActiveConnectionId,
      copyLegacyConnection,
      copyDbxConnection,
      handleDeleteConnection,
      handleDeleteDbxConnection,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setActiveObject,
      setDbxDatabases,
      setDbxObjects,
      setError,
      setLoading,
      setQueryResult,
      setSchema,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  const runConnectionGroupContextMenuAction = useCallback(
    async (action: "copyName" | "newConnection" | "newGroup" | "renameGroup" | "deleteGroup") => {
      const menu = contextMenu?.kind === "connection-group" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      if (action === "copyName") {
        copyNodeName(menu.groupName);
        return;
      }
      if (action === "newConnection") {
        openNewConnectionDialog(menu.groupName);
        return;
      }
      if (action === "newGroup") {
        const childName = await prompt(t("database.newConnectionGroupPrompt"), {
          title: t("database.newConnectionGroupPrompt"),
          defaultValue: t("database.newConnectionGroupDefault"),
        });
        if (!childName) return;
        addExtraDbxConnectionGroup(`${menu.groupName}/${childName}`);
        return;
      }
      if (action === "renameGroup") {
        await renameDbxConnectionGroup(menu.groupName);
        return;
      }
      await deleteDbxConnectionGroup(menu.groupName);
    },
    [
      addExtraDbxConnectionGroup,
      contextMenu,
      copyNodeName,
      deleteDbxConnectionGroup,
      openNewConnectionDialog,
      renameDbxConnectionGroup,
      setContextMenu,
      t,
    ],
  );

  return { runContextMenuAction, runConnectionGroupContextMenuAction };
}
