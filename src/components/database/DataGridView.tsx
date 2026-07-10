import type { CSSProperties, KeyboardEventHandler } from "react";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import type { AeroricDbConnectionConfig, DbObject, DbQueryResult } from "../../types";
import {
  dbxGridColumnSortable,
  dbxGridColumnType,
  quoteSqlName,
  valueToText,
} from "../../lib/databaseUtils";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { DBX_GRID_DEFAULT_COLUMN_WIDTH, type DbxDataGridController } from "./useDbxDataGrid";
import type { DatabaseRow, DbxGridContextMenuState } from "./databaseGridState";

type Props = {
  variant: "table" | "query";
  queryResult: DbQueryResult | null;
  activeDbxConnection: AeroricDbConnectionConfig | null;
  activeConnectionReadOnly: boolean;
  activeObject: DbObject | null;
  tableColumns: string[];
  showRowIdColumn: boolean;
  loading: boolean;
  grid: DbxDataGridController;
  onKeyDown: KeyboardEventHandler<HTMLDivElement>;
  onSortColumn: (column: string) => void | Promise<void>;
  onOpenContextMenu: (menu: DbxGridContextMenuState) => void;
  onUpdateCell: (
    row: DatabaseRow,
    column: string,
    value: string,
    original: string,
  ) => void | Promise<void>;
};

function dbxDataTypeStyle(dataType: string): CSSProperties {
  const normalized = dataType.toLowerCase().trim();
  if (/\b(tinyint|smallint|mediumint|bigint|integer|int|serial|bigserial)\b/.test(normalized)) {
    return s.databaseTypeInteger;
  }
  if (/\b(varchar|char|character varying|nchar|nvarchar|string)\b/.test(normalized)) {
    return s.databaseTypeString;
  }
  if (/\b(text|clob|longtext|mediumtext|tinytext)\b/.test(normalized)) {
    return s.databaseTypeText;
  }
  if (/\b(decimal|numeric|number|float|double|real|money)\b/.test(normalized)) {
    return s.databaseTypeNumber;
  }
  if (/\b(date|time|timestamp|datetime|interval|year)\b/.test(normalized)) {
    return s.databaseTypeDate;
  }
  if (/\b(bool|boolean|bit)\b/.test(normalized)) {
    return s.databaseTypeBoolean;
  }
  if (/\b(json|jsonb|xml|array|map|struct)\b/.test(normalized)) {
    return s.databaseTypeJson;
  }
  if (/\b(blob|binary|varbinary|bytea|bytes|image)\b/.test(normalized)) {
    return s.databaseTypeBinary;
  }
  return s.databaseTypeDefault;
}

