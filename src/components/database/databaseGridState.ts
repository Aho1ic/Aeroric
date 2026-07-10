import type { DataGridContextFilterMode, DbRow } from "../../types";
import { quoteSqlName, valueToText } from "../../lib/databaseUtils";

export type DatabaseRow = DbRow;

export type TableExportFormat = "csv" | "json" | "markdown" | "insertSql" | "updateSql" | "xlsx";

export type DbxGridHeaderContextMenuState = {
  x: number;
  y: number;
  connectionId: string;
  columnIndex: number;
  column: string;
  kind: "dbx-grid-header";
};

export type DbxGridCellContextMenuState = {
  x: number;
  y: number;
  connectionId: string;
  rowIndex: number;
  columnIndex: number;
  column: string;
  value: unknown;
  kind: "dbx-grid-cell";
};

export type DbxGridContextMenuState = DbxGridHeaderContextMenuState | DbxGridCellContextMenuState;

export type DbxGridCellContextMenuAction =
  | "copyValue"
  | "copyColumnName"
  | "previewValue"
  | "previewRow"
  | "previewColumn"
  | "sortAscending"
  | "sortDescending"
  | "clearSort"
  | "filterEquals"
  | "filterNotEquals"
  | "filterLike"
  | "filterNotLike"
  | "filterLessThan"
  | "filterGreaterThan"
  | "filterIsNull"
  | "filterIsNotNull"
  | "clearFilter"
  | "copyRowJson"
  | "copyRowInsert"
  | "copyRowInsertWithoutPrimaryKeys"
  | "copyRowUpdate"
  | "copyAllTsv";

export type DbxGridHeaderContextMenuAction =
  | "copyColumnName"
  | "previewColumn"
  | "copyAlterColumnSql"
  | "sortAscending"
  | "sortDescending"
  | "clearSort";

export type DbxPendingCellEdit = {
  rowIndex: number;
  columnIndex: number;
  column: string;
  value: string;
  original: string;
};

export type DbxPendingCellEdits = Record<string, DbxPendingCellEdit>;

export type VisibleGridColumn = { column: string; index: number };

export function nextDbxOrderByForColumn(currentOrderBy: string, column: string): string {
  const asc = `${quoteSqlName(column)} ASC`;
  const desc = `${quoteSqlName(column)} DESC`;
  const normalized = currentOrderBy.trim().toLowerCase();
  if (normalized === asc.toLowerCase()) return desc;
  if (normalized === desc.toLowerCase()) return "";
  return asc;
}

export function dbxOrderByForColumn(column: string, direction: "ASC" | "DESC" | null): string {
  return direction ? `${quoteSqlName(column)} ${direction}` : "";
}

export function dbxFilterModeForCellAction(
  action: DbxGridCellContextMenuAction,
): DataGridContextFilterMode | null {
  if (action === "filterEquals") return "equals";
  if (action === "filterNotEquals") return "not-equals";
  if (action === "filterLike") return "like";
  if (action === "filterNotLike") return "not-like";
  if (action === "filterLessThan") return "less-than";
  if (action === "filterGreaterThan") return "greater-than";
  if (action === "filterIsNull") return "is-null";
  if (action === "filterIsNotNull") return "is-not-null";
  return null;
}

export function combineDbxGridWhereCondition(currentWhere: string, condition: string): string {
  const current = currentWhere.trim();
  return current ? `(${current}) AND (${condition})` : condition;
}

export function visibleDbxGridColumns(
  columns: string[],
  hiddenColumns: ReadonlySet<string>,
): VisibleGridColumn[] {
  return columns
    .map((column, index) => ({ column, index }))
    .filter(({ column }) => !hiddenColumns.has(column));
}

export function filterDbxGridRows(rows: DatabaseRow[], search: string): DatabaseRow[] {
  const query = search.trim().toLowerCase();
  if (!query) return rows;
  return rows.filter((row) =>
    row.values.some((value) => valueToText(value).toLowerCase().includes(query)),
  );
}

export function filterDbxGridColumnOptions(columns: string[], search: string): string[] {
  const query = search.trim().toLowerCase();
  if (!query) return columns;
  return columns.filter((column) => column.toLowerCase().includes(query));
}

