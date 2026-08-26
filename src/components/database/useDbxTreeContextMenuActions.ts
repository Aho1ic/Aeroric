/**
 * 侧边树上六个 dbx 右键菜单的动作执行器:对象 / 数据库 / schema / 列 / 表子对象 / 对象分组。
 *
 * 从 `DatabaseView.tsx` 抽出:六者形状一致 ——「读一条菜单状态 → 关菜单 → 找连接 → 按 action
 * 分支」,依赖集合又高度重叠(菜单状态、连接列表、一堆 load 与 drop 回调、四个对话框控制器),
 * 所以合成一个显式的 `Deps` 一次传进来,而不是拆成六个 hook 各接一份。
 *
 * 分支顺序、i18n key 与每一处 setState 都与原来逐字一致 —— 好几支分支会重新拉数据,顺序换了
 * 行为就变了;而各分支写的状态子集并不相同(例如对象菜单的 openErDiagram 不动 schema),
 * 所以那段重复的「切连接 + 切库 + 切工作区」没有折成公共 helper。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了各自的依赖数组 ——
 * 这些 setter 的身份本来就不变,行为不受影响。
 */

import { useCallback, type SetStateAction } from "react";

import { useI18n } from "../../i18n";
import { confirm, prompt } from "../../lib/appDialog";
import { databaseApi } from "../../lib/databaseApi";
import type {
  AeroricDbConnectionConfig,
  DbExecuteResult,
  DbQueryResult,
  DbxColumnInfo,
  DbxObjectInfo,
} from "../../types";
import type { DbxGridColumnFuzzyFilters, TableExportFormat } from "./databaseGridState";
import type { WorkspaceTab } from "./databaseWorkspaceStore";
import {
  canRenameDbxObject,
  contextMenuPinnedNodeId,
  dbxCreateTableDraft,
  dbxCreateViewDraft,
  dbxObjectKey,
  dbxObjectRenameType,
  isDbxProcedureObject,
  isDbxTableObject,
  isSqlDbxConnection,
  listAllDbxObjects,
  supportsDbxTableTruncate,
  uniqueDbxObjectName,
  type DatabaseContextMenuState,
  type DbWorkspaceMode,
  type DbxDatabaseContextMenuAction,
  type DbxObjectContextMenuAction,
  type DbxSchemaContextMenuAction,
} from "./databaseViewModel";
import type { CreateSchemaDialogState } from "./useCreateContainerDialogs";
import type { DatabaseExportDialogState } from "./useDatabaseExportDialog";
import type { TableImportDialogState } from "./useTableImportDialog";
import type { TableInfoPanelState } from "./useTableInfoPanel";

/** 六个执行器都对某个对象动手,签名统一成「连接 + 库 + 对象」。 */
type DbxObjectAction = (
  connection: AeroricDbConnectionConfig,
  database: string | null,
  object: DbxObjectInfo,
) => Promise<void>;

