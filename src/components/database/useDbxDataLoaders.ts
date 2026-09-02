/**
 * dbx 那五支加载器:拉一批表的列、拉一个库、拉一个模式、连上一条连接、拉一个对象的数据页;
 * 外加一支后补的 `reloadDbxObjectMetadata`(见它自己的注释,故意不走下面这条请求号)。
 *
 * 从 `DatabaseView.tsx` 抽出。这五支是 dbx 侧最核心的一层 —— 侧边树的每一次展开、右键菜单里
 * 每一个「刷新」、网格的每一次翻页排序筛选,最后都落到它们身上,所以外面几乎每个 hook 都要
 * 拿其中一支当依赖;它们自己反过来只依赖 state setter 与两支 redis/mongo 加载器,不依赖任何
 * 动作层,这才能单独成一层。
 *
 * 原文里这五支并不相邻(`openErDiagram` 一带的几支「打开某个面板」夹在中间),但那几支只会
 * 调用加载器、不会被加载器调用,所以整块提到最前面那支的位置上,顺序仍然成立。
 *
 * `dbxLoadSequenceRef` 的用法与原来逐字一致:每支入口先 `next()` 领一个号,每个 await 之后
 * 用 `isCurrent()` 对号,过期的响应连 `setLoading(false)` 都不做 —— 交给那个更晚的请求收尾,
 * 少一次对号就会出现「切了连接又被上一条的结果盖回去」。
 * `loadDbxConnection` 还要额外 `legacyLoadSequenceRef.current.invalidate()`,把 legacy 那条路
 * 正在飞的请求一起作废。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 与两个 ref 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 它们的身份本来就不变,行为不受影响。
 */

import { useCallback, type MutableRefObject, type SetStateAction } from "react";

import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import type {
  AeroricDbConnectionConfig,
  DbExecuteResult,
  DbObject,
  DbQueryResult,
  DbSchema,
  DbxColumnInfo,
  DbxDatabaseInfo,
  DbxObjectInfo,
  RedisDatabaseInfo,
} from "../../types";
import { combineDbxGridWhereFilters, type DbxGridColumnFuzzyFilters } from "./databaseGridState";
import type { WorkspaceTab } from "./databaseWorkspaceStore";
import {
  DBX_KEYLESS_GRID_EDIT_DB_TYPES,
  configuredTargetDatabase,
  dbxColumnsToDbColumns,
  dbxObjectKey,
  dbxRowsToDatabaseRows,
  deriveDbxSchemas,
  filterDbxDatabasesForConnection,
  isDbxTableObject,
  isDbxViewObject,
  isSqlDbxConnection,
  listAllDbxObjects,
  omitDbxCacheEntriesForSchema,
  type DbWorkspaceMode,
} from "./databaseViewModel";
import type { RequestSequence } from "./requestSequence";
import type { DbxDataGridController } from "./useDbxDataGrid";

export interface DbxDataLoadersDeps {
  grid: DbxDataGridController;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxObject: DbxObjectInfo | null;
  /** 两个请求号:dbx 自己那条 + 要被 `loadDbxConnection` 作废的 legacy 那条。 */
  dbxLoadSequenceRef: MutableRefObject<RequestSequence>;
  legacyLoadSequenceRef: MutableRefObject<RequestSequence>;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setPage: (page: number) => void;
  setSchema: (schema: DbSchema | null) => void;
  setSql: (sql: string) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setWorkspaceTabs: (value: SetStateAction<WorkspaceTab[]>) => void;
  setActiveTabId: (id: string) => void;
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setActiveDbxDatabase: (database: string | null) => void;
  setActiveDbxSchema: (schema: string | null) => void;
  setActiveDbxObject: (object: DbxObjectInfo | null) => void;
  setActiveObject: (object: DbObject | null) => void;
  setActiveMongoDocumentId: (id: string | null) => void;
  setActiveMongoWorkspaceDatabase: (database: string | null) => void;
  setDbxDatabases: (databases: DbxDatabaseInfo[]) => void;
  setDbxSchemas: (value: SetStateAction<string[]>) => void;
  setDbxObjects: (value: SetStateAction<DbxObjectInfo[]>) => void;
  setDbxColumnsByTable: (value: SetStateAction<Record<string, DbxColumnInfo[]>>) => void;
  /** redis / mongo 那两支库列表加载器,`loadDbxConnection` 按 dbType 二选一。 */
  loadRedisSidebarDatabases: (
    connection: AeroricDbConnectionConfig,
  ) => Promise<RedisDatabaseInfo[]>;
  loadMongoSidebarDatabases: (connection: AeroricDbConnectionConfig) => Promise<string[]>;
}

