/**
 * 网格上两个右键菜单的动作执行器:表头菜单与单元格菜单。
 *
 * 从 `DatabaseView.tsx` 抽出:两者都只做「读一条菜单状态 → 关菜单 → 按 action 分支」,
 * 用到的东西要么在 `useDbxDataGrid` 里(预览、排序、过滤、每页条数),要么是 `DatabaseView`
 * 已经建好的几个 helper。所以这里接一个显式的 `Deps`,不自己持有任何状态。
 *
 * 分支顺序与原来逐字一致 —— 排序/过滤那几支都会重新拉一次数据,顺序换了行为就变了。
 */

import { useCallback } from "react";

import { databaseApi } from "../../lib/databaseApi";
import {
  cellPreviewText,
  dbxGridColumnSortable,
  dbxGridRowsToJson,
  dbxGridRowsToTsv,
} from "../../lib/databaseUtils";
import type {
  AeroricDbConnectionConfig,
  DataGridContextFilterConditionOptions,
  DataGridContextFilterMode,
  DataGridCopyInsertStatementOptions,
  DataGridCopyUpdateStatementOptions,
  DbQueryResult,
  DbxObjectInfo,
} from "../../types";
import {
  combineDbxGridWhereCondition,
  dbxFilterModeForCellAction,
  dbxOrderByForColumn,
  type DatabaseRow,
  type DbxGridCellContextMenuAction,
  type DbxGridCellContextMenuState,
  type DbxGridColumnFuzzyFilters,
  type DbxGridHeaderContextMenuAction,
  type DbxGridHeaderContextMenuState,
} from "./databaseGridState";
import type { DatabaseContextMenuState } from "./databaseViewModel";
import type { DbxDataGridController } from "./useDbxDataGrid";

export interface DbxGridContextMenuActionsDeps {
  grid: DbxDataGridController;
  contextMenu: DatabaseContextMenuState | null;
  setContextMenu: (menu: DatabaseContextMenuState | null) => void;
  queryResult: DbQueryResult | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxObject: DbxObjectInfo | null;
  activeDbxDatabase: string | null;
  copyNodeName: (name: string) => void;
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
  /** 单元格菜单里「按此值过滤」那一组的参数,拿不到列元数据就返回 null。 */
  buildDbxGridContextFilterOptions: (
    menu: DbxGridCellContextMenuState,
    mode: DataGridContextFilterMode,
  ) => DataGridContextFilterConditionOptions | null;
  /** 有选中行就复制选中的那些,否则只复制右键那一行。 */
  dbxGridContextRows: (menu: DbxGridCellContextMenuState) => DatabaseRow[];
  buildDbxGridCopyOptions: (
    rows: DatabaseRow[],
    excludePrimaryKeys?: boolean,
  ) => {
    insert: DataGridCopyInsertStatementOptions;
    update: DataGridCopyUpdateStatementOptions | null;
  } | null;
  onError: (message: string) => void;
}

export interface DbxGridContextMenuActions {
  runDbxGridHeaderContextMenuAction: (action: DbxGridHeaderContextMenuAction) => Promise<void>;
  runDbxGridCellContextMenuAction: (action: DbxGridCellContextMenuAction) => Promise<void>;
}