export interface DbxTreeContextMenuActionsDeps {
  contextMenu: DatabaseContextMenuState | null;
  setContextMenu: (menu: DatabaseContextMenuState | null) => void;
  dbxConnections: AeroricDbConnectionConfig[];
  /** 重命名后要判断当前打开的是不是同一个对象,只用到 name 与 schema。 */
  activeDbxObject: DbxObjectInfo | null;
  /** 对象菜单的「导出数据库」在菜单没带库名时回退到它。 */
  activeDbxDatabase: string | null;
  dbxObjects: DbxObjectInfo[];
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setActiveDbxDatabase: (database: string | null) => void;
  setActiveDbxSchema: (schema: string | null) => void;
  setActiveDbxObject: (object: DbxObjectInfo | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setWorkspaceTabs: (value: SetStateAction<WorkspaceTab[]>) => void;
  setActiveTabId: (id: string) => void;
  setSql: (sql: string) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  setDbxObjects: (objects: DbxObjectInfo[]) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  togglePinnedTreeNode: (nodeId: string) => void;
  copyNodeName: (name: string) => void;
  loadDbxObject: (
    object: DbxObjectInfo,
    nextPage: number,
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
    whereInput?: string | null,
    orderBy?: string | null,
    pageSize?: number,
    columnFuzzyFiltersOverride?: DbxGridColumnFuzzyFilters,
  ) => Promise<void>;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  loadDbxDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
  loadDbxSchema: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    schemaName: string,
  ) => Promise<void>;
  loadDbxColumnsForTables: (
    objects: DbxObjectInfo[],
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
  ) => Promise<void>;
  showDbxObjectSource: DbxObjectAction;
  showDbxObjectDdl: DbxObjectAction;
  openDbxObjectStructure: DbxObjectAction;
  writeDbxProcedureExecutionDraft: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
  ) => void;
  writeDbxObjectSqlDraft: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    mode: "select" | "insert" | "update",
  ) => void;
  openQueryHistory: () => void;
  exportDbxTableObject: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    format: TableExportFormat,
  ) => Promise<void>;
  copyDbxObjectStructure: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    format: "markdown" | "tsv",
  ) => Promise<void>;
  copyDbxObjectStructureDdl: DbxObjectAction;
  exportDbxObjectStructure: DbxObjectAction;
  dropDbxObject: DbxObjectAction;
  dropDbxDatabase: (connection: AeroricDbConnectionConfig, database: string) => Promise<void>;
  dropDbxSchema: (
    connection: AeroricDbConnectionConfig,
    database: string,
    schemaName: string,
  ) => Promise<void>;
  dropDbxColumn: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    column: DbxColumnInfo,
  ) => Promise<void>;
  dropDbxTableChildObject: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
    childObject: DbxObjectInfo,
  ) => Promise<void>;
  saveDbxDefaultDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
  tableInfo: Pick<TableInfoPanelState, "loadDdlForObject">;
  tableImport: Pick<TableImportDialogState, "open">;
  databaseExport: Pick<DatabaseExportDialogState, "open">;
  createSchema: Pick<CreateSchemaDialogState, "open">;
}

export interface DbxTreeContextMenuActions {
  runDbxObjectContextMenuAction: (action: DbxObjectContextMenuAction) => Promise<void>;
  runDbxDatabaseContextMenuAction: (action: DbxDatabaseContextMenuAction) => Promise<void>;
  runDbxSchemaContextMenuAction: (action: DbxSchemaContextMenuAction) => Promise<void>;
  runDbxColumnContextMenuAction: (
    action: "copyName" | "openFieldLineage" | "dropColumn",
  ) => Promise<void>;
  runDbxTableChildContextMenuAction: (action: "copyName" | "dropTableChildObject") => Promise<void>;
  runDbxObjectGroupContextMenuAction: (
    action: "createTable" | "createView" | "refresh",
  ) => Promise<void>;
}

