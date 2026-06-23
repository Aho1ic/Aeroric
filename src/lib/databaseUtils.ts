/**
 * Database utility functions extracted from DatabaseView.tsx
 */

// DBX grid constants
const DBX_GRID_MIN_COLUMN_WIDTH = 72;
const DBX_GRID_MAX_COLUMN_WIDTH = 520;
const DBX_GRID_AUTOFIT_CHAR_WIDTH = 8;
const DBX_GRID_AUTOFIT_PADDING = 48;

// DatabaseRow type definition
type DatabaseRow = { rowId?: number | null; keyValues: Array<{ column: string; value: unknown }>; values: unknown[] };

/**
 * Escape a value for TSV output
 */
export function escapeTsvCell(value: string): string {
  return value.replace(/\t/g, " ").replace(/\r?\n/g, " ");
}

/**
 * Convert grid rows to TSV format
 */
export function dbxGridRowsToTsv(
  columns: Array<{ column: string; index: number }>,
  rows: DatabaseRow[],
): string {
  return [
    columns.map(({ column }) => escapeTsvCell(column)).join("\t"),
    ...rows.map((row) =>
      columns
        .map(({ index }) => escapeTsvCell(valueToText(row.values[index])))
        .join("\t"),
    ),
  ].join("\n");
}

/**
 * Convert grid rows to JSON format
 */
export function dbxGridRowsToJson(
  columns: Array<{ column: string; index: number }>,
  rows: DatabaseRow[],
): string {
  const objects = rows.map((row) =>
    Object.fromEntries(
      columns.map(({ column, index }) => [column, row.values[index] ?? null]),
    ),
  );
  return JSON.stringify(objects.length === 1 ? objects[0] : objects, null, 2);
}

/**
 * Check if the target element is a text editing shortcut target
 */
export function isTextEditingShortcutTarget(
  target: EventTarget | null,
): boolean {
  if (
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement
  )
    return true;
  if (target instanceof HTMLInputElement) {
    return !["button", "checkbox", "radio", "reset", "submit"].includes(
      target.type,
    );
  }
  return target instanceof HTMLElement && target.isContentEditable;
}

/**
 * Check if a grid value is null
 */
export function isNullGridValue(value: unknown): boolean {
  return value === null || value === undefined;
}

/**
 * Convert a value to text representation
 */
export function valueToText(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "string") return value;
  return String(value);
}

/**
 * Quote a SQL name with double quotes
 */
export function quoteSqlName(name: string): string {
  return `"${name.replace(/"/g, '""')}"`;
}

/**
 * Convert a value to a SQL literal
 */
export function sqlLiteral(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "number") return String(value);
  if (typeof value === "boolean") return value ? "TRUE" : "FALSE";
  return `'${String(value).replace(/'/g, "''")}'`;
}

/**
 * Clamp a DBX grid column width to min/max values
 */
export function clampDbxGridColumnWidth(width: number): number {
  return Math.min(DBX_GRID_MAX_COLUMN_WIDTH, Math.max(DBX_GRID_MIN_COLUMN_WIDTH, Math.round(width)));
}

/**
 * Estimate a DBX grid column width based on content
 */
export function estimateDbxGridColumnWidth(column: string, columnIndex: number, rows: DatabaseRow[], columnType = ""): number {
  const headerLength = Math.max(column.length, columnType.trim().length);
  const longestTextLength = rows.reduce((length, row) => {
    const text = valueToText(row.values[columnIndex]);
    return Math.max(length, Math.min(text.length, 60));
  }, headerLength);
  return clampDbxGridColumnWidth(longestTextLength * DBX_GRID_AUTOFIT_CHAR_WIDTH + DBX_GRID_AUTOFIT_PADDING);
}

/**
 * Initialize DBX grid column widths
 */
export function initialDbxGridColumnWidths(columns: string[], rows: DatabaseRow[], columnTypes: string[] = []): Record<string, number> {
  return Object.fromEntries(columns.map((column, index) => [column, estimateDbxGridColumnWidth(column, index, rows, columnTypes[index] ?? "")]));
}

/**
 * Check if a DBX grid column is sortable
 */
export function dbxGridColumnSortable(result: { columnSortables?: boolean[] } | null, columnIndex: number): boolean {
  const sortable = result?.columnSortables?.[columnIndex];
  return sortable === undefined ? true : sortable;
}

/**
 * Get the type of a DBX grid column
 */
export function dbxGridColumnType(result: { columnTypes?: string[] } | null, columnIndex: number): string | null {
  const columnType = result?.columnTypes?.[columnIndex];
  return typeof columnType === "string" && columnType.trim() ? columnType.trim() : null;
}

/**
 * Convert text to cell value
 */
export function textToCellValue(value: string): string | null {
  return value.trim().toUpperCase() === "NULL" ? null : value;
}

/**
 * Get cell preview text
 */
export function cellPreviewText(value: unknown): { text: string; json: boolean } {
  if (value === null || value === undefined) return { text: "NULL", json: false };
  if (typeof value === "string") {
    const trimmed = value.trim();
    if ((trimmed.startsWith("{") && trimmed.endsWith("}")) || (trimmed.startsWith("[") && trimmed.endsWith("]"))) {
      try {
        return { text: JSON.stringify(JSON.parse(trimmed), null, 2), json: true };
      } catch {
        return { text: value, json: false };
      }
    }
    return { text: value, json: false };
  }
  if (typeof value === "object") {
    try {
      return { text: JSON.stringify(value, null, 2), json: true };
    } catch {
      return { text: String(value), json: false };
    }
  }
  return { text: String(value), json: false };
}

/**
 * Generate a row key for a database row
 */
export function rowKeyFor(row: DatabaseRow) {
  return {
    rowId: row.rowId ?? null,
    keyValues: row.keyValues
      .filter((item) => item.column !== "__aeroric_rowid__")
      .map((item) => ({
        column: item.column,
        value: item.value === null || item.value === undefined ? null : String(item.value),
      })),
  };
}

/**
 * Create a connection name from an endpoint
 */
export function createConnectionName(endpoint: { kind: string; path: string }): string {
  const path = endpoint.kind === "local" ? endpoint.path : endpoint.path;
  const name = path.split("/").filter(Boolean).pop();
  return name || "SQLite";
}