export function pruneDbxGridHiddenColumns(
  current: ReadonlySet<string>,
  columns: string[],
): Set<string> {
  if (current.size === 0) return current as Set<string>;
  const available = new Set(columns);
  const next = new Set([...current].filter((column) => available.has(column)));
  if (columns.length > 0 && next.size >= columns.length) next.delete(columns[0]);
  return next.size === current.size ? (current as Set<string>) : next;
}

export function pruneDbxGridColumnWidths(
  current: Readonly<Record<string, number>>,
  columns: string[],
): Record<string, number> {
  const available = new Set(columns);
  const entries = Object.entries(current).filter(([column]) => available.has(column));
  return entries.length === Object.keys(current).length
    ? (current as Record<string, number>)
    : Object.fromEntries(entries);
}

export function toggleDbxGridColumn(
  current: ReadonlySet<string>,
  columns: string[],
  column: string,
): Set<string> {
  const next = new Set(current);
  if (next.has(column)) {
    next.delete(column);
  } else {
    const visibleCount = columns.filter((item) => !next.has(item)).length;
    if (visibleCount <= 1) return current as Set<string>;
    next.add(column);
  }
  return next;
}

export function invertDbxGridColumns(current: ReadonlySet<string>, columns: string[]): Set<string> {
  if (columns.length <= 1) return new Set();
  const next = new Set<string>();
  columns.forEach((column) => {
    if (!current.has(column)) next.add(column);
  });
  if (next.size >= columns.length) next.delete(columns[0]);
  return next;
}

export function toggleDbxGridRowSelection(
  current: ReadonlySet<number>,
  rowIndex: number,
  selectionAnchor: number | null,
  rowCount: number,
  extendSelection: boolean,
): { selectedRows: Set<number>; selectionAnchor: number | null } {
  if (rowIndex < 0) {
    return {
      selectedRows: current as Set<number>,
      selectionAnchor,
    };
  }
  if (extendSelection && selectionAnchor !== null) {
    const maxRowIndex = Math.max(0, rowCount - 1);
    const anchor = Math.max(0, Math.min(selectionAnchor, maxRowIndex));
    const target = Math.max(0, Math.min(rowIndex, maxRowIndex));
    const [start, end] = anchor < target ? [anchor, target] : [target, anchor];
    return {
      selectedRows: new Set(Array.from({ length: end - start + 1 }, (_, offset) => start + offset)),
      selectionAnchor: rowIndex,
    };
  }
  const selectedRows = new Set(current);
  if (selectedRows.has(rowIndex)) selectedRows.delete(rowIndex);
  else selectedRows.add(rowIndex);
  return { selectedRows, selectionAnchor: rowIndex };
}

export function stageDbxPendingCellEdit(
  current: Readonly<DbxPendingCellEdits>,
  edit: DbxPendingCellEdit,
): DbxPendingCellEdits {
  const key = `${edit.rowIndex}:${edit.columnIndex}`;
  const next = { ...current };
  if (edit.value === edit.original) delete next[key];
  else next[key] = edit;
  return next;
}

export function dbxPendingCellEditsToDirtyRows(
  edits: Readonly<DbxPendingCellEdits>,
  convertValue: (value: string) => unknown,
): Array<[number, Array<[number, unknown]>]> {
  const dirtyRowsByIndex = new Map<number, Array<[number, unknown]>>();
  for (const edit of Object.values(edits)) {
    const values = dirtyRowsByIndex.get(edit.rowIndex) ?? [];
    values.push([edit.columnIndex, convertValue(edit.value)]);
    dirtyRowsByIndex.set(edit.rowIndex, values);
  }
  return Array.from(dirtyRowsByIndex.entries());
}

export function dbxGridContextRowIndexes(
  selectedRows: ReadonlySet<number>,
  contextRowIndex: number,
): number[] {
  return selectedRows.has(contextRowIndex) && selectedRows.size > 0
    ? Array.from(selectedRows).sort((left, right) => left - right)
    : [contextRowIndex];
}