export function DataGridView({
  variant,
  queryResult,
  activeDbxConnection,
  activeConnectionReadOnly,
  activeObject,
  tableColumns,
  showRowIdColumn,
  loading,
  grid,
  onKeyDown,
  onSortColumn,
  onOpenContextMenu,
  onUpdateCell,
}: Props) {
  const { t } = useI18n();
  const {
    dbxGridOrderByInput,
    dbxGridColumnWidths,
    dbxGridSelectedRows,
    dbxSelectedCell,
    setDbxSelectedCell,
    dbxEditingCell,
    setDbxEditingCell,
    dbxPendingCellEdits,
    dbxHoveredCell,
    setDbxHoveredCell,
    setDbxCellDetail,
    resizingDbxGridColumn,
  } = grid.state;
  const { visibleTableColumns, dbxGridTableMinWidth, activeDbxGridColumnsByName, tableRows } =
    grid.derived;
  const {
    startDbxGridColumnResize,
    autoFitDbxGridColumn,
    toggleDbxGridRowSelection,
    stageDbxCellEdit,
  } = grid.actions;

  if (tableColumns.length === 0) {
    return <div style={s.databaseEmpty}>{t("database.empty")}</div>;
  }

  return (
    <div
      style={s.databaseTableWrap}
      data-grid-variant={variant}
      role={queryResult && activeDbxConnection ? "grid" : undefined}
      tabIndex={queryResult && activeDbxConnection ? 0 : undefined}
      aria-label={queryResult && activeDbxConnection ? t("database.gridData") : undefined}
      onKeyDown={queryResult && activeDbxConnection ? onKeyDown : undefined}
    >
      <table style={{ ...s.databaseTable, minWidth: dbxGridTableMinWidth }}>
        <thead>
          <tr>
            {queryResult && activeDbxConnection && (
              <th style={{ ...s.databaseTh, ...s.databaseGridControlTh, width: 42 }}>#</th>
            )}
            {showRowIdColumn && <th style={{ ...s.databaseTh, width: 86 }}>rowid</th>}
            {visibleTableColumns.map(({ column }) => {
              const columnWidth = dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH;
              const columnIndex = tableColumns.indexOf(column);
              const sortable = dbxGridColumnSortable(queryResult, columnIndex);
              const dbxColumnInfo = activeDbxGridColumnsByName.get(column.toLowerCase());
              const legacyColumnInfo = activeObject?.columns.find(
                (item) => item.name.toLowerCase() === column.toLowerCase(),
              );
              const columnType =
                dbxColumnInfo?.data_type ??
                dbxGridColumnType(queryResult, columnIndex) ??
                legacyColumnInfo?.dataType ??
                "";
              const columnTypeStyle = columnType
                ? dbxDataTypeStyle(columnType)
                : s.databaseTypeDefault;
              const columnComment = dbxColumnInfo?.comment?.trim() ?? "";
              const nullableText = dbxColumnInfo
                ? dbxColumnInfo.is_nullable
                  ? t("database.yes")
                  : t("database.no")
                : legacyColumnInfo
                  ? legacyColumnInfo.nullable
                    ? t("database.yes")
                    : t("database.no")
                  : "-";
              const primaryKeyText = dbxColumnInfo
                ? dbxColumnInfo.is_primary_key
                  ? t("database.yes")
                  : t("database.no")
                : legacyColumnInfo
                  ? legacyColumnInfo.primaryKey
                    ? t("database.yes")
                    : t("database.no")
                  : "-";
              const defaultValue =
                dbxColumnInfo?.column_default ?? legacyColumnInfo?.defaultValue ?? "-";
              const columnDetailsTitle = t("database.gridColumnDetails", {
                name: column,
                type: columnType || "-",
                comment: columnComment || "-",
                nullable: nullableText,
                primaryKey: primaryKeyText,
                defaultValue: defaultValue || "-",
              });
              const columnHeaderContent = (
                <span style={s.databaseGridHeaderStack}>
                  <span style={s.databaseGridHeaderName}>{column}</span>
                  <span
                    style={{ ...s.databaseGridHeaderTypeLine, ...columnTypeStyle }}
                    title={
                      columnType ? t("database.gridColumnType", { type: columnType }) : undefined
                    }
                  >
                    {columnType || "-"}
                  </span>
                  <span style={s.databaseGridHeaderCommentLine} title={columnComment || undefined}>
                    {columnComment || "-"}
                  </span>
                </span>
              );
              return (
                <th
                  key={column}
                  aria-label={column}
                  style={{
                    ...s.databaseTh,
                    width: columnWidth,
                    minWidth: columnWidth,
                    maxWidth: columnWidth,
                    paddingRight: queryResult && activeDbxConnection ? 12 : 8,
                  }}
                  title={columnDetailsTitle}
                  onContextMenu={
                    queryResult && activeDbxConnection
                      ? (event) => {
                          event.preventDefault();
                          onOpenContextMenu({
                            x: event.clientX,
                            y: event.clientY,
                            connectionId: activeDbxConnection.id,
                            columnIndex,
                            column,
                            kind: "dbx-grid-header",
                          });
                        }
                      : undefined
                  }
                >
                  {queryResult && activeDbxConnection ? (
                    <>
                      {sortable ? (
                        <button
                          type="button"
                          style={{
                            ...s.databaseGridHeaderButton,
                            cursor: loading ? "default" : "pointer",
                          }}
                          aria-label={column}
                          disabled={loading}
                          onClick={() => void onSortColumn(column)}
                        >
                          {columnHeaderContent}
                          <span style={s.databaseGridHeaderSortIcon}>
                            {dbxGridOrderByInput.toLowerCase() ===
                            `${quoteSqlName(column)} asc`.toLowerCase() ? (
                              <ArrowUp size={14} aria-label={t("database.sortAscending")} />
                            ) : dbxGridOrderByInput.toLowerCase() ===
                              `${quoteSqlName(column)} desc`.toLowerCase() ? (
                              <ArrowDown size={14} aria-label={t("database.sortDescending")} />
                            ) : (
                              <ArrowUpDown
                                size={14}
                                aria-hidden="true"
                                style={{ color: "var(--text-hint)" }}
                              />
                            )}
                          </span>
                        </button>
                      ) : (
                        <span
                          style={{
                            display: "inline-flex",
                            alignItems: "center",
                            width: "100%",
                            minHeight: 50,
                            minWidth: 0,
                            paddingRight: 8,
                          }}
                        >
                          {columnHeaderContent}
                        </span>
                      )}
                      <button
                        type="button"
                        aria-label={t("database.gridResizeColumn", { column })}
                        title={t("database.gridResizeColumn", { column })}
                        onPointerDown={(event) => startDbxGridColumnResize(column, event)}
                        onDoubleClick={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          autoFitDbxGridColumn(column);
                        }}
                        style={{
                          position: "absolute",
                          top: 0,
                          right: 0,
                          bottom: 0,
                          width: 8,
                          padding: 0,
                          border: "none",
                          borderRight:
                            resizingDbxGridColumn === column
                              ? "1px solid var(--accent)"
                              : "1px solid transparent",
                          background:
                            resizingDbxGridColumn === column ? "var(--bg-hover)" : "transparent",
                          cursor: "col-resize",
                        }}
                      />
                    </>
                  ) : (
                    column
                  )}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {tableRows.map((row, rowIndex) => {
            const dbxRowIndex =
              queryResult && activeDbxConnection ? queryResult.rows.indexOf(row) : -1;
            const rowSelected = dbxRowIndex >= 0 && dbxGridSelectedRows.has(dbxRowIndex);
            return (
              <tr
                key={`${row.rowId ?? "sql"}:${rowIndex}`}
                style={rowSelected ? s.databaseGridRowSelected : undefined}
              >
                {queryResult && activeDbxConnection && (
                  <td
                    style={{
                      ...s.databaseTd,
                      ...s.databaseGridControlTd,
                      ...(rowSelected ? s.databaseGridRowSelected : undefined),
                    }}
                  >
                    <button
                      type="button"
                      aria-label={t("database.selectRow", { row: rowIndex + 1 })}
                      disabled={dbxRowIndex < 0 || loading}
                      style={{
                        ...s.databaseGridRowNumberButton,
                        ...(rowSelected ? s.databaseGridRowNumberButtonSelected : undefined),
                      }}
                      onClick={(event) => {
                        if (dbxRowIndex >= 0) toggleDbxGridRowSelection(dbxRowIndex, event);
                      }}
                    >
                      {dbxRowIndex >= 0 ? dbxRowIndex + 1 : rowIndex + 1}
                    </button>
                  </td>
                )}
                {showRowIdColumn && (
                  <td
                    style={{
                      ...s.databaseTd,
                      color: "var(--text-hint)",
                      ...(rowSelected ? s.databaseGridRowSelected : undefined),
                    }}
                  >
                    {row.rowId ?? "-"}
                  </td>
                )}
                {visibleTableColumns.map(({ column, index: columnIndex }) => {
                  const original = valueToText(row.values[columnIndex]);
                  const pendingEdit = dbxPendingCellEdits[`${dbxRowIndex}:${columnIndex}`] ?? null;
                  const displayValue = pendingEdit?.value ?? original;
                  const previewable = Boolean(queryResult && activeDbxConnection);
                  const isCellSelected =
                    dbxSelectedCell?.rowIndex === dbxRowIndex &&
                    dbxSelectedCell?.columnIndex === columnIndex;
                  const isCellEditing =
                    dbxEditingCell?.rowIndex === dbxRowIndex &&
                    dbxEditingCell?.columnIndex === columnIndex;
                  const showCellPreview =
                    previewable &&
                    dbxHoveredCell?.rowIndex === dbxRowIndex &&
                    dbxHoveredCell?.columnIndex === columnIndex;
                  const editable = Boolean(
                    queryResult &&
                    activeObject?.objectType === "table" &&
                    queryResult.editable &&
                    !activeConnectionReadOnly &&
                    !activeDbxConnection?.readOnly,
                  );
                  return (
                    <td
                      key={`${column}:${columnIndex}`}
                      style={{
                        ...s.databaseTd,
                        ...(rowSelected ? s.databaseGridRowSelected : undefined),
                        ...(isCellSelected ? s.databaseCellSelected : undefined),
                        width: dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH,
                        minWidth: dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH,
                        maxWidth: dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH,
                      }}
                      title={displayValue}
                      onMouseEnter={() => setDbxHoveredCell({ rowIndex: dbxRowIndex, columnIndex })}
                      onMouseLeave={() =>
                        setDbxHoveredCell((current) =>
                          current?.rowIndex === dbxRowIndex && current.columnIndex === columnIndex
                            ? null
                            : current,
                        )
                      }
                      onClick={() =>
                        setDbxSelectedCell({ rowIndex: dbxRowIndex, columnIndex, column })
                      }
                      onDoubleClick={() => {
                        if (editable) {
                          setDbxEditingCell({
                            rowIndex: dbxRowIndex,
                            columnIndex,
                            column,
                          });
                        }
                      }}
                      onContextMenu={
                        queryResult && activeDbxConnection
                          ? (event) => {
                              event.preventDefault();
                              onOpenContextMenu({
                                x: event.clientX,
                                y: event.clientY,
                                connectionId: activeDbxConnection.id,
                                rowIndex: dbxRowIndex,
                                columnIndex,
                                column,
                                value: row.values[columnIndex],
                                kind: "dbx-grid-cell",
                              });
                            }
                          : undefined
                      }
                    >
                      <div style={s.databaseGridCellContent}>
                        {isCellEditing && editable ? (
                          <input
                            style={{
                              ...s.databaseCellInput,
                              minWidth: 0,
                              flex: "1 1 auto",
                              borderColor: "var(--border-focus)",
                              background: "var(--bg-input)",
                            }}
                            defaultValue={displayValue}
                            autoFocus
                            onFocus={(event) => event.currentTarget.select()}
                            onBlur={(event) => {
                              if (activeDbxConnection) {
                                stageDbxCellEdit(
                                  dbxRowIndex,
                                  columnIndex,
                                  column,
                                  event.currentTarget.value,
                                  original,
                                );
                              } else {
                                void onUpdateCell(row, column, event.currentTarget.value, original);
                              }
                              setDbxEditingCell(null);
                            }}
                            onKeyDown={(event) => {
                              if (event.key === "Escape") setDbxEditingCell(null);
                            }}
                          />
                        ) : (
                          <span style={s.databaseGridCellValue}>{displayValue}</span>
                        )}
                        {showCellPreview && (
                          <button
                            type="button"
                            aria-label={t("database.previewCellValue", { column })}
                            title={t("database.previewCellValue", { column })}
                            onClick={(event) => {
                              event.stopPropagation();
                              const columnInfo = activeDbxGridColumnsByName.get(
                                column.toLowerCase(),
                              );
                              setDbxCellDetail({
                                column,
                                columnIndex,
                                rowIndex: dbxRowIndex,
                                value: row.values[columnIndex],
                                columnInfo,
                              });
                            }}
                            style={s.databaseGridCellPreviewButton}
                          >
                            i
                          </button>
                        )}
                      </div>
                    </td>
                  );
                })}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
