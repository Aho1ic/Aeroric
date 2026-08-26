/**
 * 「导出数据库」对话框的全部状态与动作。
 *
 * 从 `DatabaseView.tsx` 抽出,手法与 `useTableImportDialog.ts` / `useVisibleDatabasesDialog.ts`
 * 一致。外部依赖收窄成显式参数(见 `DatabaseExportDialogDeps`)。
 *
 * 导出只读源库、写本地文件,所以没有生产库确认闸门 —— 与导入(`useTableImportDialog`)
 * 的区别就在这里。
 */

import { useCallback, useMemo, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";

import type { AeroricDbConnectionConfig } from "../../types";
import { databaseApi } from "../../lib/databaseApi";
import {
  DBX_TABLE_LIKE_OBJECT_TYPES,
  isDbxTableObject,
  isDbxViewObject,
  isSqlDbxConnection,
  listAllDbxObjects,
} from "./databaseViewModel";

export interface DatabaseExportTarget {
  connectionId: string;
  database: string;
  schema: string | null;
  /** 从表节点右键进来时预先勾上的表;为空表示勾全部。 */
  preselectedTables: string[];
}

export interface DatabaseExportDialogDeps {
  /** 用于把 `target.connectionId` 解析回连接配置。 */
  dbxConnections: AeroricDbConnectionConfig[];
  /** 打开对话框时收起右键菜单。 */
  closeContextMenu: () => void;
  /** DatabaseView 的全局忙碌态 / 错误条,导出期间与其保持一致。 */
  setGlobalLoading: (loading: boolean) => void;
  setGlobalError: (error: string | null) => void;
}

export interface DatabaseExportDialogState {
  target: DatabaseExportTarget | null;
  connection: AeroricDbConnectionConfig | null;
  /** 源库里可导出的表 / 视图名。 */
  tables: string[];
  /** 经搜索词过滤后的列表,列表区渲染的就是它。 */
  filteredTables: string[];
  selection: Set<string>;
  search: string;
  includeStructure: boolean;
  includeData: boolean;
  includeObjects: boolean;
  dropTableIfExists: boolean;
  loading: boolean;
  error: string;
  /** 一张表都没勾、或三个内容开关全关时导出结果为空,所以禁止执行。 */
  canRun: boolean;
  open: (
    connection: AeroricDbConnectionConfig,
    database: string,
    schema?: string | null,
    preselectedTables?: string[],
  ) => Promise<void>;
  close: () => void;
  setSearch: (search: string) => void;
  setSelection: (updater: (current: Set<string>) => Set<string>) => void;
  toggleTable: (table: string) => void;
  setIncludeStructure: (include: boolean) => void;
  setIncludeData: (include: boolean) => void;
  setIncludeObjects: (include: boolean) => void;
  setDropTableIfExists: (drop: boolean) => void;
  submit: () => Promise<void>;
}

export function useDatabaseExportDialog({
  dbxConnections,
  closeContextMenu,
  setGlobalLoading,
  setGlobalError,
}: DatabaseExportDialogDeps): DatabaseExportDialogState {
  const [target, setTarget] = useState<DatabaseExportTarget | null>(null);
  const [tables, setTables] = useState<string[]>([]);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState("");
  const [includeStructure, setIncludeStructure] = useState(true);
  const [includeData, setIncludeData] = useState(true);
  const [includeObjects, setIncludeObjects] = useState(true);
  const [dropTableIfExists, setDropTableIfExists] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const connection = useMemo(
    () => dbxConnections.find((item) => item.id === target?.connectionId) ?? null,
    [dbxConnections, target?.connectionId],
  );
  const filteredTables = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return tables;
    return tables.filter((name) => name.toLowerCase().includes(query));
  }, [search, tables]);
  const canRun =
    Boolean(connection && target) &&
    selection.size > 0 &&
    (includeStructure || includeData || includeObjects);

  const close = useCallback(() => {
    setTarget(null);
    setTables([]);
    setSelection(new Set());
    setSearch("");
    setIncludeStructure(true);
    setIncludeData(true);
    setIncludeObjects(true);
    setDropTableIfExists(false);
    setError("");
  }, []);

  const open = useCallback(
    async (
      nextConnection: AeroricDbConnectionConfig,
      database: string,
      schema: string | null = null,
      preselectedTables: string[] = [],
    ) => {
      if (!isSqlDbxConnection(nextConnection)) return;
      closeContextMenu();
      setTarget({ connectionId: nextConnection.id, database, schema, preselectedTables });
      setTables([]);
      setSelection(new Set());
      setSearch("");
      setIncludeStructure(true);
      setIncludeData(true);
      setIncludeObjects(true);
      setDropTableIfExists(false);
      setLoading(true);
      setError("");
      try {
        await databaseApi.dbxConnect(nextConnection.id);
        const objects = await listAllDbxObjects(nextConnection.id, database, schema, {
          objectTypes: DBX_TABLE_LIKE_OBJECT_TYPES,
        });
        const tableNames = Array.from(
          new Set(
            objects
              .filter((object) => isDbxTableObject(object) || isDbxViewObject(object))
              .map((object) => object.name)
              .filter(Boolean),
          ),
        ).sort((left, right) =>
          left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }),
        );
        // 预选表可能已经不在库里(树是缓存的),过滤掉之后若一张不剩就退回全选。
        const preselected = preselectedTables.filter((table) => tableNames.includes(table));
        setTables(tableNames);
        setSelection(new Set(preselected.length > 0 ? preselected : tableNames));
      } catch (err) {
        setTables([]);
        setSelection(new Set());
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [closeContextMenu],
  );

  const toggleTable = useCallback((table: string) => {
    setSelection((current) => {
      const next = new Set(current);
      if (next.has(table)) next.delete(table);
      else next.add(table);
      return next;
    });
  }, []);

  const submit = useCallback(async () => {
    if (!connection || !target || !canRun) return;
    const safeName =
      (target.database || "database").replace(/[\\/:*?"<>|]+/g, "_").trim() || "database";
    const filePath = await saveDialog({
      defaultPath: `${safeName}.sql`,
      filters: [{ name: "SQL", extensions: ["sql"] }],
    });
    if (typeof filePath !== "string" || !filePath.trim()) return;

    // 全选时传 undefined:让后端走"整库导出"而不是逐表列举。
    const selectedTables =
      selection.size === tables.length ? undefined : tables.filter((table) => selection.has(table));
    setGlobalLoading(true);
    setLoading(true);
    setGlobalError(null);
    setError("");
    try {
      await databaseApi.dbxExportDatabase({
        exportId: `export:${Date.now()}:${Math.random().toString(36).slice(2)}`,
        connectionId: connection.id,
        database: target.database,
        schema: target.schema || target.database,
        filePath: filePath.trim(),
        selectedTables,
        includeStructure,
        includeData,
        includeObjects,
        dropTableIfExists,
        batchSize: 1000,
      });
      close();
    } catch (err) {
      setGlobalError(String(err));
      setError(String(err));
    } finally {
      setGlobalLoading(false);
      setLoading(false);
    }
  }, [
    canRun,
    close,
    connection,
    dropTableIfExists,
    includeData,
    includeObjects,
    includeStructure,
    selection,
    setGlobalError,
    setGlobalLoading,
    tables,
    target,
  ]);

  return {
    target,
    connection,
    tables,
    filteredTables,
    selection,
    search,
    includeStructure,
    includeData,
    includeObjects,
    dropTableIfExists,
    loading,
    error,
    canRun,
    open,
    close,
    setSearch,
    setSelection,
    toggleTable,
    setIncludeStructure,
    setIncludeData,
    setIncludeObjects,
    setDropTableIfExists,
    submit,
  };
}