export function useDbxTreeContextMenuActions(
  deps: DbxTreeContextMenuActionsDeps,
): DbxTreeContextMenuActions {
  const { t } = useI18n();
  const {
    contextMenu,
    setContextMenu,
    dbxConnections,
    activeDbxObject,
    activeDbxDatabase,
    dbxObjects,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setActiveDbxObject,
    setWorkspaceMode,
    setWorkspaceTabs,
    setActiveTabId,
    setSql,
    setSqlResult,
    setQueryResult,
    setDbxObjects,
    setLoading,
    setError,
    togglePinnedTreeNode,
    copyNodeName,
    loadDbxObject,
    loadDbxConnection,
    loadDbxDatabase,
    loadDbxSchema,
    loadDbxColumnsForTables,
    showDbxObjectSource,
    showDbxObjectDdl,
    openDbxObjectStructure,
    writeDbxProcedureExecutionDraft,
    writeDbxObjectSqlDraft,
    openQueryHistory,
    exportDbxTableObject,
    copyDbxObjectStructure,
    copyDbxObjectStructureDdl,
    exportDbxObjectStructure,
    dropDbxObject,
    dropDbxDatabase,
    dropDbxSchema,
    dropDbxColumn,
    dropDbxTableChildObject,
    saveDbxDefaultDatabase,
    tableInfo,
    tableImport,
    databaseExport,
    createSchema,
  } = deps;

  const runDbxObjectContextMenuAction = useCallback(
    async (action: DbxObjectContextMenuAction) => {
      const menu = contextMenu?.kind === "dbx-object" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection || !isSqlDbxConnection(connection)) return;

      if (action === "togglePin") {
        const nodeId = contextMenuPinnedNodeId(menu);
        if (nodeId) togglePinnedTreeNode(nodeId);
        return;
      }
      if (action === "copyName") {
        copyNodeName(dbxObjectKey(menu.object));
        return;
      }
      if (action === "viewData") {
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        await loadDbxObject(menu.object, 1, connection, menu.database);
        return;
      }
      if (action === "editView" || action === "viewSource") {
        await showDbxObjectSource(connection, menu.database, menu.object);
        return;
      }
      if (action === "executeProcedure") {
        if (isDbxProcedureObject(menu.object)) {
          writeDbxProcedureExecutionDraft(connection, menu.database, menu.object);
        }
        return;
      }
      if (action === "editStructure") {
        await openDbxObjectStructure(connection, menu.database, menu.object);
        return;
      }
      if (action === "renameObject") {
        const objectType = dbxObjectRenameType(menu.object);
        if (!objectType || !canRenameDbxObject(connection, menu.object)) return;
        const newName = await prompt(t("database.renameObjectNamePrompt"), {
          title: t("database.renameObjectNamePrompt"),
          defaultValue: menu.object.name,
        });
        if (!newName || newName === menu.object.name) return;
        setLoading(true);
        setError(null);
        try {
          const sql = await databaseApi.dbxBuildRenameObjectSql({
            databaseType: connection.dbType,
            objectType,
            schema: menu.object.schema ?? null,
            oldName: menu.object.name,
            newName,
          });
          const ok = await confirm(
            `${t("database.confirmRenameObject", { oldName: dbxObjectKey(menu.object), newName })}\n\n${sql}`,
            {
              title: t("database.renameObject"),
              kind: "warning",
              okLabel: t("database.renameObject"),
              cancelLabel: t("common.cancel"),
            },
          );
          if (!ok) return;
          await databaseApi.dbxExecuteQuery({
            connectionId: connection.id,
            database: menu.database,
            schema: menu.object.schema ?? null,
            sql,
          });
          await loadDbxConnection(connection);
          if (
            activeDbxObject?.name === menu.object.name &&
            activeDbxObject?.schema === menu.object.schema
          ) {
            setActiveDbxObject({ ...menu.object, name: newName });
          }
        } catch (err) {
          setError(String(err));
        } finally {
          setLoading(false);
        }
        return;
      }
      if (action === "viewDdl") {
        await showDbxObjectDdl(connection, menu.database, menu.object);
        return;
      }
      if (action === "tableInfo") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.object.schema ?? null);
        setActiveDbxObject(menu.object);
        setWorkspaceMode("table-info");
        const tabId = `table-info:${menu.object.name}`;
        setWorkspaceTabs((prev) =>
          prev.some((t) => t.id === tabId)
            ? prev
            : [
                ...prev,
                {
                  id: tabId,
                  mode: "table-info",
                  label: `${t("database.tableProperties")}: ${menu.object.name}`,
                  closable: true,
                },
              ],
        );
        setActiveTabId(tabId);
        await loadDbxColumnsForTables([menu.object], connection, menu.database);
        await tableInfo.loadDdlForObject(connection, menu.database, menu.object);
        return;
      }
      if (
        action === "newQuery" ||
        action === "newSqlSelect" ||
        action === "newSqlInsert" ||
        action === "newSqlUpdate"
      ) {
        writeDbxObjectSqlDraft(
          connection,
          menu.database,
          menu.object,
          action === "newQuery" || action === "newSqlSelect"
            ? "select"
            : action === "newSqlInsert"
              ? "insert"
              : "update",
        );
        return;
      }
      if (action === "queryHistory") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.object.schema ?? null);
        setActiveDbxObject(menu.object);
        openQueryHistory();
        return;
      }
      if (action === "openErDiagram") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setWorkspaceMode("er-diagram");
        await loadDbxColumnsForTables(dbxObjects, connection, menu.database);
        return;
      }
      if (action === "dataCompare") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxObject(menu.object);
        setWorkspaceMode("data-compare");
        return;
      }
      if (action === "importData") {
        await tableImport.open(connection, menu.database, menu.object);
        return;
      }
      if (action === "exportDatabase") {
        const exportDatabase = menu.database || activeDbxDatabase;
        if (!exportDatabase) return;
        await databaseExport.open(connection, exportDatabase, menu.object.schema ?? null, [
          menu.object.name,
        ]);
        return;
      }
      if (action.startsWith("export")) {
        const formatByAction: Partial<Record<typeof action, TableExportFormat>> = {
          exportCsv: "csv",
          exportJson: "json",
          exportMarkdown: "markdown",
          exportInsertSql: "insertSql",
          exportUpdateSql: "updateSql",
          exportXlsx: "xlsx",
        };
        const format = formatByAction[action];
        if (format) await exportDbxTableObject(connection, menu.database, menu.object, format);
        return;
      }
      if (action === "copyStructureTsv" || action === "copyStructureMarkdown") {
        await copyDbxObjectStructure(
          connection,
          menu.database,
          menu.object,
          action === "copyStructureTsv" ? "tsv" : "markdown",
        );
        return;
      }
      if (action === "copyStructureDdl") {
        await copyDbxObjectStructureDdl(connection, menu.database, menu.object);
        return;
      }
      if (action === "exportStructure") {
        await exportDbxObjectStructure(connection, menu.database, menu.object);
        return;
      }
      if (action === "duplicateStructure") {
        if (!isDbxTableObject(menu.object) || connection.readOnly) return;
        const defaultName = uniqueDbxObjectName(
          `${menu.object.name}_copy`,
          menu.object.schema,
          dbxObjects,
        );
        const targetName = await prompt(t("database.duplicateStructureNamePrompt"), {
          title: t("database.duplicateStructureNamePrompt"),
          defaultValue: defaultName,
        });
        if (!targetName || targetName === menu.object.name) return;
        setLoading(true);
        setError(null);
        try {
          const sql = await databaseApi.dbxBuildDuplicateTableStructureSql({
            databaseType: connection.dbType,
            schema: menu.object.schema ?? null,
            sourceName: menu.object.name,
            targetName,
          });
          const ok = await confirm(
            `${t("database.confirmDuplicateStructure", { source: dbxObjectKey(menu.object), target: targetName })}\n\n${sql}`,
            {
              title: t("database.duplicateStructure"),
              kind: "warning",
              okLabel: t("database.duplicateStructure"),
              cancelLabel: t("common.cancel"),
            },
          );
          if (!ok) return;
          await databaseApi.dbxExecuteQuery({
            connectionId: connection.id,
            database: menu.database,
            schema: menu.object.schema ?? null,
            sql,
          });
          await loadDbxConnection(connection);
        } catch (err) {
          setError(String(err));
        } finally {
          setLoading(false);
        }
        return;
      }
      if (action === "dropObject") {
        await dropDbxObject(connection, menu.database, menu.object);
        return;
      }
      if (action === "refresh") {
        await loadDbxConnection(connection);
        return;
      }
      if (!isDbxTableObject(menu.object)) return;
      if (action !== "emptyTable" && action !== "truncateTable" && action !== "dropTable") return;
      if (action === "truncateTable" && !supportsDbxTableTruncate(connection)) return;

      const tableOptions = {
        databaseType: connection.dbType,
        schema: menu.object.schema ?? null,
        tableName: menu.object.name,
      };
      const actionConfig = {
        emptyTable: {
          title: t("database.emptyTable"),
          message: t("database.confirmEmptyTable", { name: dbxObjectKey(menu.object) }),
          okLabel: t("database.emptyTable"),
          buildSql: () => databaseApi.dbxBuildEmptyTableSql(tableOptions),
        },
        truncateTable: {
          title: t("database.truncateTable"),
          message: t("database.confirmTruncateTable", { name: dbxObjectKey(menu.object) }),
          okLabel: t("database.truncateTable"),
          buildSql: () => databaseApi.dbxBuildTruncateTableSql(tableOptions),
        },
        dropTable: {
          title: t("database.dropTable"),
          message: t("database.confirmDropTable", { name: dbxObjectKey(menu.object) }),
          okLabel: t("database.dropTable"),
          buildSql: () => databaseApi.dbxBuildDropTableSql(tableOptions),
        },
      }[action];
      if (!actionConfig) return;

      setLoading(true);
      setError(null);
      try {
        const sql = await actionConfig.buildSql();
        const ok = await confirm(`${actionConfig.message}\n\n${sql}`, {
          title: actionConfig.title,
          kind: "warning",
          okLabel: actionConfig.okLabel,
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database: menu.database,
          sql,
        });
        if (action === "dropTable") {
          await loadDbxConnection(connection);
        } else {
          setActiveDbxConnectionId(connection.id);
          setActiveDbxDatabase(menu.database);
          await loadDbxObject(menu.object, 1, connection, menu.database);
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [
      contextMenu,
      setContextMenu,
      dbxConnections,
      t,
      togglePinnedTreeNode,
      copyNodeName,
      setActiveDbxConnectionId,
      loadDbxObject,
      showDbxObjectSource,
      writeDbxProcedureExecutionDraft,
      openDbxObjectStructure,
      loadDbxConnection,
      activeDbxObject?.name,
      activeDbxObject?.schema,
      showDbxObjectDdl,
      setActiveConnectionId,
      setWorkspaceTabs,
      setActiveTabId,
      loadDbxColumnsForTables,
      tableInfo,
      writeDbxObjectSqlDraft,
      openQueryHistory,
      dbxObjects,
      tableImport,
      activeDbxDatabase,
      databaseExport,
      exportDbxTableObject,
      copyDbxObjectStructure,
      copyDbxObjectStructureDdl,
      exportDbxObjectStructure,
      dropDbxObject,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setActiveDbxSchema,
      setError,
      setLoading,
      setWorkspaceMode,
    ],
  );

  const runDbxDatabaseContextMenuAction = useCallback(
    async (action: DbxDatabaseContextMenuAction) => {
      const menu = contextMenu?.kind === "dbx-database" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection || !isSqlDbxConnection(connection)) return;

      if (action === "togglePin") {
        const nodeId = contextMenuPinnedNodeId(menu);
        if (nodeId) togglePinnedTreeNode(nodeId);
        return;
      }
      if (action === "copyName") {
        copyNodeName(menu.database);
        return;
      }
      if (action === "openObjectBrowser") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode("object-browser");
        return;
      }
      if (action === "newQuery") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setWorkspaceMode("query");
        setSql("");
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "queryHistory") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        openQueryHistory();
        return;
      }
      if (action === "executeSqlFile") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setWorkspaceMode("sql-file");
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "createSchema") {
        createSchema.open(connection, menu.database);
        return;
      }
      if (action === "createTable") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode("query");
        setSql(dbxCreateTableDraft(null));
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "refresh") {
        await loadDbxDatabase(connection, menu.database);
        return;
      }
      if (action === "dataTransfer" || action === "schemaDiff" || action === "dataCompare") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode(
          action === "dataTransfer"
            ? "transfer"
            : action === "schemaDiff"
              ? "schema-diff"
              : "data-compare",
        );
        return;
      }
      if (action === "openErDiagram" || action === "databaseSearch") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode(action === "openErDiagram" ? "er-diagram" : "database-search");
        try {
          const objects = await listAllDbxObjects(connection.id, menu.database, null);
          setDbxObjects(objects);
          if (action === "openErDiagram") {
            await loadDbxColumnsForTables(objects, connection, menu.database);
          }
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "exportDatabase") {
        await databaseExport.open(connection, menu.database, null);
        return;
      }
      if (action === "setDefaultDatabase") {
        await saveDbxDefaultDatabase(connection, menu.database);
        return;
      }
      if (action === "clearDefaultDatabase") {
        await saveDbxDefaultDatabase(connection, null);
        return;
      }
      if (action === "closeDatabaseConnection") {
        await databaseApi.dbxDisconnect(connection.id);
        await loadDbxConnection(connection);
        return;
      }

      await dropDbxDatabase(connection, menu.database);
    },
    [
      contextMenu,
      copyNodeName,
      createSchema,
      dbxConnections,
      dropDbxDatabase,
      loadDbxColumnsForTables,
      loadDbxConnection,
      loadDbxDatabase,
      databaseExport,
      openQueryHistory,
      saveDbxDefaultDatabase,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setContextMenu,
      togglePinnedTreeNode,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setDbxObjects,
      setError,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  const runDbxSchemaContextMenuAction = useCallback(
    async (action: DbxSchemaContextMenuAction) => {
      const menu = contextMenu?.kind === "dbx-schema" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection || !isSqlDbxConnection(connection)) return;

      if (action === "togglePin") {
        const nodeId = contextMenuPinnedNodeId(menu);
        if (nodeId) togglePinnedTreeNode(nodeId);
        return;
      }
      if (action === "copyName") {
        copyNodeName(menu.schema);
        return;
      }
      if (action === "openObjectBrowser") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("object-browser");
        return;
      }
      if (action === "newQuery") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("query");
        setSql("");
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "queryHistory") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        openQueryHistory();
        return;
      }
      if (action === "executeSqlFile") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("sql-file");
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "createTable") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("query");
        setSql(dbxCreateTableDraft(menu.schema));
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "refresh") {
        await loadDbxSchema(connection, menu.database, menu.schema);
        return;
      }
      if (action === "dataTransfer" || action === "schemaDiff" || action === "dataCompare") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode(
          action === "dataTransfer"
            ? "transfer"
            : action === "schemaDiff"
              ? "schema-diff"
              : "data-compare",
        );
        return;
      }
      if (action === "openErDiagram" || action === "databaseSearch") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode(action === "openErDiagram" ? "er-diagram" : "database-search");
        try {
          const objects = await listAllDbxObjects(connection.id, menu.database, menu.schema);
          setDbxObjects(objects);
          if (action === "openErDiagram") {
            await loadDbxColumnsForTables(objects, connection, menu.database);
          }
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "exportDatabase") {
        await databaseExport.open(connection, menu.database, menu.schema);
        return;
      }
      await dropDbxSchema(connection, menu.database, menu.schema);
    },
    [
      contextMenu,
      copyNodeName,
      dbxConnections,
      dropDbxSchema,
      loadDbxColumnsForTables,
      loadDbxSchema,
      databaseExport,
      openQueryHistory,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setContextMenu,
      togglePinnedTreeNode,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setDbxObjects,
      setError,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  const runDbxColumnContextMenuAction = useCallback(
    async (action: "copyName" | "openFieldLineage" | "dropColumn") => {
      const menu = contextMenu?.kind === "dbx-column" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection) return;
      if (action === "copyName") {
        copyNodeName(menu.column.name);
        return;
      }
      if (action === "openFieldLineage") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.object.schema ?? null);
        setActiveDbxObject(menu.object);
        setWorkspaceMode("field-lineage");
        return;
      }
      await dropDbxColumn(connection, menu.database, menu.object, menu.column);
    },
    [
      contextMenu,
      copyNodeName,
      dbxConnections,
      dropDbxColumn,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setContextMenu,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setActiveDbxSchema,
      setWorkspaceMode,
    ],
  );

  const runDbxTableChildContextMenuAction = useCallback(
    async (action: "copyName" | "dropTableChildObject") => {
      const menu = contextMenu?.kind === "dbx-table-child" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection) return;
      if (action === "copyName") {
        copyNodeName(menu.childObject.name);
        return;
      }
      await dropDbxTableChildObject(connection, menu.database, menu.object, menu.childObject);
    },
    [contextMenu, copyNodeName, dbxConnections, dropDbxTableChildObject, setContextMenu],
  );

  const runDbxObjectGroupContextMenuAction = useCallback(
    async (action: "createTable" | "createView" | "refresh") => {
      const menu = contextMenu?.kind === "dbx-object-group" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection || !isSqlDbxConnection(connection)) return;

      if (action === "createTable" || action === "createView") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("query");
        setSql(
          action === "createTable"
            ? dbxCreateTableDraft(menu.schema)
            : dbxCreateViewDraft(menu.schema),
        );
        setSqlResult(null);
        setQueryResult(null);
        return;
      }

      if (menu.schema) {
        await loadDbxSchema(connection, menu.database, menu.schema);
      } else {
        await loadDbxDatabase(connection, menu.database);
      }
    },
    [
      contextMenu,
      dbxConnections,
      loadDbxDatabase,
      loadDbxSchema,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setContextMenu,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  return {
    runDbxObjectContextMenuAction,
    runDbxDatabaseContextMenuAction,
    runDbxSchemaContextMenuAction,
    runDbxColumnContextMenuAction,
    runDbxTableChildContextMenuAction,
    runDbxObjectGroupContextMenuAction,
  };
}
