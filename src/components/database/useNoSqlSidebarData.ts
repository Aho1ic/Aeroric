/**
 * redis / mongo 侧边树的数据层:八张缓存、五支加载器、五支「点开某个节点」的选择器。
 *
 * 从 `DatabaseView.tsx` 抽出。这一支和别的抽取不同 —— 它把 state 也一起搬了进来:
 * 那十个 `useState` 除了这五支加载器与五支选择器,外面只在侧边树与 MongoBrowser 的 props 上读一次,
 * 所以整块搬进来后 `DatabaseView` 只剩「拿到值往下传」。
 *
 * 缓存的 key 拼法是全局约定,搬进来后一字未改:redis 键表与游标用 `connectionId:db`,
 * mongo 集合用 `connectionId:database`,mongo 文档用 `connectionId:database:collection`。
 *
 * 五支加载器都把错误咽进 `setError` 再返回一个空列表 —— 调用方(尤其 `loadDbxConnection`)
 * 拿返回值接着往下走,不能靠 throw 中断,这个约定保持原样。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 这些 setter 的身份本来就不变,行为不受影响。
 */

import { useCallback, useState } from "react";

import { databaseApi } from "../../lib/databaseApi";
import type { AeroricDbConnectionConfig, RedisDatabaseInfo, RedisKeyInfo } from "../../types";
import {
  MONGO_SIDEBAR_DOCUMENT_PREVIEW_LIMIT,
  mongoDocumentId,
  type DbWorkspaceMode,
  type MongoSidebarDocumentQuery,
  type RedisSidebarScanState,
} from "./databaseViewModel";

export interface NoSqlSidebarDataDeps {
  setError: (error: string | null) => void;
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setActiveDbxDatabase: (database: string | null) => void;
  /** redis 拿它存当前选中的 key,mongo 存当前选中的集合 —— 沿用原来的复用方式。 */
  setActiveDbxSchema: (schema: string | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
}

export interface NoSqlSidebarDataState {
  redisDatabasesByConnection: Record<string, RedisDatabaseInfo[]>;
  redisKeysByDatabase: Record<string, RedisKeyInfo[]>;
  redisScanStateByDatabase: Record<string, RedisSidebarScanState>;
  mongoDatabasesByConnection: Record<string, string[]>;
  mongoCollectionsByDatabase: Record<string, string[]>;
  mongoDocumentsByCollection: Record<string, unknown[]>;
  mongoDocumentTotalsByCollection: Record<string, number>;
  mongoDocumentQueriesByCollection: Record<string, MongoSidebarDocumentQuery>;
  setMongoDocumentQueriesByCollection: (
    updater: (
      current: Record<string, MongoSidebarDocumentQuery>,
    ) => Record<string, MongoSidebarDocumentQuery>,
  ) => void;
  activeMongoDocumentId: string | null;
  setActiveMongoDocumentId: (id: string | null) => void;
  activeMongoWorkspaceDatabase: string | null;
  setActiveMongoWorkspaceDatabase: (database: string | null) => void;
}

export interface NoSqlSidebarDataLoaders {
  /** 五支都不 throw:出错时写 `setError` 并返回空列表,调用方照常往下走。 */
  loadRedisSidebarDatabases: (
    connection: AeroricDbConnectionConfig,
  ) => Promise<RedisDatabaseInfo[]>;
  loadRedisSidebarKeys: (
    connection: AeroricDbConnectionConfig,
    database: number,
    append?: boolean,
  ) => Promise<RedisKeyInfo[]>;
  loadMongoSidebarDatabases: (connection: AeroricDbConnectionConfig) => Promise<string[]>;
  loadMongoSidebarCollections: (
    connection: AeroricDbConnectionConfig,
    database: string,
  ) => Promise<string[]>;
  loadMongoSidebarDocuments: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
    append?: boolean,
    queryOverride?: MongoSidebarDocumentQuery,
  ) => Promise<unknown[]>;
}

export interface NoSqlSidebarDataSelectors {
  selectRedisSidebarDatabase: (connection: AeroricDbConnectionConfig, database: number) => void;
  selectRedisSidebarKey: (
    connection: AeroricDbConnectionConfig,
    database: number,
    keyRaw: string,
  ) => void;
  selectMongoSidebarDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string,
  ) => Promise<void>;
  selectMongoSidebarCollection: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
  ) => Promise<void>;
  selectMongoSidebarDocument: (
    connection: AeroricDbConnectionConfig,
    database: string,
    collection: string,
    document: unknown,
  ) => Promise<void>;
}

