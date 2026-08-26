/**
 * 侧边树上「连接层」的十支动作:选中一条连接(legacy / dbx 各一支)、删除一条连接(各一支)、
 * 切只读、拉一页 legacy 表数据、刷新当前连接,以及复制连接(各一支)与复制节点名。
 *
 * 从 `DatabaseView.tsx` 抽出:这一批原本在文件里就是连续的一段,共同点是都以「一条连接」为单位
 * 动作 —— 要么切当前连接并把上一条的库 / 对象 / 结果清掉,要么改那份连接配置再落盘。
 *
 * 两条请求号的用法与原来逐字一致:`handleSelectConnection` 切到 legacy 之前先
 * `dbxLoadSequenceRef.current.invalidate()`,把 dbx 那条路正在飞的请求作废;`loadTable` 走的是
 * legacy 自己那条号,每个 await 之后 `isCurrent()` 对号,过期的响应连 `setLoading(false)` 都不做。
 *
 * `loadTable` 与 `refresh` 是这段里唯二会去拉数据的:前者是 legacy 侧的「表数据页」加载器,
 * 后者只是按当前连接类型重新走一遍 `inspect` / `loadDbxConnection`。真正的加载器都从 `deps`
 * 传进来,这一层不自己实现。
 *
 * 删除那两支的确认弹窗共用同一套 i18n key(`database.confirmDeleteConnection` /
 * `database.deleteConnection` / `file.delete` / `common.cancel`),逐字保留;删完之后
 * 「当前连接正好是被删的那条」才顺移到 `next[0]`,这个条件不能省。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 与两个 ref 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 它们的身份本来就不变,行为不受影响。
 */

import { useCallback, type MutableRefObject } from "react";

import { useI18n } from "../../i18n";
import { confirm } from "../../lib/appDialog";
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
  DbxDatabaseInfo,
  DbxObjectInfo,
} from "../../types";
import { PAGE_SIZE, type DbWorkspaceMode } from "./databaseViewModel";
import type { RequestSequence } from "./requestSequence";

export interface ConnectionLifecycleActionsDeps {
  connections: DbConnectionConfig[];
  saveConnections: (next: DbConnectionConfig[]) => void;
  activeConnection: DbConnectionConfig | null;
  activeConnectionId: string | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxConnectionId: string | null;
  activeEndpoint: DbEndpoint | null;
  projectRoot: string | undefined;
  /** 两条请求号:legacy 自己那条 + 要被 `handleSelectConnection` 作废的 dbx 那条。 */
  legacyLoadSequenceRef: MutableRefObject<RequestSequence>;
  dbxLoadSequenceRef: MutableRefObject<RequestSequence>;
  /** 两支连接级加载器,`handleSelectConnection` / `handleSelectDbxConnection` / `refresh` 要用。 */
  inspect: (connection: DbConnectionConfig) => Promise<void>;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setPage: (page: number) => void;
  setSql: (sql: string) => void;
  setSchema: (schema: DbSchema | null) => void;
  setSqlResult: (result: DbExecuteResult | null) => void;
  setQueryResult: (result: DbQueryResult | null) => void;
  setWorkspaceMode: (mode: DbWorkspaceMode) => void;
  setDbxConnections: (next: AeroricDbConnectionConfig[]) => void;
  setActiveConnectionId: (id: string | null) => void;
  setActiveDbxConnectionId: (id: string | null) => void;
  setActiveDbxDatabase: (database: string | null) => void;
  setActiveDbxObject: (object: DbxObjectInfo | null) => void;
  setActiveObject: (object: DbObject | null) => void;
  setDbxDatabases: (databases: DbxDatabaseInfo[]) => void;
  setDbxObjects: (objects: DbxObjectInfo[]) => void;
}

export interface ConnectionLifecycleActions {
  handleSelectConnection: (connection: DbConnectionConfig) => void;
  handleSelectDbxConnection: (connection: AeroricDbConnectionConfig) => void;
  handleDeleteConnection: (connectionId: string) => Promise<void>;
  handleDeleteDbxConnection: (connectionId: string) => Promise<void>;
  toggleReadOnly: () => void;
  loadTable: (object: DbObject, nextPage: number) => Promise<void>;
  refresh: () => void;
  copyDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  copyLegacyConnection: (connection: DbConnectionConfig) => void;
  copyNodeName: (name: string) => void;
}

