/**
 * 「选择要显示的数据库」对话框的全部状态与动作。
 *
 * 从 `DatabaseView.tsx` 抽出,手法与 `useTableImportDialog.ts` 一致:状态 + 派生值 +
 * 动作先搬进独立 hook,再把 JSX 拆成纯展示组件。外部依赖收窄成显式参数
 * (见 `VisibleDatabasesDialogDeps`),不再隐式依赖 DatabaseView 里其余上百个 state。
 *
 * 这一簇的写入落在**连接配置**上(`dbx.visible_databases`),不是数据库本身,
 * 所以没有生产库确认闸门。
 */

import { useCallback, useMemo, useState } from "react";

import type { AeroricDbConnectionConfig } from "../../types";
import { databaseApi } from "../../lib/databaseApi";
import {
  configuredVisibleDatabases,
  isSystemDatabaseName,
  normalizeVisibleDatabaseSelection,
} from "./databaseViewModel";

export interface VisibleDatabasesDialogDeps {
  /** 用于把 `visibleDatabaseConnectionId` 解析回连接配置。 */
  dbxConnections: AeroricDbConnectionConfig[];
  /** 保存后刷新连接列表(可见库集合会改变侧边栏树)。 */
  setDbxConnections: (connections: AeroricDbConnectionConfig[]) => void;
  /** 打开对话框时收起右键菜单。 */
  closeContextMenu: () => void;
  /** DatabaseView 的全局忙碌态 / 错误条,保存期间与其保持一致。 */
  setGlobalLoading: (loading: boolean) => void;
  setGlobalError: (error: string | null) => void;
  /** 当前展开的连接;改的是它时要重新加载一次,否则树里还留着被隐藏的库。 */
  activeDbxConnectionId: string | null;
  loadDbxConnection: (connection: AeroricDbConnectionConfig) => Promise<void>;
}

export interface VisibleDatabasesDialogState {
  connection: AeroricDbConnectionConfig | null;
  /** 后端报回来的全部库名。 */
  names: string[];
  /** 去掉系统库(除非勾了显示)后的列表,也是"全选"的作用域。 */
  listedNames: string[];
  /** 再经搜索词过滤后的列表,列表区渲染的就是它。 */
  filteredNames: string[];
  selection: Set<string>;
  search: string;
  showSystem: boolean;
  /** 有系统库时才显示那个开关。 */
  hasSystemNames: boolean;
  loading: boolean;
  error: string;
  /** 空选择会把整棵树清空,所以禁止保存。 */
  canSave: boolean;
  /** 该连接当前是否配了白名单 —— 没配就没什么可"显示全部"的。 */
  hasConfiguredSelection: boolean;
  open: (connection: AeroricDbConnectionConfig) => Promise<void>;
  close: () => void;
  setSearch: (search: string) => void;
  setSelection: (selection: Set<string>) => void;
  toggleSelection: (database: string) => void;
  setShowSystem: (showSystem: boolean) => void;
  save: () => Promise<void>;
  showAll: () => Promise<void>;
}

