/**
 * dbx 数据网格的编辑层:改单元格、存暂存改动、插行、删行、复制选中行,以及网格上的复制快捷键。
 *
 * 从 `DatabaseView.tsx` 抽出:这 11 支原本在文件里就是连续的一段,彼此串成一条链 ——
 * 四个 `build*Options` 把「连接 + 表元数据 + 当前页行」打包成后端要的入参,
 * `updateCell` / `savePendingGridChanges` / `deleteDbxRowsByIndexes` 又都走同一套
 * 「先 execute:false 拿预览 → 记进 SQL 预览面板 → confirm → 再 execute:true」的两段式,
 * 所以合成一个 hook,而不是按动作拆成几个。
 *
 * 每支的分支顺序、i18n key 与预览/回滚文案的拼接都与原来逐字一致 —— 尤其 dbx 与 legacy 两条路
 * 是「先看 dbx 三件套齐不齐,齐就在 dbx 分支里 return」,顺序换了 legacy 连接会被误判。
 *
 * 与原文唯一的差别:原先直接闭包捕获的 `setError` 现在从 `deps` 进来,
 * `react-hooks/exhaustive-deps` 不再认得它是稳定引用,于是补进了那 4 个依赖数组 ——
 * 它的身份本来就不变,行为不受影响。
 */

import { useCallback, type KeyboardEvent as ReactKeyboardEvent } from "react";

import { useI18n } from "../../i18n";
import { confirm } from "../../lib/appDialog";
import { databaseApi } from "../../lib/databaseApi";
import {
  dbxGridRowsToTsv,
  isTextEditingShortcutTarget,
  rowKeyFor,
  textToCellValue,
  valueToText,
} from "../../lib/databaseUtils";
import type {
  AeroricDbConnectionConfig,
  DataGridContextFilterConditionOptions,
  DataGridContextFilterMode,
  DataGridCopyInsertStatementOptions,
  DataGridCopyUpdateStatementOptions,
  DataGridSaveStatementOptions,
  DbConnectionConfig,
  DbEndpoint,
  DbObject,
  DbQueryResult,
  DbxColumnInfo,
  DbxObjectInfo,
} from "../../types";
import {
  dbxGridContextRowIndexes,
  dbxPendingCellEditsToDirtyRows,
  type DatabaseRow,
  type DbxGridCellContextMenuState,
  type DbxGridColumnFuzzyFilters,
} from "./databaseGridState";
import { dbxObjectKey, isDbxTableObject } from "./databaseViewModel";
import type { DbxDataGridController } from "./useDbxDataGrid";
import type { DbxSqlPreviewDialogState } from "./useDbxSqlPreviewDialog";

export interface DbxGridEditingDeps {
  grid: DbxDataGridController;
  /** legacy 那条路只用到 `id` 与 `readOnly`。 */
  activeConnection: DbConnectionConfig | null;
  activeEndpoint: DbEndpoint | null;
  /** 主键与列元数据都从这里取;它为空时所有写操作都直接返回。 */
  activeObject: DbObject | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeDbxDatabase: string | null;
  activeDbxObject: DbxObjectInfo | null;
  queryResult: DbQueryResult | null;
  dbxColumnsByTable: Record<string, DbxColumnInfo[]>;
  /** legacy 插行时按列名逐列取值,dbx 那条路不用它。 */
  tableColumns: string[];
  page: number;
  projectRoot: string | undefined;
  setError: (error: string | null) => void;
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
  loadTable: (object: DbObject, nextPage: number) => Promise<void>;
  /** 两段式提交里把预览语句记进 SQL 预览面板,只用到 `record`。 */
  dbxSqlPreview: Pick<DbxSqlPreviewDialogState, "record">;
}

