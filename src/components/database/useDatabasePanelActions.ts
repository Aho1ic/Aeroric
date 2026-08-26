/**
 * 「打开某个工作区面板」的十二支动作:驱动管理、新建查询、查询历史(打开 / 追加 / 还原)、
 * 执行 SQL 文件面板、高级工具、用户管理,以及 ER 图、库内搜索、表结构与「打开某个 dbx 对象的结构」。
 *
 * 从 `DatabaseView.tsx` 抽出。这一批的共同形状是「切到某个 `workspaceMode`,顺手把上一屏的
 * `sqlResult` / `queryResult` 清掉,再按当前连接的能力给出或清掉那句错误提示」——
 * 提示文案与判断条件逐字保留:高级工具与 ER 图 / 库内搜索看 `dbxHasSqlObjectBrowser`,
 * 用户管理看 `supportsDbxUserAdmin(dbType)`,新建查询与执行 SQL 文件看 `activeSqlCapable`,
 * 表结构那两支看 `database.selectDbxTable`,四者不能互换。
 *
 * `addQueryHistoryEntry` 只是一支纯状态追加,并不切面板,但它原本就夹在这段里、又和
 * `restoreQueryHistoryEntry` 成对(一支写进历史、一支从历史读回),所以一并收进来。
 * 它的去重与截断也逐字保留:同 sql 同连接名的旧条目先滤掉,再留前 49 条,新条目插在最前。
 *
 * 原文里这批分成两段(`openDriverManager` 一带与 `openErDiagram` 一带),中间隔着
 * `useDbxDataLoaders`。后一段要用 `loadDbxColumnsForTables`,所以整块落在加载器之后 ——
 * 前一段的成员在那之间没有任何调用者,顺序仍然成立。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 它们的身份本来就不变,行为不受影响。
 */

import { useCallback, type SetStateAction } from "react";

import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import type {
  AeroricDbConnectionConfig,
  DatabaseDriverManifest,
  DbExecuteResult,
  DbObject,
  DbQueryResult,
  DbxObjectInfo,
} from "../../types";
import type { DatabaseAdvancedToolMode } from "./DatabaseAdvancedTools";
import { supportsDbxUserAdmin } from "./DatabaseUserAdminPanel";
import {
  isDbxTableObject,
  isSqlDbxConnection,
  listAllDbxObjects,
  type DbWorkspaceMode,
  type QueryHistoryEntry,
} from "./databaseViewModel";

export interface DatabasePanelActionsDeps {
  activeSqlCapable: boolean;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  dbxHasSqlObjectBrowser: boolean;
  dbxObjects: DbxObjectInfo[];
  selectedDbxTable: DbxObjectInfo | null;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setSql: (sql: string) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setDriverManifest: (manifest: DatabaseDriverManifest | null) => void;
  /** 这支要用 updater 形式读旧历史来去重,所以拿的是完整的 setState 签名。 */
  setQueryHistory: (value: SetStateAction<QueryHistoryEntry[]>) => void;
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setActiveDbxDatabase: (database: string | null) => void;
  setActiveDbxSchema: (schema: string | null) => void;
  setActiveDbxObject: (object: DbxObjectInfo | null) => void;
  setActiveObject: (object: DbObject | null) => void;
  setDbxObjects: (objects: DbxObjectInfo[]) => void;
  /** 来自 `useDbxDataLoaders`,ER 图与两支表结构都要靠它把列补齐。 */
  loadDbxColumnsForTables: (
    objects: DbxObjectInfo[],
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
  ) => Promise<void>;
}

export interface DatabasePanelActions {
  openDriverManager: () => Promise<void>;
  handleNewQuery: () => void;
  openQueryHistory: () => void;
  addQueryHistoryEntry: (entry: Omit<QueryHistoryEntry, "id" | "executedAt">) => void;
  restoreQueryHistoryEntry: (entry: QueryHistoryEntry) => void;
  handleExecuteSqlFile: () => void;
  openAdvancedTool: (mode: DatabaseAdvancedToolMode) => void;
  openUserAdmin: () => void;
  openErDiagram: () => Promise<void>;
  openDatabaseSearch: () => Promise<void>;
  openTableStructure: () => Promise<void>;
  openDbxObjectStructure: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    object: DbxObjectInfo,
  ) => Promise<void>;
}

