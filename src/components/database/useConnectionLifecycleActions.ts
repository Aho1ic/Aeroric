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
 * 后者按当前连接类型重新走一遍 `inspect` / `loadDbxConnection`,并在 dbx 侧尽量恢复现场 ——
 * `loadDbxConnection` 本质是「重连并回到初始态」,会把当前对象、结果与面板全清掉,所以
 * `refresh` 先记下库 / 对象 / 面板,重连完再切回去;那张表已经不在了就停在空态,不报错。
 * 真正的加载器都从 `deps` 传进来,这一层不自己实现。
 *
 * 删除那两支的确认弹窗共用同一套 i18n key(`database.confirmDeleteConnection` /
 * `database.deleteConnection` / `file.delete` / `common.cancel`),逐字保留;删完之后
 * 「当前连接正好是被删的那条」才顺移到 `next[0]`,这个条件不能省。
 *
 * 与原文唯一的差别:原先直接闭包捕获的那些 useState setter 与两个 ref 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它们是稳定引用,于是补进了依赖数组 ——
 * 它们的身份本来就不变,行为不受影响。
 */

import { useCallback, type MutableRefObject, type SetStateAction } from "react";

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
  DbxColumnInfo,
  DbxDatabaseInfo,
  DbxObjectInfo,
} from "../../types";
import { PAGE_SIZE, listAllDbxObjects, type DbWorkspaceMode } from "./databaseViewModel";
import type { RequestSequence } from "./requestSequence";

/**
 * 这几个面板都是「当前那张表」的视图,刷新后跟着表一起恢复才合理。其余面板要么是连接级
 * (query / drivers / user-admin…),要么本身就不依赖 activeDbxObject,让它们跟着
 * `loadDbxConnection` 回落到 "table" 与原行为一致。
 */
const TABLE_SCOPED_WORKSPACE_MODES = new Set<DbWorkspaceMode>([
  "table",
  "table-structure",
  "table-info",
  "field-lineage",
]);

export interface ConnectionLifecycleActionsDeps {
  connections: DbConnectionConfig[];
  saveConnections: (next: DbConnectionConfig[]) => void;
  activeConnection: DbConnectionConfig | null;
  activeConnectionId: string | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxConnectionId: string | null;
  /** `refresh` 要用它们把刷新前停在哪张表、哪个面板上记下来再恢复。 */
  activeDbxDatabase: string | null;
  activeDbxObject: DbxObjectInfo | null;
  workspaceMode: DbWorkspaceMode;
  activeEndpoint: DbEndpoint | null;
  projectRoot: string | undefined;
  /** 两条请求号:legacy 自己那条 + 要被 `handleSelectConnection` 作废的 dbx 那条。 */
  legacyLoadSequenceRef: MutableRefObject<RequestSequence>;
  dbxLoadSequenceRef: MutableRefObject<RequestSequence>;
  /** 两支连接级加载器,`handleSelectConnection` / `handleSelectDbxConnection` / `refresh` 要用。 */
  inspect: (connection: DbConnectionConfig) => Promise<void>;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
  /** `refresh` 恢复现场要用:先切回原来的库,再重开原来那张表。 */
  loadDbxDatabase: (
    connection: AeroricDbConnectionConfig,
    database: string | null,
  ) => Promise<void>;
  loadDbxObject: (
    object: DbxObjectInfo,
    nextPage: number,
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
  ) => Promise<void>;
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
  /** 换连接时要清:`dbxObjectKey` 的键不含连接,不清会串台。 */
  setDbxColumnsByTable: (value: SetStateAction<Record<string, DbxColumnInfo[]>>) => void;
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
    activeDbxDatabase,
    activeDbxObject,
    activeEndpoint,
    projectRoot,
    workspaceMode,
    legacyLoadSequenceRef,
    dbxLoadSequenceRef,
    inspect,
    loadDbxConnection,
    loadDbxDatabase,
    loadDbxObject,
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
    setDbxColumnsByTable,
  } = deps;

  const handleSelectConnection = useCallback(
    (connection: DbConnectionConfig) => {
      dbxLoadSequenceRef.current.invalidate();
      setActiveDbxConnectionId(null);
      setDbxDatabases([]);
      setDbxObjects([]);
      // 与 `loadDbxConnection` 同一个理由:列缓存的键不含连接,留着会串到下一条连接上。
      setDbxColumnsByTable({});
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
      setDbxColumnsByTable,
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

  /**
   * 侧边栏那颗刷新。
   *
   * `loadDbxConnection` 本身是「重连并回到初始态」——它会清掉当前库 / 当前对象 / 结果集,
   * 于是原来点一下刷新,打开的表连同数据一起没了,看着就像「刷新不刷新表信息」。
   * 这里记下刷新前停在哪张表,重连完如果那张表还在就回到它;表已经被删掉才停在空态。
   */
  const refresh = useCallback(() => {
    if (activeConnection) inspect(activeConnection);
    if (!activeDbxConnection) return;
    const previousDatabase = activeDbxDatabase;
    const previousObject = activeDbxObject;
    const previousMode = workspaceMode;
    void (async () => {
      await loadDbxConnection(activeDbxConnection);
      if (!previousObject) return;
      // 重连会把当前库切回「配置指定的库,否则第一个可见库」。原来停在哪个库上
      // 这里无从得知(状态更新还没回到本闭包),所以只要之前有库就无条件切回去 ——
      // 多一次查询,但不会把接下来那次对象查询打到错的库上。
      if (previousDatabase) {
        await loadDbxDatabase(activeDbxConnection, previousDatabase);
      }
      // 重连后对象列表是新的:只有那张表还在才恢复,不存在就让它停在空态,
      // 而不是去查一张已经不存在的表、把错误提示丢给用户。
      const objects = await listAllDbxObjects(
        activeDbxConnection.id,
        previousDatabase,
        previousObject.schema ?? null,
      ).catch(() => [] as DbxObjectInfo[]);
      const stillThere = objects.some(
        (object) =>
          object.name === previousObject.name &&
          (object.schema ?? null) === (previousObject.schema ?? null),
      );
      if (!stillThere) return;
      await loadDbxObject(previousObject, 1, activeDbxConnection, previousDatabase);
      // `loadDbxObject` 结尾一律把面板切成 "table"。刷新前停在同一张表的另一个面板
      // (结构 / 信息 / 血缘)时要切回去,否则「刷新」会顺手把用户从属性页踢回数据页。
      if (previousMode !== "table" && TABLE_SCOPED_WORKSPACE_MODES.has(previousMode)) {
        setWorkspaceMode(previousMode);
      }
    })();
  }, [
    activeConnection,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    workspaceMode,
    inspect,
    loadDbxConnection,
    loadDbxDatabase,
    loadDbxObject,
    setWorkspaceMode,
  ]);

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
