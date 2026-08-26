/**
 * 表格底栏那三个显示值(总行数文案、页数、底栏里回显的那句 SQL),外加右键单元格时
 * 「这一下影响几行」的计数。四支都是纯派生,没有任何副作用。
 *
 * 从 `DatabaseView.tsx` 抽出:原文里这四支就是紧挨着的一段,输入也几乎重合 ——
 * 都从 `queryResult` 出发,再看网格自己的筛选 / 排序 / 选中状态。
 * `dbxGridCellContextRowCount` 严格说不属于底栏(它给的是单元格右键菜单上的行数),
 * 但它和另外三支同源同段,分开反而要把 `queryResult` 与 `dbxGrid.state` 再传一遍,所以一并收进来。
 *
 * 逐字保留的几处:
 * - `dbxGridCellContextRowCount`:只有右键菜单确实是 `dbx-grid-cell` 时才算;
 *   右键落在已选中的那一行、且确有选中时,算的是「选中行里下标仍落在当前页内」的那些,
 *   否则只看右键那一行存不存在(1 或 0)。
 * - `totalPages` 只在 `totalRows` 大于 0 时才有值,否则是 `null`(底栏据此决定显不显示页数)。
 * - `tableFooterRowCountText` 在没有 `totalRows` 时退回 `rows.length`。
 * - `tableFooterSqlText`:只有 dbx 连接且确有当前对象时才拼那句 `SELECT`,其余情况一律回显
 *   编辑器里那段 `sql.trim()`。拼的时候用的是「生效后的」`dbxGridEffectiveWhereInput`
 *   而不是输入框里的原文,`OFFSET` 只在大于 0 时才加,末尾带分号。
 */

import { useMemo } from "react";

import { useI18n } from "../../i18n";
import { quoteSqlName } from "../../lib/databaseUtils";
import type { AeroricDbConnectionConfig, DbQueryResult, DbxObjectInfo } from "../../types";
import type { DatabaseContextMenuState } from "./databaseViewModel";

export interface TableFooterDerivedDeps {
  queryResult: DbQueryResult | null;
  contextMenu: DatabaseContextMenuState;
  dbxGridSelectedRows: Set<number>;
  dbxGridOrderByInput: string;
  dbxGridEffectiveWhereInput: string;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxObject: DbxObjectInfo | null;
  page: number;
  sql: string;
}

export interface TableFooterDerived {
  dbxGridCellContextRowCount: number;
  totalPages: number | null;
  tableFooterRowCountText: string;
  tableFooterSqlText: string;
}

export function useTableFooterDerived(deps: TableFooterDerivedDeps): TableFooterDerived {
  const { t } = useI18n();
  const {
    queryResult,
    contextMenu,
    dbxGridSelectedRows,
    dbxGridOrderByInput,
    dbxGridEffectiveWhereInput,
    activeDbxConnection,
    activeDbxObject,
    page,
    sql,
  } = deps;

  const dbxGridCellContextRowCount = useMemo(() => {
    if (!queryResult || contextMenu?.kind !== "dbx-grid-cell") return 0;
    if (dbxGridSelectedRows.has(contextMenu.rowIndex) && dbxGridSelectedRows.size > 0) {
      return Array.from(dbxGridSelectedRows).filter(
        (rowIndex) => rowIndex >= 0 && rowIndex < queryResult.rows.length,
      ).length;
    }
    return queryResult.rows[contextMenu.rowIndex] ? 1 : 0;
  }, [contextMenu, dbxGridSelectedRows, queryResult]);
  const totalPages =
    queryResult?.totalRows && queryResult.totalRows > 0
      ? Math.max(1, Math.ceil(queryResult.totalRows / queryResult.pageSize))
      : null;
  const tableFooterRowCountText = useMemo(() => {
    if (!queryResult) return "";
    const totalRows = queryResult.totalRows ?? queryResult.rows.length;
    return t("database.totalRows", { count: totalRows });
  }, [queryResult, t]);
  const tableFooterSqlText = useMemo(() => {
    if (!queryResult) return sql.trim();
    if (activeDbxConnection && activeDbxObject) {
      const tableName = activeDbxObject.schema
        ? `${quoteSqlName(activeDbxObject.schema)}.${quoteSqlName(activeDbxObject.name)}`
        : quoteSqlName(activeDbxObject.name);
      const clauses = [`SELECT * FROM ${tableName}`];
      const whereInput = dbxGridEffectiveWhereInput.trim();
      const orderByInput = dbxGridOrderByInput.trim();
      if (whereInput) clauses.push(`WHERE ${whereInput}`);
      if (orderByInput) clauses.push(`ORDER BY ${orderByInput}`);
      clauses.push(`LIMIT ${queryResult.pageSize}`);
      const offset = Math.max(0, (page - 1) * queryResult.pageSize);
      if (offset > 0) clauses.push(`OFFSET ${offset}`);
      return `${clauses.join(" ")};`;
    }
    return sql.trim();
  }, [
    activeDbxConnection,
    activeDbxObject,
    dbxGridOrderByInput,
    dbxGridEffectiveWhereInput,
    page,
    queryResult,
    sql,
  ]);

  return {
    dbxGridCellContextRowCount,
    totalPages,
    tableFooterRowCountText,
    tableFooterSqlText,
  };
}