export function useDatabasePanelActions(deps: DatabasePanelActionsDeps): DatabasePanelActions {
  const { t } = useI18n();
  const {
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
  } = deps;

  const openDriverManager = useCallback(async () => {
    setWorkspaceMode("drivers");
    setError(null);
    setLoading(true);
    try {
      const manifest = await databaseApi.dbxDriverManifest();
      setDriverManifest(manifest);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [setDriverManifest, setError, setLoading, setWorkspaceMode]);

  const handleNewQuery = useCallback(() => {
    if (!activeSqlCapable) return;
    setWorkspaceMode("query");
    setSql("");
    setSqlResult(null);
    setQueryResult(null);
    setActiveObject(null);
    setActiveDbxObject(null);
  }, [
    activeSqlCapable,
    setActiveDbxObject,
    setActiveObject,
    setQueryResult,
    setSql,
    setSqlResult,
    setWorkspaceMode,
  ]);

  const openQueryHistory = useCallback(() => {
    setWorkspaceMode("query-history");
    setSqlResult(null);
    setQueryResult(null);
  }, [setQueryResult, setSqlResult, setWorkspaceMode]);

  const addQueryHistoryEntry = useCallback(
    (entry: Omit<QueryHistoryEntry, "id" | "executedAt">) => {
      const statement = entry.sql.trim();
      if (!statement) return;
      setQueryHistory((current) => [
        {
          ...entry,
          sql: statement,
          id: `history:${Date.now()}:${Math.random().toString(36).slice(2)}`,
          executedAt: Date.now(),
        },
        ...current
          .filter((item) => item.sql !== statement || item.connectionName !== entry.connectionName)
          .slice(0, 49),
      ]);
    },
    [setQueryHistory],
  );

  const restoreQueryHistoryEntry = useCallback(
    (entry: QueryHistoryEntry) => {
      setSql(entry.sql);
      setWorkspaceMode("query");
      setSqlResult(null);
      setQueryResult(null);
    },
    [setQueryResult, setSql, setSqlResult, setWorkspaceMode],
  );

  const handleExecuteSqlFile = useCallback(() => {
    setWorkspaceMode("sql-file");
    setError(activeSqlCapable ? null : t("database.selectSqlConnection"));
    setSqlResult(null);
    setQueryResult(null);
  }, [activeSqlCapable, setError, setQueryResult, setSqlResult, setWorkspaceMode, t]);

  const openAdvancedTool = useCallback(
    (mode: DatabaseAdvancedToolMode) => {
      setWorkspaceMode(mode);
      setError(
        activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"),
      );
      setSqlResult(null);
      setQueryResult(null);
    },
    [
      activeDbxConnection,
      dbxHasSqlObjectBrowser,
      setError,
      setQueryResult,
      setSqlResult,
      setWorkspaceMode,
      t,
    ],
  );

  const openUserAdmin = useCallback(() => {
    setWorkspaceMode("user-admin");
    setError(
      activeDbxConnection && supportsDbxUserAdmin(activeDbxConnection.dbType)
        ? null
        : t("database.selectUserAdminConnection"),
    );
    setSqlResult(null);
    setQueryResult(null);
  }, [activeDbxConnection, setError, setQueryResult, setSqlResult, setWorkspaceMode, t]);

  const openErDiagram = useCallback(async () => {
    setWorkspaceMode("er-diagram");
    setError(
      activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"),
    );
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser) return;
    let objects = dbxObjects;
    if (objects.length === 0) {
      try {
        objects = await listAllDbxObjects(activeDbxConnection.id, activeDbxDatabase, null);
        setDbxObjects(objects);
      } catch (err) {
        setError(String(err));
        return;
      }
    }
    await loadDbxColumnsForTables(objects, activeDbxConnection, activeDbxDatabase);
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    dbxHasSqlObjectBrowser,
    dbxObjects,
    loadDbxColumnsForTables,
    setDbxObjects,
    setError,
    setWorkspaceMode,
    t,
  ]);

  const openDatabaseSearch = useCallback(async () => {
    setWorkspaceMode("database-search");
    setError(
      activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"),
    );
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser || !activeDbxDatabase) return;
    if (dbxObjects.length === 0) {
      try {
        setDbxObjects(await listAllDbxObjects(activeDbxConnection.id, activeDbxDatabase, null));
      } catch (err) {
        setError(String(err));
      }
    }
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    dbxHasSqlObjectBrowser,
    dbxObjects.length,
    setDbxObjects,
    setError,
    setWorkspaceMode,
    t,
  ]);

  const openTableStructure = useCallback(async () => {
    setWorkspaceMode("table-structure");
    setError(activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxTable"));
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser) return;
    const targetObject = selectedDbxTable;
    if (!targetObject) return;
    setActiveDbxObject(targetObject);
    try {
      await loadDbxColumnsForTables([targetObject], activeDbxConnection, activeDbxDatabase);
    } catch (err) {
      setError(String(err));
    }
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    dbxHasSqlObjectBrowser,
    loadDbxColumnsForTables,
    selectedDbxTable,
    setActiveDbxObject,
    setError,
    setWorkspaceMode,
    t,
  ]);

  const openDbxObjectStructure = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection) || !isDbxTableObject(object)) return;
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(object.schema ?? null);
      setActiveDbxObject(object);
      setWorkspaceMode("table-structure");
      setError(null);
      try {
        await loadDbxColumnsForTables([object], connection, database);
      } catch (err) {
        setError(String(err));
      }
    },
    [
      loadDbxColumnsForTables,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setActiveDbxSchema,
      setError,
      setWorkspaceMode,
    ],
  );

  return {
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
  };
}
