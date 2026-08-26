/**
 * legacy(非 dbx)那条路的连接层:落盘一份连接列表、inspect 一条连接并顺手拉出第一张表,
 * 再加上启动时的两支 effect —— 一支首屏把连接读进来,一支在 `initialSqliteFilePath` 变化时
 * 把那个 sqlite 文件插到列表最前并立刻打开。
 *
 * 从 `DatabaseView.tsx` 抽出。`openedInitialSqliteFilePathRef` 与 `createInitialSqliteEndpoint`
 * 只有这两支 effect 在用,所以一并搬进来,不进返回值;`connections` / `setConnections` 仍留在
 * `DatabaseView`(整份文件到处都在读它),从 `deps` 进来。
 *
 * 两支 effect 的相对位置不能变:原文里它们排在 `useTableInfoPanel` / `useDbxDataGrid` 那些
 * effect 之后,所以这支 hook 也必须调用在那两支之后,否则 effect 的执行顺序会变。
 *
 * 与原文逐字一致、不能动的几处:
 * - `inspect` 走 legacy 自己那条请求号,每个 await 之后 `isCurrent()` 对号,过期的响应连
 *   `setLoading(false)` 都不做 —— 交给那个更晚的请求收尾。
 * - 首屏那支 effect 的空依赖数组与 `// Load once; inspect is intentionally not a dependency here.`
 *   连同 eslint-disable 一起保留:加上 `inspect` 会让它在每次连接列表变化后重跑一遍。
 * - 第二支 effect 用 `openedInitialSqliteFilePathRef` 去重,同一个文件路径只自动打开一次。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 与那个 ref 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 它们的身份本来就不变,行为不受影响。
 */

import {
  useCallback,
  useEffect,
  useRef,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";

import { databaseApi } from "../../lib/databaseApi";
import { quoteSqlName } from "../../lib/databaseUtils";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DbObject,
  DbQueryResult,
  DbSchema,
  SshConnection,
} from "../../types";
import {
  PAGE_SIZE,
  createSqliteFileConnection,
  sqliteEndpointKey,
  type DbWorkspaceMode,
} from "./databaseViewModel";
import type { RequestSequence } from "./requestSequence";

export interface LegacyConnectionLoaderDeps {
  connections: DbConnectionConfig[];
  /** 两支 effect 都要用 updater 形式改这份列表,所以拿的是完整的 setState 签名。 */
  setConnections: Dispatch<SetStateAction<DbConnectionConfig[]>>;
  activeConnectionId: string | null;
  projectRoot: string | undefined;
  initialSqliteFilePath: string | undefined;
  remoteConnection: SshConnection | undefined;
  remoteProjectPath: string | undefined;
  legacyLoadSequenceRef: MutableRefObject<RequestSequence>;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setPage: (page: number) => void;
  setSql: (sql: string) => void;
  setSchema: (schema: DbSchema | null) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setActiveObject: (object: DbObject | null) => void;
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setDbxConnections: (next: AeroricDbConnectionConfig[]) => void;
}

export interface LegacyConnectionLoader {
  saveConnections: (next: DbConnectionConfig[]) => void;
  inspect: (connection: DbConnectionConfig) => Promise<void>;
}

export function useLegacyConnectionLoader(
  deps: LegacyConnectionLoaderDeps,
): LegacyConnectionLoader {
  const {
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
  } = deps;
  const openedInitialSqliteFilePathRef = useRef<string | null>(null);

  const createInitialSqliteEndpoint = useCallback((): DbEndpoint | null => {
    const path = initialSqliteFilePath?.trim();
    if (!path) return null;
    if (remoteConnection) {
      return {
        kind: "ssh",
        connection: remoteConnection,
        path,
        projectPath: remoteProjectPath,
      };
    }
    return { kind: "local", path };
  }, [initialSqliteFilePath, remoteConnection, remoteProjectPath]);

  const saveConnections = useCallback(
    (next: DbConnectionConfig[]) => {
      setConnections(next);
      databaseApi.saveConnections(next).catch((err) => {
        setError(String(err));
      });
    },
    [setConnections, setError],
  );

  const inspect = useCallback(
    async (connection: DbConnectionConfig) => {
      const requestSeq = legacyLoadSequenceRef.current.next();
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const nextSchema = await databaseApi.inspect(connection.endpoint, projectRoot);
        if (!legacyLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setSchema(nextSchema);
        const firstTable =
          nextSchema.objects.find((object) => object.objectType === "table") ??
          nextSchema.objects[0] ??
          null;
        setActiveObject(firstTable);
        setPage(1);
        if (firstTable) {
          const result = await databaseApi.queryTable(
            connection.endpoint,
            firstTable.name,
            1,
            PAGE_SIZE,
            projectRoot,
          );
          if (!legacyLoadSequenceRef.current.isCurrent(requestSeq)) return;
          setQueryResult(result);
          setSql(`SELECT * FROM ${quoteSqlName(firstTable.name)}`);
        } else {
          setQueryResult(null);
        }
        if (connections.some((item) => item.id === connection.id)) {
          const now = Date.now();
          saveConnections(
            connections.map((item) =>
              item.id === connection.id ? { ...item, lastOpenedAt: now } : item,
            ),
          );
        }
      } catch (err) {
        if (!legacyLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setError(String(err));
      } finally {
        if (legacyLoadSequenceRef.current.isCurrent(requestSeq)) setLoading(false);
      }
    },
    [
      connections,
      legacyLoadSequenceRef,
      projectRoot,
      saveConnections,
      setActiveObject,
      setError,
      setLoading,
      setPage,
      setQueryResult,
      setSchema,
      setSql,
      setSqlResult,
    ],
  );

  useEffect(() => {
    databaseApi
      .loadConnections()
      .then((items) => {
        const initialEndpoint = createInitialSqliteEndpoint();
        if (initialEndpoint) {
          const initialEndpointKey = sqliteEndpointKey(initialEndpoint);
          const initialConnection = createSqliteFileConnection(initialEndpoint);
          openedInitialSqliteFilePathRef.current = initialEndpointKey;
          setConnections([
            initialConnection,
            ...items.filter((item) => sqliteEndpointKey(item.endpoint) !== initialEndpointKey),
          ]);
          setActiveConnectionId(initialConnection.id);
          inspect(initialConnection);
        } else {
          setConnections(items);
        }
        if (!initialEndpoint && items[0]) {
          setActiveConnectionId(items[0].id);
          inspect(items[0]);
        }
      })
      .catch((err) => setError(String(err)));
    databaseApi
      .dbxListConnections()
      .then((items) => {
        setDbxConnections(items);
        if (!activeConnectionId && !items[0]) return;
      })
      .catch((err) => setError(String(err)));
    // Load once; inspect is intentionally not a dependency here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const endpoint = createInitialSqliteEndpoint();
    if (!endpoint) return;
    const endpointKey = sqliteEndpointKey(endpoint);
    if (openedInitialSqliteFilePathRef.current === endpointKey) return;
    const connection = createSqliteFileConnection(endpoint);
    openedInitialSqliteFilePathRef.current = endpointKey;
    setConnections((current) => [
      connection,
      ...current.filter((item) => sqliteEndpointKey(item.endpoint) !== endpointKey),
    ]);
    setActiveDbxConnectionId(null);
    setActiveConnectionId(connection.id);
    setWorkspaceMode("table");
    inspect(connection);
  }, [
    createInitialSqliteEndpoint,
    inspect,
    setActiveConnectionId,
    setActiveDbxConnectionId,
    setConnections,
    setWorkspaceMode,
  ]);

  return { saveConnections, inspect };
}