export function useConnectionLifecycleActions(
  deps: ConnectionLifecycleActionsDeps,
): ConnectionLifecycleActions {
  const { t } = useI18n();
  const {
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
  } = deps;

  const handleSelectConnection = useCallback(
    (connection: DbConnectionConfig) => {
      dbxLoadSequenceRef.current.invalidate();
      setActiveDbxConnectionId(null);
      setDbxDatabases([]);
      setDbxObjects([]);
      setActiveDbxDatabase(null);
      setActiveDbxObject(null);
      setActiveConnectionId(connection.id);
      setWorkspaceMode("table");
      inspect(connection);
    },
    [
      dbxLoadSequenceRef,
      inspect,
      setActiveConnectionId,
      setActiveDbxConnectionId,
      setActiveDbxDatabase,
      setActiveDbxObject,
      setDbxDatabases,
      setDbxObjects,
      setWorkspaceMode,
    ],
  );

  const handleSelectDbxConnection = useCallback(
    (connection: AeroricDbConnectionConfig) => {
      loadDbxConnection(connection);
    },
    [loadDbxConnection],
  );

  const handleDeleteConnection = useCallback(
    async (connectionId: string) => {
      const ok = await confirm(t("database.confirmDeleteConnection"), {
        title: t("database.deleteConnection"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      const next = connections.filter((connection) => connection.id !== connectionId);
      saveConnections(next);
      if (activeConnectionId === connectionId) {
        setActiveConnectionId(next[0]?.id ?? null);
        setSchema(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      }
    },
    [
      activeConnectionId,
      connections,
      saveConnections,
      setActiveConnectionId,
      setActiveObject,
      setQueryResult,
      setSchema,
      setSqlResult,
      t,
    ],
  );

  const handleDeleteDbxConnection = useCallback(
    async (connectionId: string) => {
      const ok = await confirm(t("database.confirmDeleteConnection"), {
        title: t("database.deleteConnection"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxDeleteConnection(connectionId);
        const next = await databaseApi.dbxListConnections();
        setDbxConnections(next);
        if (activeDbxConnectionId === connectionId) {
          setActiveDbxConnectionId(next[0]?.id ?? null);
          setSchema(null);
          setActiveObject(null);
          setQueryResult(null);
          setSqlResult(null);
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [
      activeDbxConnectionId,
      setActiveDbxConnectionId,
      setActiveObject,
      setDbxConnections,
      setError,
      setLoading,
      setQueryResult,
      setSchema,
      setSqlResult,
      t,
    ],
  );

  const toggleReadOnly = useCallback(() => {
    if (!activeConnection) return;
    const next = connections.map((connection) =>
      connection.id === activeConnection.id
        ? { ...connection, readOnly: !connection.readOnly }
        : connection,
    );
    saveConnections(next);
  }, [activeConnection, connections, saveConnections]);

  const loadTable = useCallback(
    async (object: DbObject, nextPage: number) => {
      if (!activeEndpoint) return;
      const requestSeq = legacyLoadSequenceRef.current.next();
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const result = await databaseApi.queryTable(
          activeEndpoint,
          object.name,
          nextPage,
          PAGE_SIZE,
          projectRoot,
        );
        if (!legacyLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setActiveObject(object);
        setWorkspaceMode("table");
        setPage(nextPage);
        setQueryResult(result);
        setSql(`SELECT * FROM ${quoteSqlName(object.name)}`);
      } catch (err) {
        if (!legacyLoadSequenceRef.current.isCurrent(requestSeq)) return;
        setError(String(err));
      } finally {
        if (legacyLoadSequenceRef.current.isCurrent(requestSeq)) setLoading(false);
      }
    },
    [
      activeEndpoint,
      legacyLoadSequenceRef,
      projectRoot,
      setActiveObject,
      setError,
      setLoading,
      setPage,
      setQueryResult,
      setSql,
      setSqlResult,
      setWorkspaceMode,
    ],
  );

  const refresh = useCallback(() => {
    if (activeConnection) inspect(activeConnection);
    if (activeDbxConnection) void loadDbxConnection(activeDbxConnection);
  }, [activeConnection, activeDbxConnection, inspect, loadDbxConnection]);

  const copyDbxConnection = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      const copy: AeroricDbConnectionConfig = {
        ...connection,
        id: `dbx:${Date.now()}:${Math.random().toString(36).slice(2)}`,
        name: `${connection.name} (Copy)`,
        createdAt: Date.now(),
        lastOpenedAt: Date.now(),
      };
      await databaseApi.dbxSaveConnection(copy);
      setDbxConnections(await databaseApi.dbxListConnections());
    },
    [setDbxConnections],
  );

  const copyLegacyConnection = useCallback(
    (connection: DbConnectionConfig) => {
      const now = Date.now();
      saveConnections([
        {
          ...connection,
          id: `db:${now}:${Math.random().toString(36).slice(2)}`,
          name: `${connection.name} (Copy)`,
          createdAt: now,
          lastOpenedAt: now,
        },
        ...connections,
      ]);
    },
    [connections, saveConnections],
  );

  const copyNodeName = useCallback(
    (name: string) => {
      navigator.clipboard?.writeText(name).catch((err) => {
        setError(String(err));
      });
    },
    [setError],
  );

  return {
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
  };
}
