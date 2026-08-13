import {
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEventHandler,
} from "react";
import { ArrowDown, ArrowUp, ArrowUpDown, Funnel } from "lucide-react";
import * as Popover from "@radix-ui/react-popover";
import type { AeroricDbConnectionConfig, DbObject, DbQueryResult } from "../../types";
import {
  dbxGridColumnSortable,
  dbxGridColumnType,
  quoteSqlName,
  valueToText,
} from "../../lib/databaseUtils";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { zLayers } from "../../styles/zLayers";
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
  canInsertRows: boolean;
  loading: boolean;
  grid: DbxDataGridController;
  onKeyDown: KeyboardEventHandler<HTMLDivElement>;
  onSortColumn: (column: string) => void | Promise<void>;
  onFilterColumn: (column: string, value: string) => void | Promise<void>;
  onOpenContextMenu: (menu: DbxGridContextMenuState) => void;
  onUpdateCell: (
    row: DatabaseRow,
    column: string,
    value: string,
    original: string,
  ) => void | Promise<void>;
};

type ColumnFuzzyFilterProps = {
  column: string;
  value: string;
  visible: boolean;
  loading: boolean;
  right: number;
  onApply: (value: string) => void | Promise<void>;
};

function ColumnFuzzyFilter({
  column,
  value,
  visible,
  loading,
  right,
  onApply,
}: ColumnFuzzyFilterProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(value);
  const shown = visible || open || Boolean(value);

  const apply = async (nextValue: string) => {
    await onApply(nextValue);
    setOpen(false);
  };

  return (
    <Popover.Root
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (nextOpen) setDraft(value);
      }}
    >
      <Popover.Trigger asChild>
        <button
          type="button"
          className="database-grid-column-filter"
          data-shown={shown}
          aria-label={t("database.columnFuzzyFilter", { column })}
          aria-hidden={!shown}
          tabIndex={shown ? 0 : -1}
          title={t("database.columnFuzzyFilter", { column })}
          disabled={loading}
          onClick={(event) => event.stopPropagation()}
          style={{
            position: "absolute",
            top: "50%",
            right,
            transform: "translateY(-50%)",
            width: 26,
            height: 26,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            padding: 0,
            border: "1px solid var(--border-dim)",
            borderRadius: "var(--radius-sm)",
            background: value ? "var(--control-active-bg)" : "var(--bg-panel)",
            color: value ? "var(--accent)" : "var(--text-muted)",
            cursor: loading ? "default" : "pointer",
            transition: "opacity 0.14s ease, color 0.14s ease, background 0.14s ease",
            zIndex: 2,
          }}
        >
          <Funnel size={13} strokeWidth={2} aria-hidden="true" />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          sideOffset={6}
          align="end"
          style={{
            width: 268,
            padding: 12,
            border: "1px solid var(--border-medium)",
            borderRadius: "var(--radius-md)",
            background: "var(--bg-card)",
            boxShadow: "var(--shadow-popover)",
            color: "var(--text-primary)",
            zIndex: zLayers.popover,
          }}
        >
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void apply(draft.trim());
            }}
          >
            <label
              htmlFor={`database-column-filter-${column}`}
              style={{ display: "block", marginBottom: 8, fontSize: 12, fontWeight: 700 }}
            >
              {t("database.columnFuzzyFilterTitle", { column })}
            </label>
            <input
              id={`database-column-filter-${column}`}
              autoFocus
              value={draft}
              onChange={(event) => setDraft(event.currentTarget.value)}
              placeholder={t("database.columnFuzzyFilterPlaceholder")}
              style={{
                width: "100%",
                height: 34,
                boxSizing: "border-box",
                padding: "0 10px",
                border: "1px solid var(--border-medium)",
                borderRadius: "var(--radius-sm)",
                outline: "none",
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                fontFamily: "var(--font-ui)",
                fontSize: 12.5,
              }}
            />
            <div
              style={{
                display: "flex",
                justifyContent: "flex-end",
                gap: 8,
                marginTop: 10,
              }}
            >
              {value && (
                <button
                  type="button"
                  disabled={loading}
                  onClick={() => void apply("")}
                  style={{
                    height: 30,
                    padding: "0 10px",
                    border: "1px solid var(--border-dim)",
                    borderRadius: "var(--radius-sm)",
                    background: "transparent",
                    color: "var(--text-muted)",
                    cursor: "pointer",
                  }}
                >
                  {t("common.clear")}
                </button>
              )}
              <button
                type="submit"
                disabled={loading || !draft.trim()}
                style={{
                  height: 30,
                  padding: "0 12px",
                  border: "1px solid var(--accent)",
                  borderRadius: "var(--radius-sm)",
                  background: "var(--accent)",
                  color: "var(--primary-action-fg)",
                  cursor: loading || !draft.trim() ? "default" : "pointer",
                  opacity: loading || !draft.trim() ? 0.55 : 1,
                  fontWeight: 700,
                }}
              >
                {t("common.apply")}
              </button>
            </div>
          </form>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

type CellEditorProps = {
  value: string;
  onCancel: () => void;
  onCommit: (value: string) => void;
};

