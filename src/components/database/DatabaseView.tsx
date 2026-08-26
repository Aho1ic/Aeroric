import { useRef, useState } from "react";
import { Database, RefreshCcw } from "lucide-react";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbExecuteResult,
  DatabaseDriverManifest,
  DbxColumnInfo,
  DbxDatabaseInfo,
  DbxObjectInfo,
  DbObject,
  DbQueryResult,
  DbSchema,
  SshConnection,
} from "../../types";
import { useI18n } from "../../i18n";
import { Button as DbxButton } from "../ui/Button";
import { ConnectionDialog } from "./ConnectionDialog";
import s from "../../styles";
import { DatabaseSidebarTree } from "./DatabaseSidebarTree";
import { supportsDbxUserAdmin } from "./DatabaseUserAdminPanel";
import { useDbxDataGrid } from "./useDbxDataGrid";
import { useDbxDataLoaders } from "./useDbxDataLoaders";
import { useDbxGridInputs } from "./useDbxGridInputs";
import { useDbxSelectionDerived } from "./useDbxSelectionDerived";
import { useDatabasePanelActions } from "./useDatabasePanelActions";
import { useDatabaseSidebarResize } from "./useDatabaseSidebarResize";
import { useDatabaseWorkspaceDerived } from "./useDatabaseWorkspaceDerived";
import { useLegacyConnectionLoader } from "./useLegacyConnectionLoader";
import { useConnectionLifecycleActions } from "./useConnectionLifecycleActions";
import { useActiveObjectActions } from "./useActiveObjectActions";
import { useConnectionDialogActions } from "./useConnectionDialogActions";
import { useContextMenuConnections } from "./useContextMenuConnections";
import { useDbxConnectionConfigActions } from "./useDbxConnectionConfigActions";
import { DatabaseWorkspaceProvider, useDatabaseWorkspaceStore } from "./DatabaseWorkspaceContext";
import { createRequestSequence } from "./requestSequence";
import { TableImportDialog } from "./TableImportDialog";
import { useDatabaseExportDialog } from "./useDatabaseExportDialog";
import { useTableInfoPanel } from "./useTableInfoPanel";
import { useTableFooterDerived } from "./useTableFooterDerived";
import { DatabaseExportDialog } from "./DatabaseExportDialog";
import { useVisibleDatabasesDialog } from "./useVisibleDatabasesDialog";
import { VisibleDatabasesDialog } from "./VisibleDatabasesDialog";
import { useTableImportDialog } from "./useTableImportDialog";
import { CreateDatabaseDialog, CreateSchemaDialog } from "./CreateContainerDialogs";
import { useCreateDatabaseDialog, useCreateSchemaDialog } from "./useCreateContainerDialogs";
import { DbxSqlPreviewDialog } from "./DbxSqlPreviewDialog";
import { useDbxSqlPreviewDialog } from "./useDbxSqlPreviewDialog";
import { DbxValuePreviewDialogs } from "./DbxValuePreviewDialogs";
import { DatabaseTopToolbar } from "./DatabaseTopToolbar";
import { DatabaseWorkspaceTabBar, DatabaseWorkspaceTopbar } from "./DatabaseWorkspaceHeader";
import { DatabaseWorkspacePanels } from "./DatabaseWorkspacePanels";
import { DatabaseWorkspaceGridStack } from "./DatabaseWorkspaceGridStack";
import { DbxCellDetailPanel } from "./DbxCellDetailPanel";
import {
  DatabaseExportProgressOverlay,
  type DatabaseExportProgress,
} from "./DatabaseExportProgressOverlay";
import { useDbxGridContextMenuActions } from "./useDbxGridContextMenuActions";
import { useDbxGridEditing } from "./useDbxGridEditing";
import { useDbxObjectOperations } from "./useDbxObjectOperations";
import { useDbxGridPresentation } from "./useDbxGridPresentation";
import { useNoSqlContextMenuActions } from "./useNoSqlContextMenuActions";
import { useNoSqlSidebarData } from "./useNoSqlSidebarData";
import { useConnectionContextMenuActions } from "./useConnectionContextMenuActions";
import { useConnectionOrganizeActions } from "./useConnectionOrganizeActions";
import { useDbxTreeContextMenuActions } from "./useDbxTreeContextMenuActions";
import { useSqlFilePanel } from "./useSqlFilePanel";
import { useSqlEditorActions } from "./useSqlEditorActions";
import { DatabaseContextMenu } from "./DatabaseContextMenu";
import { useDatabaseWorkspaceTabs } from "./useDatabaseWorkspaceTabs";

export { dbxColumnInfoToEditableStructureColumn } from "./databaseViewModel";
import {
  PAGE_SIZE,
  type DbWorkspaceMode,
  loadPinnedTreeNodeIds,
  loadExtraDbxConnectionGroups,
  type QueryHistoryEntry,
  isDbxRoutineLikeObject,
} from "./databaseViewModel";

interface Props {
  projectRoot?: string;
  initialSqliteFilePath?: string;
  remoteConnection?: SshConnection;
  remoteProjectPath?: string;
  sshConnections?: SshConnection[];
}

export function DatabaseView(props: Props) {
  return (
    <DatabaseWorkspaceProvider>
      <DatabaseViewContent {...props} />
    </DatabaseWorkspaceProvider>
  );
}