export interface DbxGridEditing {
  /** 单元格右键菜单那几支要自己拼后端入参,所以这三个 builder 也一并返回。 */
  dbxGridContextRows: (menu: DbxGridCellContextMenuState) => DatabaseRow[];
  buildDbxGridCopyOptions: (
    rows: DatabaseRow[],
    excludePrimaryKeys?: boolean,
  ) => {
    insert: DataGridCopyInsertStatementOptions;
    update: DataGridCopyUpdateStatementOptions | null;
  } | null;
  buildDbxGridContextFilterOptions: (
    menu: DbxGridCellContextMenuState,
    mode: DataGridContextFilterMode,
  ) => DataGridContextFilterConditionOptions | null;
  updateCell: (row: DatabaseRow, column: string, value: string, original: string) => Promise<void>;
  savePendingGridChanges: () => Promise<void>;
  insertRow: () => void;
  deleteDbxRowsByIndexes: (
    deletedRows: number[],
    confirmMessage: string,
    title: string,
  ) => Promise<void>;
  deleteSelectedDbxRows: () => Promise<void>;
  copySelectedDbxRows: () => Promise<void>;
  handleDbxGridKeyDown: (event: ReactKeyboardEvent<HTMLDivElement>) => void;
}

export function useDbxGridEditing(deps: DbxGridEditingDeps): DbxGridEditing {
  const { t } = useI18n();
  const {
    grid,
    activeConnection,
    activeEndpoint,
    activeObject,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    queryResult,
    dbxColumnsByTable,
    tableColumns,
    page,
    projectRoot,
    setError,
    loadDbxObject,
    loadTable,
    dbxSqlPreview,
  } = deps;
  const {
    dbxGridWhereInput,
    dbxGridOrderByInput,
    dbxGridSelectedRows,
    setDbxGridSelectedRows,
    dbxSelectedCell,
    dbxPendingCellEdits,
    setDbxPendingCellEdits,
    dbxNewRows,
    setDbxNewRows,
  } = grid.state;
  const { visibleTableColumns } = grid.derived;
  const { appendDbxNewRow } = grid.actions;

  const buildDbxGridSaveOptions = useCallback(
    (
      overrides: Pick<DataGridSaveStatementOptions, "dirtyRows" | "deletedRows" | "newRows">,
    ): DataGridSaveStatementOptions | null => {
      if (!activeDbxConnection || !activeDbxObject || !queryResult || !activeObject) return null;
      const columns = queryResult.columns;
      const metadataColumns = dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? [];
      return {
        databaseType: activeDbxConnection.dbType,
        tableMeta: {
          schema: activeDbxObject.schema ?? null,
          tableName: activeDbxObject.name,
          primaryKeys: activeObject.primaryKeys,
          columns: metadataColumns,
        },
        columns,
        sourceColumns: columns,
        rows: queryResult.rows.map((row) => row.values),
        dirtyRows: overrides.dirtyRows ?? [],
        deletedRows: overrides.deletedRows ?? [],
        newRows: overrides.newRows ?? [],
      };
    },
    [activeDbxConnection, activeDbxObject, activeObject, dbxColumnsByTable, queryResult],
  );

  const dbxGridContextRows = useCallback(
    (menu: DbxGridCellContextMenuState): DatabaseRow[] => {
      if (!queryResult) return [];
      return dbxGridContextRowIndexes(dbxGridSelectedRows, menu.rowIndex)
        .map((rowIndex) => queryResult.rows[rowIndex])
        .filter((row): row is DatabaseRow => Boolean(row));
    },
    [dbxGridSelectedRows, queryResult],
  );

  const buildDbxGridCopyOptions = useCallback(
    (
      rows: DatabaseRow[],
      excludePrimaryKeys = false,
    ): {
      insert: DataGridCopyInsertStatementOptions;
      update: DataGridCopyUpdateStatementOptions | null;
    } | null => {
      if (
        !activeDbxConnection ||
        !activeDbxObject ||
        !queryResult ||
        visibleTableColumns.length === 0 ||
        rows.length === 0
      )
        return null;
      const columns = visibleTableColumns.map(({ column }) => column);
      const rowValues = rows.map((row) =>
        visibleTableColumns.map(({ index }) => row.values[index] ?? null),
      );
      const metadataColumns = dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? [];
      const primaryKeys = activeObject?.primaryKeys ?? queryResult.primaryKeys ?? [];
      const tableMeta = {
        schema: activeDbxObject.schema ?? null,
        tableName: activeDbxObject.name,
        primaryKeys,
        columns: metadataColumns,
      };
      return {
        insert: {
          databaseType: activeDbxConnection.dbType,
          tableMeta,
          columns,
          sourceColumns: columns,
          rows: rowValues,
          excludePrimaryKeys,
        },
        update:
          primaryKeys.length > 0
            ? {
                databaseType: activeDbxConnection.dbType,
                tableMeta,
                columns,
                sourceColumns: columns,
                rows: rowValues,
              }
            : null,
      };
    },
    [
      activeDbxConnection,
      activeDbxObject,
      activeObject?.primaryKeys,
      dbxColumnsByTable,
      queryResult,
      visibleTableColumns,
    ],
  );

  const buildDbxGridContextFilterOptions = useCallback(
    (
      menu: DbxGridCellContextMenuState,
      mode: DataGridContextFilterMode,
    ): DataGridContextFilterConditionOptions | null => {
      if (!activeDbxConnection || !activeDbxObject || !queryResult) return null;
      const metadataColumns = dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? [];
      const columnInfo =
        metadataColumns.find((column) => column.name.toLowerCase() === menu.column.toLowerCase()) ??
        null;
      return {
        databaseType: activeDbxConnection.dbType,
        columnName: menu.column,
        mode,
        value: menu.value ?? null,
        columnInfo,
      };
    },
    [activeDbxConnection, activeDbxObject, dbxColumnsByTable, queryResult],
  );

  const updateCell = useCallback(
    async (row: DatabaseRow, column: string, value: string, original: string) => {
      if (value === original) return;
      if (activeDbxConnection && activeDbxObject && queryResult) {
        if (!activeObject || activeDbxConnection.readOnly || !queryResult.editable) return;
        const rowIndex = queryResult.rows.indexOf(row);
        const columnIndex = queryResult.columns.indexOf(column);
        if (rowIndex < 0 || columnIndex < 0) return;
        const options = buildDbxGridSaveOptions({
          dirtyRows: [[rowIndex, [[columnIndex, textToCellValue(value)]]]],
        });
        if (!options) return;
        setError(null);
        try {
          const preview = await databaseApi.dbxUpdateCell({
            connectionId: activeDbxConnection.id,
            database: activeDbxDatabase,
            schema: activeDbxObject.schema ?? null,
            options,
            execute: false,
          });
          if (preview.validationError) {
            setError(preview.validationError);
            return;
          }
          if (preview.statements.length === 0) return;
          dbxSqlPreview.record(preview, t("database.confirmUpdateCell", { column }));
          const rollback = preview.rollbackStatements.length
            ? `\n\n${t("database.gridRollbackSql")}\n${preview.rollbackStatements.join("\n")}`
            : "";
          const ok = await confirm(
            `${t("database.confirmUpdateCell", { column })}\n\n${preview.statements.join("\n")}${rollback}`,
            {
              title: t("database.updateCell"),
              kind: "warning",
              okLabel: t("database.updateCell"),
              cancelLabel: t("common.cancel"),
            },
          );
          if (!ok) return;
          const executed = await databaseApi.dbxUpdateCell({
            connectionId: activeDbxConnection.id,
            database: activeDbxDatabase,
            schema: activeDbxObject.schema ?? null,
            options,
            execute: true,
          });
          if (executed.validationError) {
            setError(executed.validationError);
            return;
          }
          await loadDbxObject(
            activeDbxObject,
            page,
            activeDbxConnection,
            activeDbxDatabase,
            dbxGridWhereInput,
            dbxGridOrderByInput,
          );
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (!activeEndpoint || !activeObject || activeConnection?.readOnly) return;
      setError(null);
      try {
        await databaseApi.updateCell({
          endpoint: activeEndpoint,
          table: activeObject.name,
          rowKey: rowKeyFor(row),
          column,
          value: textToCellValue(value),
          readOnly: activeConnection?.readOnly ?? false,
          connectionId: activeConnection?.id,
          projectRoot,
        });
        await loadTable(activeObject, page);
      } catch (err) {
        setError(String(err));
      }
    },
    [
      activeConnection?.id,
      activeConnection?.readOnly,
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      activeEndpoint,
      activeObject,
      buildDbxGridSaveOptions,
      dbxGridOrderByInput,
      dbxGridWhereInput,
      dbxSqlPreview,
      loadDbxObject,
      loadTable,
      page,
      projectRoot,
      queryResult,
      setError,
      t,
    ],
  );

  const savePendingGridChanges = useCallback(async () => {
    const dirtyRows = dbxPendingCellEditsToDirtyRows(dbxPendingCellEdits, textToCellValue);
    const newRows = dbxNewRows.map((row) =>
      row.map((value) => (typeof value === "string" ? textToCellValue(value) : (value ?? null))),
    );
    if (dirtyRows.length === 0 && newRows.length === 0) return;

    if (activeDbxConnection && activeDbxObject && queryResult) {
      if (
        !activeObject ||
        activeDbxConnection.readOnly ||
        (dirtyRows.length > 0 && !queryResult.editable)
      )
        return;
      const options = buildDbxGridSaveOptions({ dirtyRows, newRows });
      if (!options) return;
      const saveRequest =
        dirtyRows.length > 0 ? databaseApi.dbxUpdateCell : databaseApi.dbxInsertRow;
      setError(null);
      try {
        const preview = await saveRequest({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: activeDbxObject.schema ?? null,
          options,
          execute: false,
        });
        if (preview.validationError) {
          setError(preview.validationError);
          return;
        }
        if (preview.statements.length === 0) return;
        dbxSqlPreview.record(preview, t("database.saveGridChanges"));
        const rollback = preview.rollbackStatements.length
          ? `\n\n${t("database.gridRollbackSql")}\n${preview.rollbackStatements.join("\n")}`
          : "";
        const ok = await confirm(
          `${t("database.confirmSaveGridChanges")}\n\n${preview.statements.join("\n")}${rollback}`,
          {
            title: t("database.saveGridChanges"),
            kind: "warning",
            okLabel: t("common.save"),
            cancelLabel: t("common.cancel"),
          },
        );
        if (!ok) return;
        const executed = await saveRequest({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: activeDbxObject.schema ?? null,
          options,
          execute: true,
        });
        if (executed.validationError) {
          setError(executed.validationError);
          return;
        }
        setDbxPendingCellEdits({});
        setDbxNewRows([]);
        await loadDbxObject(
          activeDbxObject,
          page,
          activeDbxConnection,
          activeDbxDatabase,
          dbxGridWhereInput,
          dbxGridOrderByInput,
        );
      } catch (err) {
        setError(String(err));
      }
      return;
    }

    if (!activeEndpoint || !activeObject || activeConnection?.readOnly || newRows.length === 0)
      return;
    setError(null);
    try {
      for (const row of dbxNewRows) {
        const values = tableColumns.flatMap((column, index) => {
          const value = row[index];
          if (value === undefined) return [];
          return [
            {
              column,
              value:
                value === null
                  ? null
                  : textToCellValue(typeof value === "string" ? value : String(value)),
            },
          ];
        });
        if (values.length === 0) continue;
        await databaseApi.insertRow({
          endpoint: activeEndpoint,
          table: activeObject.name,
          values,
          readOnly: activeConnection?.readOnly ?? false,
          connectionId: activeConnection?.id,
          projectRoot,
        });
      }
      setDbxNewRows([]);
      await loadTable(activeObject, page);
    } catch (err) {
      setError(String(err));
    }
  }, [
    activeConnection?.id,
    activeConnection?.readOnly,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    activeEndpoint,
    activeObject,
    buildDbxGridSaveOptions,
    dbxGridOrderByInput,
    dbxGridWhereInput,
    dbxNewRows,
    dbxPendingCellEdits,
    dbxSqlPreview,
    loadDbxObject,
    loadTable,
    page,
    projectRoot,
    queryResult,
    setDbxNewRows,
    setDbxPendingCellEdits,
    setError,
    t,
    tableColumns,
  ]);

  const insertRow = useCallback(() => {
    if (activeDbxConnection && activeDbxObject && queryResult) {
      if (!activeObject || !isDbxTableObject(activeDbxObject) || activeDbxConnection.readOnly)
        return;
      appendDbxNewRow();
      return;
    }
    if (!activeEndpoint || !activeObject || activeConnection?.readOnly || !queryResult) return;
    appendDbxNewRow();
  }, [
    activeConnection?.readOnly,
    activeDbxConnection,
    activeDbxObject,
    activeEndpoint,
    activeObject,
    appendDbxNewRow,
    queryResult,
  ]);

  const deleteDbxRowsByIndexes = useCallback(
    async (deletedRows: number[], confirmMessage: string, title: string) => {
      if (!activeDbxConnection || !activeDbxObject || !activeObject || !queryResult) return;
      if (activeDbxConnection.readOnly || !queryResult.editable) return;
      const normalizedDeletedRows = Array.from(new Set(deletedRows))
        .filter((rowIndex) => rowIndex >= 0 && rowIndex < queryResult.rows.length)
        .sort((left, right) => left - right);
      if (normalizedDeletedRows.length === 0) return;
      const options = buildDbxGridSaveOptions({ deletedRows: normalizedDeletedRows });
      if (!options) return;
      setError(null);
      try {
        const preview = await databaseApi.dbxDeleteRows({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: activeDbxObject.schema ?? null,
          options,
          execute: false,
        });
        if (preview.validationError) {
          setError(preview.validationError);
          return;
        }
        if (preview.statements.length === 0) return;
        dbxSqlPreview.record(preview, confirmMessage);
        const rollback = preview.rollbackStatements.length
          ? `\n\n${t("database.gridRollbackSql")}\n${preview.rollbackStatements.join("\n")}`
          : "";
        const ok = await confirm(
          `${confirmMessage}\n\n${preview.statements.join("\n")}${rollback}`,
          {
            title,
            kind: "warning",
            okLabel: t("file.delete"),
            cancelLabel: t("common.cancel"),
          },
        );
        if (!ok) return;
        const executed = await databaseApi.dbxDeleteRows({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: activeDbxObject.schema ?? null,
          options,
          execute: true,
        });
        if (executed.validationError) {
          setError(executed.validationError);
          return;
        }
        setDbxGridSelectedRows(new Set());
        await loadDbxObject(
          activeDbxObject,
          page,
          activeDbxConnection,
          activeDbxDatabase,
          dbxGridWhereInput,
          dbxGridOrderByInput,
        );
      } catch (err) {
        setError(String(err));
      }
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      activeObject,
      buildDbxGridSaveOptions,
      dbxGridOrderByInput,
      dbxGridWhereInput,
      dbxSqlPreview,
      loadDbxObject,
      page,
      queryResult,
      setDbxGridSelectedRows,
      setError,
      t,
    ],
  );

  const deleteSelectedDbxRows = useCallback(async () => {
    await deleteDbxRowsByIndexes(
      Array.from(dbxGridSelectedRows),
      t("database.confirmDeleteSelectedRows", { count: dbxGridSelectedRows.size }),
      t("database.deleteSelectedRows"),
    );
  }, [dbxGridSelectedRows, deleteDbxRowsByIndexes, t]);

  const copySelectedDbxRows = useCallback(async () => {
    if (!queryResult || dbxGridSelectedRows.size === 0 || visibleTableColumns.length === 0) return;
    const rows = Array.from(dbxGridSelectedRows)
      .sort((left, right) => left - right)
      .map((rowIndex) => queryResult.rows[rowIndex])
      .filter((row): row is DatabaseRow => Boolean(row));
    if (rows.length === 0) return;
    try {
      await navigator.clipboard?.writeText(dbxGridRowsToTsv(visibleTableColumns, rows));
    } catch (err) {
      setError(String(err));
    }
  }, [dbxGridSelectedRows, queryResult, setError, visibleTableColumns]);

  const handleDbxGridKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      if (isTextEditingShortcutTarget(event.target)) return;
      if (
        !(event.metaKey || event.ctrlKey) ||
        event.altKey ||
        event.shiftKey ||
        event.key.toLowerCase() !== "c"
      )
        return;
      if (!queryResult || !activeDbxConnection) return;
      if (dbxGridSelectedRows.size > 0) {
        event.preventDefault();
        void copySelectedDbxRows();
        return;
      }
      if (dbxSelectedCell) {
        const row = queryResult.rows[dbxSelectedCell.rowIndex];
        if (row) {
          const value = row.values[dbxSelectedCell.columnIndex];
          const text = valueToText(value);
          navigator.clipboard?.writeText(text);
        }
        event.preventDefault();
        return;
      }
    },
    [
      activeDbxConnection,
      copySelectedDbxRows,
      dbxGridSelectedRows.size,
      queryResult,
      dbxSelectedCell,
    ],
  );

  return {
    dbxGridContextRows,
    buildDbxGridCopyOptions,
    buildDbxGridContextFilterOptions,
    updateCell,
    savePendingGridChanges,
    insertRow,
    deleteDbxRowsByIndexes,
    deleteSelectedDbxRows,
    copySelectedDbxRows,
    handleDbxGridKeyDown,
  };
}