function CellEditor({ value, onCancel, onCommit }: CellEditorProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  useLayoutEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  return (
    <input
      ref={inputRef}
      type="text"
      style={{
        ...s.databaseCellInput,
        minWidth: 0,
        flex: "1 1 auto",
        borderColor: "var(--border-focus)",
        background: "var(--bg-input)",
      }}
      defaultValue={value}
      onBlur={(event) => onCommit(event.currentTarget.value)}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        onCancel();
      }}
    />
  );
}

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
  canInsertRows,
  loading,
  grid,
  onKeyDown,
  onSortColumn,
  onFilterColumn,
  onOpenContextMenu,
  onUpdateCell,
}: Props) {
  const { t } = useI18n();
  const tableWrapRef = useRef<HTMLDivElement>(null);
  const [hoveredHeaderColumn, setHoveredHeaderColumn] = useState<string | null>(null);
  const previousNewRowCountRef = useRef(grid.state.dbxNewRows.length);
  const {
    dbxGridOrderByInput,
    dbxGridColumnFuzzyFilters,
    dbxGridColumnWidths,
    dbxGridSelectedRows,
    dbxSelectedCell,
    setDbxSelectedCell,
    dbxEditingCell,
    setDbxEditingCell,
    dbxPendingCellEdits,
    dbxNewRows,
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
    stageDbxNewRowCellEdit,
  } = grid.actions;

  useLayoutEffect(() => {
    const previousCount = previousNewRowCountRef.current;
    previousNewRowCountRef.current = dbxNewRows.length;
    if (dbxNewRows.length <= previousCount) return;

    const tableWrap = tableWrapRef.current;
    if (!tableWrap) return;
    if (typeof tableWrap.scrollTo === "function") {
      tableWrap.scrollTo({
        top: tableWrap.scrollHeight,
        left: tableWrap.scrollLeft,
        behavior: "auto",
      });
      return;
    }
    tableWrap.scrollTop = tableWrap.scrollHeight;
  }, [dbxNewRows.length]);

  if (tableColumns.length === 0) {
    return <div style={s.databaseEmpty}>{t("database.empty")}</div>;
  }

  return (
    <div
      ref={tableWrapRef}
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
                  className="database-grid-column-header"
                  aria-label={column}
                  style={{
                    ...s.databaseTh,
                    width: columnWidth,
                    minWidth: columnWidth,
                    maxWidth: columnWidth,
                    paddingRight: queryResult && activeDbxConnection ? 12 : 8,
                  }}
                  title={columnDetailsTitle}
                  onMouseEnter={() => setHoveredHeaderColumn(column)}
                  onMouseLeave={() =>
                    setHoveredHeaderColumn((current) => (current === column ? null : current))
                  }
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
                      {variant === "table" && (
                        <ColumnFuzzyFilter
                          column={column}
                          value={dbxGridColumnFuzzyFilters[column]?.value ?? ""}
                          visible={hoveredHeaderColumn === column}
                          loading={loading}
                          right={sortable ? 46 : 12}
                          onApply={(value) => onFilterColumn(column, value)}
                        />
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
            const draftRowIndex = dbxNewRows.findIndex((values) => values === row.values);
            const persistedDbxRowIndex =
              queryResult && activeDbxConnection ? queryResult.rows.indexOf(row) : -1;
            const dbxRowIndex =
              draftRowIndex >= 0
                ? (queryResult?.rows.length ?? 0) + draftRowIndex
                : persistedDbxRowIndex;
            const rowSelected =
              draftRowIndex < 0 &&
              persistedDbxRowIndex >= 0 &&
              dbxGridSelectedRows.has(persistedDbxRowIndex);
            return (
              <tr
                key={
                  draftRowIndex >= 0 ? `new:${draftRowIndex}` : `${row.rowId ?? "sql"}:${rowIndex}`
                }
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
                      disabled={draftRowIndex >= 0 || persistedDbxRowIndex < 0 || loading}
                      style={{
                        ...s.databaseGridRowNumberButton,
                        ...(rowSelected ? s.databaseGridRowNumberButtonSelected : undefined),
                      }}
                      onClick={(event) => {
                        if (persistedDbxRowIndex >= 0) {
                          toggleDbxGridRowSelection(persistedDbxRowIndex, event);
                        }
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
                  const original =
                    draftRowIndex >= 0 && row.values[columnIndex] == null
                      ? ""
                      : valueToText(row.values[columnIndex]);
                  const pendingEdit =
                    draftRowIndex < 0
                      ? (dbxPendingCellEdits[`${persistedDbxRowIndex}:${columnIndex}`] ?? null)
                      : null;
                  const displayValue = pendingEdit?.value ?? original;
                  const previewable = Boolean(
                    draftRowIndex < 0 && queryResult && activeDbxConnection,
                  );
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
                    draftRowIndex >= 0
                      ? canInsertRows && !activeConnectionReadOnly && !activeDbxConnection?.readOnly
                      : queryResult &&
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
                        draftRowIndex < 0 && queryResult && activeDbxConnection
                          ? (event) => {
                              event.preventDefault();
                              onOpenContextMenu({
                                x: event.clientX,
                                y: event.clientY,
                                connectionId: activeDbxConnection.id,
                                rowIndex: persistedDbxRowIndex,
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
                          <CellEditor
                            value={displayValue}
                            onCancel={() => setDbxEditingCell(null)}
                            onCommit={(value) => {
                              if (draftRowIndex >= 0) {
                                stageDbxNewRowCellEdit(draftRowIndex, columnIndex, value);
                              } else if (activeDbxConnection) {
                                stageDbxCellEdit(dbxRowIndex, columnIndex, column, value, original);
                              } else {
                                void onUpdateCell(row, column, value, original);
                              }
                              setDbxEditingCell(null);
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