export function useDbxGridContextMenuActions(
  deps: DbxGridContextMenuActionsDeps,
): DbxGridContextMenuActions {
  const {
    grid,
    contextMenu,
    setContextMenu,
    queryResult,
    activeDbxConnection,
    activeDbxObject,
    activeDbxDatabase,
    copyNodeName,
    loadDbxObject,
    buildDbxGridContextFilterOptions,
    dbxGridContextRows,
    buildDbxGridCopyOptions,
    onError,
  } = deps;
  const {
    dbxGridWhereInput,
    setDbxGridWhereInput,
    dbxGridOrderByInput,
    setDbxGridOrderByInput,
    dbxGridPageSize,
    setDbxGridColumnFuzzyFilters,
    setDbxCellPreview,
    setDbxRowPreview,
    setDbxRowPreviewSearch,
    setDbxColumnPreview,
    setDbxColumnPreviewSearch,
  } = grid.state;
  const { visibleTableColumns } = grid.derived;

  /** 三支排序动作共用:算好 order by 写回输入框,再从第一页重新拉。 */
  const applySortAction = useCallback(
    async (
      action: "sortAscending" | "sortDescending" | "clearSort",
      column: string,
      columnIndex: number,
    ) => {
      if (!activeDbxConnection || !activeDbxObject || !queryResult) return;
      if (!dbxGridColumnSortable(queryResult, columnIndex)) return;
      const nextOrderBy =
        action === "sortAscending"
          ? dbxOrderByForColumn(column, "ASC")
          : action === "sortDescending"
            ? dbxOrderByForColumn(column, "DESC")
            : dbxOrderByForColumn(column, null);
      setDbxGridOrderByInput(nextOrderBy);
      await loadDbxObject(
        activeDbxObject,
        1,
        activeDbxConnection,
        activeDbxDatabase,
        dbxGridWhereInput,
        nextOrderBy,
      );
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      dbxGridWhereInput,
      loadDbxObject,
      queryResult,
      setDbxGridOrderByInput,
    ],
  );

  /** 写剪贴板失败只弹一条错误,和原来一致。 */
  const writeClipboard = useCallback(
    async (text: string) => {
      try {
        await navigator.clipboard?.writeText(text);
      } catch (err) {
        onError(String(err));
      }
    },
    [onError],
  );

  const runDbxGridHeaderContextMenuAction = useCallback(
    async (action: DbxGridHeaderContextMenuAction) => {
      const menu: DbxGridHeaderContextMenuState | null =
        contextMenu?.kind === "dbx-grid-header" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      if (action === "copyColumnName") {
        copyNodeName(menu.column);
        return;
      }
      if (action === "previewColumn") {
        setDbxColumnPreviewSearch("");
        setDbxColumnPreview({ column: menu.column, columnIndex: menu.columnIndex });
        return;
      }
      if (action === "sortAscending" || action === "sortDescending" || action === "clearSort") {
        await applySortAction(action, menu.column, menu.columnIndex);
      }
    },
    [
      applySortAction,
      contextMenu,
      copyNodeName,
      setContextMenu,
      setDbxColumnPreview,
      setDbxColumnPreviewSearch,
    ],
  );

  const runDbxGridCellContextMenuAction = useCallback(
    async (action: DbxGridCellContextMenuAction) => {
      const menu = contextMenu?.kind === "dbx-grid-cell" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      if (action === "copyValue") {
        await writeClipboard(cellPreviewText(menu.value).text);
        return;
      }
      if (action === "copyColumnName") {
        copyNodeName(menu.column);
        return;
      }
      if (action === "previewValue") {
        setDbxCellPreview({ column: menu.column, value: menu.value });
        return;
      }
      if (action === "previewRow") {
        const row = queryResult?.rows[menu.rowIndex];
        if (row) {
          setDbxRowPreviewSearch("");
          setDbxRowPreview({ rowIndex: menu.rowIndex, row });
        }
        return;
      }
      if (action === "previewColumn") {
        setDbxColumnPreviewSearch("");
        setDbxColumnPreview({ column: menu.column, columnIndex: menu.columnIndex });
        return;
      }
      if (action === "sortAscending" || action === "sortDescending" || action === "clearSort") {
        await applySortAction(action, menu.column, menu.columnIndex);
        return;
      }
      if (action === "clearFilter") {
        setDbxGridWhereInput("");
        setDbxGridColumnFuzzyFilters({});
        if (activeDbxConnection && activeDbxObject) {
          await loadDbxObject(
            activeDbxObject,
            1,
            activeDbxConnection,
            activeDbxDatabase,
            "",
            dbxGridOrderByInput,
            dbxGridPageSize,
            {},
          );
        }
        return;
      }
      const filterMode = dbxFilterModeForCellAction(action);
      if (filterMode) {
        const options = buildDbxGridContextFilterOptions(menu, filterMode);
        if (!options || !activeDbxConnection || !activeDbxObject) return;
        try {
          const condition = await databaseApi.dbxBuildDataGridContextFilterCondition(options);
          if (!condition) return;
          const nextWhere = combineDbxGridWhereCondition(dbxGridWhereInput, condition);
          setDbxGridWhereInput(nextWhere);
          await loadDbxObject(
            activeDbxObject,
            1,
            activeDbxConnection,
            activeDbxDatabase,
            nextWhere,
            dbxGridOrderByInput,
          );
        } catch (err) {
          onError(String(err));
        }
        return;
      }
      if (action === "copyAllTsv") {
        if (!queryResult || visibleTableColumns.length === 0) return;
        await writeClipboard(dbxGridRowsToTsv(visibleTableColumns, queryResult.rows));
        return;
      }
      if (action === "copyRowJson") {
        const rows = dbxGridContextRows(menu);
        if (rows.length === 0) return;
        await writeClipboard(dbxGridRowsToJson(visibleTableColumns, rows));
        return;
      }
      if (action === "copyRowInsert" || action === "copyRowInsertWithoutPrimaryKeys") {
        const rows = dbxGridContextRows(menu);
        const options = buildDbxGridCopyOptions(rows, action === "copyRowInsertWithoutPrimaryKeys");
        if (!options) return;
        try {
          const statement = await databaseApi.dbxBuildDataGridCopyInsertStatement(options.insert);
          if (statement) await navigator.clipboard?.writeText(statement);
        } catch (err) {
          onError(String(err));
        }
        return;
      }
      if (action === "copyRowUpdate") {
        const rows = dbxGridContextRows(menu);
        const options = buildDbxGridCopyOptions(rows);
        if (!options?.update) return;
        try {
          const statements = await databaseApi.dbxBuildDataGridCopyUpdateStatements(options.update);
          if (statements.length > 0) await navigator.clipboard?.writeText(statements.join("\n"));
        } catch (err) {
          onError(String(err));
        }
      }
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      applySortAction,
      buildDbxGridContextFilterOptions,
      buildDbxGridCopyOptions,
      contextMenu,
      copyNodeName,
      dbxGridContextRows,
      dbxGridOrderByInput,
      dbxGridPageSize,
      dbxGridWhereInput,
      loadDbxObject,
      onError,
      queryResult,
      setContextMenu,
      setDbxCellPreview,
      setDbxColumnPreview,
      setDbxColumnPreviewSearch,
      setDbxGridColumnFuzzyFilters,
      setDbxGridWhereInput,
      setDbxRowPreview,
      setDbxRowPreviewSearch,
      visibleTableColumns,
      writeClipboard,
    ],
  );

  return { runDbxGridHeaderContextMenuAction, runDbxGridCellContextMenuAction };
}