export function useVisibleDatabasesDialog({
  dbxConnections,
  setDbxConnections,
  closeContextMenu,
  setGlobalLoading,
  setGlobalError,
  activeDbxConnectionId,
  loadDbxConnection,
}: VisibleDatabasesDialogDeps): VisibleDatabasesDialogState {
  const [connectionId, setConnectionId] = useState<string | null>(null);
  const [names, setNames] = useState<string[]>([]);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState("");
  const [showSystem, setShowSystemState] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const connection = useMemo(
    () => dbxConnections.find((item) => item.id === connectionId) ?? null,
    [dbxConnections, connectionId],
  );
  const listedNames = useMemo(
    () =>
      showSystem ? names : names.filter((name) => !isSystemDatabaseName(connection?.dbType, name)),
    [connection?.dbType, names, showSystem],
  );
  const filteredNames = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return listedNames;
    return listedNames.filter((name) => name.toLowerCase().includes(query));
  }, [listedNames, search]);
  const hasSystemNames = useMemo(
    () => names.some((name) => isSystemDatabaseName(connection?.dbType, name)),
    [connection?.dbType, names],
  );
  const canSave = selection.size > 0;
  const hasConfiguredSelection = Boolean(connection && configuredVisibleDatabases(connection));

  const close = useCallback(() => {
    setConnectionId(null);
    setNames([]);
    setSelection(new Set());
    setSearch("");
    setShowSystemState(false);
    setError("");
  }, []);

  /** 三种引擎的"库"概念不同:Redis 是编号,MongoDB 直接给名字,SQL 走库列表。 */
  const loadNames = useCallback(
    async (nextConnection: AeroricDbConnectionConfig): Promise<string[]> => {
      await databaseApi.dbxConnect(nextConnection.id);
      if (nextConnection.dbType === "redis") {
        return (await databaseApi.dbxRedisListDatabases(nextConnection.id)).map((database) =>
          String(database.db),
        );
      }
      if (nextConnection.dbType === "mongodb") {
        return databaseApi.dbxMongoListDatabases(nextConnection.id);
      }
      return (await databaseApi.dbxListDatabases(nextConnection.id)).map(
        (database) => database.name,
      );
    },
    [],
  );

  const open = useCallback(
    async (nextConnection: AeroricDbConnectionConfig) => {
      closeContextMenu();
      setConnectionId(nextConnection.id);
      setNames([]);
      setSelection(new Set());
      setSearch("");
      setShowSystemState(false);
      setLoading(true);
      setError("");
      try {
        const nextNames = await loadNames(nextConnection);
        const configured = configuredVisibleDatabases(nextConnection);
        // 没配过白名单时默认勾选所有非系统库,和树里当前看到的内容一致。
        const initialSelection = configured
          ? normalizeVisibleDatabaseSelection(configured, nextNames)
          : nextNames.filter((name) => !isSystemDatabaseName(nextConnection.dbType, name));
        setNames(nextNames);
        setSelection(new Set(initialSelection));
        // 已有选择里含系统库时必须把开关打开,否则它们会被下面的过滤悄悄摘掉。
        setShowSystemState(
          initialSelection.some((name) => isSystemDatabaseName(nextConnection.dbType, name)),
        );
      } catch (err) {
        setNames([]);
        setSelection(new Set());
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [closeContextMenu, loadNames],
  );

  const toggleSelection = useCallback((database: string) => {
    setSelection((current) => {
      const next = new Set(current);
      if (next.has(database)) next.delete(database);
      else next.add(database);
      return next;
    });
  }, []);

  const setShowSystem = useCallback(
    (nextShowSystem: boolean) => {
      setShowSystemState(nextShowSystem);
      // 关掉开关时同步摘掉已勾的系统库:留着会存进配置,但界面上再也看不见。
      if (!nextShowSystem) {
        setSelection(
          (current) =>
            new Set([...current].filter((name) => !isSystemDatabaseName(connection?.dbType, name))),
        );
      }
    },
    [connection?.dbType],
  );

  /** `undefined` 表示删掉白名单(显示全部),数组表示写入白名单。 */
  const saveConfig = useCallback(
    async (nextConnection: AeroricDbConnectionConfig, visibleDatabases: string[] | undefined) => {
      const currentDbx =
        nextConnection.dbx && typeof nextConnection.dbx === "object"
          ? (nextConnection.dbx as Record<string, unknown>)
          : {};
      const nextDbx = { ...currentDbx };
      if (visibleDatabases) {
        nextDbx.visible_databases = visibleDatabases;
      } else {
        delete nextDbx.visible_databases;
      }
      const saved: AeroricDbConnectionConfig = { ...nextConnection, dbx: nextDbx };
      setGlobalLoading(true);
      setGlobalError(null);
      try {
        await databaseApi.dbxSaveConnection(saved);
        setDbxConnections(await databaseApi.dbxListConnections());
        if (activeDbxConnectionId === nextConnection.id) {
          await loadDbxConnection(saved);
        }
        close();
      } catch (err) {
        setGlobalError(String(err));
        setError(String(err));
      } finally {
        setGlobalLoading(false);
      }
    },
    [
      activeDbxConnectionId,
      close,
      loadDbxConnection,
      setDbxConnections,
      setGlobalError,
      setGlobalLoading,
    ],
  );

  const save = useCallback(async () => {
    if (!connection || !canSave) return;
    await saveConfig(connection, normalizeVisibleDatabaseSelection([...selection], names));
  }, [canSave, connection, names, saveConfig, selection]);

  const showAll = useCallback(async () => {
    if (!connection) return;
    await saveConfig(connection, undefined);
  }, [connection, saveConfig]);

  return {
    connection,
    names,
    listedNames,
    filteredNames,
    selection,
    search,
    showSystem,
    hasSystemNames,
    loading,
    error,
    canSave,
    hasConfiguredSelection,
    open,
    close,
    setSearch,
    setSelection,
    toggleSelection,
    setShowSystem,
    save,
    showAll,
  };
}