function DatabaseViewContent({
  projectRoot,
  initialSqliteFilePath,
  remoteConnection,
  remoteProjectPath,
}: Props) {
  const { t } = useI18n();
  const [connections, setConnections] = useState<DbConnectionConfig[]>([]);
  const [dbxConnections, setDbxConnections] = useState<AeroricDbConnectionConfig[]>([]);
  const activeConnectionId = useDatabaseWorkspaceStore(
    (state) => state.navigation.activeConnectionId,
  );
  const setActiveConnectionId = useDatabaseWorkspaceStore((state) => state.setActiveConnectionId);
  const activeDbxConnectionId = useDatabaseWorkspaceStore(
    (state) => state.navigation.activeDbxConnectionId,
  );
  const setActiveDbxConnectionId = useDatabaseWorkspaceStore(
    (state) => state.setActiveDbxConnectionId,
  );
  const [schema, setSchema] = useState<DbSchema | null>(null);
  const [dbxDatabases, setDbxDatabases] = useState<DbxDatabaseInfo[]>([]);
  const [dbxSchemas, setDbxSchemas] = useState<string[]>([]);
  const [dbxObjects, setDbxObjects] = useState<DbxObjectInfo[]>([]);
  const [activeDbxDatabase, setActiveDbxDatabase] = useState<string | null>(null);
  const [activeDbxSchema, setActiveDbxSchema] = useState<string | null>(null);
  const [activeDbxObject, setActiveDbxObject] = useState<DbxObjectInfo | null>(null);
  const [activeObject, setActiveObject] = useState<DbObject | null>(null);
  const [queryResult, setQueryResult] = useState<DbQueryResult | null>(null);
  const [sqlResult, setSqlResult] = useState<DbExecuteResult | null>(null);
  const [page, setPage] = useState(1);
  const [sql, setSql] = useState("");
  const [queryHistory, setQueryHistory] = useState<QueryHistoryEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [exportProgress, setExportProgress] = useState<DatabaseExportProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [workspaceMode, setWorkspaceMode] = useState<DbWorkspaceMode>("table");
  const {
    workspaceTabs,
    setWorkspaceTabs,
    activeTabId,
    setActiveTabId,
    shortWorkspaceTabIds,
    contextMenu,
    setContextMenu,
    activateWorkspaceTab,
    closeWorkspaceTab,
    runWorkspaceTabContextMenuAction,
  } = useDatabaseWorkspaceTabs(setWorkspaceMode);

  const connectionDialogOpen = useDatabaseWorkspaceStore((state) => state.dialogs.connectionOpen);
  const editingDbxConnectionId = useDatabaseWorkspaceStore(
    (state) => state.dialogs.editingConnectionId,
  );
  const setConnectionDialog = useDatabaseWorkspaceStore((state) => state.setConnectionDialog);
  const [driverManifest, setDriverManifest] = useState<DatabaseDriverManifest | null>(null);
  // 「执行 SQL 文件」面板的表单三件套都在 useSqlFilePanel 里。
  const sqlFile = useSqlFilePanel({ projectRoot });
  const [dbxColumnsByTable, setDbxColumnsByTable] = useState<Record<string, DbxColumnInfo[]>>({});
  // redis / mongo 侧边树的缓存、加载器与选择器都在 useNoSqlSidebarData 里。
  const noSqlSidebar = useNoSqlSidebarData({
    setError,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setWorkspaceMode,
  });
  const {
    redisDatabasesByConnection,
    redisKeysByDatabase,
    redisScanStateByDatabase,
    mongoDatabasesByConnection,
    mongoCollectionsByDatabase,
    mongoDocumentsByCollection,
    mongoDocumentTotalsByCollection,
    setMongoDocumentQueriesByCollection,
    activeMongoDocumentId,
    setActiveMongoDocumentId,
    activeMongoWorkspaceDatabase,
    setActiveMongoWorkspaceDatabase,
  } = noSqlSidebar.state;
  const {
    loadRedisSidebarDatabases,
    loadRedisSidebarKeys,
    loadMongoSidebarDatabases,
    loadMongoSidebarCollections,
    loadMongoSidebarDocuments,
  } = noSqlSidebar.loaders;
  const {
    selectRedisSidebarDatabase,
    selectRedisSidebarKey,
    selectMongoSidebarDatabase,
    selectMongoSidebarCollection,
    selectMongoSidebarDocument,
  } = noSqlSidebar.selectors;

  // 「预览 SQL」对话框:内容与显隐都在 useDbxSqlPreviewDialog 里。
  const dbxSqlPreview = useDbxSqlPreviewDialog();
  const legacyLoadSequenceRef = useRef(createRequestSequence());
  const dbxLoadSequenceRef = useRef(createRequestSequence());
  // 侧边栏宽度与拖拽改宽自成一层,状态都在 useDatabaseSidebarResize 里。
  const { databaseSidebarWidth, resizingDatabaseSidebar, startDatabaseSidebarResize } =
    useDatabaseSidebarResize();
  const [pinnedTreeNodeIds, setPinnedTreeNodeIds] = useState<Set<string>>(loadPinnedTreeNodeIds);
  const [extraDbxConnectionGroups, setExtraDbxConnectionGroups] = useState<string[]>(
    loadExtraDbxConnectionGroups,
  );

  // 右键菜单这一层的接线(关菜单、侧边树十四个弹菜单回调)与它要用的那批派生值
  // (八种节点对应的连接、可移动目标、是否当前连接、是否置顶)都在 useContextMenuConnections 里。
  const {
    closeContextMenu,
    sidebarContextMenus,
    contextMenuConnections,
    contextMenuDbxConnectionHasMoveTargets,
    contextMenuConnectionActive,
    contextMenuTreeNodePinned,
  } = useContextMenuConnections({
    contextMenu,
    setContextMenu,
    dbxConnections,
    extraDbxConnectionGroups,
    activeConnectionId,
    activeDbxConnectionId,
    pinnedTreeNodeIds,
  });

  // 「当前选中了什么」那一整段纯派生(三条连接、endpoint、可用连接子集、表属性面板要的四组)
  // 都在 useDbxSelectionDerived 里。
  const {
    activeConnection,
    activeDbxConnection,
    editingDbxConnection,
    activeEndpoint,
    dbxHasSqlObjectBrowser,
    sqlDbxConnections,
    dbxTableObjects,
    selectedDbxTable,
    selectedDbxInfoObject,
    selectedDbxInfoObjectKey,
    selectedDbxInfoColumns,
    selectedDbxInfoIndexes,
    selectedDbxInfoForeignKeys,
    selectedDbxInfoTriggers,
    visibleDbxDatabases,
  } = useDbxSelectionDerived({
    connections,
    dbxConnections,
    activeConnectionId,
    activeDbxConnectionId,
    editingDbxConnectionId,
    activeDbxObject,
    dbxObjects,
    dbxColumnsByTable,
    dbxDatabases,
  });
  // 「表属性」面板:tab / 搜索 / DDL 状态都在 useTableInfoPanel 里,
  // 四份列表仍由这里从 dbxObjects 与 dbxColumnsByTable 派生后传进去。
  const tableInfo = useTableInfoPanel({
    connection: activeDbxConnection,
    database: activeDbxDatabase,
    object: selectedDbxInfoObject,
    objectKey: selectedDbxInfoObjectKey,
    columns: selectedDbxInfoColumns,
    indexes: selectedDbxInfoIndexes,
    foreignKeys: selectedDbxInfoForeignKeys,
    triggers: selectedDbxInfoTriggers,
  });
  // 喂给 useDbxDataGrid 的那几个派生输入(表头 / 原始行 / rowid 列 / 当前表列元数据)
  // 连同「能不能跑 SQL」都在 useDbxGridInputs 里。
  const { activeSqlCapable, rawTableRows, tableColumns, showRowIdColumn, activeDbxGridColumns } =
    useDbxGridInputs({
      queryResult,
      sqlResult,
      activeEndpoint,
      activeDbxConnection,
      activeDbxObject,
      dbxHasSqlObjectBrowser,
      dbxColumnsByTable,
    });
  const dbxGrid = useDbxDataGrid({
    initialPageSize: PAGE_SIZE,
    tableColumns,
    rawTableRows,
    queryResult,
    activeDbxConnection,
    activeDbxGridColumns,
    activeObject,
    showRowIdColumn,
  });
  const { dbxGridWhereInput, dbxGridOrderByInput, dbxGridSelectedRows } = dbxGrid.state;
  const { dbxGridEffectiveWhereInput } = dbxGrid.derived;
  // 底栏那三个显示值与右键单元格的行数计数都是纯派生,都在 useTableFooterDerived 里。
  const { dbxGridCellContextRowCount, totalPages, tableFooterRowCountText, tableFooterSqlText } =
    useTableFooterDerived({
      queryResult,
      contextMenu,
      dbxGridSelectedRows,
      dbxGridOrderByInput,
      dbxGridEffectiveWhereInput,
      activeDbxConnection,
      activeDbxObject,
      page,
      sql,
    });
  // legacy 那条路的连接层(落盘、inspect,以及启动时读连接 / 打开初始 sqlite 文件那两支 effect)
  // 都在 useLegacyConnectionLoader 里。
  const { saveConnections, inspect } = useLegacyConnectionLoader({
    connections,
    setConnections,
    activeConnectionId,
    projectRoot,
    initialSqliteFilePath,
    remoteConnection,
    remoteProjectPath,
    legacyLoadSequenceRef,
    setLoading,
    setError,
    setPage,
    setSql,
    setSchema,
    setSqlResult,
    setQueryResult,
    setWorkspaceMode,
    setActiveObject,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setDbxConnections,
  });

  // dbx 的五支加载器(列 / 库 / 模式 / 连接 / 对象数据页)都在 useDbxDataLoaders 里。
  const {
    loadDbxColumnsForTables,
    loadDbxDatabase,
    loadDbxSchema,
    loadDbxConnection,
    loadDbxObject,
  } = useDbxDataLoaders({
    grid: dbxGrid,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    dbxLoadSequenceRef,
    legacyLoadSequenceRef,
    setLoading,
    setError,
    setPage,
    setSchema,
    setSql,
    setSqlResult,
    setQueryResult,
    setWorkspaceMode,
    setWorkspaceTabs,
    setActiveTabId,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setActiveDbxObject,
    setActiveObject,
    setActiveMongoDocumentId,
    setActiveMongoWorkspaceDatabase,
    setDbxDatabases,
    setDbxSchemas,
    setDbxObjects,
    setDbxColumnsByTable,
    loadRedisSidebarDatabases,
    loadMongoSidebarDatabases,
  });

  // 「打开某个工作区面板」那一批动作(驱动 / 查询 / 历史 / SQL 文件 / 高级工具 / 用户管理 /
  // ER 图 / 库内搜索 / 表结构)都在 useDatabasePanelActions 里。
  const {
    openDriverManager,
    handleNewQuery,
    openQueryHistory,
    addQueryHistoryEntry,
    restoreQueryHistoryEntry,
    handleExecuteSqlFile,
    openAdvancedTool,
    openUserAdmin,
    openErDiagram,
    openDatabaseSearch,
    openTableStructure,
    openDbxObjectStructure,
  } = useDatabasePanelActions({
    activeSqlCapable,
    activeDbxConnection,
    activeDbxDatabase,
    dbxHasSqlObjectBrowser,
    dbxObjects,
    selectedDbxTable,
    setLoading,
    setError,
    setSql,
    setSqlResult,
    setQueryResult,
    setWorkspaceMode,
    setDriverManifest,
    setQueryHistory,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setActiveDbxObject,
    setActiveObject,
    setDbxObjects,
    loadDbxColumnsForTables,
  });

  // 连接对话框那一层(新增 legacy 连接 / 打开新建与编辑 / 关闭 / dbx 存好之后的收尾)
  // 都在 useConnectionDialogActions 里,`newConnectionGroup` 也由它自己持有。
  const {
    newConnectionGroup,
    addConnection,
    openNewConnectionDialog,
    openEditDbxConnectionDialog,
    closeConnectionDialog,
    handleConnectionSaved,
  } = useConnectionDialogActions({
    connections,
    saveConnections,
    inspect,
    loadDbxConnection,
    setError,
    setDbxConnections,
    setActiveConnectionId,
    setConnectionDialog,
  });

  // 网格的展示层动作(重拉、清筛选排序、表头排序、列上模糊筛选、换每页条数)都在 useDbxGridPresentation 里。
  const {
    reloadActiveDbxGrid,
    resetActiveDbxGrid,
    toggleDbxGridColumnSort,
    applyDbxGridColumnFuzzyFilter,
    changeDbxGridPageSize,
  } = useDbxGridPresentation({
    grid: dbxGrid,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    activeDbxGridColumns,
    queryResult,
    setError,
    loadDbxObject,
  });

  // 连接层那一批动作(选中 / 删除 / 只读 / 拉 legacy 表数据 / 刷新 / 复制)都在
  // useConnectionLifecycleActions 里。
  const {
    handleSelectConnection,
    handleSelectDbxConnection,
    handleDeleteConnection,
    handleDeleteDbxConnection,
    toggleReadOnly,
    loadTable,
    refresh,
    copyDbxConnection,
    copyLegacyConnection,
    copyNodeName,
  } = useConnectionLifecycleActions({
    connections,
    saveConnections,
    activeConnection,
    activeConnectionId,
    activeDbxConnection,
    activeDbxConnectionId,
    activeEndpoint,
    projectRoot,
    legacyLoadSequenceRef,
    dbxLoadSequenceRef,
    inspect,
    loadDbxConnection,
    setLoading,
    setError,
    setPage,
    setSql,
    setSchema,
    setSqlResult,
    setQueryResult,
    setWorkspaceMode,
    setDbxConnections,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxObject,
    setActiveObject,
    setDbxDatabases,
    setDbxObjects,
  });

  // 「选择要显示的数据库」对话框:状态、派生值与动作都在 useVisibleDatabasesDialog 里。
  const visibleDatabasesDialog = useVisibleDatabasesDialog({
    dbxConnections,
    setDbxConnections,
    closeContextMenu,
    setGlobalLoading: setLoading,
    setGlobalError: setError,
    activeDbxConnectionId,
    loadDbxConnection,
  });
  // 会改写某条 dbx 连接自己那份 `dbx` 配置的两支动作(设默认库 / 新建并 ATTACH DuckDB 文件),
  // 连同 `reloadImportedObject` 这层加载器适配,都在 useDbxConnectionConfigActions 里。
  const { saveDbxDefaultDatabase, createDuckDbAttachedDatabaseFile, reloadImportedObject } =
    useDbxConnectionConfigActions({
      activeDbxConnectionId,
      loadDbxConnection,
      loadDbxObject,
      setLoading,
      setError,
      setDbxConnections,
    });

  // 「导出数据库」对话框:状态、派生值与动作都在 useDatabaseExportDialog 里。
  const databaseExport = useDatabaseExportDialog({
    dbxConnections,
    closeContextMenu,
    setGlobalLoading: setLoading,
    setGlobalError: setError,
  });
  // 「导入文件到表」对话框:状态、派生值与动作都在 useTableImportDialog 里,
  // 这里只把它需要的几个外部依赖接进去(见 TableImportDialogDeps)。
  const tableImport = useTableImportDialog({
    dbxConnections,
    closeContextMenu,
    setGlobalLoading: setLoading,
    setGlobalError: setError,
    reloadImportedObject,
    t,
  });

  // 「整理连接」那一批动作(改名、写元数据、置顶、分组增删改)都在 useConnectionOrganizeActions 里。
  const {
    renameLegacyConnection,
    renameDbxConnection,
    toggleDbxConnectionPinned,
    togglePinnedTreeNode,
    addExtraDbxConnectionGroup,
    moveDbxConnectionToGroup,
    renameDbxConnectionGroup,
    deleteDbxConnectionGroup,
  } = useConnectionOrganizeActions({
    connections,
    saveConnections,
    dbxConnections,
    setDbxConnections,
    setLoading,
    setError,
    setPinnedTreeNodeIds,
    setExtraDbxConnectionGroups,
  });

  // 「新建数据库」对话框:状态与动作都在 useCreateDatabaseDialog 里。
  const createDatabase = useCreateDatabaseDialog({
    dbxConnections,
    setGlobalLoading: setLoading,
    setGlobalError: setError,
    loadDbxConnection,
  });
  // dbx 对象层的动作(看 DDL/源码、写 SQL 草稿、导出、drop)都在 useDbxObjectOperations 里。
  const {
    showDbxObjectDdl,
    showDbxObjectSource,
    writeDbxProcedureExecutionDraft,
    writeDbxObjectSqlDraft,
    exportDbxTableObject,
    exportActiveDbxGrid,
    copyDbxObjectStructure,
    exportDbxObjectStructure,
    copyDbxObjectStructureDdl,
    dropDbxObject,
    dropDbxColumn,
    dropDbxTableChildObject,
    dropDbxDatabase,
    dropDbxSchema,
  } = useDbxObjectOperations({
    grid: dbxGrid,
    activeObject,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    queryResult,
    setLoading,
    setError,
    setExportProgress,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setActiveDbxObject,
    setWorkspaceMode,
    setSql,
    setSqlResult,
    setQueryResult,
    loadDbxConnection,
    loadDbxDatabase,
    loadDbxColumnsForTables,
    copyNodeName,
    databaseExport,
  });

  // 「新建 schema」对话框:状态与动作都在 useCreateSchemaDialog 里。
  const createSchema = useCreateSchemaDialog({
    dbxConnections,
    setGlobalLoading: setLoading,
    setGlobalError: setError,
    loadDbxDatabase,
  });
  // 侧边树上六个 dbx 右键菜单的动作执行器都在 useDbxTreeContextMenuActions 里。
  const {
    runDbxObjectContextMenuAction,
    runDbxDatabaseContextMenuAction,
    runDbxSchemaContextMenuAction,
    runDbxColumnContextMenuAction,
    runDbxTableChildContextMenuAction,
    runDbxObjectGroupContextMenuAction,
  } = useDbxTreeContextMenuActions({
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
  });

  // redis / mongo 五种节点的右键动作执行器都在 useNoSqlContextMenuActions 里。
  const { runNoSqlContextMenuAction } = useNoSqlContextMenuActions({
    contextMenu,
    setContextMenu,
    dbxConnections,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setActiveMongoDocumentId,
    setActiveMongoWorkspaceDatabase,
    setWorkspaceMode,
    setSql,
    setSqlResult,
    setQueryResult,
    togglePinnedTreeNode,
    copyNodeName,
    saveDbxDefaultDatabase,
    loadRedisSidebarDatabases,
    loadMongoSidebarDatabases,
    loadRedisSidebarKeys,
    loadMongoSidebarCollections,
    loadMongoSidebarDocuments,
    selectRedisSidebarDatabase,
    selectRedisSidebarKey,
    selectMongoSidebarDatabase,
    selectMongoSidebarCollection,
    selectMongoSidebarDocument,
  });

  // 连接节点与连接分组节点的右键动作执行器都在 useConnectionContextMenuActions 里。
  const { runContextMenuAction, runConnectionGroupContextMenuAction } =
    useConnectionContextMenuActions({
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
    });

  // SQL 编辑器上的执行与拖拽三支动作都在 useSqlEditorActions 里。
  const { runSql, handleSqlDragOver, handleSqlDrop, executeSqlFileFromPanel } = useSqlEditorActions(
    {
      sql,
      setSql,
      activeConnection,
      activeEndpoint,
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxSchema,
      dbxHasSqlObjectBrowser,
      projectRoot,
      setLoading,
      setError,
      setWorkspaceMode,
      setSqlResult,
      setQueryResult,
      setSchema,
      addQueryHistoryEntry,
      sqlFile,
    },
  );

  // 网格的编辑层(改单元格 / 存暂存改动 / 插删行 / 复制)都在 useDbxGridEditing 里。
  const {
    dbxGridContextRows,
    buildDbxGridCopyOptions,
    buildDbxGridContextFilterOptions,
    updateCell,
    savePendingGridChanges,
    insertRow,
    deleteSelectedDbxRows,
    copySelectedDbxRows,
    handleDbxGridKeyDown,
  } = useDbxGridEditing({
    grid: dbxGrid,
    activeConnection,
    activeEndpoint,
    activeObject,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    queryResult,
    dbxColumnsByTable,
    tableColumns,
    page,
    projectRoot,
    setError,
    loadDbxObject,
    loadTable,
    dbxSqlPreview,
  });

  // 表头 / 单元格两个右键菜单的动作执行器都在 useDbxGridContextMenuActions 里。
  const { runDbxGridHeaderContextMenuAction, runDbxGridCellContextMenuAction } =
    useDbxGridContextMenuActions({
      grid: dbxGrid,
      contextMenu,
      setContextMenu,
      queryResult,
      activeDbxConnection,
      activeDbxObject,
      activeDbxDatabase,
      copyNodeName,
      loadDbxObject,
      buildDbxGridContextFilterOptions,
      dbxGridContextRows,
      buildDbxGridCopyOptions,
      onError: setError,
    });

  // 工作区这一层的四个派生值与标题文案都在 useDatabaseWorkspaceDerived 里。
  const {
    activeDbxGridPrimaryKeys,
    canInsertActiveTable,
    hideDatabaseWorkspaceTopbar,
    databaseWorkspaceTitle,
  } = useDatabaseWorkspaceDerived({
    workspaceMode,
    activeObject,
    activeConnection,
    activeDbxConnection,
    activeDbxObject,
    queryResult,
    sqlResult,
  });

  // 「当前这张表」的两支动作(按页重拉 / 打开表属性)都在 useActiveObjectActions 里。
  const { loadActiveObjectPage, openActiveTableProperties } = useActiveObjectActions({
    activeObject,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    dbxGridWhereInput,
    dbxGridOrderByInput,
    tableInfo,
    loadDbxObject,
    loadDbxColumnsForTables,
    loadTable,
    setWorkspaceMode,
    setWorkspaceTabs,
    setActiveTabId,
  });

  return (
    <div
      style={{ ...s.databaseRoot, gridTemplateColumns: `${databaseSidebarWidth}px minmax(0, 1fr)` }}
    >
      <DatabaseTopToolbar
        sqlCapable={activeSqlCapable}
        busy={loading}
        onNewConnection={openNewConnectionDialog}
        onNewQuery={handleNewQuery}
        onExecuteSqlFile={handleExecuteSqlFile}
        onOpenDriverManager={openDriverManager}
        onOpenAdvancedTool={openAdvancedTool}
        onOpenUserAdmin={openUserAdmin}
        onOpenErDiagram={openErDiagram}
        onOpenDatabaseSearch={openDatabaseSearch}
        onOpenTableStructure={openTableStructure}
      />
      <aside style={{ ...s.databaseSidebar, width: databaseSidebarWidth }}>
        <div style={s.databaseSidebarHeader}>
          <div style={s.databaseTitleRow}>
            <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
              <Database size={16} />
              <span style={s.databaseTitle}>{t("database.title")}</span>
            </div>
            <DbxButton
              variant="ghost"
              size="icon-sm"
              icon={RefreshCcw}
              onClick={refresh}
              disabled={(!activeConnection && !activeDbxConnection) || loading}
              title={t("common.refresh")}
            />
          </div>
          {activeConnection && (
            <DbxButton
              variant={activeConnection.readOnly ? "destructive" : "outline"}
              size="sm"
              onClick={toggleReadOnly}
              style={{ justifyContent: "flex-start" }}
            >
              {activeConnection.readOnly ? t("database.readOnlyOn") : t("database.readOnlyOff")}
            </DbxButton>
          )}
        </div>

        <DatabaseSidebarTree
          connections={connections}
          dbxConnections={dbxConnections}
          extraDbxConnectionGroups={extraDbxConnectionGroups}
          activeConnectionId={activeConnectionId}
          activeDbxConnectionId={activeDbxConnectionId}
          activeDbxConnection={activeDbxConnection}
          activeDbxDatabase={activeDbxDatabase}
          activeDbxSchema={activeDbxSchema}
          activeObject={activeObject}
          activeDbxObject={activeDbxObject}
          userAdminActive={workspaceMode === "user-admin"}
          dbxHasSqlObjectBrowser={dbxHasSqlObjectBrowser}
          visibleDbxDatabases={visibleDbxDatabases}
          dbxSchemas={dbxSchemas}
          legacyObjects={schema?.objects ?? []}
          dbxObjects={dbxObjects}
          dbxColumnsByTable={dbxColumnsByTable}
          redisDatabasesByConnection={redisDatabasesByConnection}
          redisKeysByDatabase={redisKeysByDatabase}
          redisScanStateByDatabase={redisScanStateByDatabase}
          mongoDatabasesByConnection={mongoDatabasesByConnection}
          mongoCollectionsByDatabase={mongoCollectionsByDatabase}
          mongoDocumentsByCollection={mongoDocumentsByCollection}
          mongoDocumentTotalsByCollection={mongoDocumentTotalsByCollection}
          activeMongoDocumentId={activeMongoDocumentId}
          pinnedTreeNodeIds={pinnedTreeNodeIds}
          onSelectConnection={handleSelectConnection}
          onSelectDbxConnection={handleSelectDbxConnection}
          onDeleteConnection={handleDeleteConnection}
          onDeleteDbxConnection={handleDeleteDbxConnection}
          onSelectDatabase={loadDbxDatabase}
          onSelectDbxSchema={(connection, database, schemaName) => {
            void loadDbxSchema(connection, database, schemaName);
          }}
          onSelectLegacyObject={(object) => loadTable(object, 1)}
          onSelectDbxObject={(object) => {
            if (isDbxRoutineLikeObject(object) && activeDbxConnection) {
              void showDbxObjectSource(activeDbxConnection, activeDbxDatabase, object);
            } else {
              void loadDbxObject(object, 1);
            }
          }}
          onOpenUserAdmin={(connection) => {
            void (async () => {
              await loadDbxConnection(connection);
              setWorkspaceMode("user-admin");
              setError(
                supportsDbxUserAdmin(connection.dbType)
                  ? null
                  : t("database.selectUserAdminConnection"),
              );
              setSqlResult(null);
              setQueryResult(null);
            })();
          }}
          onOpenNoSqlWorkspace={() => {
            if (activeDbxConnection)
              setWorkspaceMode(activeDbxConnection.dbType === "redis" ? "redis" : "mongo");
          }}
          onSelectRedisDatabase={selectRedisSidebarDatabase}
          onExpandRedisDatabase={(connection, database) => {
            void loadRedisSidebarKeys(connection, database);
          }}
          onLoadMoreRedisKeys={(connection, database) => {
            void loadRedisSidebarKeys(connection, database, true);
          }}
          onSelectRedisKey={selectRedisSidebarKey}
          onSelectMongoDatabase={(connection, database) => {
            void selectMongoSidebarDatabase(connection, database);
          }}
          onExpandMongoDatabase={(connection, database) => {
            void loadMongoSidebarCollections(connection, database);
          }}
          onSelectMongoCollection={(connection, database, collection) => {
            void selectMongoSidebarCollection(connection, database, collection);
          }}
          onExpandMongoCollection={(connection, database, collection) => {
            void loadMongoSidebarDocuments(connection, database, collection);
          }}
          onLoadMoreMongoDocuments={(connection, database, collection) => {
            void loadMongoSidebarDocuments(connection, database, collection, true);
          }}
          onSelectMongoDocument={(connection, database, collection, document) => {
            void selectMongoSidebarDocument(connection, database, collection, document);
          }}
          onRenameConnection={renameLegacyConnection}
          onRenameDbxConnection={(connection) => {
            void renameDbxConnection(connection);
          }}
          onRefreshConnection={inspect}
          onRefreshDbxConnection={(connection) => {
            void loadDbxConnection(connection);
          }}
          onRefreshDatabase={(connection, database) => {
            void loadDbxDatabase(connection, database);
          }}
          onRefreshDbxSchema={(connection, database, schemaName) => {
            void loadDbxSchema(connection, database, schemaName);
          }}
          onCopyNodeName={copyNodeName}
          onDropDatabase={(connection, database) => {
            void dropDbxDatabase(connection, database);
          }}
          onDropDbxSchema={(connection, database, schemaName) => {
            void dropDbxSchema(connection, database, schemaName);
          }}
          onDropDbxObject={(connection, database, object) => {
            void dropDbxObject(connection, database, object);
          }}
          onDropDbxColumn={(connection, database, object, column) => {
            void dropDbxColumn(connection, database, object, column);
          }}
          onDropDbxTableChildObject={(connection, database, object, childObject) => {
            void dropDbxTableChildObject(connection, database, object, childObject);
          }}
          {...sidebarContextMenus}
        />
        <button
          type="button"
          role="separator"
          aria-label={t("database.resizeSidebar")}
          aria-orientation="vertical"
          title={t("database.resizeSidebar")}
          onPointerDown={startDatabaseSidebarResize}
          style={{
            ...s.databaseSidebarResizeHandle,
            ...(resizingDatabaseSidebar ? s.databaseSidebarResizeHandleActive : undefined),
          }}
        />
      </aside>

      <main style={s.databaseMain}>
        {workspaceTabs.length > 0 && (
          <DatabaseWorkspaceTabBar
            tabs={workspaceTabs}
            activeTabId={activeTabId}
            shortTabIds={shortWorkspaceTabIds}
            onActivate={activateWorkspaceTab}
            onClose={closeWorkspaceTab}
            onOpenContextMenu={setContextMenu}
          />
        )}

        {workspaceTabs.length === 0 && !hideDatabaseWorkspaceTopbar && (
          <DatabaseWorkspaceTopbar
            title={databaseWorkspaceTitle(workspaceMode)}
            endpoint={activeEndpoint}
            connection={activeDbxConnection}
            error={error}
          />
        )}

        {/* 按 workspaceMode 挑面板的那条三元链在 DatabaseWorkspacePanels 里,链尾回落到 fallback。 */}
        <DatabaseWorkspacePanels
          workspaceMode={workspaceMode}
          loading={loading}
          queryHistory={queryHistory}
          setQueryHistory={setQueryHistory}
          restoreQueryHistoryEntry={restoreQueryHistoryEntry}
          sqlFile={sqlFile}
          activeSqlCapable={activeSqlCapable}
          executeSqlFileFromPanel={executeSqlFileFromPanel}
          driverManifest={driverManifest}
          openDriverManager={openDriverManager}
          activeDbxConnection={activeDbxConnection}
          activeDbxDatabase={activeDbxDatabase}
          activeDbxSchema={activeDbxSchema}
          activeMongoWorkspaceDatabase={activeMongoWorkspaceDatabase}
          activeMongoDocumentId={activeMongoDocumentId}
          setMongoDocumentQueriesByCollection={setMongoDocumentQueriesByCollection}
          loadMongoSidebarDocuments={loadMongoSidebarDocuments}
          dbxHasSqlObjectBrowser={dbxHasSqlObjectBrowser}
          selectedDbxTable={selectedDbxTable}
          sqlDbxConnections={sqlDbxConnections}
          dbxTableObjects={dbxTableObjects}
          dbxColumnsByTable={dbxColumnsByTable}
          dbxObjects={dbxObjects}
          loadDbxObject={loadDbxObject}
          selectedDbxInfoObject={selectedDbxInfoObject}
          tableInfo={tableInfo}
          showDbxObjectDdl={showDbxObjectDdl}
          fallback={
            <DatabaseWorkspaceGridStack
              grid={dbxGrid}
              workspaceMode={workspaceMode}
              loading={loading}
              error={error}
              page={page}
              totalPages={totalPages}
              sql={sql}
              setSql={setSql}
              activeSqlCapable={activeSqlCapable}
              runSql={runSql}
              handleSqlDragOver={handleSqlDragOver}
              handleSqlDrop={handleSqlDrop}
              queryResult={queryResult}
              sqlResult={sqlResult}
              tableColumns={tableColumns}
              showRowIdColumn={showRowIdColumn}
              canInsertActiveTable={canInsertActiveTable}
              hideDatabaseWorkspaceTopbar={hideDatabaseWorkspaceTopbar}
              activeConnection={activeConnection}
              activeObject={activeObject}
              activeDbxConnection={activeDbxConnection}
              activeDbxDatabase={activeDbxDatabase}
              activeDbxObject={activeDbxObject}
              dbxSqlPreview={dbxSqlPreview}
              tableImport={tableImport}
              tableFooterRowCountText={tableFooterRowCountText}
              tableFooterSqlText={tableFooterSqlText}
              loadActiveObjectPage={loadActiveObjectPage}
              insertRow={insertRow}
              savePendingGridChanges={savePendingGridChanges}
              openActiveTableProperties={openActiveTableProperties}
              exportActiveDbxGrid={exportActiveDbxGrid}
              copySelectedDbxRows={copySelectedDbxRows}
              deleteSelectedDbxRows={deleteSelectedDbxRows}
              resetActiveDbxGrid={resetActiveDbxGrid}
              reloadActiveDbxGrid={reloadActiveDbxGrid}
              changeDbxGridPageSize={changeDbxGridPageSize}
              handleDbxGridKeyDown={handleDbxGridKeyDown}
              toggleDbxGridColumnSort={toggleDbxGridColumnSort}
              applyDbxGridColumnFuzzyFilter={applyDbxGridColumnFuzzyFilter}
              setContextMenu={setContextMenu}
              updateCell={updateCell}
            />
          }
        />
        <DbxCellDetailPanel grid={dbxGrid} queryResult={queryResult} onUpdateCell={updateCell} />
      </main>
      <DatabaseExportProgressOverlay progress={exportProgress} />
      <DbxSqlPreviewDialog state={dbxSqlPreview} />
      {/* 列 / 行 / 单元格三个值预览对话框:内容都在 dbxGrid 里,这里只是渲染。 */}
      <DbxValuePreviewDialogs
        grid={dbxGrid}
        queryResult={queryResult}
        onCopyColumnName={copyNodeName}
        onError={setError}
      />
      <ConnectionDialog
        open={connectionDialogOpen}
        editingConnection={editingDbxConnection}
        initialConnectionGroup={newConnectionGroup}
        projectRoot={projectRoot}
        onAddLocalConnection={(endpoint) => addConnection(endpoint)}
        onSaved={handleConnectionSaved}
        onClose={closeConnectionDialog}
      />
      <CreateDatabaseDialog state={createDatabase} busy={loading} />
      <CreateSchemaDialog state={createSchema} busy={loading} />
      <VisibleDatabasesDialog state={visibleDatabasesDialog} />
      <DatabaseExportDialog state={databaseExport} />
      <TableImportDialog state={tableImport} />
      {contextMenu && (
        <DatabaseContextMenu
          menu={contextMenu}
          onDismiss={closeContextMenu}
          connections={contextMenuConnections}
          connectionActive={contextMenuConnectionActive}
          connectionHasMoveTargets={contextMenuDbxConnectionHasMoveTargets}
          treeNodePinned={contextMenuTreeNodePinned}
          shortWorkspaceTabIds={shortWorkspaceTabIds}
          grid={{
            orderByInput: dbxGridOrderByInput,
            queryResult,
            primaryKeys: activeDbxGridPrimaryKeys,
            cellRowCount: dbxGridCellContextRowCount,
          }}
          actions={{
            connection: (action) => void runContextMenuAction(action),
            connectionGroup: (action) => void runConnectionGroupContextMenuAction(action),
            database: (action) => void runDbxDatabaseContextMenuAction(action),
            schema: (action) => void runDbxSchemaContextMenuAction(action),
            object: (action) => void runDbxObjectContextMenuAction(action),
            objectGroup: (action) => void runDbxObjectGroupContextMenuAction(action),
            tableChild: (action) => void runDbxTableChildContextMenuAction(action),
            column: (action) => void runDbxColumnContextMenuAction(action),
            noSql: (action) => void runNoSqlContextMenuAction(action),
            gridHeader: (action) => void runDbxGridHeaderContextMenuAction(action),
            gridCell: (action) => void runDbxGridCellContextMenuAction(action),
            workspaceTab: runWorkspaceTabContextMenuAction,
          }}
        />
      )}
    </div>
  );
}