/** 分组与 `useDbxDataGrid` 一致,调用方按组解构。 */
export interface NoSqlSidebarData {
  state: NoSqlSidebarDataState;
  loaders: NoSqlSidebarDataLoaders;
  selectors: NoSqlSidebarDataSelectors;
}

export function useNoSqlSidebarData(deps: NoSqlSidebarDataDeps): NoSqlSidebarData {
  const {
    setError,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setActiveDbxDatabase,
    setActiveDbxSchema,
    setWorkspaceMode,
  } = deps;
  const [redisDatabasesByConnection, setRedisDatabasesByConnection] = useState<
    Record<string, RedisDatabaseInfo[]>
  >({});
  const [redisKeysByDatabase, setRedisKeysByDatabase] = useState<Record<string, RedisKeyInfo[]>>(
    {},
  );
  const [redisScanStateByDatabase, setRedisScanStateByDatabase] = useState<
    Record<string, RedisSidebarScanState>
  >({});
  const [mongoDatabasesByConnection, setMongoDatabasesByConnection] = useState<
    Record<string, string[]>
  >({});
  const [mongoCollectionsByDatabase, setMongoCollectionsByDatabase] = useState<
    Record<string, string[]>
  >({});
  const [mongoDocumentsByCollection, setMongoDocumentsByCollection] = useState<
    Record<string, unknown[]>
  >({});
  const [mongoDocumentTotalsByCollection, setMongoDocumentTotalsByCollection] = useState<
    Record<string, number>
  >({});
  const [mongoDocumentQueriesByCollection, setMongoDocumentQueriesByCollection] = useState<
    Record<string, MongoSidebarDocumentQuery>
  >({});
  const [activeMongoDocumentId, setActiveMongoDocumentId] = useState<string | null>(null);
  const [activeMongoWorkspaceDatabase, setActiveMongoWorkspaceDatabase] = useState<string | null>(
    null,
  );

  const loadRedisSidebarDatabases = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      try {
        const databases = await databaseApi.dbxRedisListDatabases(connection.id);
        setRedisDatabasesByConnection((current) => ({ ...current, [connection.id]: databases }));
        return databases;
      } catch (err) {
        setError(String(err));
        return [] as RedisDatabaseInfo[];
      }
    },
    [setError],
  );

  const loadRedisSidebarKeys = useCallback(
    async (connection: AeroricDbConnectionConfig, database: number, append = false) => {
      const databaseKey = `${connection.id}:${database}`;
      const cursor = append ? (redisScanStateByDatabase[databaseKey]?.cursor ?? 0) : 0;
      try {
        const result = await databaseApi.dbxRedisScanKeys({
          connectionId: connection.id,
          db: database,
          cursor,
          pattern: "*",
          count: 100,
        });
        setRedisKeysByDatabase((current) => ({
          ...current,
          [databaseKey]: append ? [...(current[databaseKey] ?? []), ...result.keys] : result.keys,
        }));
        setRedisScanStateByDatabase((current) => ({
          ...current,
          [databaseKey]: { cursor: result.cursor, totalKeys: result.total_keys },
        }));
        return result.keys;
      } catch (err) {
        setError(String(err));
        return [] as RedisKeyInfo[];
      }
    },
    [redisScanStateByDatabase, setError],
  );

  const loadMongoSidebarDatabases = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      try {
        const databases = await databaseApi.dbxMongoListDatabases(connection.id);
        setMongoDatabasesByConnection((current) => ({ ...current, [connection.id]: databases }));
        return databases;
      } catch (err) {
        setError(String(err));
        return [] as string[];
      }
    },
    [setError],
  );

  const loadMongoSidebarCollections = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string) => {
      try {
        const collections = await databaseApi.dbxMongoListCollections(connection.id, database);
        setMongoCollectionsByDatabase((current) => ({
          ...current,
          [`${connection.id}:${database}`]: collections,
        }));
        return collections;
      } catch (err) {
        setError(String(err));
        return [] as string[];
      }
    },
    [setError],
  );

  const loadMongoSidebarDocuments = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string,
      collection: string,
      append = false,
      queryOverride?: MongoSidebarDocumentQuery,
    ) => {
      const key = `${connection.id}:${database}:${collection}`;
      try {
        const query = queryOverride ??
          mongoDocumentQueriesByCollection[key] ?? {
            filter: "{}",
            sort: "{}",
            projection: "{}",
          };
        const skip = append ? (mongoDocumentsByCollection[key]?.length ?? 0) : 0;
        const result = await databaseApi.dbxMongoFindDocuments({
          connectionId: connection.id,
          database,
          collection,
          filter: query.filter,
          projection: query.projection,
          sort: query.sort,
          skip,
          limit: MONGO_SIDEBAR_DOCUMENT_PREVIEW_LIMIT,
        });
        const nextDocuments = append
          ? [...(mongoDocumentsByCollection[key] ?? []), ...result.documents]
          : result.documents;
        setMongoDocumentsByCollection((current) => ({
          ...current,
          [key]: nextDocuments,
        }));
        setMongoDocumentTotalsByCollection((current) => ({
          ...current,
          [key]: result.total,
        }));
        return nextDocuments;
      } catch (err) {
        setError(String(err));
        return [] as unknown[];
      }
    },
    [mongoDocumentQueriesByCollection, mongoDocumentsByCollection, setError],
  );

  const selectRedisSidebarDatabase = useCallback(
    (connection: AeroricDbConnectionConfig, database: number) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(`db${database}`);
      setActiveDbxSchema(null);
      setActiveMongoDocumentId(null);
      setActiveMongoWorkspaceDatabase(null);
      setWorkspaceMode("redis");
    },
    [
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setWorkspaceMode,
    ],
  );

  const selectRedisSidebarKey = useCallback(
    (connection: AeroricDbConnectionConfig, database: number, keyRaw: string) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(`db${database}`);
      setActiveDbxSchema(keyRaw);
      setActiveMongoDocumentId(null);
      setActiveMongoWorkspaceDatabase(null);
      setWorkspaceMode("redis");
    },
    [
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setWorkspaceMode,
    ],
  );

  const selectMongoSidebarDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(null);
      setActiveMongoDocumentId(null);
      setActiveMongoWorkspaceDatabase(database);
      setWorkspaceMode("mongo");
      await loadMongoSidebarCollections(connection, database);
    },
    [
      loadMongoSidebarCollections,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setWorkspaceMode,
    ],
  );

  const selectMongoSidebarCollection = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string, collection: string) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(collection);
      setActiveMongoDocumentId(null);
      setActiveMongoWorkspaceDatabase(database);
      setWorkspaceMode("mongo");
      if (!mongoCollectionsByDatabase[`${connection.id}:${database}`]) {
        await loadMongoSidebarCollections(connection, database);
      }
    },
    [
      loadMongoSidebarCollections,
      mongoCollectionsByDatabase,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setWorkspaceMode,
    ],
  );

  const selectMongoSidebarDocument = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string,
      collection: string,
      document: unknown,
    ) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(collection);
      setActiveMongoDocumentId(mongoDocumentId(document));
      setActiveMongoWorkspaceDatabase(database);
      setWorkspaceMode("mongo");
      if (!mongoDocumentsByCollection[`${connection.id}:${database}:${collection}`]) {
        await loadMongoSidebarDocuments(connection, database, collection);
      }
    },
    [
      loadMongoSidebarDocuments,
      mongoDocumentsByCollection,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxSchema,
      setWorkspaceMode,
    ],
  );

  return {
    state: {
      redisDatabasesByConnection,
      redisKeysByDatabase,
      redisScanStateByDatabase,
      mongoDatabasesByConnection,
      mongoCollectionsByDatabase,
      mongoDocumentsByCollection,
      mongoDocumentTotalsByCollection,
      mongoDocumentQueriesByCollection,
      setMongoDocumentQueriesByCollection,
      activeMongoDocumentId,
      setActiveMongoDocumentId,
      activeMongoWorkspaceDatabase,
      setActiveMongoWorkspaceDatabase,
    },
    loaders: {
      loadRedisSidebarDatabases,
      loadRedisSidebarKeys,
      loadMongoSidebarDatabases,
      loadMongoSidebarCollections,
      loadMongoSidebarDocuments,
    },
    selectors: {
      selectRedisSidebarDatabase,
      selectRedisSidebarKey,
      selectMongoSidebarDatabase,
      selectMongoSidebarCollection,
      selectMongoSidebarDocument,
    },
  };
}
