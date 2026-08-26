/**
 * dbx 网格「换一种看法」的五支动作:重新拉一遍、清掉筛选排序、点表头排序、列上模糊筛选、换每页条数。
 *
 * 从 `DatabaseView.tsx` 抽出:这五支原本在文件里就是连续的一段,共同点是都不改数据,只是
 * 「先把新的展示参数写回 grid state,再用这套参数重新调一次 `loadDbxObject`」——
 * 真正拉数据的 `loadDbxObject` 留在 `DatabaseView`,从 `deps` 传进来。
 *
 * 参数顺序与原来逐字一致:`loadDbxObject` 的第 5、6 个位置参数是 where / orderBy,
 * `resetActiveDbxGrid` 特意传空串而不是当前值,才能把筛选与排序一起清掉。
 *
 * 与原文唯一的差别:原先直接闭包捕获的 `setError` 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它是稳定引用,于是补进了那个依赖数组 ——
 * 它的身份本来就不变,行为不受影响。
 */

import { useCallback } from "react";

import { databaseApi } from "../../lib/databaseApi";
import { dbxGridColumnSortable } from "../../lib/databaseUtils";
import type {
  AeroricDbConnectionConfig,
  DbQueryResult,
  DbxColumnInfo,
  DbxObjectInfo,
} from "../../types";
import { nextDbxOrderByForColumn, type DbxGridColumnFuzzyFilters } from "./databaseGridState";
import type { DbxDataGridController } from "./useDbxDataGrid";

export interface DbxGridPresentationDeps {
  grid: DbxDataGridController;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxObject: DbxObjectInfo | null;
  activeDbxGridColumns: DbxColumnInfo[];
  queryResult: DbQueryResult | null;
  setError: (error: string | null) => void;
  /** 留在 `DatabaseView` 的那支加载器;返回值这五支都不看。 */
  loadDbxObject: (
    object: DbxObjectInfo,
    nextPage: number,
    connection?: AeroricDbConnectionConfig | null,
    database?: string | null,
    whereInput?: string | null,
    orderBy?: string | null,
    pageSize?: number,
    columnFuzzyFiltersOverride?: DbxGridColumnFuzzyFilters,
  ) => Promise<unknown>;
}

export interface DbxGridPresentation {
  reloadActiveDbxGrid: (whereInput?: string, orderBy?: string) => Promise<void>;
  resetActiveDbxGrid: () => Promise<void>;
  toggleDbxGridColumnSort: (column: string) => Promise<void>;
  applyDbxGridColumnFuzzyFilter: (column: string, value: string) => Promise<void>;
  changeDbxGridPageSize: (nextPageSize: number) => Promise<void>;
}

export function useDbxGridPresentation(deps: DbxGridPresentationDeps): DbxGridPresentation {
  const {
    grid,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    activeDbxGridColumns,
    queryResult,
    setError,
    loadDbxObject,
  } = deps;
  const {
    dbxGridWhereInput,
    dbxGridOrderByInput,
    setDbxGridOrderByInput,
    dbxGridPageSize,
    setDbxGridPageSize,
    dbxGridColumnFuzzyFilters,
    setDbxGridColumnFuzzyFilters,
  } = grid.state;
  const { resetGridPresentation } = grid.actions;

  const reloadActiveDbxGrid = useCallback(
    async (whereInput = dbxGridWhereInput, orderBy = dbxGridOrderByInput) => {
      if (!activeDbxConnection || !activeDbxObject) return;
      await loadDbxObject(
        activeDbxObject,
        1,
        activeDbxConnection,
        activeDbxDatabase,
        whereInput,
        orderBy,
      );
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      dbxGridOrderByInput,
      dbxGridWhereInput,
      loadDbxObject,
    ],
  );

  const resetActiveDbxGrid = useCallback(async () => {
    resetGridPresentation();
    if (!activeDbxConnection || !activeDbxObject) return;
    await loadDbxObject(
      activeDbxObject,
      1,
      activeDbxConnection,
      activeDbxDatabase,
      "",
      "",
      dbxGridPageSize,
      {},
    );
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    dbxGridPageSize,
    loadDbxObject,
    resetGridPresentation,
  ]);

  const toggleDbxGridColumnSort = useCallback(
    async (column: string) => {
      if (!activeDbxConnection || !activeDbxObject || !queryResult) return;
      const columnIndex = queryResult.columns.indexOf(column);
      if (!dbxGridColumnSortable(queryResult, columnIndex)) return;
      const nextOrderBy = nextDbxOrderByForColumn(dbxGridOrderByInput, column);
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
      dbxGridOrderByInput,
      dbxGridWhereInput,
      loadDbxObject,
      queryResult,
      setDbxGridOrderByInput,
    ],
  );

  const applyDbxGridColumnFuzzyFilter = useCallback(
    async (column: string, value: string) => {
      if (!activeDbxConnection || !activeDbxObject || !queryResult) return;
      const normalizedValue = value.trim();
      const nextFilters: DbxGridColumnFuzzyFilters = { ...dbxGridColumnFuzzyFilters };
      if (!normalizedValue) {
        delete nextFilters[column];
      } else {
        try {
          const columnInfo =
            activeDbxGridColumns.find((item) => item.name.toLowerCase() === column.toLowerCase()) ??
            null;
          const condition = await databaseApi.dbxBuildDataGridContextFilterCondition({
            databaseType: activeDbxConnection.dbType,
            columnName: column,
            mode: "like",
            value: normalizedValue,
            columnInfo,
          });
          if (!condition) return;
          nextFilters[column] = { value: normalizedValue, condition };
        } catch (err) {
          setError(String(err));
          return;
        }
      }
      setDbxGridColumnFuzzyFilters(nextFilters);
      await loadDbxObject(
        activeDbxObject,
        1,
        activeDbxConnection,
        activeDbxDatabase,
        dbxGridWhereInput,
        dbxGridOrderByInput,
        dbxGridPageSize,
        nextFilters,
      );
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxGridColumns,
      activeDbxObject,
      dbxGridColumnFuzzyFilters,
      dbxGridOrderByInput,
      dbxGridPageSize,
      dbxGridWhereInput,
      loadDbxObject,
      queryResult,
      setDbxGridColumnFuzzyFilters,
      setError,
    ],
  );

  const changeDbxGridPageSize = useCallback(
    async (nextPageSize: number) => {
      setDbxGridPageSize(nextPageSize);
      if (!activeDbxConnection || !activeDbxObject || !queryResult) return;
      await loadDbxObject(
        activeDbxObject,
        1,
        activeDbxConnection,
        activeDbxDatabase,
        dbxGridWhereInput,
        dbxGridOrderByInput,
        nextPageSize,
      );
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      dbxGridOrderByInput,
      dbxGridWhereInput,
      loadDbxObject,
      queryResult,
      setDbxGridPageSize,
    ],
  );

  return {
    reloadActiveDbxGrid,
    resetActiveDbxGrid,
    toggleDbxGridColumnSort,
    applyDbxGridColumnFuzzyFilter,
    changeDbxGridPageSize,
  };
}
