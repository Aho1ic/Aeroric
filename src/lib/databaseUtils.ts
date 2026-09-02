/**
 * Database utility functions extracted from DatabaseView.tsx
 */

// DBX grid constants
const DBX_GRID_MIN_COLUMN_WIDTH = 72;
const DBX_GRID_MAX_COLUMN_WIDTH = 520;
const DBX_GRID_AUTOFIT_CHAR_WIDTH = 8;
const DBX_GRID_AUTOFIT_PADDING = 48;

// DatabaseRow type definition
type DatabaseRow = {
  rowId?: number | null;
  keyValues: Array<{ column: string; value: unknown }>;
  values: unknown[];
};

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
      columns.map(({ index }) => escapeTsvCell(valueToText(row.values[index]))).join("\t"),
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
    Object.fromEntries(columns.map(({ column, index }) => [column, row.values[index] ?? null])),
  );
  return JSON.stringify(objects.length === 1 ? objects[0] : objects, null, 2);
}

/**
 * Check if the target element is a text editing shortcut target
 */
export function isTextEditingShortcutTarget(target: EventTarget | null): boolean {
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  if (target instanceof HTMLInputElement) {
    return !["button", "checkbox", "radio", "reset", "submit"].includes(target.type);
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
 * 用反引号引标识符的方言。取自 dbx-core `sql_dialect/identifiers.rs` 的
 * `quote_table_identifier`——那边这一组共用 backtick 分支。
 *
 * 为什么这份表只能抄一遍而不能去问后端:dbx-core 的 `connection_identifier_quote` 是
 * 异步且按连接实例走的,没有对应的 tauri command;而 `table_select.rs` 的
 * `uses_connection_identifier_quote` 只覆盖 Kingbase / Jdbc / Spanner / Informix /
 * Gaussdb / OpenGauss / Postgres,MySQL 这类照样回落到上面那张静态表。
 * 已逐项核对与静态表一致。
 *
 * 已知边界(当前不可达):dbx-core 对 Kingbase 优先用连接自报的引号,所以 MySQL 兼容
 * 模式下的 Kingbase 会与本表不符。前端没有 kingbase 这个 dbType,走不到。
 */
const BACKTICK_QUOTE_DB_TYPES = new Set([
  "mysql",
  "mariadb",
  "clickhouse",
  "doris",
  "starrocks",
  "goldendb",
  "manticoresearch",
  "hive",
  "kyuubi",
  "impala",
  "spark",
  "databricks",
  "databend",
  "tdengine",
  "access",
  "bigquery",
  "spanner",
  "questdb",
  "neo4j",
]);

/**
 * 按方言引一个标识符。
 *
 * 为什么不能一律用 `quoteSqlName` 的双引号:MySQL 默认没开 `ANSI_QUOTES`,`"id"` 是
 * **字符串常量**而不是列名。`ORDER BY "id" ASC` 因此让每行的排序键都是同一个常量 ——
 * SQL 能跑、不报错、顺序原样不动。表头排序点了没反应就是这么来的。
 *
 * 只用于前端需要自行拼 SQL 片段的地方(目前只有 ORDER BY)。WHERE 条件走后端的
 * `dbx_build_data_grid_context_filter_condition`,那条路本来就是方言感知的。
 *
 * 注意:**写入排序片段和回读它(判断当前升/降序)必须用同一支函数**,否则图标状态会
 * 与真实排序脱节。
 */
export function quoteSqlIdentifierForDbType(
  name: string,
  dbType: string | null | undefined,
): string {
  const normalized = dbType?.trim().toLowerCase() ?? "";
  if (BACKTICK_QUOTE_DB_TYPES.has(normalized)) {
    return `\`${name.replace(/`/g, "``")}\``;
  }
  if (normalized === "sqlserver") {
    return `[${name.replace(/]/g, "]]")}]`;
  }
  return quoteSqlName(name);
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
  return Math.min(
    DBX_GRID_MAX_COLUMN_WIDTH,
    Math.max(DBX_GRID_MIN_COLUMN_WIDTH, Math.round(width)),
  );
}

/**
 * Estimate a DBX grid column width based on content
 */
export function estimateDbxGridColumnWidth(
  column: string,
  columnIndex: number,
  rows: DatabaseRow[],
  columnType = "",
): number {
  const headerLength = Math.max(column.length, columnType.trim().length);
  const longestTextLength = rows.reduce((length, row) => {
    const text = valueToText(row.values[columnIndex]);
    return Math.max(length, Math.min(text.length, 60));
  }, headerLength);
  return clampDbxGridColumnWidth(
    longestTextLength * DBX_GRID_AUTOFIT_CHAR_WIDTH + DBX_GRID_AUTOFIT_PADDING,
  );
}

/**
 * Initialize DBX grid column widths
 */
export function initialDbxGridColumnWidths(
  columns: string[],
  rows: DatabaseRow[],
  columnTypes: string[] = [],
): Record<string, number> {
  return Object.fromEntries(
    columns.map((column, index) => [
      column,
      estimateDbxGridColumnWidth(column, index, rows, columnTypes[index] ?? ""),
    ]),
  );
}

/**
 * Check if a DBX grid column is sortable
 */
export function dbxGridColumnSortable(
  result: { columnSortables?: boolean[] } | null,
  columnIndex: number,
): boolean {
  const sortable = result?.columnSortables?.[columnIndex];
  return sortable === undefined ? true : sortable;
}

/**
 * Get the type of a DBX grid column
 */
export function dbxGridColumnType(
  result: { columnTypes?: string[] } | null,
  columnIndex: number,
): string | null {
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
    if (
      (trimmed.startsWith("{") && trimmed.endsWith("}")) ||
      (trimmed.startsWith("[") && trimmed.endsWith("]"))
    ) {
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