export interface DbxDataLoaders {
  loadDbxColumnsForTables: (
    objects: DbxObjectInfo[],
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
  ) => Promise<void>;
  reloadDbxObjectMetadata: (
    object: DbxObjectInfo,
    connection: AeroricDbConnectionConfig,
    database: string | null,
    shouldApply?: () => boolean,
  ) => Promise<void>;
  loadDbxDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
  loadDbxSchema: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
    schemaName: string,
  ) => Promise<void>;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
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
}

export function useDbxDataLoaders(deps: DbxDataLoadersDeps): DbxDataLoaders {
  const { t } = useI18n();
  const {
    grid,
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
  } = deps;
  const { dbxGridPageSize, dbxGridColumnFuzzyFilters } = grid.state;
  const { initializeLoadedGrid } = grid.actions;

  const loadDbxDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null) => {
      const requestSeq = dbxLoadSequenceRef.current.next();
      setLoading(true);
      setError(null);
      try {
        const [objects, schemas] = await Promise.all([
          listAllDbxObjects(connection.id, database, null),
          databaseApi
            .dbxListSchemas(connection.id, database)
            .then((value) => (Array.isArray(value) ? value : []))
            .catch(() => [] as string[]),
        ]);
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setActiveDbxDatabase(database);
        setActiveDbxSchema(null);
        setDbxSchemas(schemas.length > 0 ? schemas : deriveDbxSchemas(objects));
        setDbxObjects(objects);
        // 同 `loadDbxConnection`:键里没有库名,换库同样会撞。这支的每个调用点都是「切库」
        // 或「改完 DDL 重列一遍」,想要列的那一处是在它返回之后自己再拉,清掉不会踩到谁。
        setDbxColumnsByTable({});
        setActiveDbxObject(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      } catch (err) {
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setError(String(err));
      } finally {
        if (dbxLoadSequenceRef.current.isCurrent(requestSeq)) setLoading(false);
      }
    },
    [
      dbxLoadSequenceRef,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setActiveDbxSchema,
      setActiveObject,
      setDbxColumnsByTable,
      setDbxObjects,
      setDbxSchemas,
      setError,
      setLoading,
      setQueryResult,
      setSqlResult,
    ],
  );

  const loadDbxSchema = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, schemaName: string) => {
      const requestSeq = dbxLoadSequenceRef.current.next();
      setLoading(true);
      setError(null);
      try {
        const objects = await listAllDbxObjects(connection.id, database, schemaName);
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setActiveDbxDatabase(database);
        setActiveDbxSchema(schemaName);
        setDbxSchemas((current) =>
          current.includes(schemaName) ? current : [...current, schemaName].sort(),
        );
        setDbxObjects((current) => {
          const currentWithoutSchema = current.filter((object) => object.schema !== schemaName);
          return [...currentWithoutSchema, ...objects];
        });
        // 这支是按模式合并的(上面那行只换掉本模式的对象),所以列缓存也只丢本模式的键 ——
        // 整份清掉会把同库其他模式已经展开好的列一起带走。
        setDbxColumnsByTable((current) => omitDbxCacheEntriesForSchema(current, schemaName));
        setActiveDbxObject(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      } catch (err) {
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setError(String(err));
      } finally {
        if (dbxLoadSequenceRef.current.isCurrent(requestSeq)) setLoading(false);
      }
    },
    [
      dbxLoadSequenceRef,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setActiveDbxSchema,
      setActiveObject,
      setDbxColumnsByTable,
      setDbxObjects,
      setDbxSchemas,
      setError,
      setLoading,
      setQueryResult,
      setSqlResult,
    ],
  );

  const loadDbxConnection = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      const requestSeq = dbxLoadSequenceRef.current.next();
      legacyLoadSequenceRef.current.invalidate();
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setSchema(null);
      setActiveObject(null);
      setActiveDbxObject(null);
      setQueryResult(null);
      setSqlResult(null);
      setSql("");
      setWorkspaceMode("table");
      setError(null);
      setActiveMongoWorkspaceDatabase(null);
      setActiveMongoDocumentId(null);
      setDbxDatabases([]);
      setDbxSchemas([]);
      setDbxObjects([]);
      // 列缓存的键是 `模式.表名`(见 `dbxObjectKey`),不含连接与库 —— 两条连接里同名的
      // `public.users` 会撞到同一个键上。上面那几份状态都清了,这一份也必须清,否则换连接后
      // 属性页 / 侧边栏搜索会拿上一条连接的列元数据来显示(它们只读缓存、不会自己重拉)。
      setDbxColumnsByTable({});

      if (["redis", "mongodb"].includes(connection.dbType)) {
        setActiveDbxDatabase(null);
        setActiveDbxSchema(null);
        setWorkspaceMode(connection.dbType === "redis" ? "redis" : "mongo");
        await databaseApi.dbxConnect(connection.id);
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        if (connection.dbType === "redis") {
          const databases = await loadRedisSidebarDatabases(connection);
          if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
          const firstDb = databases[0]?.db;
          setActiveDbxDatabase(firstDb == null ? null : `db${firstDb}`);
        } else {
          const databases = await loadMongoSidebarDatabases(connection);
          if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
          const database = databases[0] ?? null;
          setActiveDbxDatabase(database);
        }
        return;
      }

      setLoading(true);
      try {
        await databaseApi.dbxConnect(connection.id);
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        const databases = await databaseApi.dbxListDatabases(connection.id);
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        const targetDatabase = configuredTargetDatabase(connection);
        const visibleDatabases = filterDbxDatabasesForConnection(databases, connection);
        if (targetDatabase && visibleDatabases.length === 0) {
          setActiveDbxDatabase(null);
          setActiveDbxSchema(null);
          setError(t("database.configuredDatabaseMissing", { database: targetDatabase }));
          return;
        }
        setDbxDatabases(visibleDatabases);
        const database = targetDatabase ?? visibleDatabases[0]?.name ?? null;
        setActiveDbxDatabase(database);
        setActiveDbxSchema(null);
        const [objects, schemas] = await Promise.all([
          listAllDbxObjects(connection.id, database, null),
          databaseApi
            .dbxListSchemas(connection.id, database)
            .then((value) => (Array.isArray(value) ? value : []))
            .catch(() => [] as string[]),
        ]);
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setDbxSchemas(schemas.length > 0 ? schemas : deriveDbxSchemas(objects));
        setDbxObjects(objects);
      } catch (err) {
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setError(String(err));
      } finally {
        if (dbxLoadSequenceRef.current.isCurrent(requestSeq)) setLoading(false);
      }
    },
    [
      dbxLoadSequenceRef,
      legacyLoadSequenceRef,
      loadMongoSidebarDatabases,
      loadRedisSidebarDatabases,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setActiveDbxSchema,
      setActiveMongoDocumentId,
      setActiveMongoWorkspaceDatabase,
      setActiveObject,
      setDbxColumnsByTable,
      setDbxDatabases,
      setDbxObjects,
      setDbxSchemas,
      setError,
      setLoading,
      setQueryResult,
      setSchema,
      setSql,
      setSqlResult,
      setWorkspaceMode,
      t,
    ],
  );

  const loadDbxObject = useCallback(
    async (
      object: DbxObjectInfo,
      nextPage: number,
      connection = activeDbxConnection,
      database = activeDbxDatabase,
      whereInput?: string | null,
      orderBy?: string | null,
      pageSize = dbxGridPageSize,
      columnFuzzyFiltersOverride?: DbxGridColumnFuzzyFilters,
    ) => {
      if (!connection) return;
      const requestSeq = dbxLoadSequenceRef.current.next();
      const normalizedWhereInput = whereInput?.trim() ?? "";
      const normalizedOrderBy = orderBy?.trim() ?? "";
      const sameDbxObject =
        activeDbxConnection?.id === connection.id &&
        activeDbxDatabase === database &&
        activeDbxObject?.name === object.name &&
        activeDbxObject?.schema === object.schema;
      const activeColumnFuzzyFilters =
        columnFuzzyFiltersOverride ?? (sameDbxObject ? dbxGridColumnFuzzyFilters : {});
      const effectiveWhereInput = combineDbxGridWhereFilters(
        normalizedWhereInput,
        activeColumnFuzzyFilters,
      );
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const result = await databaseApi.dbxQueryTableData({
          connectionId: connection.id,
          database,
          schema: object.schema ?? null,
          table: object.name,
          page: nextPage,
          pageSize,
          whereInput: effectiveWhereInput || null,
          orderBy: normalizedOrderBy || null,
        });
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        let objectColumns: DbxColumnInfo[] = [];
        if (isDbxTableObject(object)) {
          try {
            objectColumns = await databaseApi.dbxGetColumns(
              connection.id,
              object.name,
              database,
              object.schema ?? null,
            );
            if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
            setDbxColumnsByTable((current) => ({
              ...current,
              [dbxObjectKey(object)]: objectColumns,
            }));
          } catch {
            objectColumns = [];
          }
        }
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        const primaryKeys = objectColumns
          .filter((column) => column.is_primary_key)
          .map((column) => column.name);
        const editable =
          isDbxTableObject(object) &&
          !connection.readOnly &&
          (primaryKeys.length > 0 || DBX_KEYLESS_GRID_EDIT_DB_TYPES.has(connection.dbType));
        const resultRows = dbxRowsToDatabaseRows(result.result.rows);
        const headerColumnTypes = result.result.columns.map((column, index) => {
          const metadataColumn = objectColumns.find(
            (item) => item.name.toLowerCase() === column.toLowerCase(),
          );
          return metadataColumn?.data_type ?? result.result.column_types?.[index] ?? "";
        });
        setActiveDbxObject(object);
        setActiveDbxSchema(object.schema ?? null);
        setWorkspaceMode("table");
        const tabId = `table:${object.name}`;
        setWorkspaceTabs((prev) =>
          prev.some((t) => t.id === tabId)
            ? prev
            : [...prev, { id: tabId, mode: "table", label: object.name, closable: true }],
        );
        setActiveTabId(tabId);
        setActiveObject({
          name: object.name,
          objectType: object.object_type,
          columns: dbxColumnsToDbColumns(objectColumns),
          indexes: [],
          foreignKeys: [],
          triggers: [],
          editable,
          primaryKeys,
          hasRowId: false,
        });
        setPage(nextPage);
        setQueryResult({
          columns: result.result.columns,
          columnTypes: result.result.column_types,
          columnSortables: result.result.column_sortables,
          rows: resultRows,
          page: nextPage,
          pageSize,
          totalRows: result.totalRows ?? null,
          editable,
          primaryKeys,
          hasRowId: false,
        });
        initializeLoadedGrid({
          sameDbxObject,
          columns: result.result.columns,
          rows: resultRows,
          columnTypes: headerColumnTypes,
          whereInput: normalizedWhereInput,
          orderByInput: normalizedOrderBy,
        });
        setSql(result.sql);
      } catch (err) {
        if (!dbxLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setError(String(err));
      } finally {
        if (dbxLoadSequenceRef.current.isCurrent(requestSeq)) setLoading(false);
      }
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject?.name,
      activeDbxObject?.schema,
      dbxGridColumnFuzzyFilters,
      dbxGridPageSize,
      dbxLoadSequenceRef,
      initializeLoadedGrid,
      setActiveDbxObject,
      setActiveDbxSchema,
      setActiveObject,
      setActiveTabId,
      setDbxColumnsByTable,
      setError,
      setLoading,
      setPage,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
      setWorkspaceTabs,
    ],
  );

  const loadDbxColumnsForTables = useCallback(
    async (
      objects: DbxObjectInfo[],
      connection = activeDbxConnection,
      database = activeDbxDatabase,
    ) => {
      if (!connection || !isSqlDbxConnection(connection)) return;
      const nextColumns: Record<string, DbxColumnInfo[]> = {};
      for (const object of objects
        .filter((item) => isDbxTableObject(item) || isDbxViewObject(item))
        .slice(0, 12)) {
        try {
          nextColumns[dbxObjectKey(object)] = await databaseApi.dbxGetColumns(
            connection.id,
            object.name,
            database,
            object.schema ?? null,
          );
        } catch {
          nextColumns[dbxObjectKey(object)] = [];
        }
      }
      setDbxColumnsByTable((current) => ({ ...current, ...nextColumns }));
    },
    [activeDbxConnection, activeDbxDatabase, setDbxColumnsByTable],
  );

  /**
   * 「表属性」面板的原地刷新:重新拉一张表的列,并把它所在模式的对象列表(索引 / 外键 / 触发器
   * 都在里面)换成服务端的最新一份。
   *
   * 与上面五支的三点不同,都是有意的:
   * - **不领 `dbxLoadSequenceRef` 的号**。那条号是「当前在看什么」的唯一真相,领了就会把正在飞的
   *   网格请求判成过期。这里只刷元数据、不动 `activeDbxObject` 与结果集,防串台交给调用方传进来的
   *   `shouldApply`(面板自己那条号 + 表标识)。
   * - **不 `setLoading`**。那是全局遮罩,属性页的刷新用面板上的按钮态表示就够了。
   * - **对象列表按模式合并**,与 `loadDbxSchema` 同一手法:只换掉这张表所在模式的那批,别的模式
   *   已经展开过的内容保留。
   */
  const reloadDbxObjectMetadata = useCallback(
    async (
      object: DbxObjectInfo,
      connection: AeroricDbConnectionConfig,
      database: string | null,
      shouldApply: () => boolean = () => true,
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      const schema = object.schema ?? null;
      const [columns, objects] = await Promise.all([
        databaseApi
          .dbxGetColumns(connection.id, object.name, database, schema)
          .catch(() => [] as DbxColumnInfo[]),
        listAllDbxObjects(connection.id, database, schema).catch(() => null),
      ]);
      if (!shouldApply()) return;
      setDbxColumnsByTable((current) => ({ ...current, [dbxObjectKey(object)]: columns }));
      if (!objects) return;
      setDbxObjects((current) => [
        ...current.filter((item) => (item.schema ?? null) !== schema),
        ...objects,
      ]);
    },
    [setDbxColumnsByTable, setDbxObjects],
  );

  return {
    loadDbxColumnsForTables,
    reloadDbxObjectMetadata,
    loadDbxDatabase,
    loadDbxSchema,
    loadDbxConnection,
    loadDbxObject,
  };
}
