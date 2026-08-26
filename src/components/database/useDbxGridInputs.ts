/**
 * 喂给 `useDbxDataGrid` 的那几个派生输入:表头、原始行、要不要显示 rowid 列、当前表的列元数据,
 * 外加一支「当前这条连接支不支持跑 SQL」。
 *
 * 从 `DatabaseView.tsx` 抽出:原文里这五支就是紧挨着的一段,排在 `useDbxDataGrid` 之前,
 * 共同点是都只从 `queryResult` / `sqlResult` / 当前对象这几份状态里挑出网格要的那部分。
 * `activeSqlCapable` 不进网格(它管的是工具栏上几个按钮的可用性),但它同段同源,一并收进来。
 *
 * `useMemo` 与普通 `const` 的分布逐字保留:返回数组的三支包了 memo —— `useDbxDataGrid` 内部
 * 拿它们当依赖,引用一变就会重算;返回布尔的两支没包。
 *
 * 逐字保留的几处:
 * - `rawTableRows` / `tableColumns` 的回退顺序都是 `queryResult` → `sqlResult` → 空数组,
 *   顺序不能反(执行 SQL 之后两份结果可能同时在)。
 * - `showRowIdColumn` 三个条件同时成立才为真:有 `queryResult`、不是 dbx 连接、且结果自带 rowid。
 * - `activeDbxGridColumns` 取不到时回退到 `EMPTY_DBX_COLUMNS` 常量而不是新建 `[]`,保持引用稳定。
 * - `activeSqlCapable`:legacy 有 endpoint 就算,dbx 侧还要额外满足 `dbxHasSqlObjectBrowser`。
 */

import { useMemo } from "react";

import type {
  AeroricDbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DbQueryResult,
  DbxColumnInfo,
  DbxObjectInfo,
} from "../../types";
import { EMPTY_DBX_COLUMNS, dbxObjectKey } from "./databaseViewModel";

export interface DbxGridInputsDeps {
  queryResult: DbQueryResult | null;
  sqlResult: DbExecuteResult | null;
  activeEndpoint: DbEndpoint | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxObject: DbxObjectInfo | null;
  dbxHasSqlObjectBrowser: boolean;
  dbxColumnsByTable: Record<string, DbxColumnInfo[]>;
}

export interface DbxGridInputs {
  activeSqlCapable: boolean;
  rawTableRows: DbQueryResult["rows"];
  tableColumns: DbQueryResult["columns"];
  showRowIdColumn: boolean;
  activeDbxGridColumns: DbxColumnInfo[];
}

export function useDbxGridInputs(deps: DbxGridInputsDeps): DbxGridInputs {
  const {
    queryResult,
    sqlResult,
    activeEndpoint,
    activeDbxConnection,
    activeDbxObject,
    dbxHasSqlObjectBrowser,
    dbxColumnsByTable,
  } = deps;

  const activeSqlCapable = Boolean(
    activeEndpoint || (activeDbxConnection && dbxHasSqlObjectBrowser),
  );
  const rawTableRows = useMemo(
    () => queryResult?.rows ?? sqlResult?.rows ?? [],
    [queryResult, sqlResult],
  );
  const tableColumns = useMemo(
    () => queryResult?.columns ?? sqlResult?.columns ?? [],
    [queryResult, sqlResult],
  );
  const showRowIdColumn = Boolean(queryResult && !activeDbxConnection && queryResult.hasRowId);
  const activeDbxGridColumns = useMemo(() => {
    if (!activeDbxObject) return EMPTY_DBX_COLUMNS;
    return dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? EMPTY_DBX_COLUMNS;
  }, [activeDbxObject, dbxColumnsByTable]);

  return {
    activeSqlCapable,
    rawTableRows,
    tableColumns,
    showRowIdColumn,
    activeDbxGridColumns,
  };
}
