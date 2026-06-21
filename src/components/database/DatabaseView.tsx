import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DragEvent, KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  ChevronLeft,
  ChevronRight,
  CheckSquare,
  Database,
  FileCode,
  FilePlus,
  Plus,
  Play,
  Plug,
  Shield,
  SlidersHorizontal,
  RefreshCcw,
  Server,
  Trash2,
  Wrench,
  GitCompare,
  GitMerge,
  Network,
  Pin,
  Table2,
  Copy,
  Download,
  Eraser,
  Eye,
  Search,
  Square,
  UsersRound,
} from "lucide-react";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DatabaseDriverManifest,
  DbxColumnInfo,
  DbxDatabaseType,
  DbxDatabaseInfo,
  DbxObjectInfo,
  DbxObjectSourceKind,
  DbxQueryResult,
  DbObject,
  DbQueryResult,
  DbSchema,
  DataGridContextFilterConditionOptions,
  DataGridContextFilterMode,
  DataGridCopyInsertStatementOptions,
  DataGridCopyUpdateStatementOptions,
  DataGridSaveStatementOptions,
  EditableStructureColumn,
  RedisDatabaseInfo,
  RedisKeyInfo,
  SshConnection,
  TableChildObjectType,
  TableExportRequest,
  TableImportMode,
  TableImportPreview,
} from "../../types";
import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import { DbxButton, DbxButtonGroup, DbxDialogFooterButton, DbxSegmentedButton } from "./DbxButton";
import {
  dbxGridRowsToTsv,
  dbxGridRowsToJson,
  isTextEditingShortcutTarget,
  isNullGridValue,
  valueToText,
  quoteSqlName,
  sqlLiteral,
  clampDbxGridColumnWidth,
  estimateDbxGridColumnWidth,
  initialDbxGridColumnWidths,
  dbxGridColumnSortable,
  dbxGridColumnType,
  textToCellValue,
  cellPreviewText,
  rowKeyFor,
  createConnectionName,
} from "../../lib/databaseUtils";
import s from "../../styles";
import { DatabaseAdvancedTools, type DatabaseAdvancedToolMode } from "./DatabaseAdvancedTools";
import { DatabaseSearchPanel } from "./DatabaseSearchPanel";
import { DatabaseSidebarTree } from "./DatabaseSidebarTree";
import { ErDiagramPanel } from "./ErDiagramPanel";
import { MongoBrowser } from "./MongoBrowser";
import { RedisBrowser } from "./RedisBrowser";
import { TableStructurePanel } from "./TableStructurePanel";
import { DatabaseUserAdminPanel, supportsDbxUserAdmin } from "./DatabaseUserAdminPanel";

interface Props {
  projectRoot?: string;
  remoteConnection?: SshConnection;
  remoteProjectPath?: string;
  sshConnections?: SshConnection[];
}

const PAGE_SIZE = 100;
const MONGO_SIDEBAR_DOCUMENT_PREVIEW_LIMIT = 20;
const DBX_GRID_PAGE_SIZE_OPTIONS = [50, 100, 200, 500, 1000] as const;
const DBX_GRID_DEFAULT_COLUMN_WIDTH = 180;
const EMPTY_DBX_COLUMNS: DbxColumnInfo[] = [];
type DatabaseRow = { rowId?: number | null; keyValues: Array<{ column: string; value: unknown }>; values: unknown[] };
type RedisSidebarScanState = { cursor: number; totalKeys: number };
type MongoSidebarDocumentQuery = { filter: string; sort: string };
type DbxObjectGroupKey = "tables" | "views" | "procedures" | "functions" | "sequences" | "packages";
type DbWizardStep = "type" | "config";
type DbConfigTab = "connection" | "tls" | "transport" | "advanced";
type DbWorkspaceMode =
  | "table"
  | "query"
  | "sql-file"
  | "drivers"
  | "query-history"
  | "redis"
  | "mongo"
  | "transfer"
  | "schema-diff"
  | "data-compare"
  | "user-admin"
  | "er-diagram"
  | "database-search"
  | "table-structure"
  | "table-info"
  | "object-browser"
  | "field-lineage";
type DatabaseContextMenuState = {
  x: number;
  y: number;
  connectionId: string;
  kind: "legacy" | "dbx";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string | null;
  object: DbxObjectInfo;
  kind: "dbx-object";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string;
  kind: "dbx-database";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string;
  schema: string;
  kind: "dbx-schema";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string | null;
  object: DbxObjectInfo;
  column: DbxColumnInfo;
  kind: "dbx-column";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string | null;
  object: DbxObjectInfo;
  childObject: DbxObjectInfo;
  childObjectType: Exclude<TableChildObjectType, "COLUMN">;
  kind: "dbx-table-child";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string;
  schema: string | null;
  groupKey: DbxObjectGroupKey;
  label: string;
  kind: "dbx-object-group";
} | {
  x: number;
  y: number;
  groupName: string;
  kind: "connection-group";
} | {
  x: number;
  y: number;
  connectionId: string;
  kind: "user-admin";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: number;
  kind: "redis-database";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: number;
  keyRaw: string;
  kind: "redis-key";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string;
  kind: "mongo-database";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string;
  collection: string;
  kind: "mongo-collection";
} | {
  x: number;
  y: number;
  connectionId: string;
  database: string;
  collection: string;
  document: unknown;
  kind: "mongo-document";
} | {
  x: number;
  y: number;
  connectionId: string;
  columnIndex: number;
  column: string;
  kind: "dbx-grid-header";
} | {
  x: number;
  y: number;
  connectionId: string;
  rowIndex: number;
  columnIndex: number;
  column: string;
  value: unknown;
  kind: "dbx-grid-cell";
} | null;
type DbxContextMenuItem<Action extends string> = [action: Action, labelKey: string];
type DbxGridCellContextMenuAction =
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
type DbxGridHeaderContextMenuAction =
  | "copyColumnName"
  | "previewColumn"
  | "copyAlterColumnSql"
  | "sortAscending"
  | "sortDescending"
  | "clearSort";
type DbxDatabaseContextMenuAction =
  | "copyName"
  | "togglePin"
  | "openObjectBrowser"
  | "newQuery"
  | "queryHistory"
  | "setDefaultDatabase"
  | "clearDefaultDatabase"
  | "createTable"
  | "createSchema"
  | "executeSqlFile"
  | "openErDiagram"
  | "databaseSearch"
  | "refresh"
  | "dataTransfer"
  | "schemaDiff"
  | "dataCompare"
  | "exportDatabase"
  | "closeDatabaseConnection"
  | "dropDatabase";
type DbxSchemaContextMenuAction =
  | "copyName"
  | "togglePin"
  | "openObjectBrowser"
  | "newQuery"
  | "queryHistory"
  | "createTable"
  | "executeSqlFile"
  | "openErDiagram"
  | "databaseSearch"
  | "refresh"
  | "dataTransfer"
  | "schemaDiff"
  | "dataCompare"
  | "exportDatabase"
  | "dropSchema";
type NoSqlContextMenuAction =
  | "copyName"
  | "newQuery"
  | "openWorkspace"
  | "refresh"
  | "deleteDocument"
  | "deleteRedisKey"
  | "flushRedisDb"
  | "setDefaultDatabase"
  | "clearDefaultDatabase"
  | "togglePin";
type NoSqlDatabaseContextMenuState = Extract<
  NonNullable<DatabaseContextMenuState>,
  { kind: "redis-database" | "mongo-database" }
>;
type DbxGridCellContextMenuState = Extract<
  NonNullable<DatabaseContextMenuState>,
  { kind: "dbx-grid-cell" }
>;
type DbxGridHeaderContextMenuState = Extract<
  NonNullable<DatabaseContextMenuState>,
  { kind: "dbx-grid-header" }
>;
type TableInfoTab = "columns" | "indexes" | "foreignKeys" | "triggers" | "ddl";
type DbxObjectContextMenuAction =
  | "copyName"
  | "togglePin"
  | "viewData"
  | "editView"
  | "viewSource"
  | "executeProcedure"
  | "editStructure"
  | "renameObject"
  | "viewDdl"
  | "tableInfo"
  | "newQuery"
  | "newSqlSelect"
  | "newSqlInsert"
  | "newSqlUpdate"
  | "queryHistory"
  | "openErDiagram"
  | "importData"
  | "dataCompare"
  | "exportCsv"
  | "exportJson"
  | "exportMarkdown"
  | "exportInsertSql"
  | "exportUpdateSql"
  | "exportXlsx"
  | "exportDatabase"
  | "exportStructure"
  | "copyStructureTsv"
  | "copyStructureMarkdown"
  | "copyStructureDdl"
  | "duplicateStructure"
  | "emptyTable"
  | "truncateTable"
  | "dropTable"
  | "dropObject"
  | "refresh";
type DbxObjectContextMenuItem = [action: DbxObjectContextMenuAction, labelKey: string];

const PINNED_TREE_NODE_IDS_STORAGE_KEY = "aeroric:database:pinned-nosql-tree-nodes";
const EXTRA_DBX_CONNECTION_GROUPS_STORAGE_KEY = "aeroric:database:extra-dbx-connection-groups";

function loadPinnedTreeNodeIds() {
  if (typeof window === "undefined") return new Set<string>();
  try {
    const parsed = JSON.parse(window.localStorage.getItem(PINNED_TREE_NODE_IDS_STORAGE_KEY) ?? "[]");
    return new Set(Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === "string") : []);
  } catch {
    return new Set<string>();
  }
}

function savePinnedTreeNodeIds(ids: Set<string>) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(PINNED_TREE_NODE_IDS_STORAGE_KEY, JSON.stringify([...ids]));
}

function loadExtraDbxConnectionGroups() {
  if (typeof window === "undefined") return [];
  try {
    const parsed = JSON.parse(window.localStorage.getItem(EXTRA_DBX_CONNECTION_GROUPS_STORAGE_KEY) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.filter((value): value is string => typeof value === "string" && value.trim().length > 0)
      : [];
  } catch {
    return [];
  }
}

function saveExtraDbxConnectionGroups(groups: string[]) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(EXTRA_DBX_CONNECTION_GROUPS_STORAGE_KEY, JSON.stringify(groups));
}

function contextMenuPinnedNodeId(menu: DatabaseContextMenuState): string | null {
  if (!menu) return null;
  if (menu.kind === "dbx-database") return `dbx-database:${menu.database}`;
  if (menu.kind === "dbx-schema") return `dbx-schema:${menu.database}:${menu.schema}`;
  if (menu.kind === "dbx-object" && (isDbxTableObject(menu.object) || isDbxViewObject(menu.object))) {
    return `dbx-object:${normalizeDbxObjectType(menu.object.object_type)}:${menu.object.schema ?? ""}:${menu.object.name}`;
  }
  if (menu.kind === "redis-database") return `redis-database:${menu.connectionId}:${menu.database}`;
  if (menu.kind === "mongo-database") return `mongo-database:${menu.connectionId}:${menu.database}`;
  if (menu.kind === "mongo-collection") return `mongo-collection:${menu.connectionId}:${menu.database}:${menu.collection}`;
  return null;
}

type DbProfileKey =
  | "sqlite"
  | "mysql"
  | "postgres"
  | "redis"
  | "mongodb"
  | "sqlserver"
  | "oracle"
  | "duckdb"
  | "clickhouse";
type DbProfileViewMode = "icon" | "list";
type QueryHistoryEntry = {
  id: string;
  sql: string;
  connectionName: string;
  database: string | null;
  schema: string | null;
  executedAt: number;
  rowsAffected?: number | null;
  executionTimeMs?: number | null;
};

interface DbProfile {
  key: DbProfileKey;
  label: string;
  accent: string;
  port: number;
  user: string;
  localFile?: boolean;
  iconText: string;
}

const DB_PROFILES: DbProfile[] = [
  { key: "sqlite", label: "SQLite", accent: "#3f8fce", port: 0, user: "", localFile: true, iconText: "S" },
  { key: "mysql", label: "MySQL", accent: "#2f6f9f", port: 3306, user: "root", iconText: "My" },
  { key: "postgres", label: "PostgreSQL", accent: "#336791", port: 5432, user: "postgres", iconText: "Pg" },
  { key: "redis", label: "Redis", accent: "#d82c20", port: 6379, user: "", iconText: "R" },
  { key: "mongodb", label: "MongoDB", accent: "#13aa52", port: 27017, user: "", iconText: "M" },
  { key: "sqlserver", label: "SQL Server", accent: "#cc2927", port: 1433, user: "sa", iconText: "MS" },
  { key: "oracle", label: "Oracle", accent: "#f80000", port: 1521, user: "system", iconText: "O" },
  { key: "duckdb", label: "DuckDB", accent: "#b68b00", port: 0, user: "", localFile: true, iconText: "D" },
  { key: "clickhouse", label: "ClickHouse", accent: "#d6a700", port: 8123, user: "default", iconText: "C" },
];

const MYSQL_COMPATIBLE_CREATE_DATABASE_PROFILES = new Set(["mysql", "mariadb", "tidb", "oceanbase", "doris", "starrocks", "custom_mysql"]);
const CREATE_DATABASE_DB_TYPES = new Set(["mysql", "postgres", "sqlserver", "oracle", "clickhouse", "duckdb"]);
const DBX_DATABASE_CREATE_NODE_DB_TYPES = new Set<DbxDatabaseType>(["mysql", "postgres", "sqlserver", "oracle", "clickhouse"]);
const DBX_TABLE_STRUCTURE_DB_TYPES = new Set<DbxDatabaseType>(["sqlite", "mysql", "postgres", "duckdb", "sqlserver", "oracle", "clickhouse"]);
const DBX_SCHEMA_AWARE_DB_TYPES = new Set<DbxDatabaseType>(["postgres", "sqlserver", "oracle", "duckdb"]);
const DBX_TREE_SCHEMA_DB_TYPES = new Set<DbxDatabaseType>(["postgres", "sqlserver", "duckdb"]);
const DBX_DIAGRAM_DB_TYPES = new Set<DbxDatabaseType>(["sqlite", "mysql", "postgres", "sqlserver", "oracle"]);
const DBX_NO_TRUNCATE_DB_TYPES = new Set<string>(["sqlite", "rqlite", "turso", "duckdb", "influxdb", "manticoresearch"]);
const SYSTEM_DATABASE_NAMES: Partial<Record<DbxDatabaseType, ReadonlySet<string>>> = {
  mysql: new Set(["information_schema", "mysql", "performance_schema", "sys"]),
  postgres: new Set(["template0", "template1"]),
  clickhouse: new Set(["information_schema", "system"]),
  sqlserver: new Set(["master", "model", "msdb", "tempdb"]),
  mongodb: new Set(["admin", "config", "local"]),
};
type TableExportFormat = "csv" | "json" | "markdown" | "insertSql" | "updateSql" | "xlsx";
type ParsedConnectionUrl = {
  host?: string;
  port?: string;
  user?: string;
  password?: string;
  database?: string;
  urlParams?: string;
};
type RedisConnectionMode = "standalone" | "sentinel" | "cluster";
type TransportLayerDraft = {
  id: string;
  type: "ssh" | "proxy";
  enabled: boolean;
  name: string;
  host: string;
  port: string;
  user: string;
  username: string;
  password: string;
  keyPath: string;
  keyPassphrase: string;
  connectTimeoutSecs: string;
  exposeLan: boolean;
  useSshAgent: boolean;
  proxyType: "socks5" | "http";
};

const CONNECTION_COLOR_OPTIONS = [
  { value: "", labelKey: "database.colorNone" },
  { value: "#22c55e", labelKey: "database.colorGreen" },
  { value: "#eab308", labelKey: "database.colorYellow" },
  { value: "#f97316", labelKey: "database.colorOrange" },
  { value: "#ef4444", labelKey: "database.colorRed" },
  { value: "#3b82f6", labelKey: "database.colorBlue" },
  { value: "#a855f7", labelKey: "database.colorPurple" },
] as const;

function DatabaseProfileIcon({ profile, size = 23 }: { profile: DbProfile; size?: number }) {
  return (
    <span
      aria-hidden="true"
      style={{
        width: size,
        height: size,
        borderRadius: profile.key === "redis" ? 5 : profile.key === "mongodb" ? 999 : 7,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        background: `${profile.accent}22`,
        border: `1px solid ${profile.accent}66`,
        color: profile.accent,
        fontSize: Math.max(9, size * 0.42),
        fontWeight: 800,
        fontFamily: "var(--font-ui)",
        lineHeight: 1,
      }}
    >
      {profile.iconText}
    </span>
  );
}

function endpointLabel(endpoint: DbEndpoint): string {
  if (endpoint.kind === "local") return endpoint.path;
  return `${endpoint.connection.name}: ${endpoint.path}`;
}

export function dbxColumnInfoToEditableStructureColumn(column: DbxColumnInfo, originalPosition: number): EditableStructureColumn {
  return {
    id: `existing:${column.name}`,
    name: column.name,
    dataType: column.data_type,
    isNullable: column.is_nullable,
    defaultValue: column.column_default ?? "",
    comment: column.comment ?? "",
    isPrimaryKey: column.is_primary_key,
    originalPosition,
    original: {
      name: column.name,
      data_type: "",
      is_nullable: !column.is_nullable,
      column_default: null,
      is_primary_key: column.is_primary_key,
      extra: null,
      comment: null,
    },
    markedForDrop: false,
  };
}

function dbxRowsToDatabaseRows(rows: unknown[][]) {
  return rows.map((row) => ({ rowId: null, keyValues: [], values: row }));
}

function dbxColumnsToDbColumns(columns: DbxColumnInfo[]) {
  return columns.map((column, index) => ({
    name: column.name,
    dataType: column.data_type,
    nullable: column.is_nullable,
    notNull: !column.is_nullable,
    primaryKey: column.is_primary_key,
    primaryKeyOrdinal: column.is_primary_key ? index + 1 : 0,
    defaultValue: column.column_default,
  }));
}

function dbxQueryToExecuteResult(result: DbxQueryResult, message?: string): DbExecuteResult {
  return {
    columns: result.columns,
    rows: dbxRowsToDatabaseRows(result.rows),
    rowsAffected: result.affected_rows,
    message: message ?? `${result.affected_rows} affected · ${result.execution_time_ms} ms`,
  };
}

function dbxSqlFileResultsToExecuteResult(results: DbxQueryResult[]): DbExecuteResult {
  const last = results[results.length - 1] ?? null;
  const statementCount = results.length;
  return {
    columns: last?.columns ?? [],
    rows: dbxRowsToDatabaseRows(last?.rows ?? []),
    rowsAffected: results.reduce((sum, item) => sum + (item.affected_rows ?? 0), 0),
    message: `${statementCount} statement${statementCount === 1 ? "" : "s"} executed`,
  };
}

function isSqlDbxConnection(connection: AeroricDbConnectionConfig | null | undefined) {
  return Boolean(connection && !["redis", "mongodb"].includes(connection.dbType));
}

function configuredTargetDatabase(connection: AeroricDbConnectionConfig | null | undefined): string | null {
  if (!connection?.dbx || typeof connection.dbx !== "object") return null;
  const database = (connection.dbx as { database?: unknown }).database;
  return typeof database === "string" && database.trim() ? database.trim() : null;
}

function isDbxDefaultDatabase(connection: AeroricDbConnectionConfig | null | undefined, database: string): boolean {
  return configuredTargetDatabase(connection) === database;
}

function configuredVisibleDatabases(connection: AeroricDbConnectionConfig | null | undefined): string[] | undefined {
  if (!connection?.dbx || typeof connection.dbx !== "object") return undefined;
  const configured = (connection.dbx as { visible_databases?: unknown }).visible_databases;
  if (!Array.isArray(configured)) return undefined;
  return configured.filter((name): name is string => typeof name === "string" && name.trim().length > 0);
}

function isSystemDatabaseName(databaseType: DbxDatabaseType | undefined, name: string) {
  if (!databaseType) return false;
  return SYSTEM_DATABASE_NAMES[databaseType]?.has(name.toLowerCase()) ?? false;
}

function filterDbxDatabasesForConnection(databases: DbxDatabaseInfo[], connection: AeroricDbConnectionConfig): DbxDatabaseInfo[] {
  const targetDatabase = configuredTargetDatabase(connection);
  if (targetDatabase) return databases.filter((database) => database.name === targetDatabase);

  const visibleDatabases = configuredVisibleDatabases(connection);
  if (visibleDatabases) {
    const visible = new Set(visibleDatabases);
    return databases.filter((database) => visible.has(database.name));
  }

  return databases.filter((database) => !isSystemDatabaseName(connection.dbType, database.name));
}

function normalizeVisibleDatabaseSelection(selectedNames: string[], databaseNames: string[]): string[] {
  const available = new Set(databaseNames);
  const seen = new Set<string>();
  return selectedNames.filter((name) => {
    if (!available.has(name) || seen.has(name)) return false;
    seen.add(name);
    return true;
  });
}

function autoMapImportColumns(sourceColumns: string[], targetColumns: string[]): Record<string, string> {
  const targetByLower = new Map(targetColumns.map((column) => [column.toLowerCase(), column]));
  return Object.fromEntries(
    sourceColumns.map((sourceColumn) => [sourceColumn, targetByLower.get(sourceColumn.toLowerCase()) ?? ""]),
  );
}

function normalizeRedisNodeList(value: string): string {
  return value
    .split(/[\n,]+/)
    .map((node) => node.trim())
    .filter(Boolean)
    .join("\n");
}

function firstRedisEndpoint(nodes: string, defaultPort: number): { host: string; port: number } | null {
  const first = normalizeRedisNodeList(nodes).split("\n")[0]?.trim();
  if (!first) return null;
  const match = first.match(/^\[([^\]]+)\](?::(\d+))?$/) ?? first.match(/^([^:]+)(?::(\d+))?$/);
  if (!match) return null;
  const host = match[1]?.trim();
  if (!host) return null;
  const port = Number.parseInt(match[2] ?? "", 10);
  return { host, port: Number.isFinite(port) && port > 0 ? port : defaultPort };
}

function createTransportLayerDraft(type: "ssh" | "proxy", index: number): TransportLayerDraft {
  return {
    id: `transport:${Date.now()}:${Math.random().toString(36).slice(2)}`,
    type,
    enabled: true,
    name: type === "ssh" ? `SSH ${index}` : `Proxy ${index}`,
    host: "",
    port: type === "ssh" ? "22" : "1080",
    user: "",
    username: "",
    password: "",
    keyPath: "",
    keyPassphrase: "",
    connectTimeoutSecs: "5",
    exposeLan: false,
    useSshAgent: false,
    proxyType: "socks5",
  };
}

function transportLayerPayload(layer: TransportLayerDraft): Record<string, unknown> {
  const port = Number.parseInt(layer.port, 10);
  const connectTimeoutSecs = Number.parseInt(layer.connectTimeoutSecs, 10);
  if (layer.type === "proxy") {
    return {
      id: layer.id,
      type: "proxy",
      enabled: layer.enabled,
      name: layer.name.trim(),
      proxy_type: layer.proxyType,
      host: layer.host.trim(),
      port: Number.isFinite(port) && port > 0 ? port : 1080,
      username: layer.username.trim(),
      password: layer.password,
    };
  }
  return {
    id: layer.id,
    type: "ssh",
    enabled: layer.enabled,
    name: layer.name.trim(),
    host: layer.host.trim(),
    port: Number.isFinite(port) && port > 0 ? port : 22,
    user: layer.user.trim(),
    password: layer.password,
    key_path: layer.keyPath.trim(),
    key_passphrase: layer.keyPassphrase,
    connect_timeout_secs: Number.isFinite(connectTimeoutSecs) && connectTimeoutSecs > 0 ? connectTimeoutSecs : 5,
    expose_lan: layer.exposeLan,
    use_ssh_agent: layer.useSshAgent,
  };
}

function dbxConfigRecord(connection: AeroricDbConnectionConfig | null | undefined): Record<string, unknown> {
  return connection?.dbx && typeof connection.dbx === "object" ? (connection.dbx as Record<string, unknown>) : {};
}

function dbxString(config: Record<string, unknown>, key: string, fallback = ""): string {
  const value = config[key];
  return typeof value === "string" ? value : fallback;
}

function dbxNumberString(config: Record<string, unknown>, key: string, fallback: string): string {
  const value = config[key];
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  if (typeof value === "string" && value.trim()) return value;
  return fallback;
}

function dbxBoolean(config: Record<string, unknown>, key: string, fallback = false): boolean {
  const value = config[key];
  return typeof value === "boolean" ? value : fallback;
}

function hasEnabledDbxTransportLayers(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  const layers = dbxConfigRecord(connection).transport_layers;
  return Array.isArray(layers) && layers.some((layer) => {
    if (!layer || typeof layer !== "object") return false;
    return (layer as { enabled?: unknown }).enabled !== false;
  });
}

function dbxConnectionFinalProxyPort(connection: AeroricDbConnectionConfig | null | undefined): number | null {
  if (!connection) return null;
  const config = dbxConfigRecord(connection);
  const value = config.final_proxy_port ?? config.finalProxyPort;
  const port = typeof value === "number" ? value : typeof value === "string" ? Number.parseInt(value, 10) : NaN;
  return Number.isFinite(port) && port > 0 ? port : null;
}

function dbxConnectionLocalFilePath(connection: AeroricDbConnectionConfig | null | undefined): string | null {
  if (!connection || (connection.dbType !== "sqlite" && connection.dbType !== "duckdb")) return null;
  const path = dbxString(dbxConfigRecord(connection), "host").trim();
  if (!path || path === ":memory:") return null;
  return path;
}

function sqliteBackupSourcePath(connection: AeroricDbConnectionConfig | null | undefined): string | null {
  if (!connection || connection.dbType !== "sqlite") return null;
  return dbxConnectionLocalFilePath(connection);
}

function defaultSqliteBackupFileName(connection: AeroricDbConnectionConfig): string {
  const source = sqliteBackupSourcePath(connection) || connection.name || "database.db";
  const rawFileName = source.split(/[\\/]/).filter(Boolean).pop() || "database.db";
  const fileName =
    rawFileName
      .split("")
      .map((char) => (char.charCodeAt(0) < 32 || /[<>:"/\\|?*]/.test(char) ? "_" : char))
      .join("")
      .trim()
      .replace(/[. ]+$/g, "") || "database.db";
  const dotIndex = fileName.lastIndexOf(".");
  return dotIndex <= 0 ? `${fileName}.backup.db` : `${fileName.slice(0, dotIndex)}.backup${fileName.slice(dotIndex)}`;
}

function profileForDbxConnection(connection: AeroricDbConnectionConfig): DbProfile {
  const config = dbxConfigRecord(connection);
  const driverProfile = dbxString(config, "driver_profile");
  return (
    DB_PROFILES.find((profile) => profile.key === driverProfile) ??
    DB_PROFILES.find((profile) => profile.key === connection.dbType) ??
    DB_PROFILES[0]
  );
}

function transportLayerDraftFromPayload(value: unknown, index: number): TransportLayerDraft {
  const layer = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const type = layer.type === "proxy" ? "proxy" : "ssh";
  return {
    id: dbxString(layer, "id", `transport:existing:${index}`),
    type,
    enabled: dbxBoolean(layer, "enabled", true),
    name: dbxString(layer, "name", type === "ssh" ? `SSH ${index}` : `Proxy ${index}`),
    host: dbxString(layer, "host"),
    port: dbxNumberString(layer, "port", type === "ssh" ? "22" : "1080"),
    user: dbxString(layer, "user"),
    username: dbxString(layer, "username"),
    password: dbxString(layer, "password"),
    keyPath: dbxString(layer, "key_path"),
    keyPassphrase: dbxString(layer, "key_passphrase"),
    connectTimeoutSecs: dbxNumberString(layer, "connect_timeout_secs", "5"),
    exposeLan: dbxBoolean(layer, "expose_lan"),
    useSshAgent: dbxBoolean(layer, "use_ssh_agent"),
    proxyType: layer.proxy_type === "http" ? "http" : "socks5",
  };
}

function normalizeConnectionUrl(raw: string, dbType: DbxDatabaseType): string {
  const value = raw.trim();
  if (!value) return "";
  if (value.startsWith("jdbc:")) {
    if (dbType === "postgres" && value.startsWith("jdbc:postgresql:")) return value.slice("jdbc:".length);
    if (dbType === "mysql" && value.startsWith("jdbc:mysql:")) return value.slice("jdbc:".length);
    if (dbType === "clickhouse" && value.startsWith("jdbc:clickhouse:")) return value.slice("jdbc:".length);
    if (dbType === "sqlserver" && value.startsWith("jdbc:sqlserver:")) return value.slice("jdbc:".length);
    if (dbType === "oracle" && value.startsWith("jdbc:oracle:")) return value;
  }
  return value;
}

function parseSqlServerConnectionUrl(raw: string): ParsedConnectionUrl | null {
  const normalized = normalizeConnectionUrl(raw, "sqlserver");
  const match = normalized.match(/^sqlserver:\/\/([^;/?]+)(.*)$/i);
  if (!match) return null;
  const hostPort = match[1] ?? "";
  const rest = match[2] ?? "";
  const [host, port] = hostPort.split(":");
  const params = Object.fromEntries(
    rest
      .split(";")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const separatorIndex = part.indexOf("=");
        return separatorIndex >= 0 ? [part.slice(0, separatorIndex), part.slice(separatorIndex + 1)] : [part, ""];
      }),
  );
  const database = params.databaseName || params.database || params.initialCatalog;
  const user = params.user || params.username;
  const password = params.password;
  const urlParams = Object.entries(params)
    .filter(([key]) => !["databaseName", "database", "initialCatalog", "user", "username", "password"].includes(key))
    .map(([key, value]) => `${key}=${value}`)
    .join(";");
  return {
    host: host ? decodeURIComponent(host) : undefined,
    port,
    database,
    user,
    password,
    urlParams,
  };
}

function parseOracleConnectionUrl(raw: string): ParsedConnectionUrl | null {
  const value = raw.trim();
  const serviceMatch = value.match(/^jdbc:oracle:thin:@\/\/([^:/?#]+)(?::(\d+))?\/([^?#]+)(?:\?(.+))?$/i);
  if (serviceMatch) {
    return {
      host: decodeURIComponent(serviceMatch[1] ?? ""),
      port: serviceMatch[2],
      database: serviceMatch[3] ? decodeURIComponent(serviceMatch[3]) : undefined,
      urlParams: serviceMatch[4],
    };
  }
  const sidMatch = value.match(/^jdbc:oracle:thin:@([^:/?#]+)(?::(\d+))?:([^?#]+)(?:\?(.+))?$/i);
  if (!sidMatch) return null;
  return {
    host: decodeURIComponent(sidMatch[1] ?? ""),
    port: sidMatch[2],
    database: sidMatch[3] ? decodeURIComponent(sidMatch[3]) : undefined,
    urlParams: sidMatch[4],
  };
}

function parseStandardConnectionUrl(raw: string, dbType: DbxDatabaseType): ParsedConnectionUrl | null {
  const normalized = normalizeConnectionUrl(raw, dbType);
  const parsed = new URL(normalized);
  const database = parsed.pathname.replace(/^\/+/, "");
  return {
    host: parsed.hostname ? decodeURIComponent(parsed.hostname) : undefined,
    port: parsed.port,
    user: parsed.username ? decodeURIComponent(parsed.username) : undefined,
    password: parsed.password ? decodeURIComponent(parsed.password) : undefined,
    database: database ? decodeURIComponent(database) : undefined,
    urlParams: parsed.searchParams.toString(),
  };
}

function parseConnectionUrl(raw: string, dbType: DbxDatabaseType): ParsedConnectionUrl | null {
  const value = raw.trim();
  if (!value) return null;
  try {
    if (dbType === "sqlserver") return parseSqlServerConnectionUrl(value);
    if (dbType === "oracle") return parseOracleConnectionUrl(value);
    return parseStandardConnectionUrl(value, dbType);
  } catch {
    return null;
  }
}

function dbxDriverProfile(connection: AeroricDbConnectionConfig | null | undefined): string | null {
  if (!connection?.dbx || typeof connection.dbx !== "object") return null;
  const profile = (connection.dbx as { driver_profile?: unknown }).driver_profile;
  return typeof profile === "string" && profile.trim() ? profile.trim() : null;
}

function canCreateDatabaseForConnection(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && CREATE_DATABASE_DB_TYPES.has(connection.dbType));
}

function canSetCreateDatabaseCharset(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  if (!connection) return false;
  return connection.dbType === "mysql" || MYSQL_COMPATIBLE_CREATE_DATABASE_PROFILES.has(dbxDriverProfile(connection) ?? "");
}

function ensureDuckDbFileExtension(path: string): string {
  return /\.(duckdb|db)$/i.test(path) ? path : `${path}.duckdb`;
}

function duckDbAttachedDatabaseNameFromPath(path: string): string {
  const fileName = path.split(/[\\/]/).pop() ?? path;
  const withoutExtension = fileName.replace(/\.(duckdb|db)$/i, "");
  const normalized = withoutExtension.trim().replace(/[^A-Za-z0-9_]+/g, "_").replace(/^_+|_+$/g, "");
  return normalized || "duckdb_database";
}

function uniqueDuckDbAttachedDatabaseName(baseName: string, existingNames: string[]): string {
  const normalizedExisting = new Set(existingNames.map((name) => name.toLowerCase()));
  if (!normalizedExisting.has(baseName.toLowerCase())) return baseName;
  for (let index = 2; ; index += 1) {
    const candidate = `${baseName}_${index}`;
    if (!normalizedExisting.has(candidate.toLowerCase())) return candidate;
  }
}

function dbxAttachedDatabaseRecords(connection: AeroricDbConnectionConfig): Array<Record<string, unknown>> {
  if (!connection.dbx || typeof connection.dbx !== "object") return [];
  const attachedDatabases = (connection.dbx as { attached_databases?: unknown }).attached_databases;
  return Array.isArray(attachedDatabases)
    ? attachedDatabases.filter((database): database is Record<string, unknown> => Boolean(database && typeof database === "object"))
    : [];
}

function dbxObjectKey(object: DbxObjectInfo) {
  return object.schema ? `${object.schema}.${object.name}` : object.name;
}

function dbxCreateTableDraft(schema: string | null): string {
  const prefix = schema ? `${schema}.` : "";
  return `CREATE TABLE ${prefix}table_name (\n  id INTEGER PRIMARY KEY,\n  name TEXT NOT NULL\n);`;
}

function dbxCreateViewDraft(schema: string | null): string {
  const viewName = schema ? `${schema}.new_view` : "new_view";
  return `CREATE VIEW ${viewName} AS\nSELECT\n  *\nFROM table_name;\n`;
}

function mongoDocumentId(document: unknown, fallback = 0) {
  if (document && typeof document === "object" && "_id" in document) return String((document as { _id: unknown })._id);
  return `#${fallback + 1}`;
}

function mongoDocumentRawId(document: unknown): unknown | null {
  if (!document || typeof document !== "object" || !("_id" in document)) return null;
  return (document as { _id: unknown })._id;
}

function deriveDbxSchemas(objects: DbxObjectInfo[]): string[] {
  return Array.from(
    new Set(
      objects
        .map((object) => object.schema?.trim() ?? "")
        .filter(Boolean),
    ),
  ).sort((left, right) => left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }));
}

function isDbxTableObject(object: DbxObjectInfo | null | undefined): boolean {
  return object?.object_type?.toLowerCase() === "table";
}

function isDbxViewObject(object: DbxObjectInfo | null | undefined): boolean {
  return object?.object_type?.toLowerCase() === "view";
}

function normalizeDbxObjectType(objectType: string) {
  return objectType.toUpperCase().replace(/[\s-]+/g, "_");
}

function dbxTableChildObjectType(object: DbxObjectInfo): Exclude<TableChildObjectType, "COLUMN"> | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType.includes("FOREIGN_KEY")) return "FOREIGN_KEY";
  if (objectType.includes("TRIGGER")) return "TRIGGER";
  if (objectType.includes("INDEX")) return "INDEX";
  return null;
}

function sameDbxName(left: string | null | undefined, right: string | null | undefined) {
  return (left ?? "").trim().toLowerCase() === (right ?? "").trim().toLowerCase();
}

function uniqueDbxObjectName(baseName: string, schema: string | null | undefined, objects: DbxObjectInfo[]) {
  const normalizedExisting = new Set(
    objects
      .filter((object) => sameDbxName(object.schema, schema))
      .map((object) => object.name.trim().toLowerCase())
      .filter(Boolean),
  );
  if (!normalizedExisting.has(baseName.toLowerCase())) return baseName;
  for (let index = 2; ; index += 1) {
    const candidate = `${baseName}_${index}`;
    if (!normalizedExisting.has(candidate.toLowerCase())) return candidate;
  }
}

function dbxChildObjectBelongsToTable(childObject: DbxObjectInfo, tableObject: DbxObjectInfo) {
  if (!childObject.parent_name || !sameDbxName(childObject.parent_name, tableObject.name)) return false;
  if (!tableObject.schema) return true;
  return !childObject.parent_schema || sameDbxName(childObject.parent_schema, tableObject.schema);
}

function dbxTableChildDropLabelKey(objectType: TableChildObjectType) {
  if (objectType === "INDEX") return "database.dropIndex";
  if (objectType === "FOREIGN_KEY") return "database.dropForeignKey";
  if (objectType === "TRIGGER") return "database.dropTrigger";
  return "database.dropColumn";
}

function dbxObjectSourceKind(object: DbxObjectInfo): DbxObjectSourceKind | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType === "VIEW" || objectType.includes("VIEW")) return "VIEW";
  if (objectType.includes("PROCEDURE")) return "PROCEDURE";
  if (objectType.includes("FUNCTION")) return "FUNCTION";
  if (objectType.includes("SEQUENCE")) return "SEQUENCE";
  if (objectType === "PACKAGE_BODY" || objectType.includes("PACKAGE_BODY")) return "PACKAGE_BODY";
  if (objectType.includes("PACKAGE")) return "PACKAGE";
  return null;
}

type DbxRenameObjectType = "TABLE" | "VIEW" | "PROCEDURE" | "FUNCTION";

function dbxObjectRenameType(object: DbxObjectInfo): DbxRenameObjectType | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType === "TABLE" || objectType.includes("TABLE")) return "TABLE";
  if (objectType === "VIEW" || objectType.includes("VIEW")) return "VIEW";
  if (objectType.includes("PROCEDURE")) return "PROCEDURE";
  if (objectType.includes("FUNCTION")) return "FUNCTION";
  return null;
}

function canRenameDbxObject(connection: AeroricDbConnectionConfig | null | undefined, object: DbxObjectInfo | null | undefined): boolean {
  if (!connection || !object || connection.readOnly) return false;
  const objectType = dbxObjectRenameType(object);
  if (!objectType) return false;
  if (connection.dbType === "sqlserver") return true;
  if (connection.dbType === "sqlite") return objectType === "TABLE";
  if (connection.dbType === "mysql" || connection.dbType === "postgres" || connection.dbType === "oracle") {
    return objectType === "TABLE" || objectType === "VIEW";
  }
  return false;
}

function supportsDbxObjectBrowserTreeNode(
  connection: AeroricDbConnectionConfig | null | undefined,
  nodeType: "database" | "schema",
): boolean {
  if (!connection || !isSqlDbxConnection(connection)) return false;
  if (nodeType === "database" && DBX_SCHEMA_AWARE_DB_TYPES.has(connection.dbType) && connection.dbType !== "sqlserver") {
    return false;
  }
  return true;
}

function supportsDbxTableStructureEditing(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && DBX_TABLE_STRUCTURE_DB_TYPES.has(connection.dbType));
}

function supportsDbxSqlFileExecution(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && isSqlDbxConnection(connection));
}

function supportsDbxDiagram(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && DBX_DIAGRAM_DB_TYPES.has(connection.dbType));
}

function supportsDbxDatabaseSearch(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && isSqlDbxConnection(connection));
}

function canCreateDbxSchema(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && DBX_TREE_SCHEMA_DB_TYPES.has(connection.dbType));
}

function canDropDbxDatabaseNode(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && DBX_DATABASE_CREATE_NODE_DB_TYPES.has(connection.dbType));
}

function canDropDbxSchemaNode(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return canCreateDbxSchema(connection);
}

function supportsDbxTableTruncate(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && !DBX_NO_TRUNCATE_DB_TYPES.has(connection.dbType));
}

function isDbxRoutineLikeObject(object: DbxObjectInfo | null | undefined): boolean {
  if (!object) return false;
  const sourceKind = dbxObjectSourceKind(object);
  return sourceKind !== null && sourceKind !== "VIEW";
}

function isDbxProcedureObject(object: DbxObjectInfo | null | undefined): boolean {
  return object ? dbxObjectSourceKind(object) === "PROCEDURE" : false;
}

function isDbxDroppableRoutineObject(object: DbxObjectInfo | null | undefined): boolean {
  const sourceKind = object ? dbxObjectSourceKind(object) : null;
  return sourceKind === "PROCEDURE" || sourceKind === "FUNCTION";
}

function dbxDatabaseContextMenuItems(
  connection: AeroricDbConnectionConfig | null | undefined,
  database: string,
  pinned: boolean,
): DbxContextMenuItem<DbxDatabaseContextMenuAction>[] {
  const items: DbxContextMenuItem<DbxDatabaseContextMenuAction>[] = [];
  const add = (action: DbxDatabaseContextMenuAction, labelKey: string) => items.push([action, labelKey]);
  add("togglePin", pinned ? "database.unpin" : "database.pin");
  add("copyName", "database.copyName");
  if (supportsDbxObjectBrowserTreeNode(connection, "database")) add("openObjectBrowser", "database.openObjectBrowser");
  add("newQuery", "database.newQuery");
  add("queryHistory", "database.queryHistory");
  add(
    connection && isDbxDefaultDatabase(connection, database) ? "clearDefaultDatabase" : "setDefaultDatabase",
    connection && isDbxDefaultDatabase(connection, database) ? "database.clearDefaultDatabase" : "database.setDefaultDatabase",
  );
  if (supportsDbxTableStructureEditing(connection)) add("createTable", "database.createTable");
  if (canCreateDbxSchema(connection)) add("createSchema", "database.createSchema");
  if (supportsDbxSqlFileExecution(connection)) add("executeSqlFile", "database.executeSqlFile");
  if (supportsDbxDiagram(connection)) add("openErDiagram", "database.erDiagram");
  if (supportsDbxDatabaseSearch(connection)) add("databaseSearch", "database.databaseSearch");
  add("refresh", "database.refresh");
  add("dataTransfer", "database.dataTransfer");
  add("schemaDiff", "database.schemaDiff");
  add("dataCompare", "database.dataCompare");
  add("exportDatabase", "database.databaseExport");
  add("closeDatabaseConnection", "database.closeDatabaseConnection");
  if (canDropDbxDatabaseNode(connection)) add("dropDatabase", "database.dropDatabase");
  return items;
}

function dbxSchemaContextMenuItems(
  connection: AeroricDbConnectionConfig | null | undefined,
  pinned: boolean,
): DbxContextMenuItem<DbxSchemaContextMenuAction>[] {
  const items: DbxContextMenuItem<DbxSchemaContextMenuAction>[] = [];
  const add = (action: DbxSchemaContextMenuAction, labelKey: string) => items.push([action, labelKey]);
  add("togglePin", pinned ? "database.unpin" : "database.pin");
  add("copyName", "database.copyName");
  if (supportsDbxObjectBrowserTreeNode(connection, "schema")) add("openObjectBrowser", "database.openObjectBrowser");
  add("newQuery", "database.newQuery");
  add("queryHistory", "database.queryHistory");
  if (supportsDbxTableStructureEditing(connection)) add("createTable", "database.createTable");
  if (supportsDbxSqlFileExecution(connection)) add("executeSqlFile", "database.executeSqlFile");
  if (supportsDbxDiagram(connection)) add("openErDiagram", "database.erDiagram");
  if (supportsDbxDatabaseSearch(connection)) add("databaseSearch", "database.databaseSearch");
  add("refresh", "database.refresh");
  add("dataTransfer", "database.dataTransfer");
  add("schemaDiff", "database.schemaDiff");
  add("dataCompare", "database.dataCompare");
  add("exportDatabase", "database.databaseExport");
  if (canDropDbxSchemaNode(connection)) add("dropSchema", "database.dropSchema");
  return items;
}

function noSqlDatabaseContextMenuItems(
  menu: NoSqlDatabaseContextMenuState,
  connection: AeroricDbConnectionConfig | null | undefined,
  pinned: boolean,
): DbxContextMenuItem<NoSqlContextMenuAction>[] {
  const items: DbxContextMenuItem<NoSqlContextMenuAction>[] = [];
  const add = (action: NoSqlContextMenuAction, labelKey: string) => items.push([action, labelKey]);
  const database = menu.kind === "redis-database" ? String(menu.database) : menu.database;
  const isDefault = configuredTargetDatabase(connection) === database;
  add("togglePin", pinned ? "database.unpin" : "database.pin");
  add("newQuery", "database.newQuery");
  add(isDefault ? "clearDefaultDatabase" : "setDefaultDatabase", isDefault ? "database.clearDefaultDatabase" : "database.setDefaultDatabase");
  if (menu.kind === "redis-database") add("flushRedisDb", "database.redisFlushDb");
  return items;
}

function noSqlCollectionContextMenuItems(pinned: boolean): DbxContextMenuItem<NoSqlContextMenuAction>[] {
  return [["togglePin", pinned ? "database.unpin" : "database.pin"]];
}

function dbxObjectDropLabelKey(object: DbxObjectInfo) {
  const sourceKind = dbxObjectSourceKind(object);
  if (sourceKind === "PROCEDURE") return "database.dropProcedure";
  if (sourceKind === "FUNCTION") return "database.dropFunction";
  if (sourceKind === "VIEW") return "database.dropView";
  return "database.dropObject";
}

function dbxObjectContextMenuItems(
  object: DbxObjectInfo,
  connection: AeroricDbConnectionConfig | null | undefined,
  pinned: boolean,
): DbxObjectContextMenuItem[] {
  const items: DbxObjectContextMenuItem[] = [];
  const add = (action: DbxObjectContextMenuAction, labelKey: string) => items.push([action, labelKey]);
  const isTable = isDbxTableObject(object);
  const isView = isDbxViewObject(object);

  if (isDbxRoutineLikeObject(object)) {
    if (isDbxProcedureObject(object)) add("executeProcedure", "database.executeProcedure");
    add("viewSource", "database.viewSource");
    if (canRenameDbxObject(connection, object)) add("renameObject", "database.renameObject");
    if (isDbxDroppableRoutineObject(object)) add("dropObject", dbxObjectDropLabelKey(object));
    if (!isDbxDroppableRoutineObject(object)) add("copyName", "database.copyName");
    return items;
  }

  if (isTable || isView) add("togglePin", pinned ? "database.unpin" : "database.pin");
  add("copyName", "database.copyName");

  add("viewData", "database.viewData");
  if (isView) {
    add("editView", "database.editView");
    add("viewSource", "database.viewSource");
  }
  add("viewDdl", "database.viewDdl");
  if (isTable) add("editStructure", "database.editStructure");
  if (isTable || isView) add("tableInfo", "database.tableInfo");
  if (canRenameDbxObject(connection, object)) add("renameObject", "database.renameObject");
  if (isView) add("dropObject", dbxObjectDropLabelKey(object));

  if (isTable) {
    add("newSqlSelect", "database.newSqlSelect");
    add("newSqlInsert", "database.newSqlInsert");
    add("newSqlUpdate", "database.newSqlUpdate");
  } else {
    add("newQuery", "database.newQuery");
  }

  add("queryHistory", "database.queryHistory");
  add("openErDiagram", "database.erDiagram");
  if (isTable) add("importData", "database.tableImport");
  if (isTable) add("dataCompare", "database.dataCompare");

  if (isTable || isView) {
    add("exportCsv", "database.exportCsv");
    add("exportJson", "database.exportJson");
    add("exportMarkdown", "database.exportMarkdown");
    add("exportInsertSql", "database.exportInsertSql");
    add("exportUpdateSql", "database.exportUpdateSql");
    add("exportXlsx", "database.exportXlsx");
    add("exportDatabase", "database.databaseExport");
    add("exportStructure", "database.exportStructure");
    add("copyStructureDdl", "database.copyStructureDdl");
    add("copyStructureTsv", "database.copyStructureTsv");
    add("copyStructureMarkdown", "database.copyStructureMarkdown");
  }

  if (isTable) {
    if (!connection?.readOnly) add("duplicateStructure", "database.duplicateStructure");
    if (supportsDbxTableTruncate(connection)) add("truncateTable", "database.truncateTable");
    add("emptyTable", "database.emptyTable");
    add("dropTable", "database.dropTable");
  } else if (!isView) {
    add("dropObject", dbxObjectDropLabelKey(object));
  }

  add("refresh", "database.refresh");
  return items;
}

function dbxObjectDropConfirmLabelKey(object: DbxObjectInfo) {
  const sourceKind = dbxObjectSourceKind(object);
  if (sourceKind === "PROCEDURE") return "database.confirmDropProcedure";
  if (sourceKind === "FUNCTION") return "database.confirmDropFunction";
  if (sourceKind === "VIEW") return "database.confirmDropView";
  return "database.confirmDropObject";
}

function dbxQualifiedSqlName(object: DbxObjectInfo): string {
  return object.schema ? `${quoteSqlName(object.schema)}.${quoteSqlName(object.name)}` : quoteSqlName(object.name);
}

function nextDbxOrderByForColumn(currentOrderBy: string, column: string): string {
  const asc = `${quoteSqlName(column)} ASC`;
  const desc = `${quoteSqlName(column)} DESC`;
  const normalized = currentOrderBy.trim().toLowerCase();
  if (normalized === asc.toLowerCase()) return desc;
  if (normalized === desc.toLowerCase()) return "";
  return asc;
}

function dbxOrderByForColumn(column: string, direction: "ASC" | "DESC" | null): string {
  return direction ? `${quoteSqlName(column)} ${direction}` : "";
}

function dbxFilterModeForCellAction(action: DbxGridCellContextMenuAction): DataGridContextFilterMode | null {
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

function combineDbxGridWhereCondition(currentWhere: string, condition: string): string {
  const current = currentWhere.trim();
  return current ? `(${current}) AND (${condition})` : condition;
}

function GuidancePanel({ title, message }: { title: string; message: string }) {
  return (
    <div style={s.databaseWorkspacePanel}>
      <div>
        <div style={s.databaseWorkspaceTitle}>{title}</div>
        <div style={s.databaseDialogHint}>{message}</div>
      </div>
    </div>
  );
}

export function DatabaseView({
  projectRoot,
}: Props) {
  const { t } = useI18n();
  const [connections, setConnections] = useState<DbConnectionConfig[]>([]);
  const [dbxConnections, setDbxConnections] = useState<AeroricDbConnectionConfig[]>([]);
  const [activeConnectionId, setActiveConnectionId] = useState<string | null>(null);
  const [activeDbxConnectionId, setActiveDbxConnectionId] = useState<string | null>(null);
  const [schema, setSchema] = useState<DbSchema | null>(null);
  const [dbxDatabases, setDbxDatabases] = useState<DbxDatabaseInfo[]>([]);
  const [dbxSchemas, setDbxSchemas] = useState<string[]>([]);
  const [dbxObjects, setDbxObjects] = useState<DbxObjectInfo[]>([]);
  const [activeDbxDatabase, setActiveDbxDatabase] = useState<string | null>(null);
  const [activeDbxSchema, setActiveDbxSchema] = useState<string | null>(null);
  const [activeDbxObject, setActiveDbxObject] = useState<DbxObjectInfo | null>(null);
  const [activeObject, setActiveObject] = useState<DbObject | null>(null);
  const [queryResult, setQueryResult] = useState<DbQueryResult | null>(null);
  const [sqlResult, setSqlResult] = useState<DbExecuteResult | null>(null);
  const [page, setPage] = useState(1);
  const [sql, setSql] = useState("");
  const [queryHistory, setQueryHistory] = useState<QueryHistoryEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [exportProgress, setExportProgress] = useState<{ active: boolean; format: string; filePath: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [workspaceMode, setWorkspaceMode] = useState<DbWorkspaceMode>("table");
  const [connectionDialogOpen, setConnectionDialogOpen] = useState(false);
  const [connectionTestResult, setConnectionTestResult] = useState<{ success: boolean; message: string } | null>(null);
  const [editingDbxConnectionId, setEditingDbxConnectionId] = useState<string | null>(null);
  const [driverManifest, setDriverManifest] = useState<DatabaseDriverManifest | null>(null);
  const [wizardStep, setWizardStep] = useState<DbWizardStep>("type");
  const [selectedProfileKey, setSelectedProfileKey] = useState<DbProfileKey>("sqlite");
  const [profileViewMode, setProfileViewMode] = useState<DbProfileViewMode>("icon");
  const [profileSearch, setProfileSearch] = useState("");
  const [configTab, setConfigTab] = useState<DbConfigTab>("connection");
  const [draftName, setDraftName] = useState("SQLite");
  const [draftConnectionGroup, setDraftConnectionGroup] = useState("");
  const [draftColor, setDraftColor] = useState("");
  const [draftHost, setDraftHost] = useState("127.0.0.1");
  const [draftPort, setDraftPort] = useState("0");
  const [draftUser, setDraftUser] = useState("");
  const [draftDatabase, setDraftDatabase] = useState("");
  const [draftPassword, setDraftPassword] = useState("");
  const [draftFilePath, setDraftFilePath] = useState("");
  const [draftReadOnly, setDraftReadOnly] = useState(false);
  const [draftUrlParams, setDraftUrlParams] = useState("");
  const [draftConnectionString, setDraftConnectionString] = useState("");
  const [draftConnectTimeoutSecs, setDraftConnectTimeoutSecs] = useState("5");
  const [draftQueryTimeoutSecs, setDraftQueryTimeoutSecs] = useState("30");
  const [draftIdleTimeoutSecs, setDraftIdleTimeoutSecs] = useState("60");
  const [draftKeepaliveIntervalSecs, setDraftKeepaliveIntervalSecs] = useState("0");
  const [draftCaCertPath, setDraftCaCertPath] = useState("");
  const [draftClientCertPath, setDraftClientCertPath] = useState("");
  const [draftClientKeyPath, setDraftClientKeyPath] = useState("");
  const [draftRedisConnectionMode, setDraftRedisConnectionMode] = useState<RedisConnectionMode>("standalone");
  const [draftRedisSentinelMaster, setDraftRedisSentinelMaster] = useState("");
  const [draftRedisSentinelNodes, setDraftRedisSentinelNodes] = useState("");
  const [draftRedisSentinelUsername, setDraftRedisSentinelUsername] = useState("");
  const [draftRedisSentinelPassword, setDraftRedisSentinelPassword] = useState("");
  const [draftRedisSentinelTls, setDraftRedisSentinelTls] = useState(false);
  const [draftRedisClusterNodes, setDraftRedisClusterNodes] = useState("");
  const [draftRedisKeySeparator, setDraftRedisKeySeparator] = useState(":");
  const [draftMongoUseUrl, setDraftMongoUseUrl] = useState(false);
  const [tlsEnabled, setTlsEnabled] = useState(false);
  const [draftTlsMode, setDraftTlsMode] = useState("");
  const [draftOracleConnectionType, setDraftOracleConnectionType] = useState<"service_name" | "sid">("service_name");
  const [draftOracleSysdba, setDraftOracleSysdba] = useState(false);
  const [transportEnabled, setTransportEnabled] = useState(false);
  const [draftTransportLayers, setDraftTransportLayers] = useState<TransportLayerDraft[]>([]);
  const [selectedTransportLayerId, setSelectedTransportLayerId] = useState<string | null>(null);
  const [createDatabaseConnectionId, setCreateDatabaseConnectionId] = useState<string | null>(null);
  const [createDatabaseName, setCreateDatabaseName] = useState("");
  const [createDatabaseCharset, setCreateDatabaseCharset] = useState("utf8mb4");
  const [createDatabaseCollation, setCreateDatabaseCollation] = useState("utf8mb4_unicode_ci");
  const [createSchemaTarget, setCreateSchemaTarget] = useState<{ connectionId: string; database: string } | null>(null);
  const [createSchemaName, setCreateSchemaName] = useState("");
  const [sqlFilePath, setSqlFilePath] = useState("");
  const [sqlFilePreview, setSqlFilePreview] = useState("");
  const [sqlFileTimeoutSecs, setSqlFileTimeoutSecs] = useState("60");
  const [dbxColumnsByTable, setDbxColumnsByTable] = useState<Record<string, DbxColumnInfo[]>>({});
  const [redisDatabasesByConnection, setRedisDatabasesByConnection] = useState<Record<string, RedisDatabaseInfo[]>>({});
  const [redisKeysByDatabase, setRedisKeysByDatabase] = useState<Record<string, RedisKeyInfo[]>>({});
  const [redisScanStateByDatabase, setRedisScanStateByDatabase] = useState<Record<string, RedisSidebarScanState>>({});
  const [mongoDatabasesByConnection, setMongoDatabasesByConnection] = useState<Record<string, string[]>>({});
  const [mongoCollectionsByDatabase, setMongoCollectionsByDatabase] = useState<Record<string, string[]>>({});
  const [mongoDocumentsByCollection, setMongoDocumentsByCollection] = useState<Record<string, unknown[]>>({});
  const [mongoDocumentTotalsByCollection, setMongoDocumentTotalsByCollection] = useState<Record<string, number>>({});
  const [mongoDocumentQueriesByCollection, setMongoDocumentQueriesByCollection] = useState<Record<string, MongoSidebarDocumentQuery>>({});
  const [activeMongoDocumentId, setActiveMongoDocumentId] = useState<string | null>(null);
  const [activeMongoWorkspaceDatabase, setActiveMongoWorkspaceDatabase] = useState<string | null>(null);
  const [dbxGridWhereInput, setDbxGridWhereInput] = useState("");
  const [dbxGridOrderByInput, setDbxGridOrderByInput] = useState("");
  const [dbxGridSearch, setDbxGridSearch] = useState("");
  const [dbxGridColumnSearch, setDbxGridColumnSearch] = useState("");
  const [dbxGridHiddenColumns, setDbxGridHiddenColumns] = useState<Set<string>>(new Set());
  const [dbxGridColumnWidths, setDbxGridColumnWidths] = useState<Record<string, number>>({});
  const [resizingDbxGridColumn, setResizingDbxGridColumn] = useState<string | null>(null);
  const [dbxGridPageSize, setDbxGridPageSize] = useState(PAGE_SIZE);
  const [dbxGridSelectedRows, setDbxGridSelectedRows] = useState<Set<number>>(new Set());
  const [dbxGridExportFormat, setDbxGridExportFormat] = useState<TableExportFormat>("csv");
  const [dbxSqlPreviewOpen, setDbxSqlPreviewOpen] = useState(false);
  const [dbxSqlPreviewStatements, setDbxSqlPreviewStatements] = useState<string[]>([]);
  const [dbxSqlPreviewRollback, setDbxSqlPreviewRollback] = useState<string[]>([]);
  const [dbxSqlPreviewDescription, setDbxSqlPreviewDescription] = useState("");
  const [dbxCellPreview, setDbxCellPreview] = useState<{ column: string; value: unknown } | null>(null);
  const [dbxRowPreview, setDbxRowPreview] = useState<{ rowIndex: number; row: DatabaseRow } | null>(null);
  const [dbxRowPreviewSearch, setDbxRowPreviewSearch] = useState("");
  const [dbxColumnPreview, setDbxColumnPreview] = useState<{ column: string; columnIndex: number } | null>(null);
  const [dbxColumnPreviewSearch, setDbxColumnPreviewSearch] = useState("");
  const [visibleDatabaseConnectionId, setVisibleDatabaseConnectionId] = useState<string | null>(null);
  const [visibleDatabaseNames, setVisibleDatabaseNames] = useState<string[]>([]);
  const [visibleDatabaseSelection, setVisibleDatabaseSelection] = useState<Set<string>>(new Set());
  const [visibleDatabaseSearch, setVisibleDatabaseSearch] = useState("");
  const [visibleDatabaseShowSystem, setVisibleDatabaseShowSystem] = useState(false);
  const [visibleDatabaseLoading, setVisibleDatabaseLoading] = useState(false);
  const [visibleDatabaseError, setVisibleDatabaseError] = useState("");
  const [databaseExportTarget, setDatabaseExportTarget] = useState<{
    connectionId: string;
    database: string;
    schema: string | null;
    preselectedTables: string[];
  } | null>(null);
  const [databaseExportTables, setDatabaseExportTables] = useState<string[]>([]);
  const [databaseExportSelection, setDatabaseExportSelection] = useState<Set<string>>(new Set());
  const [databaseExportSearch, setDatabaseExportSearch] = useState("");
  const [databaseExportIncludeStructure, setDatabaseExportIncludeStructure] = useState(true);
  const [databaseExportIncludeData, setDatabaseExportIncludeData] = useState(true);
  const [databaseExportIncludeObjects, setDatabaseExportIncludeObjects] = useState(true);
  const [databaseExportDropTableIfExists, setDatabaseExportDropTableIfExists] = useState(false);
  const [databaseExportLoading, setDatabaseExportLoading] = useState(false);
  const [databaseExportError, setDatabaseExportError] = useState("");
  const [tableImportTarget, setTableImportTarget] = useState<{
    connectionId: string;
    database: string | null;
    object: DbxObjectInfo;
  } | null>(null);
  const [tableImportColumns, setTableImportColumns] = useState<DbxColumnInfo[]>([]);
  const [tableImportPreview, setTableImportPreview] = useState<TableImportPreview | null>(null);
  const [tableImportMappings, setTableImportMappings] = useState<Record<string, string>>({});
  const [tableImportMode, setTableImportMode] = useState<TableImportMode>("append");
  const [tableImportBatchSize, setTableImportBatchSize] = useState("500");
  const [tableImportLoading, setTableImportLoading] = useState(false);
  const [tableImportError, setTableImportError] = useState("");
  const [contextMenu, setContextMenu] = useState<DatabaseContextMenuState>(null);
  const [tableInfoActiveTab, setTableInfoActiveTab] = useState<TableInfoTab>("columns");
  const [tableInfoSearch, setTableInfoSearch] = useState("");
  const [tableInfoDdl, setTableInfoDdl] = useState("");
  const [tableInfoDdlLoading, setTableInfoDdlLoading] = useState(false);
  const [tableInfoDdlError, setTableInfoDdlError] = useState("");
  const dbxGridColumnResizeStartRef = useRef({ column: "", x: 0, width: DBX_GRID_DEFAULT_COLUMN_WIDTH });
  const [pinnedTreeNodeIds, setPinnedTreeNodeIds] = useState<Set<string>>(loadPinnedTreeNodeIds);
  const [extraDbxConnectionGroups, setExtraDbxConnectionGroups] = useState<string[]>(loadExtraDbxConnectionGroups);

  const activeConnection = useMemo(
    () => connections.find((connection) => connection.id === activeConnectionId) ?? null,
    [activeConnectionId, connections],
  );
  const activeDbxConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === activeDbxConnectionId) ?? null,
    [activeDbxConnectionId, dbxConnections],
  );
  const editingDbxConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === editingDbxConnectionId) ?? null,
    [dbxConnections, editingDbxConnectionId],
  );

  const activeEndpoint = activeConnection?.endpoint ?? null;
  const selectedProfile = DB_PROFILES.find((profile) => profile.key === selectedProfileKey) ?? DB_PROFILES[0];
  const filteredProfiles = useMemo(() => {
    const query = profileSearch.trim().toLowerCase();
    if (!query) return DB_PROFILES;
    return DB_PROFILES.filter((profile) => profile.label.toLowerCase().includes(query) || profile.key.includes(query));
  }, [profileSearch]);
  const dbxHasSqlObjectBrowser = isSqlDbxConnection(activeDbxConnection);
  const sqlDbxConnections = useMemo(() => dbxConnections.filter((connection) => isSqlDbxConnection(connection)), [dbxConnections]);
  const dbxTableObjects = useMemo(() => dbxObjects.filter((object) => isDbxTableObject(object)), [dbxObjects]);
  const selectedDbxTable = useMemo(
    () => activeDbxObject && isDbxTableObject(activeDbxObject) ? activeDbxObject : dbxTableObjects[0] ?? null,
    [activeDbxObject, dbxTableObjects],
  );
  const selectedDbxInfoObject = useMemo(
    () =>
      activeDbxObject && (isDbxTableObject(activeDbxObject) || isDbxViewObject(activeDbxObject))
        ? activeDbxObject
        : selectedDbxTable,
    [activeDbxObject, selectedDbxTable],
  );
  const selectedDbxInfoObjectKey = selectedDbxInfoObject ? dbxObjectKey(selectedDbxInfoObject) : "";
  const selectedDbxInfoColumns = selectedDbxInfoObject ? dbxColumnsByTable[selectedDbxInfoObjectKey] ?? EMPTY_DBX_COLUMNS : EMPTY_DBX_COLUMNS;
  const selectedDbxInfoChildObjects = useMemo(
    () =>
      selectedDbxInfoObject
        ? dbxObjects.filter((object) => Boolean(dbxTableChildObjectType(object)) && dbxChildObjectBelongsToTable(object, selectedDbxInfoObject))
        : [],
    [dbxObjects, selectedDbxInfoObject],
  );
  const selectedDbxInfoIndexes = useMemo(
    () => selectedDbxInfoChildObjects.filter((object) => dbxTableChildObjectType(object) === "INDEX"),
    [selectedDbxInfoChildObjects],
  );
  const selectedDbxInfoForeignKeys = useMemo(
    () => selectedDbxInfoChildObjects.filter((object) => dbxTableChildObjectType(object) === "FOREIGN_KEY"),
    [selectedDbxInfoChildObjects],
  );
  const selectedDbxInfoTriggers = useMemo(
    () => selectedDbxInfoChildObjects.filter((object) => dbxTableChildObjectType(object) === "TRIGGER"),
    [selectedDbxInfoChildObjects],
  );
  const tableInfoQuery = tableInfoSearch.trim().toLowerCase();
  const filteredDbxInfoColumns = useMemo(() => {
    if (!tableInfoQuery) return selectedDbxInfoColumns;
    return selectedDbxInfoColumns.filter((column) =>
      [column.name, column.data_type, column.column_default ?? ""].some((value) => value.toLowerCase().includes(tableInfoQuery)),
    );
  }, [selectedDbxInfoColumns, tableInfoQuery]);
  const filterTableInfoObjects = useCallback(
    (objects: DbxObjectInfo[]) => {
      if (!tableInfoQuery) return objects;
      return objects.filter((object) =>
        [object.name, object.schema ?? "", object.object_type].some((value) => value.toLowerCase().includes(tableInfoQuery)),
      );
    },
    [tableInfoQuery],
  );
  const filteredDbxInfoIndexes = useMemo(() => filterTableInfoObjects(selectedDbxInfoIndexes), [filterTableInfoObjects, selectedDbxInfoIndexes]);
  const filteredDbxInfoForeignKeys = useMemo(() => filterTableInfoObjects(selectedDbxInfoForeignKeys), [filterTableInfoObjects, selectedDbxInfoForeignKeys]);
  const filteredDbxInfoTriggers = useMemo(() => filterTableInfoObjects(selectedDbxInfoTriggers), [filterTableInfoObjects, selectedDbxInfoTriggers]);
  useEffect(() => {
    setTableInfoActiveTab("columns");
    setTableInfoSearch("");
    setTableInfoDdl("");
    setTableInfoDdlError("");
  }, [selectedDbxInfoObjectKey]);
  const loadTableInfoDdl = useCallback(async () => {
    if (!activeDbxConnection || !selectedDbxInfoObject || tableInfoDdlLoading) return;
    setTableInfoDdlLoading(true);
    setTableInfoDdlError("");
    try {
      const ddl = await databaseApi.dbxGetTableDdl(
        activeDbxConnection.id,
        selectedDbxInfoObject.name,
        activeDbxDatabase,
        selectedDbxInfoObject.schema ?? null,
      );
      setTableInfoDdl(ddl);
    } catch (err) {
      setTableInfoDdlError(String(err));
    } finally {
      setTableInfoDdlLoading(false);
    }
  }, [activeDbxConnection, activeDbxDatabase, selectedDbxInfoObject, tableInfoDdlLoading]);
  useEffect(() => {
    if (tableInfoActiveTab === "ddl" && !tableInfoDdl && !tableInfoDdlLoading && !tableInfoDdlError) {
      void loadTableInfoDdl();
    }
  }, [loadTableInfoDdl, tableInfoActiveTab, tableInfoDdl, tableInfoDdlError, tableInfoDdlLoading]);
  const activeSqlCapable = Boolean(activeEndpoint || (activeDbxConnection && dbxHasSqlObjectBrowser));
  const rawTableRows = useMemo(() => queryResult?.rows ?? sqlResult?.rows ?? [], [queryResult, sqlResult]);
  const tableColumns = useMemo(() => queryResult?.columns ?? sqlResult?.columns ?? [], [queryResult, sqlResult]);
  const showRowIdColumn = Boolean(queryResult && !activeDbxConnection && queryResult.hasRowId);
  const visibleTableColumns = useMemo(
    () =>
      tableColumns
        .map((column, index) => ({ column, index }))
        .filter(({ column }) => !dbxGridHiddenColumns.has(column)),
    [dbxGridHiddenColumns, tableColumns],
  );
  const visibleDbxGridDataColumnsWidth = useMemo(
    () => visibleTableColumns.reduce((sum, { column }) => sum + (dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH), 0),
    [dbxGridColumnWidths, visibleTableColumns],
  );
  const dbxGridTableMinWidth = useMemo(() => {
    if (!queryResult || !activeDbxConnection) return undefined;
    return 42 + 74 + (showRowIdColumn ? 86 : 0) + visibleDbxGridDataColumnsWidth;
  }, [activeDbxConnection, queryResult, showRowIdColumn, visibleDbxGridDataColumnsWidth]);
  const activeDbxGridColumns = useMemo(() => {
    if (!activeDbxObject) return EMPTY_DBX_COLUMNS;
    return dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? EMPTY_DBX_COLUMNS;
  }, [activeDbxObject, dbxColumnsByTable]);
  const activeDbxGridColumnsByName = useMemo(
    () => new Map(activeDbxGridColumns.map((column) => [column.name.toLowerCase(), column])),
    [activeDbxGridColumns],
  );
  const filteredDbxGridColumnOptions = useMemo(() => {
    const query = dbxGridColumnSearch.trim().toLowerCase();
    if (!query) return tableColumns;
    return tableColumns.filter((column) => column.toLowerCase().includes(query));
  }, [dbxGridColumnSearch, tableColumns]);
  const dbxGridNullColumns = useMemo(() => {
    if (rawTableRows.length === 0) return new Set<string>();
    return new Set(
      tableColumns.filter((_, columnIndex) => rawTableRows.every((row) => isNullGridValue(row.values[columnIndex]))),
    );
  }, [rawTableRows, tableColumns]);
  const tableRows = useMemo(() => {
    const query = dbxGridSearch.trim().toLowerCase();
    if (!query) return rawTableRows;
    return rawTableRows.filter((row) => row.values.some((value) => valueToText(value).toLowerCase().includes(query)));
  }, [dbxGridSearch, rawTableRows]);
  const formattedDbxCellPreview = useMemo(
    () => dbxCellPreview ? cellPreviewText(dbxCellPreview.value) : null,
    [dbxCellPreview],
  );
  const dbxRowPreviewFields = useMemo(
    () =>
      dbxRowPreview
        ? visibleTableColumns.map(({ column, index }) => ({
            column,
            type: dbxGridColumnType(queryResult, index),
            value: dbxRowPreview.row.values[index] ?? null,
            preview: cellPreviewText(dbxRowPreview.row.values[index] ?? null).text,
          }))
        : [],
    [dbxRowPreview, queryResult, visibleTableColumns],
  );
  const filteredDbxRowPreviewFields = useMemo(() => {
    const query = dbxRowPreviewSearch.trim().toLowerCase();
    if (!query) return dbxRowPreviewFields;
    return dbxRowPreviewFields.filter((field) =>
      [field.column, field.type ?? "", field.preview].some((value) => value.toLowerCase().includes(query)),
    );
  }, [dbxRowPreviewFields, dbxRowPreviewSearch]);
  const dbxColumnPreviewFields = useMemo(() => {
    if (!dbxColumnPreview || !queryResult || !activeDbxConnection) return [];
    return tableRows.map((row) => {
      const rowIndex = queryResult.rows.indexOf(row);
      const value = row.values[dbxColumnPreview.columnIndex] ?? null;
      return {
        rowNumber: rowIndex >= 0 ? rowIndex + 1 : 0,
        value,
        preview: cellPreviewText(value).text,
      };
    });
  }, [activeDbxConnection, dbxColumnPreview, queryResult, tableRows]);
  const filteredDbxColumnPreviewFields = useMemo(() => {
    const query = dbxColumnPreviewSearch.trim().toLowerCase();
    if (!query) return dbxColumnPreviewFields;
    return dbxColumnPreviewFields.filter((field) => field.preview.toLowerCase().includes(query) || String(field.rowNumber).includes(query));
  }, [dbxColumnPreviewFields, dbxColumnPreviewSearch]);
  const visibleDbxGridRowIndexes = useMemo(
    () =>
      queryResult && activeDbxConnection
        ? tableRows
            .map((row) => queryResult.rows.indexOf(row))
            .filter((rowIndex) => rowIndex >= 0)
        : [],
    [activeDbxConnection, queryResult, tableRows],
  );
  const allVisibleDbxGridRowsSelected =
    visibleDbxGridRowIndexes.length > 0 && visibleDbxGridRowIndexes.every((rowIndex) => dbxGridSelectedRows.has(rowIndex));
  const dbxGridCellContextRowCount = useMemo(() => {
    if (!queryResult || contextMenu?.kind !== "dbx-grid-cell") return 0;
    if (dbxGridSelectedRows.has(contextMenu.rowIndex) && dbxGridSelectedRows.size > 0) {
      return Array.from(dbxGridSelectedRows).filter((rowIndex) => rowIndex >= 0 && rowIndex < queryResult.rows.length).length;
    }
    return queryResult.rows[contextMenu.rowIndex] ? 1 : 0;
  }, [contextMenu, dbxGridSelectedRows, queryResult]);
  const totalPages =
    queryResult?.totalRows && queryResult.totalRows > 0
      ? Math.max(1, Math.ceil(queryResult.totalRows / queryResult.pageSize))
      : null;
  const activeDbxTargetDatabase = configuredTargetDatabase(activeDbxConnection);
  const visibleDbxDatabases = activeDbxTargetDatabase
    ? dbxDatabases.filter((database) => database.name === activeDbxTargetDatabase)
    : dbxDatabases;
  const visibleDatabaseConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === visibleDatabaseConnectionId) ?? null,
    [dbxConnections, visibleDatabaseConnectionId],
  );
  const listedVisibleDatabaseNames = useMemo(
    () =>
      visibleDatabaseShowSystem
        ? visibleDatabaseNames
        : visibleDatabaseNames.filter((name) => !isSystemDatabaseName(visibleDatabaseConnection?.dbType, name)),
    [visibleDatabaseConnection?.dbType, visibleDatabaseNames, visibleDatabaseShowSystem],
  );
  const filteredVisibleDatabaseNames = useMemo(() => {
    const query = visibleDatabaseSearch.trim().toLowerCase();
    if (!query) return listedVisibleDatabaseNames;
    return listedVisibleDatabaseNames.filter((name) => name.toLowerCase().includes(query));
  }, [listedVisibleDatabaseNames, visibleDatabaseSearch]);
  const visibleDatabaseHasSystemNames = useMemo(
    () => visibleDatabaseNames.some((name) => isSystemDatabaseName(visibleDatabaseConnection?.dbType, name)),
    [visibleDatabaseConnection?.dbType, visibleDatabaseNames],
  );
  const visibleDatabaseCanSave = visibleDatabaseSelection.size > 0;
  const databaseExportConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === databaseExportTarget?.connectionId) ?? null,
    [databaseExportTarget?.connectionId, dbxConnections],
  );
  const filteredDatabaseExportTables = useMemo(() => {
    const query = databaseExportSearch.trim().toLowerCase();
    if (!query) return databaseExportTables;
    return databaseExportTables.filter((name) => name.toLowerCase().includes(query));
  }, [databaseExportSearch, databaseExportTables]);
  const databaseExportCanRun =
    Boolean(databaseExportConnection && databaseExportTarget) &&
    databaseExportSelection.size > 0 &&
    (databaseExportIncludeStructure || databaseExportIncludeData || databaseExportIncludeObjects);
  const tableImportConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === tableImportTarget?.connectionId) ?? null,
    [dbxConnections, tableImportTarget?.connectionId],
  );
  const tableImportTargetColumnNames = useMemo(() => tableImportColumns.map((column) => column.name), [tableImportColumns]);
  const tableImportMappedColumns = useMemo(
    () =>
      tableImportPreview
        ? tableImportPreview.columns
            .map((sourceColumn) => ({
              sourceColumn,
              targetColumn: tableImportMappings[sourceColumn] ?? "",
            }))
            .filter((mapping) => mapping.targetColumn)
        : [],
    [tableImportMappings, tableImportPreview],
  );
  const tableImportCanRun = Boolean(tableImportConnection && tableImportTarget && tableImportPreview && tableImportMappedColumns.length > 0);
  const selectedTransportLayer = useMemo(
    () => draftTransportLayers.find((layer) => layer.id === selectedTransportLayerId) ?? draftTransportLayers[0] ?? null,
    [draftTransportLayers, selectedTransportLayerId],
  );

  const saveConnections = useCallback((next: DbConnectionConfig[]) => {
    setConnections(next);
    databaseApi.saveConnections(next).catch((err) => {
      setError(String(err));
    });
  }, []);

  const inspect = useCallback(
    async (connection: DbConnectionConfig) => {
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const nextSchema = await databaseApi.inspect(connection.endpoint, projectRoot);
        setSchema(nextSchema);
        const firstTable =
          nextSchema.objects.find((object) => object.objectType === "table") ??
          nextSchema.objects[0] ??
          null;
        setActiveObject(firstTable);
        setPage(1);
        if (firstTable) {
          const result = await databaseApi.queryTable(
            connection.endpoint,
            firstTable.name,
            1,
            PAGE_SIZE,
            projectRoot,
          );
          setQueryResult(result);
          setSql(`SELECT * FROM ${quoteSqlName(firstTable.name)}`);
        } else {
          setQueryResult(null);
        }
        if (connections.some((item) => item.id === connection.id)) {
          const now = Date.now();
          saveConnections(
            connections.map((item) =>
              item.id === connection.id ? { ...item, lastOpenedAt: now } : item,
            ),
          );
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [connections, projectRoot, saveConnections],
  );

  useEffect(() => {
    databaseApi.loadConnections()
      .then((items) => {
        setConnections(items);
        if (items[0]) {
          setActiveConnectionId(items[0].id);
          inspect(items[0]);
        }
      })
      .catch((err) => setError(String(err)));
    databaseApi.dbxListConnections()
      .then((items) => {
        setDbxConnections(items);
        if (!activeConnectionId && !items[0]) return;
      })
      .catch((err) => setError(String(err)));
    // Load once; inspect is intentionally not a dependency here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    setDbxGridHiddenColumns((current) => {
      if (current.size === 0) return current;
      const available = new Set(tableColumns);
      const next = new Set([...current].filter((column) => available.has(column)));
      if (tableColumns.length > 0 && next.size >= tableColumns.length) next.delete(tableColumns[0]);
      return next.size === current.size ? current : next;
    });
  }, [tableColumns]);

  useEffect(() => {
    setDbxGridColumnWidths((current) => {
      const available = new Set(tableColumns);
      const entries = Object.entries(current).filter(([column]) => available.has(column));
      if (entries.length === Object.keys(current).length) return current;
      return Object.fromEntries(entries);
    });
  }, [tableColumns]);

  useEffect(() => {
    if (!resizingDbxGridColumn) return undefined;
    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const handlePointerMove = (event: PointerEvent) => {
      const { column, width, x } = dbxGridColumnResizeStartRef.current;
      const nextWidth = clampDbxGridColumnWidth(width + event.clientX - x);
      setDbxGridColumnWidths((current) => (current[column] === nextWidth ? current : { ...current, [column]: nextWidth }));
    };
    const handlePointerUp = () => {
      setResizingDbxGridColumn(null);
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [resizingDbxGridColumn]);

  const addConnection = useCallback(
    (endpoint: DbEndpoint) => {
      const now = Date.now();
      const connection: DbConnectionConfig = {
        id: `db:${now}:${Math.random().toString(36).slice(2)}`,
        name: createConnectionName(endpoint),
        endpoint,
        readOnly: false,
        createdAt: now,
        lastOpenedAt: now,
      };
      const next = [connection, ...connections];
      saveConnections(next);
      setActiveConnectionId(connection.id);
      inspect(connection);
    },
    [connections, inspect, saveConnections],
  );

  const chooseLocalDbFile = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "SQLite", extensions: ["db", "sqlite", "sqlite3"] }],
      defaultPath: projectRoot,
    });
    if (typeof selected === "string") setDraftFilePath(selected);
  }, [projectRoot]);

  const chooseTlsCertificatePath = useCallback(
    async (setter: (path: string) => void) => {
      const selected = await openDialog({
        multiple: false,
        directory: false,
        filters: [
          { name: "Certificates", extensions: ["pem", "crt", "cer", "key", "p12", "pfx"] },
          { name: "All files", extensions: ["*"] },
        ],
        defaultPath: projectRoot,
      });
      if (typeof selected === "string") setter(selected);
    },
    [projectRoot],
  );

  const resetConnectionDraft = useCallback((profile: DbProfile = selectedProfile) => {
    setWizardStep("type");
    setConfigTab("connection");
    setDraftName(profile.label);
    setDraftConnectionGroup("");
    setDraftColor("");
    setDraftHost(profile.localFile ? "" : "127.0.0.1");
    setDraftPort(String(profile.port));
    setDraftUser(profile.user);
    setDraftDatabase("");
    setDraftPassword("");
    setDraftFilePath("");
    setDraftReadOnly(false);
    setDraftUrlParams("");
    setDraftConnectionString("");
    setDraftConnectTimeoutSecs("5");
    setDraftQueryTimeoutSecs("30");
    setDraftIdleTimeoutSecs("60");
    setDraftKeepaliveIntervalSecs("0");
    setDraftCaCertPath("");
    setDraftClientCertPath("");
    setDraftClientKeyPath("");
    setDraftRedisConnectionMode("standalone");
    setDraftRedisSentinelMaster("");
    setDraftRedisSentinelNodes("");
    setDraftRedisSentinelUsername("");
    setDraftRedisSentinelPassword("");
    setDraftRedisSentinelTls(false);
    setDraftRedisClusterNodes("");
    setDraftRedisKeySeparator(":");
    setDraftMongoUseUrl(false);
    setTlsEnabled(false);
    setTransportEnabled(false);
    setDraftTransportLayers([]);
    setSelectedTransportLayerId(null);
  }, [selectedProfile]);

  const openNewConnectionDialog = useCallback((connectionGroup: unknown = null) => {
    setEditingDbxConnectionId(null);
    resetConnectionDraft(selectedProfile);
    setDraftConnectionGroup(typeof connectionGroup === "string" ? connectionGroup.trim() : "");
    setProfileSearch("");
    setProfileViewMode("icon");
    setError(null);
    setConnectionDialogOpen(true);
  }, [resetConnectionDraft, selectedProfile]);

  const openEditDbxConnectionDialog = useCallback((connection: AeroricDbConnectionConfig) => {
    const profile = profileForDbxConnection(connection);
    const config = dbxConfigRecord(connection);
    const transportLayers = Array.isArray(config.transport_layers)
      ? config.transport_layers.map((layer, index) => transportLayerDraftFromPayload(layer, index + 1))
      : [];
    setEditingDbxConnectionId(connection.id);
    setSelectedProfileKey(profile.key);
    setWizardStep("config");
    setConfigTab("connection");
    setDraftName(dbxString(config, "name", connection.name));
    setDraftConnectionGroup(connection.connectionGroup ?? "");
    setDraftColor(dbxString(config, "color"));
    setDraftHost(profile.localFile ? "" : dbxString(config, "host", "127.0.0.1"));
    setDraftPort(dbxNumberString(config, "port", String(profile.port)));
    setDraftUser(dbxString(config, "username"));
    setDraftDatabase(dbxString(config, "database"));
    setDraftPassword(dbxString(config, "password"));
    setDraftFilePath(profile.localFile ? dbxString(config, "host") : "");
    setDraftReadOnly(dbxBoolean(config, "read_only", connection.readOnly));
    setDraftUrlParams(dbxString(config, "url_params"));
    setDraftConnectionString(dbxString(config, "connection_string"));
    setDraftConnectTimeoutSecs(dbxNumberString(config, "connect_timeout_secs", "5"));
    setDraftQueryTimeoutSecs(dbxNumberString(config, "query_timeout_secs", "30"));
    setDraftIdleTimeoutSecs(dbxNumberString(config, "idle_timeout_secs", "60"));
    setDraftKeepaliveIntervalSecs(dbxNumberString(config, "keepalive_interval_secs", "0"));
    setDraftCaCertPath(dbxString(config, "ca_cert_path"));
    setDraftClientCertPath(dbxString(config, "client_cert_path"));
    setDraftClientKeyPath(dbxString(config, "client_key_path"));
    setDraftRedisConnectionMode(
      config.redis_connection_mode === "sentinel" || config.redis_connection_mode === "cluster"
        ? config.redis_connection_mode
        : "standalone",
    );
    setDraftRedisSentinelMaster(dbxString(config, "redis_sentinel_master"));
    setDraftRedisSentinelNodes(dbxString(config, "redis_sentinel_nodes"));
    setDraftRedisSentinelUsername(dbxString(config, "redis_sentinel_username"));
    setDraftRedisSentinelPassword(dbxString(config, "redis_sentinel_password"));
    setDraftRedisSentinelTls(dbxBoolean(config, "redis_sentinel_tls"));
    setDraftRedisClusterNodes(dbxString(config, "redis_cluster_nodes"));
    setDraftRedisKeySeparator(dbxString(config, "redis_key_separator", ":"));
    setDraftMongoUseUrl(connection.dbType === "mongodb" && Boolean(dbxString(config, "connection_string")));
    setTlsEnabled(dbxBoolean(config, "ssl"));
    const urlParamsStr = dbxString(config, "url_params");
    if (connection.dbType === "postgres") {
      const params = new URLSearchParams(urlParamsStr);
      setDraftTlsMode(params.get("sslmode") || "");
    } else if (connection.dbType === "mysql") {
      const params = new URLSearchParams(urlParamsStr);
      setDraftTlsMode(params.get("ssl-mode") || "");
    } else {
      setDraftTlsMode("");
    }
    if (connection.dbType === "oracle") {
      const oracleConfig = config as { oracle_connection_type?: string; oracle_sysdba?: boolean };
      setDraftOracleConnectionType(oracleConfig.oracle_connection_type === "sid" ? "sid" : "service_name");
      setDraftOracleSysdba(Boolean(oracleConfig.oracle_sysdba));
    } else {
      setDraftOracleConnectionType("service_name");
      setDraftOracleSysdba(false);
    }
    setTransportEnabled(transportLayers.length > 0);
    setDraftTransportLayers(transportLayers);
    setSelectedTransportLayerId(transportLayers[0]?.id ?? null);
    setProfileSearch("");
    setProfileViewMode("icon");
    setError(null);
    setConnectionDialogOpen(true);
  }, []);

  const closeConnectionDialog = useCallback(() => {
    setConnectionDialogOpen(false);
    setEditingDbxConnectionId(null);
  }, []);

  const openDriverManager = useCallback(async () => {
    setWorkspaceMode("drivers");
    setError(null);
    setLoading(true);
    try {
      const manifest = await databaseApi.dbxDriverManifest();
      setDriverManifest(manifest);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const handleProfileSelect = useCallback((key: DbProfileKey) => {
    const profile = DB_PROFILES.find((item) => item.key === key) ?? DB_PROFILES[0];
    setSelectedProfileKey(key);
    resetConnectionDraft(profile);
  }, [resetConnectionDraft]);

  const handleProfileDoubleClick = useCallback((key: DbProfileKey) => {
    handleProfileSelect(key);
    setWizardStep("config");
  }, [handleProfileSelect]);

  const applyConnectionUrl = useCallback(() => {
    const parsed = parseConnectionUrl(draftConnectionString, selectedProfile.key as DbxDatabaseType);
    if (!parsed || !parsed.host) {
      setError(t("database.connectionUrlParseFailed"));
      return;
    }
    setDraftHost(parsed.host);
    if (parsed.port) setDraftPort(parsed.port);
    if (parsed.user !== undefined) setDraftUser(parsed.user);
    if (parsed.password !== undefined) setDraftPassword(parsed.password);
    if (parsed.database !== undefined) setDraftDatabase(parsed.database);
    if (parsed.urlParams !== undefined) setDraftUrlParams(parsed.urlParams);
    if (selectedProfile.key === "mongodb") setDraftMongoUseUrl(true);
    setError(null);
  }, [draftConnectionString, selectedProfile.key, t]);

  const addTransportLayer = useCallback((type: "ssh" | "proxy") => {
    setTransportEnabled(true);
    setDraftTransportLayers((current) => {
      const nextLayer = createTransportLayerDraft(type, current.length + 1);
      setSelectedTransportLayerId(nextLayer.id);
      return [...current, nextLayer];
    });
  }, []);

  const updateTransportLayer = useCallback((id: string, patch: Partial<TransportLayerDraft>) => {
    setDraftTransportLayers((current) => current.map((layer) => (layer.id === id ? { ...layer, ...patch } : layer)));
  }, []);

  const copyTransportLayer = useCallback((id: string) => {
    setTransportEnabled(true);
    setDraftTransportLayers((current) => {
      const index = current.findIndex((layer) => layer.id === id);
      if (index < 0) return current;
      const source = current[index];
      const copy = {
        ...source,
        id: `transport:${Date.now()}:${Math.random().toString(36).slice(2)}`,
      };
      setSelectedTransportLayerId(copy.id);
      return [...current.slice(0, index + 1), copy, ...current.slice(index + 1)];
    });
  }, []);

  const moveTransportLayer = useCallback((id: string, direction: -1 | 1) => {
    setDraftTransportLayers((current) => {
      const index = current.findIndex((layer) => layer.id === id);
      const targetIndex = index + direction;
      if (index < 0 || targetIndex < 0 || targetIndex >= current.length) return current;
      const next = [...current];
      [next[index], next[targetIndex]] = [next[targetIndex], next[index]];
      setSelectedTransportLayerId(id);
      return next;
    });
  }, []);

  const removeTransportLayer = useCallback((id: string) => {
    setDraftTransportLayers((current) => {
      const next = current.filter((layer) => layer.id !== id);
      setSelectedTransportLayerId(next[0]?.id ?? null);
      if (next.length === 0) setTransportEnabled(false);
      return next;
    });
  }, []);

  const buildDbxConnectionDraft = useCallback((): AeroricDbConnectionConfig => {
    const now = Date.now();
    const id = editingDbxConnection?.id ?? `dbx:${now}:${Math.random().toString(36).slice(2)}`;
    const existingDbx = editingDbxConnection ? dbxConfigRecord(editingDbxConnection) : {};
    const port = Number.parseInt(draftPort, 10);
    const connectTimeoutSecs = Number.parseInt(draftConnectTimeoutSecs, 10);
    const queryTimeoutSecs = Number.parseInt(draftQueryTimeoutSecs, 10);
    const idleTimeoutSecs = Number.parseInt(draftIdleTimeoutSecs, 10);
    const keepaliveIntervalSecs = Number.parseInt(draftKeepaliveIntervalSecs, 10);
    let normalizedPort = Number.isFinite(port) && port >= 0 ? port : selectedProfile.port;
    const rawConnectionString = draftConnectionString.trim();
    const parsedMongoUrl =
      selectedProfile.key === "mongodb" && draftMongoUseUrl && rawConnectionString
        ? parseConnectionUrl(rawConnectionString, "mongodb")
        : null;
    let database = draftDatabase.trim();
    const redisSentinelNodes = normalizeRedisNodeList(draftRedisSentinelNodes);
    const redisClusterNodes = normalizeRedisNodeList(draftRedisClusterNodes);
    let host = selectedProfile.localFile ? draftFilePath.trim() : draftHost.trim();
    let username = draftUser.trim();
    let password = draftPassword;
    let urlParams = draftUrlParams.trim();
    if (selectedProfile.key === "postgres" && draftTlsMode) {
      const params = new URLSearchParams(urlParams);
      params.set("sslmode", draftTlsMode);
      urlParams = params.toString();
    } else if (selectedProfile.key === "mysql" && draftTlsMode) {
      const params = new URLSearchParams(urlParams);
      params.set("ssl-mode", draftTlsMode);
      urlParams = params.toString();
    }
    if (parsedMongoUrl) {
      host = parsedMongoUrl.host ?? host;
      const parsedMongoPort = Number.parseInt(parsedMongoUrl.port ?? "", 10);
      if (Number.isFinite(parsedMongoPort) && parsedMongoPort > 0) normalizedPort = parsedMongoPort;
      username = parsedMongoUrl.user ?? username;
      password = parsedMongoUrl.password ?? password;
      database = parsedMongoUrl.database ?? database;
      urlParams = parsedMongoUrl.urlParams ?? urlParams;
    }
    if (selectedProfile.key === "redis" && draftRedisConnectionMode === "sentinel") {
      const firstNode = firstRedisEndpoint(redisSentinelNodes, 26379);
      if (firstNode) {
        host = firstNode.host;
        normalizedPort = firstNode.port;
      }
    } else if (selectedProfile.key === "redis" && draftRedisConnectionMode === "cluster") {
      const firstNode = firstRedisEndpoint(redisClusterNodes, 6379);
      if (firstNode) {
        host = firstNode.host;
        normalizedPort = firstNode.port;
      }
    }
    if (!host) {
      throw new Error(selectedProfile.localFile ? t("database.filePathRequired") : t("database.hostRequired"));
    }
    const name = draftName.trim() || selectedProfile.label;
    const dbType = selectedProfile.key as DbxDatabaseType;
    const dbx = {
      ...existingDbx,
      id,
      name,
      db_type: dbType,
      driver_profile: selectedProfile.key,
      driver_label: selectedProfile.label,
      color: draftColor,
      url_params: urlParams || null,
      host,
      port: normalizedPort,
      username,
      password,
      database: database || null,
      connection_string: dbType === "mongodb" ? (draftMongoUseUrl ? rawConnectionString || null : null) : rawConnectionString || null,
      connect_timeout_secs: Number.isFinite(connectTimeoutSecs) && connectTimeoutSecs > 0 ? connectTimeoutSecs : 5,
      query_timeout_secs: Number.isFinite(queryTimeoutSecs) && queryTimeoutSecs >= 0 ? queryTimeoutSecs : 30,
      idle_timeout_secs: Number.isFinite(idleTimeoutSecs) && idleTimeoutSecs >= 0 ? idleTimeoutSecs : 60,
      keepalive_interval_secs: Number.isFinite(keepaliveIntervalSecs) && keepaliveIntervalSecs >= 0 ? keepaliveIntervalSecs : 0,
      ssl: tlsEnabled,
      ca_cert_path: draftCaCertPath.trim(),
      client_cert_path: draftClientCertPath.trim(),
      client_key_path: draftClientKeyPath.trim(),
      read_only: draftReadOnly,
      transport_layers: transportEnabled ? draftTransportLayers.map(transportLayerPayload) : [],
      ...(dbType === "redis"
        ? {
            redis_connection_mode: draftRedisConnectionMode,
            redis_sentinel_master: draftRedisConnectionMode === "sentinel" ? draftRedisSentinelMaster.trim() : undefined,
            redis_sentinel_nodes: draftRedisConnectionMode === "sentinel" ? redisSentinelNodes : undefined,
            redis_sentinel_username: draftRedisConnectionMode === "sentinel" ? draftRedisSentinelUsername.trim() : undefined,
            redis_sentinel_password: draftRedisConnectionMode === "sentinel" ? draftRedisSentinelPassword : undefined,
            redis_sentinel_tls: draftRedisConnectionMode === "sentinel" ? draftRedisSentinelTls : undefined,
            redis_cluster_nodes: draftRedisConnectionMode === "cluster" ? redisClusterNodes : undefined,
            redis_key_separator: draftRedisKeySeparator.trim() || ":",
          }
        : {}),
      ...(dbType === "oracle"
        ? {
            oracle_connection_type: draftOracleConnectionType,
            oracle_sysdba: draftOracleSysdba,
          }
        : {}),
    };

    return {
      id,
      name,
      dbType,
      readOnly: draftReadOnly,
      projectScope:
        editingDbxConnection
          ? editingDbxConnection.projectScope ?? null
          : projectRoot
            ? {
                kind: "local",
                projectRoot,
                remoteProjectPath: null,
                sshConnectionId: null,
              }
            : null,
      migratedFromLegacy: editingDbxConnection?.migratedFromLegacy,
      connectionGroup: editingDbxConnection ? editingDbxConnection.connectionGroup ?? null : draftConnectionGroup.trim() || null,
      pinned: editingDbxConnection?.pinned,
      dbx,
      createdAt: editingDbxConnection?.createdAt ?? now,
      lastOpenedAt: now,
    };
  }, [
    editingDbxConnection,
    draftConnectionGroup,
    draftDatabase,
    draftCaCertPath,
    draftClientCertPath,
    draftClientKeyPath,
    draftColor,
    draftConnectTimeoutSecs,
    draftConnectionString,
    draftFilePath,
    draftHost,
    draftIdleTimeoutSecs,
    draftKeepaliveIntervalSecs,
    draftName,
    draftPassword,
    draftPort,
    draftQueryTimeoutSecs,
    draftReadOnly,
    draftTransportLayers,
    draftMongoUseUrl,
    draftRedisClusterNodes,
    draftRedisConnectionMode,
    draftRedisKeySeparator,
    draftRedisSentinelMaster,
    draftRedisSentinelNodes,
    draftRedisSentinelPassword,
    draftRedisSentinelTls,
    draftRedisSentinelUsername,
    draftTlsMode,
    draftOracleConnectionType,
    draftOracleSysdba,
    draftUrlParams,
    draftUser,
    projectRoot,
    selectedProfile.key,
    selectedProfile.label,
    selectedProfile.localFile,
    selectedProfile.port,
    t,
    tlsEnabled,
    transportEnabled,
  ]);

  const handleNewQuery = useCallback(() => {
    if (!activeSqlCapable) return;
    setWorkspaceMode("query");
    setSql("");
    setSqlResult(null);
    setQueryResult(null);
    setActiveObject(null);
    setActiveDbxObject(null);
  }, [activeSqlCapable]);

  const openQueryHistory = useCallback(() => {
    setWorkspaceMode("query-history");
    setSqlResult(null);
    setQueryResult(null);
  }, []);

  const addQueryHistoryEntry = useCallback((entry: Omit<QueryHistoryEntry, "id" | "executedAt">) => {
    const statement = entry.sql.trim();
    if (!statement) return;
    setQueryHistory((current) => [
      {
        ...entry,
        sql: statement,
        id: `history:${Date.now()}:${Math.random().toString(36).slice(2)}`,
        executedAt: Date.now(),
      },
      ...current.filter((item) => item.sql !== statement || item.connectionName !== entry.connectionName).slice(0, 49),
    ]);
  }, []);

  const restoreQueryHistoryEntry = useCallback((entry: QueryHistoryEntry) => {
    setSql(entry.sql);
    setWorkspaceMode("query");
    setSqlResult(null);
    setQueryResult(null);
  }, []);

  const chooseSqlFile = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "SQL", extensions: ["sql"] }],
      defaultPath: projectRoot,
    });
    if (typeof selected !== "string") return;
    setSqlFilePath(selected);
    try {
      const preview = await databaseApi.readSqlFile(selected);
      setSqlFilePreview(preview.slice(0, 8000));
    } catch {
      setSqlFilePreview("");
    }
  }, [projectRoot]);

  const handleExecuteSqlFile = useCallback(() => {
    setWorkspaceMode("sql-file");
    setError(activeSqlCapable ? null : t("database.selectSqlConnection"));
    setSqlResult(null);
    setQueryResult(null);
  }, [activeSqlCapable, t]);

  const openAdvancedTool = useCallback((mode: DatabaseAdvancedToolMode) => {
    setWorkspaceMode(mode);
    setError(activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"));
    setSqlResult(null);
    setQueryResult(null);
  }, [activeDbxConnection, dbxHasSqlObjectBrowser, t]);

  const openUserAdmin = useCallback(() => {
    setWorkspaceMode("user-admin");
    setError(activeDbxConnection && supportsDbxUserAdmin(activeDbxConnection.dbType) ? null : t("database.selectUserAdminConnection"));
    setSqlResult(null);
    setQueryResult(null);
  }, [activeDbxConnection, t]);

  const loadDbxColumnsForTables = useCallback(
    async (objects: DbxObjectInfo[], connection = activeDbxConnection, database = activeDbxDatabase) => {
      if (!connection || !isSqlDbxConnection(connection)) return;
      const nextColumns: Record<string, DbxColumnInfo[]> = {};
      for (const object of objects.filter((item) => isDbxTableObject(item) || isDbxViewObject(item)).slice(0, 12)) {
        try {
          nextColumns[dbxObjectKey(object)] = await databaseApi.dbxGetColumns(
            connection.id,
            object.name,
            database,
            object.schema ?? null,
          );
        } catch {
          nextColumns[dbxObjectKey(object)] = [];
        }
      }
      setDbxColumnsByTable((current) => ({ ...current, ...nextColumns }));
    },
    [activeDbxConnection, activeDbxDatabase],
  );

  const openErDiagram = useCallback(async () => {
    setWorkspaceMode("er-diagram");
    setError(activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"));
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser) return;
    let objects = dbxObjects;
    if (objects.length === 0) {
      try {
        objects = await databaseApi.dbxListObjects(activeDbxConnection.id, activeDbxDatabase, null);
        setDbxObjects(objects);
      } catch (err) {
        setError(String(err));
        return;
      }
    }
    await loadDbxColumnsForTables(objects, activeDbxConnection, activeDbxDatabase);
  }, [activeDbxConnection, activeDbxDatabase, dbxHasSqlObjectBrowser, dbxObjects, loadDbxColumnsForTables, t]);

  const openDatabaseSearch = useCallback(async () => {
    setWorkspaceMode("database-search");
    setError(activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"));
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser || !activeDbxDatabase) return;
    if (dbxObjects.length === 0) {
      try {
        setDbxObjects(await databaseApi.dbxListObjects(activeDbxConnection.id, activeDbxDatabase, null));
      } catch (err) {
        setError(String(err));
      }
    }
  }, [activeDbxConnection, activeDbxDatabase, dbxHasSqlObjectBrowser, dbxObjects.length, t]);

  const openTableStructure = useCallback(async () => {
    setWorkspaceMode("table-structure");
    setError(activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxTable"));
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser) return;
    const targetObject = selectedDbxTable;
    if (!targetObject) return;
    setActiveDbxObject(targetObject);
    try {
      await loadDbxColumnsForTables([targetObject], activeDbxConnection, activeDbxDatabase);
    } catch (err) {
      setError(String(err));
    }
  }, [activeDbxConnection, activeDbxDatabase, dbxHasSqlObjectBrowser, loadDbxColumnsForTables, selectedDbxTable, t]);

  const openDbxObjectStructure = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      if (!isSqlDbxConnection(connection) || !isDbxTableObject(object)) return;
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(object.schema ?? null);
      setActiveDbxObject(object);
      setWorkspaceMode("table-structure");
      setError(null);
      try {
        await loadDbxColumnsForTables([object], connection, database);
      } catch (err) {
        setError(String(err));
      }
    },
    [loadDbxColumnsForTables],
  );

  const executeSqlFileFromPanel = useCallback(async () => {
    if (!sqlFilePath.trim()) return;
    setLoading(true);
    setError(null);
    try {
      if (activeDbxConnection && dbxHasSqlObjectBrowser) {
        const timeoutSecs = Number.parseInt(sqlFileTimeoutSecs, 10);
        const results = await databaseApi.dbxExecuteSqlFile({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: activeDbxSchema,
          path: sqlFilePath.trim(),
          timeoutSecs: Number.isFinite(timeoutSecs) && timeoutSecs > 0 ? timeoutSecs : undefined,
        });
        setSqlResult(dbxSqlFileResultsToExecuteResult(results));
        setQueryResult(null);
      } else if (activeEndpoint) {
        const fileSql = await databaseApi.readSqlFile(sqlFilePath.trim());
        const result = await databaseApi.executeSql({
          endpoint: activeEndpoint,
          sql: fileSql,
          page: 1,
          pageSize: PAGE_SIZE,
          readOnly: activeConnection?.readOnly ?? false,
          projectRoot,
        });
        setSql(fileSql);
        setSqlResult(result);
        setQueryResult(null);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [
    activeConnection?.readOnly,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxSchema,
    activeEndpoint,
    dbxHasSqlObjectBrowser,
    projectRoot,
    sqlFilePath,
    sqlFileTimeoutSecs,
  ]);

  const loadDbxDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null) => {
      setLoading(true);
      setError(null);
      try {
        const [objects, schemas] = await Promise.all([
          databaseApi.dbxListObjects(connection.id, database, null),
          databaseApi.dbxListSchemas(connection.id, database).then((value) => (Array.isArray(value) ? value : [])).catch(() => [] as string[]),
        ]);
        setActiveDbxDatabase(database);
        setActiveDbxSchema(null);
        setDbxSchemas(schemas.length > 0 ? schemas : deriveDbxSchemas(objects));
        setDbxObjects(objects);
        setActiveDbxObject(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const loadDbxSchema = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, schemaName: string) => {
      setLoading(true);
      setError(null);
      try {
        const objects = await databaseApi.dbxListObjects(connection.id, database, schemaName);
        setActiveDbxDatabase(database);
        setActiveDbxSchema(schemaName);
        setDbxSchemas((current) => (current.includes(schemaName) ? current : [...current, schemaName].sort()));
        setDbxObjects((current) => {
          const currentWithoutSchema = current.filter((object) => object.schema !== schemaName);
          return [...currentWithoutSchema, ...objects];
        });
        setActiveDbxObject(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const loadRedisSidebarDatabases = useCallback(async (connection: AeroricDbConnectionConfig) => {
    try {
      const databases = await databaseApi.dbxRedisListDatabases(connection.id);
      setRedisDatabasesByConnection((current) => ({ ...current, [connection.id]: databases }));
      return databases;
    } catch (err) {
      setError(String(err));
      return [] as RedisDatabaseInfo[];
    }
  }, []);

  const loadRedisSidebarKeys = useCallback(async (connection: AeroricDbConnectionConfig, database: number, append = false) => {
    const databaseKey = `${connection.id}:${database}`;
    const cursor = append ? (redisScanStateByDatabase[databaseKey]?.cursor ?? 0) : 0;
    try {
      const result = await databaseApi.dbxRedisScanKeys({
        connectionId: connection.id,
        db: database,
        cursor,
        pattern: "*",
        count: 100,
      });
      setRedisKeysByDatabase((current) => ({
        ...current,
        [databaseKey]: append ? [...(current[databaseKey] ?? []), ...result.keys] : result.keys,
      }));
      setRedisScanStateByDatabase((current) => ({
        ...current,
        [databaseKey]: { cursor: result.cursor, totalKeys: result.total_keys },
      }));
      return result.keys;
    } catch (err) {
      setError(String(err));
      return [] as RedisKeyInfo[];
    }
  }, [redisScanStateByDatabase]);

  const loadMongoSidebarDatabases = useCallback(async (connection: AeroricDbConnectionConfig) => {
    try {
      const databases = await databaseApi.dbxMongoListDatabases(connection.id);
      setMongoDatabasesByConnection((current) => ({ ...current, [connection.id]: databases }));
      return databases;
    } catch (err) {
      setError(String(err));
      return [] as string[];
    }
  }, []);

  const loadMongoSidebarCollections = useCallback(async (connection: AeroricDbConnectionConfig, database: string) => {
    try {
      const collections = await databaseApi.dbxMongoListCollections(connection.id, database);
      setMongoCollectionsByDatabase((current) => ({ ...current, [`${connection.id}:${database}`]: collections }));
      return collections;
    } catch (err) {
      setError(String(err));
      return [] as string[];
    }
  }, []);

  const loadMongoSidebarDocuments = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string,
      collection: string,
      append = false,
      queryOverride?: MongoSidebarDocumentQuery,
    ) => {
      const key = `${connection.id}:${database}:${collection}`;
      try {
        const query = queryOverride ?? mongoDocumentQueriesByCollection[key] ?? { filter: "{}", sort: "{}" };
        const skip = append ? mongoDocumentsByCollection[key]?.length ?? 0 : 0;
        const result = await databaseApi.dbxMongoFindDocuments({
          connectionId: connection.id,
          database,
          collection,
          filter: query.filter,
          sort: query.sort,
          skip,
          limit: MONGO_SIDEBAR_DOCUMENT_PREVIEW_LIMIT,
        });
        const nextDocuments = append ? [...(mongoDocumentsByCollection[key] ?? []), ...result.documents] : result.documents;
        setMongoDocumentsByCollection((current) => ({
          ...current,
          [key]: nextDocuments,
        }));
        setMongoDocumentTotalsByCollection((current) => ({
          ...current,
          [key]: result.total,
        }));
        return nextDocuments;
      } catch (err) {
        setError(String(err));
        return [] as unknown[];
      }
    },
    [mongoDocumentQueriesByCollection, mongoDocumentsByCollection],
  );

  const loadDbxConnection = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setSchema(null);
      setActiveObject(null);
      setActiveDbxObject(null);
      setQueryResult(null);
      setSqlResult(null);
      setSql("");
      setWorkspaceMode("table");
      setError(null);
      setActiveMongoWorkspaceDatabase(null);
      setActiveMongoDocumentId(null);
      setDbxDatabases([]);
      setDbxSchemas([]);
      setDbxObjects([]);

      if (["redis", "mongodb"].includes(connection.dbType)) {
        setActiveDbxDatabase(null);
        setActiveDbxSchema(null);
        setWorkspaceMode(connection.dbType === "redis" ? "redis" : "mongo");
        await databaseApi.dbxConnect(connection.id);
        if (connection.dbType === "redis") {
          const databases = await loadRedisSidebarDatabases(connection);
          const firstDb = databases[0]?.db;
          setActiveDbxDatabase(firstDb == null ? null : `db${firstDb}`);
        } else {
          const databases = await loadMongoSidebarDatabases(connection);
          const database = databases[0] ?? null;
          setActiveDbxDatabase(database);
        }
        return;
      }

      setLoading(true);
      try {
        await databaseApi.dbxConnect(connection.id);
        const databases = await databaseApi.dbxListDatabases(connection.id);
        const targetDatabase = configuredTargetDatabase(connection);
        const visibleDatabases = filterDbxDatabasesForConnection(databases, connection);
        if (targetDatabase && visibleDatabases.length === 0) {
          setActiveDbxDatabase(null);
          setActiveDbxSchema(null);
          setError(t("database.configuredDatabaseMissing", { database: targetDatabase }));
          return;
        }
        setDbxDatabases(visibleDatabases);
        const database = targetDatabase ?? visibleDatabases[0]?.name ?? null;
        setActiveDbxDatabase(database);
        setActiveDbxSchema(null);
        const [objects, schemas] = await Promise.all([
          databaseApi.dbxListObjects(connection.id, database, null),
          databaseApi.dbxListSchemas(connection.id, database).then((value) => (Array.isArray(value) ? value : [])).catch(() => [] as string[]),
        ]);
        setDbxSchemas(schemas.length > 0 ? schemas : deriveDbxSchemas(objects));
        setDbxObjects(objects);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadMongoSidebarDatabases, loadRedisSidebarDatabases, t],
  );

  const selectRedisSidebarDatabase = useCallback((connection: AeroricDbConnectionConfig, database: number) => {
    setActiveConnectionId(null);
    setActiveDbxConnectionId(connection.id);
    setActiveDbxDatabase(`db${database}`);
    setActiveDbxSchema(null);
    setActiveMongoDocumentId(null);
    setActiveMongoWorkspaceDatabase(null);
    setWorkspaceMode("redis");
  }, []);

  const selectRedisSidebarKey = useCallback((connection: AeroricDbConnectionConfig, database: number, keyRaw: string) => {
    setActiveConnectionId(null);
    setActiveDbxConnectionId(connection.id);
    setActiveDbxDatabase(`db${database}`);
    setActiveDbxSchema(keyRaw);
    setActiveMongoDocumentId(null);
    setActiveMongoWorkspaceDatabase(null);
    setWorkspaceMode("redis");
  }, []);

  const selectMongoSidebarDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(null);
      setActiveMongoDocumentId(null);
      setActiveMongoWorkspaceDatabase(database);
      setWorkspaceMode("mongo");
      await loadMongoSidebarCollections(connection, database);
    },
    [loadMongoSidebarCollections],
  );

  const selectMongoSidebarCollection = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string, collection: string) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(collection);
      setActiveMongoDocumentId(null);
      setActiveMongoWorkspaceDatabase(database);
      setWorkspaceMode("mongo");
      if (!mongoCollectionsByDatabase[`${connection.id}:${database}`]) {
        await loadMongoSidebarCollections(connection, database);
      }
    },
    [loadMongoSidebarCollections, mongoCollectionsByDatabase],
  );

  const selectMongoSidebarDocument = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string, collection: string, document: unknown) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(collection);
      setActiveMongoDocumentId(mongoDocumentId(document));
      setActiveMongoWorkspaceDatabase(database);
      setWorkspaceMode("mongo");
      if (!mongoDocumentsByCollection[`${connection.id}:${database}:${collection}`]) {
        await loadMongoSidebarDocuments(connection, database, collection);
      }
    },
    [loadMongoSidebarDocuments, mongoDocumentsByCollection],
  );

  const loadDbxObject = useCallback(
    async (
      object: DbxObjectInfo,
      nextPage: number,
      connection = activeDbxConnection,
      database = activeDbxDatabase,
      whereInput?: string | null,
      orderBy?: string | null,
      pageSize = dbxGridPageSize,
    ) => {
      if (!connection) return;
      const normalizedWhereInput = whereInput?.trim() ?? "";
      const normalizedOrderBy = orderBy?.trim() ?? "";
      const sameDbxObject = activeDbxObject?.name === object.name && activeDbxObject?.schema === object.schema;
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const result = await databaseApi.dbxQueryTableData({
          connectionId: connection.id,
          database,
          schema: object.schema ?? null,
          table: object.name,
          page: nextPage,
          pageSize,
          whereInput: normalizedWhereInput || null,
          orderBy: normalizedOrderBy || null,
        });
        let objectColumns: DbxColumnInfo[] = [];
        if (isDbxTableObject(object)) {
          try {
            objectColumns = await databaseApi.dbxGetColumns(connection.id, object.name, database, object.schema ?? null);
            setDbxColumnsByTable((current) => ({ ...current, [dbxObjectKey(object)]: objectColumns }));
          } catch {
            objectColumns = [];
          }
        }
        const primaryKeys = objectColumns.filter((column) => column.is_primary_key).map((column) => column.name);
        const editable = isDbxTableObject(object) && primaryKeys.length > 0 && !connection.readOnly;
        const resultRows = dbxRowsToDatabaseRows(result.result.rows);
        setActiveDbxObject(object);
        setActiveDbxSchema(object.schema ?? null);
        setWorkspaceMode("table");
        setActiveObject({
          name: object.name,
          objectType: object.object_type,
          columns: dbxColumnsToDbColumns(objectColumns),
          indexes: [],
          foreignKeys: [],
          triggers: [],
          editable,
          primaryKeys,
          hasRowId: false,
        });
        setPage(nextPage);
        setQueryResult({
          columns: result.result.columns,
          columnTypes: result.result.column_types,
          columnSortables: result.result.column_sortables,
          rows: resultRows,
          page: nextPage,
          pageSize,
          totalRows: result.totalRows ?? null,
          editable,
          primaryKeys,
          hasRowId: false,
        });
        setDbxGridSelectedRows(new Set());
        setDbxGridWhereInput(normalizedWhereInput);
        setDbxGridOrderByInput(normalizedOrderBy);
        if (!sameDbxObject) {
          setDbxGridSearch("");
          setDbxGridColumnSearch("");
          setDbxGridHiddenColumns(new Set());
          setDbxGridColumnWidths(initialDbxGridColumnWidths(result.result.columns, resultRows));
        } else {
          setDbxGridColumnWidths((current) =>
            Object.keys(current).length === 0 ? initialDbxGridColumnWidths(result.result.columns, resultRows) : current,
          );
        }
        setSql(result.sql);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeDbxConnection, activeDbxDatabase, activeDbxObject, dbxGridPageSize],
  );

  const submitConnection = useCallback(async () => {
    if (editingDbxConnectionId || selectedProfile.key !== "sqlite") {
      setLoading(true);
      setError(null);
      try {
        const connection = buildDbxConnectionDraft();
        await databaseApi.dbxSaveConnection(connection);
        const next = await databaseApi.dbxListConnections();
        setDbxConnections(next);
        setConnectionDialogOpen(false);
        setEditingDbxConnectionId(null);
        await loadDbxConnection(connection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
      return;
    }
    const path = draftFilePath.trim();
    if (!path) return;
    addConnection({ kind: "local", path });
    setConnectionDialogOpen(false);
  }, [addConnection, buildDbxConnectionDraft, draftFilePath, editingDbxConnectionId, loadDbxConnection, selectedProfile.key]);

  const reloadActiveDbxGrid = useCallback(
    async (whereInput = dbxGridWhereInput, orderBy = dbxGridOrderByInput) => {
      if (!activeDbxConnection || !activeDbxObject) return;
      await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, whereInput, orderBy);
    },
    [activeDbxConnection, activeDbxDatabase, activeDbxObject, dbxGridOrderByInput, dbxGridWhereInput, loadDbxObject],
  );

  const resetActiveDbxGrid = useCallback(async () => {
    setDbxGridWhereInput("");
    setDbxGridOrderByInput("");
    setDbxGridSearch("");
    setDbxGridColumnSearch("");
    setDbxGridHiddenColumns(new Set());
    setDbxGridColumnWidths({});
    if (!activeDbxConnection || !activeDbxObject) return;
    await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, "", "");
  }, [activeDbxConnection, activeDbxDatabase, activeDbxObject, loadDbxObject]);

  const toggleDbxGridColumnSort = useCallback(
    async (column: string) => {
      if (!activeDbxConnection || !activeDbxObject || !queryResult) return;
      const columnIndex = queryResult.columns.indexOf(column);
      if (!dbxGridColumnSortable(queryResult, columnIndex)) return;
      const nextOrderBy = nextDbxOrderByForColumn(dbxGridOrderByInput, column);
      setDbxGridOrderByInput(nextOrderBy);
      await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, nextOrderBy);
    },
    [activeDbxConnection, activeDbxDatabase, activeDbxObject, dbxGridOrderByInput, dbxGridWhereInput, loadDbxObject, queryResult],
  );

  const changeDbxGridPageSize = useCallback(
    async (nextPageSize: number) => {
      setDbxGridPageSize(nextPageSize);
      if (!activeDbxConnection || !activeDbxObject || !queryResult) return;
      await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, dbxGridOrderByInput, nextPageSize);
    },
    [activeDbxConnection, activeDbxDatabase, activeDbxObject, dbxGridOrderByInput, dbxGridWhereInput, loadDbxObject, queryResult],
  );

  const toggleDbxGridColumnVisibility = useCallback((column: string) => {
    setDbxGridHiddenColumns((current) => {
      const next = new Set(current);
      if (next.has(column)) {
        next.delete(column);
      } else {
        const visibleCount = tableColumns.filter((item) => !next.has(item)).length;
        if (visibleCount <= 1) return current;
        next.add(column);
      }
      return next;
    });
  }, [tableColumns]);

  const showAllDbxGridColumns = useCallback(() => {
    setDbxGridHiddenColumns(new Set());
  }, []);

  const invertDbxGridColumnVisibility = useCallback(() => {
    setDbxGridHiddenColumns((current) => {
      if (tableColumns.length <= 1) return new Set();
      const next = new Set<string>();
      tableColumns.forEach((column) => {
        if (!current.has(column)) next.add(column);
      });
      if (next.size >= tableColumns.length) next.delete(tableColumns[0]);
      return next;
    });
  }, [tableColumns]);

  const hideNullDbxGridColumns = useCallback(() => {
    if (dbxGridNullColumns.size === 0) return;
    setDbxGridHiddenColumns((current) => {
      const next = new Set(current);
      dbxGridNullColumns.forEach((column) => next.add(column));
      if (tableColumns.length > 0 && next.size >= tableColumns.length) {
        const fallbackColumn = tableColumns.find((column) => !current.has(column)) ?? tableColumns[0];
        next.delete(fallbackColumn);
      }
      return next;
    });
  }, [dbxGridNullColumns, tableColumns]);

  const startDbxGridColumnResize = useCallback(
    (column: string, event: ReactPointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.stopPropagation();
      dbxGridColumnResizeStartRef.current = {
        column,
        x: event.clientX,
        width: dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH,
      };
      setResizingDbxGridColumn(column);
    },
    [dbxGridColumnWidths],
  );

  const autoFitDbxGridColumn = useCallback(
    (column: string) => {
      const columnIndex = tableColumns.indexOf(column);
      if (columnIndex < 0) return;
      const nextWidth = estimateDbxGridColumnWidth(column, columnIndex, rawTableRows);
      setDbxGridColumnWidths((current) => (current[column] === nextWidth ? current : { ...current, [column]: nextWidth }));
      setResizingDbxGridColumn(null);
    },
    [rawTableRows, tableColumns],
  );

  const toggleDbxGridRowSelection = useCallback((rowIndex: number) => {
    setDbxGridSelectedRows((current) => {
      const next = new Set(current);
      if (next.has(rowIndex)) {
        next.delete(rowIndex);
      } else {
        next.add(rowIndex);
      }
      return next;
    });
  }, []);

  const toggleVisibleDbxGridRowsSelection = useCallback(() => {
    setDbxGridSelectedRows((current) => {
      const next = new Set(current);
      if (allVisibleDbxGridRowsSelected) {
        visibleDbxGridRowIndexes.forEach((rowIndex) => next.delete(rowIndex));
      } else {
        visibleDbxGridRowIndexes.forEach((rowIndex) => next.add(rowIndex));
      }
      return next;
    });
  }, [allVisibleDbxGridRowsSelected, visibleDbxGridRowIndexes]);

  const testDbxConnectionDraft = useCallback(async () => {
    setLoading(true);
    setConnectionTestResult(null);
    try {
      await databaseApi.dbxTestConnection(buildDbxConnectionDraft());
      setConnectionTestResult({ success: true, message: t("database.connectionTestOk") });
    } catch (err) {
      setConnectionTestResult({ success: false, message: String(err) });
    } finally {
      setLoading(false);
    }
  }, [buildDbxConnectionDraft, t]);

  const handleSelectConnection = useCallback(
    (connection: DbConnectionConfig) => {
      setActiveDbxConnectionId(null);
      setDbxDatabases([]);
      setDbxObjects([]);
      setActiveDbxDatabase(null);
      setActiveDbxObject(null);
      setActiveConnectionId(connection.id);
      setWorkspaceMode("table");
      inspect(connection);
    },
    [inspect],
  );

  const handleSelectDbxConnection = useCallback(
    (connection: AeroricDbConnectionConfig) => {
      loadDbxConnection(connection);
    },
    [loadDbxConnection],
  );

  const handleDeleteConnection = useCallback(
    async (connectionId: string) => {
      const ok = await confirm(t("database.confirmDeleteConnection"), {
        title: t("database.deleteConnection"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      const next = connections.filter((connection) => connection.id !== connectionId);
      saveConnections(next);
      if (activeConnectionId === connectionId) {
        setActiveConnectionId(next[0]?.id ?? null);
        setSchema(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      }
    },
    [activeConnectionId, connections, saveConnections, t],
  );

  const handleDeleteDbxConnection = useCallback(
    async (connectionId: string) => {
      const ok = await confirm(t("database.confirmDeleteConnection"), {
        title: t("database.deleteConnection"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxDeleteConnection(connectionId);
        const next = await databaseApi.dbxListConnections();
        setDbxConnections(next);
        if (activeDbxConnectionId === connectionId) {
          setActiveDbxConnectionId(next[0]?.id ?? null);
          setSchema(null);
          setActiveObject(null);
          setQueryResult(null);
          setSqlResult(null);
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeDbxConnectionId, t],
  );

  const toggleReadOnly = useCallback(() => {
    if (!activeConnection) return;
    const next = connections.map((connection) =>
      connection.id === activeConnection.id
        ? { ...connection, readOnly: !connection.readOnly }
        : connection,
    );
    saveConnections(next);
  }, [activeConnection, connections, saveConnections]);

  const loadTable = useCallback(
    async (object: DbObject, nextPage: number) => {
      if (!activeEndpoint) return;
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const result = await databaseApi.queryTable(
          activeEndpoint,
          object.name,
          nextPage,
          PAGE_SIZE,
          projectRoot,
        );
        setActiveObject(object);
        setWorkspaceMode("table");
        setPage(nextPage);
        setQueryResult(result);
        setSql(`SELECT * FROM ${quoteSqlName(object.name)}`);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeEndpoint, projectRoot],
  );

  const refresh = useCallback(() => {
    if (activeConnection) inspect(activeConnection);
    if (activeDbxConnection) void loadDbxConnection(activeDbxConnection);
  }, [activeConnection, activeDbxConnection, inspect, loadDbxConnection]);

  const copyDbxConnection = useCallback(async (connection: AeroricDbConnectionConfig) => {
    const copy: AeroricDbConnectionConfig = {
      ...connection,
      id: `dbx:${Date.now()}:${Math.random().toString(36).slice(2)}`,
      name: `${connection.name} (Copy)`,
      createdAt: Date.now(),
      lastOpenedAt: Date.now(),
    };
    await databaseApi.dbxSaveConnection(copy);
    setDbxConnections(await databaseApi.dbxListConnections());
  }, []);

  const copyLegacyConnection = useCallback(
    (connection: DbConnectionConfig) => {
      const now = Date.now();
      saveConnections([
        {
          ...connection,
          id: `db:${now}:${Math.random().toString(36).slice(2)}`,
          name: `${connection.name} (Copy)`,
          createdAt: now,
          lastOpenedAt: now,
        },
        ...connections,
      ]);
    },
    [connections, saveConnections],
  );

  const copyNodeName = useCallback((name: string) => {
    navigator.clipboard?.writeText(name).catch((err) => {
      setError(String(err));
    });
  }, []);

  const closeVisibleDatabasesDialog = useCallback(() => {
    setVisibleDatabaseConnectionId(null);
    setVisibleDatabaseNames([]);
    setVisibleDatabaseSelection(new Set());
    setVisibleDatabaseSearch("");
    setVisibleDatabaseShowSystem(false);
    setVisibleDatabaseError("");
  }, []);

  const loadVisibleDatabaseNames = useCallback(async (connection: AeroricDbConnectionConfig): Promise<string[]> => {
    await databaseApi.dbxConnect(connection.id);
    if (connection.dbType === "redis") {
      return (await databaseApi.dbxRedisListDatabases(connection.id)).map((database) => String(database.db));
    }
    if (connection.dbType === "mongodb") {
      return databaseApi.dbxMongoListDatabases(connection.id);
    }
    return (await databaseApi.dbxListDatabases(connection.id)).map((database) => database.name);
  }, []);

  const openVisibleDatabasesDialog = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      setContextMenu(null);
      setVisibleDatabaseConnectionId(connection.id);
      setVisibleDatabaseNames([]);
      setVisibleDatabaseSelection(new Set());
      setVisibleDatabaseSearch("");
      setVisibleDatabaseShowSystem(false);
      setVisibleDatabaseLoading(true);
      setVisibleDatabaseError("");
      try {
        const names = await loadVisibleDatabaseNames(connection);
        const configured = configuredVisibleDatabases(connection);
        const initialSelection = configured
          ? normalizeVisibleDatabaseSelection(configured, names)
          : names.filter((name) => !isSystemDatabaseName(connection.dbType, name));
        setVisibleDatabaseNames(names);
        setVisibleDatabaseSelection(new Set(initialSelection));
        setVisibleDatabaseShowSystem(initialSelection.some((name) => isSystemDatabaseName(connection.dbType, name)));
      } catch (err) {
        setVisibleDatabaseNames([]);
        setVisibleDatabaseSelection(new Set());
        setVisibleDatabaseError(String(err));
      } finally {
        setVisibleDatabaseLoading(false);
      }
    },
    [loadVisibleDatabaseNames],
  );

  const saveVisibleDatabaseConfig = useCallback(
    async (connection: AeroricDbConnectionConfig, visibleDatabases: string[] | undefined) => {
      const currentDbx = connection.dbx && typeof connection.dbx === "object" ? (connection.dbx as Record<string, unknown>) : {};
      const nextDbx = { ...currentDbx };
      if (visibleDatabases) {
        nextDbx.visible_databases = visibleDatabases;
      } else {
        delete nextDbx.visible_databases;
      }
      const nextConnection: AeroricDbConnectionConfig = {
        ...connection,
        dbx: nextDbx,
      };
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxSaveConnection(nextConnection);
        setDbxConnections(await databaseApi.dbxListConnections());
        if (activeDbxConnectionId === connection.id) {
          await loadDbxConnection(nextConnection);
        }
        closeVisibleDatabasesDialog();
      } catch (err) {
        setError(String(err));
        setVisibleDatabaseError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeDbxConnectionId, closeVisibleDatabasesDialog, loadDbxConnection],
  );

  const saveDbxDefaultDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null) => {
      const currentDbx = connection.dbx && typeof connection.dbx === "object" ? (connection.dbx as Record<string, unknown>) : {};
      const nextDbx = { ...currentDbx };
      if (database) {
        nextDbx.database = database;
      } else {
        delete nextDbx.database;
      }
      const nextConnection: AeroricDbConnectionConfig = {
        ...connection,
        dbx: nextDbx,
      };
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxSaveConnection(nextConnection);
        setDbxConnections(await databaseApi.dbxListConnections());
        if (activeDbxConnectionId === connection.id) {
          await loadDbxConnection(nextConnection);
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeDbxConnectionId, loadDbxConnection],
  );

  const saveVisibleDatabaseSelection = useCallback(async () => {
    if (!visibleDatabaseConnection || !visibleDatabaseCanSave) return;
    const normalized = normalizeVisibleDatabaseSelection([...visibleDatabaseSelection], visibleDatabaseNames);
    await saveVisibleDatabaseConfig(visibleDatabaseConnection, normalized);
  }, [saveVisibleDatabaseConfig, visibleDatabaseCanSave, visibleDatabaseConnection, visibleDatabaseNames, visibleDatabaseSelection]);

  const showAllVisibleDatabases = useCallback(async () => {
    if (!visibleDatabaseConnection) return;
    await saveVisibleDatabaseConfig(visibleDatabaseConnection, undefined);
  }, [saveVisibleDatabaseConfig, visibleDatabaseConnection]);

  const toggleVisibleDatabaseSelection = useCallback((database: string) => {
    setVisibleDatabaseSelection((current) => {
      const next = new Set(current);
      if (next.has(database)) next.delete(database);
      else next.add(database);
      return next;
    });
  }, []);

  const closeDatabaseExportDialog = useCallback(() => {
    setDatabaseExportTarget(null);
    setDatabaseExportTables([]);
    setDatabaseExportSelection(new Set());
    setDatabaseExportSearch("");
    setDatabaseExportIncludeStructure(true);
    setDatabaseExportIncludeData(true);
    setDatabaseExportIncludeObjects(true);
    setDatabaseExportDropTableIfExists(false);
    setDatabaseExportError("");
  }, []);

  const openDatabaseExportDialog = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string,
      schema: string | null = null,
      preselectedTables: string[] = [],
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      setContextMenu(null);
      setDatabaseExportTarget({ connectionId: connection.id, database, schema, preselectedTables });
      setDatabaseExportTables([]);
      setDatabaseExportSelection(new Set());
      setDatabaseExportSearch("");
      setDatabaseExportIncludeStructure(true);
      setDatabaseExportIncludeData(true);
      setDatabaseExportIncludeObjects(true);
      setDatabaseExportDropTableIfExists(false);
      setDatabaseExportLoading(true);
      setDatabaseExportError("");
      try {
        await databaseApi.dbxConnect(connection.id);
        const objects = await databaseApi.dbxListObjects(connection.id, database, schema);
        const tableNames = Array.from(
          new Set(
            objects
              .filter((object) => isDbxTableObject(object) || isDbxViewObject(object))
              .map((object) => object.name)
              .filter(Boolean),
          ),
        ).sort((left, right) => left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }));
        const preselected = preselectedTables.filter((table) => tableNames.includes(table));
        setDatabaseExportTables(tableNames);
        setDatabaseExportSelection(new Set(preselected.length > 0 ? preselected : tableNames));
      } catch (err) {
        setDatabaseExportTables([]);
        setDatabaseExportSelection(new Set());
        setDatabaseExportError(String(err));
      } finally {
        setDatabaseExportLoading(false);
      }
    },
    [],
  );

  const toggleDatabaseExportTable = useCallback((table: string) => {
    setDatabaseExportSelection((current) => {
      const next = new Set(current);
      if (next.has(table)) next.delete(table);
      else next.add(table);
      return next;
    });
  }, []);

  const submitDatabaseExport = useCallback(async () => {
    if (!databaseExportConnection || !databaseExportTarget || !databaseExportCanRun) return;
    const safeName = (databaseExportTarget.database || "database").replace(/[\\/:*?"<>|]+/g, "_").trim() || "database";
    const filePath = await saveDialog({
      defaultPath: `${safeName}.sql`,
      filters: [{ name: "SQL", extensions: ["sql"] }],
    });
    if (typeof filePath !== "string" || !filePath.trim()) return;

    const selectedTables =
      databaseExportSelection.size === databaseExportTables.length
        ? undefined
        : databaseExportTables.filter((table) => databaseExportSelection.has(table));
    setLoading(true);
    setDatabaseExportLoading(true);
    setError(null);
    setDatabaseExportError("");
    try {
      await databaseApi.dbxExportDatabase({
        exportId: `export:${Date.now()}:${Math.random().toString(36).slice(2)}`,
        connectionId: databaseExportConnection.id,
        database: databaseExportTarget.database,
        schema: databaseExportTarget.schema || databaseExportTarget.database,
        filePath: filePath.trim(),
        selectedTables,
        includeStructure: databaseExportIncludeStructure,
        includeData: databaseExportIncludeData,
        includeObjects: databaseExportIncludeObjects,
        dropTableIfExists: databaseExportDropTableIfExists,
        batchSize: 1000,
      });
      closeDatabaseExportDialog();
    } catch (err) {
      setError(String(err));
      setDatabaseExportError(String(err));
    } finally {
      setLoading(false);
      setDatabaseExportLoading(false);
    }
  }, [
    closeDatabaseExportDialog,
    databaseExportCanRun,
    databaseExportConnection,
    databaseExportDropTableIfExists,
    databaseExportIncludeData,
    databaseExportIncludeObjects,
    databaseExportIncludeStructure,
    databaseExportSelection,
    databaseExportTables,
    databaseExportTarget,
  ]);

  const closeTableImportDialog = useCallback(() => {
    setTableImportTarget(null);
    setTableImportColumns([]);
    setTableImportPreview(null);
    setTableImportMappings({});
    setTableImportMode("append");
    setTableImportBatchSize("500");
    setTableImportError("");
  }, []);

  const openTableImportDialog = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      if (!isSqlDbxConnection(connection) || !isDbxTableObject(object)) return;
      setContextMenu(null);
      setTableImportTarget({ connectionId: connection.id, database, object });
      setTableImportColumns([]);
      setTableImportPreview(null);
      setTableImportMappings({});
      setTableImportMode("append");
      setTableImportBatchSize("500");
      setTableImportLoading(true);
      setTableImportError("");
      try {
        const columns = await databaseApi.dbxGetColumns(connection.id, object.name, database, object.schema ?? null);
        setTableImportColumns(columns);
      } catch (err) {
        setTableImportError(String(err));
      } finally {
        setTableImportLoading(false);
      }
    },
    [],
  );

  const chooseTableImportFile = useCallback(async () => {
    if (!tableImportTarget) return;
    const selected = await openDialog({
      multiple: false,
      filters: [
        { name: "Data files", extensions: ["csv", "tsv", "json", "xlsx", "xlsm", "xls"] },
        { name: "CSV", extensions: ["csv", "tsv"] },
        { name: "JSON", extensions: ["json"] },
        { name: "Excel", extensions: ["xlsx", "xlsm", "xls"] },
      ],
    });
    if (typeof selected !== "string" || !selected.trim()) return;

    setTableImportLoading(true);
    setTableImportError("");
    try {
      const preview = await databaseApi.dbxPreviewTableImportFile(selected.trim());
      setTableImportPreview(preview);
      setTableImportMappings(autoMapImportColumns(preview.columns, tableImportTargetColumnNames));
    } catch (err) {
      setTableImportPreview(null);
      setTableImportMappings({});
      setTableImportError(String(err));
    } finally {
      setTableImportLoading(false);
    }
  }, [tableImportTarget, tableImportTargetColumnNames]);

  const updateTableImportMapping = useCallback((sourceColumn: string, targetColumn: string) => {
    setTableImportMappings((current) => ({
      ...current,
      [sourceColumn]: targetColumn,
    }));
  }, []);

  const submitTableImport = useCallback(async () => {
    if (!tableImportConnection || !tableImportTarget || !tableImportPreview || tableImportMappedColumns.length === 0) return;
    setLoading(true);
    setTableImportLoading(true);
    setError(null);
    setTableImportError("");
    try {
      await databaseApi.dbxImportTableFile({
        importId: `import:${Date.now()}:${Math.random().toString(36).slice(2)}`,
        connectionId: tableImportConnection.id,
        database: tableImportTarget.database || "",
        schema: tableImportTarget.object.schema || "",
        table: tableImportTarget.object.name,
        filePath: tableImportPreview.filePath,
        mappings: tableImportMappedColumns,
        mode: tableImportMode,
        batchSize: Math.max(1, Number(tableImportBatchSize) || 500),
      });
      const importedObject = tableImportTarget.object;
      const importedDatabase = tableImportTarget.database;
      closeTableImportDialog();
      await loadDbxObject(importedObject, 1, tableImportConnection, importedDatabase);
    } catch (err) {
      setError(String(err));
      setTableImportError(String(err));
    } finally {
      setLoading(false);
      setTableImportLoading(false);
    }
  }, [
    closeTableImportDialog,
    loadDbxObject,
    tableImportBatchSize,
    tableImportConnection,
    tableImportMappedColumns,
    tableImportMode,
    tableImportPreview,
    tableImportTarget,
  ]);

  const renameLegacyConnection = useCallback(
    (connection: DbConnectionConfig) => {
      const nextName = window.prompt(t("database.renameConnectionPrompt"), connection.name)?.trim();
      if (!nextName || nextName === connection.name) return;
      saveConnections(connections.map((item) => (item.id === connection.id ? { ...item, name: nextName } : item)));
    },
    [connections, saveConnections, t],
  );

  const renameDbxConnection = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      const nextName = window.prompt(t("database.renameConnectionPrompt"), connection.name)?.trim();
      if (!nextName || nextName === connection.name) return;
      const currentDbx =
        connection.dbx && typeof connection.dbx === "object" ? (connection.dbx as Record<string, unknown>) : {};
      const nextConnection: AeroricDbConnectionConfig = {
        ...connection,
        name: nextName,
        dbx: {
          ...currentDbx,
          name: nextName,
        },
      };
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxSaveConnection(nextConnection);
        setDbxConnections(await databaseApi.dbxListConnections());
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [t],
  );

  const saveDbxConnectionMetadata = useCallback(async (connection: AeroricDbConnectionConfig, patch: Partial<AeroricDbConnectionConfig>) => {
    setLoading(true);
    setError(null);
    try {
      await databaseApi.dbxSaveConnection({ ...connection, ...patch });
      setDbxConnections(await databaseApi.dbxListConnections());
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const toggleDbxConnectionPinned = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      await saveDbxConnectionMetadata(connection, { pinned: !connection.pinned });
    },
    [saveDbxConnectionMetadata],
  );

  const togglePinnedTreeNode = useCallback((nodeId: string) => {
    setPinnedTreeNodeIds((current) => {
      const next = new Set(current);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      savePinnedTreeNodeIds(next);
      return next;
    });
  }, []);

  const addExtraDbxConnectionGroup = useCallback((groupName: string) => {
    const normalized = groupName.trim();
    if (!normalized) return;
    setExtraDbxConnectionGroups((current) => {
      if (current.some((group) => group.trim() === normalized)) return current;
      const next = [...current, normalized].sort((left, right) => left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }));
      saveExtraDbxConnectionGroups(next);
      return next;
    });
  }, []);

  const renameExtraDbxConnectionGroup = useCallback((oldName: string, newName: string) => {
    setExtraDbxConnectionGroups((current) => {
      const next = Array.from(
        new Set(current.map((group) => (group.trim() === oldName ? newName : group)).filter((group) => group.trim().length > 0)),
      ).sort((left, right) => left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }));
      saveExtraDbxConnectionGroups(next);
      return next;
    });
  }, []);

  const removeExtraDbxConnectionGroup = useCallback((groupName: string) => {
    setExtraDbxConnectionGroups((current) => {
      const next = current.filter((group) => group.trim() !== groupName);
      saveExtraDbxConnectionGroups(next);
      return next;
    });
  }, []);

  const moveDbxConnectionToGroup = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      const nextGroup = window.prompt(t("database.connectionGroupPrompt"), connection.connectionGroup ?? "")?.trim();
      if (nextGroup === undefined) return;
      await saveDbxConnectionMetadata(connection, { connectionGroup: nextGroup || null });
    },
    [saveDbxConnectionMetadata, t],
  );

  const renameDbxConnectionGroup = useCallback(
    async (groupName: string) => {
      const nextGroup = window.prompt(t("database.renameConnectionGroupPrompt"), groupName)?.trim();
      if (!nextGroup || nextGroup === groupName) return;
      setLoading(true);
      setError(null);
      try {
        renameExtraDbxConnectionGroup(groupName, nextGroup);
        await Promise.all(
          dbxConnections
            .filter((connection) => connection.connectionGroup?.trim() === groupName)
            .map((connection) => databaseApi.dbxSaveConnection({ ...connection, connectionGroup: nextGroup })),
        );
        setDbxConnections(await databaseApi.dbxListConnections());
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [dbxConnections, renameExtraDbxConnectionGroup, t],
  );

  const deleteDbxConnectionGroup = useCallback(
    async (groupName: string) => {
      const ok = await confirm(t("database.confirmDeleteConnectionGroup", { name: groupName }), {
        title: t("database.deleteConnectionGroup"),
        kind: "warning",
        okLabel: t("database.deleteConnectionGroup"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      setLoading(true);
      setError(null);
      try {
        removeExtraDbxConnectionGroup(groupName);
        await Promise.all(
          dbxConnections
            .filter((connection) => connection.connectionGroup?.trim() === groupName)
            .map((connection) => databaseApi.dbxSaveConnection({ ...connection, connectionGroup: null })),
        );
        setDbxConnections(await databaseApi.dbxListConnections());
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [dbxConnections, removeExtraDbxConnectionGroup, t],
  );

  const createDatabaseConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === createDatabaseConnectionId) ?? null,
    [createDatabaseConnectionId, dbxConnections],
  );

  const openCreateDatabaseDialog = useCallback((connection: AeroricDbConnectionConfig) => {
    setCreateDatabaseConnectionId(connection.id);
    setCreateDatabaseName("");
    setCreateDatabaseCharset("utf8mb4");
    setCreateDatabaseCollation("utf8mb4_unicode_ci");
    setError(null);
  }, []);

  const closeCreateDatabaseDialog = useCallback(() => {
    setCreateDatabaseConnectionId(null);
    setCreateDatabaseName("");
  }, []);

  const submitCreateDatabase = useCallback(async () => {
    const connection = createDatabaseConnection;
    const name = createDatabaseName.trim();
    if (!connection || !name) return;
    setLoading(true);
    setError(null);
    try {
      const sql = await databaseApi.dbxBuildCreateDatabaseSql({
        databaseType: connection.dbType,
        driverProfile: dbxDriverProfile(connection),
        name,
        charset: canSetCreateDatabaseCharset(connection) ? createDatabaseCharset : null,
        collation: canSetCreateDatabaseCharset(connection) ? createDatabaseCollation : null,
      });
      await databaseApi.dbxExecuteQuery({
        connectionId: connection.id,
        database: "",
        sql,
      });
      closeCreateDatabaseDialog();
      await loadDbxConnection(connection);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [
    closeCreateDatabaseDialog,
    createDatabaseCharset,
    createDatabaseCollation,
    createDatabaseConnection,
    createDatabaseName,
    loadDbxConnection,
  ]);

  const createDuckDbAttachedDatabaseFile = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      const selectedPath = await saveDialog({
        defaultPath: "database.duckdb",
        filters: [{ name: "DuckDB", extensions: ["duckdb", "db"] }],
      });
      if (typeof selectedPath !== "string" || !selectedPath.trim()) return;

      const path = ensureDuckDbFileExtension(selectedPath.trim());
      setLoading(true);
      setError(null);
      try {
        await databaseApi.dbxConnect(connection.id);
        const databases = await databaseApi.dbxListDatabases(connection.id);
        const name = uniqueDuckDbAttachedDatabaseName(
          duckDbAttachedDatabaseNameFromPath(path),
          databases.map((database) => database.name),
        );
        const sql = await databaseApi.dbxBuildDuckDbAttachDatabaseSql(path, name);
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database: "",
          sql,
        });

        const currentDbx =
          connection.dbx && typeof connection.dbx === "object" ? (connection.dbx as Record<string, unknown>) : {};
        const nextConnection: AeroricDbConnectionConfig = {
          ...connection,
          dbx: {
            ...currentDbx,
            attached_databases: [...dbxAttachedDatabaseRecords(connection), { name, path }],
          },
        };
        await databaseApi.dbxSaveConnection(nextConnection);
        setDbxConnections(await databaseApi.dbxListConnections());
        await loadDbxConnection(nextConnection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxConnection],
  );

  const dropDbxTableObject = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      if (!isSqlDbxConnection(connection) || !isDbxTableObject(object)) return;
      const tableOptions = {
        databaseType: connection.dbType,
        schema: object.schema ?? null,
        tableName: object.name,
      };
      setLoading(true);
      setError(null);
      try {
        const sql = await databaseApi.dbxBuildDropTableSql(tableOptions);
        const ok = await confirm(`${t("database.confirmDropTable", { name: dbxObjectKey(object) })}\n\n${sql}`, {
          title: t("database.dropTable"),
          kind: "warning",
          okLabel: t("database.dropTable"),
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database,
          sql,
        });
        await loadDbxConnection(connection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxConnection, t],
  );

  const showDbxObjectDdl = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      if (!isSqlDbxConnection(connection)) return;
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxSchema(object.schema ?? null);
      setActiveDbxObject(object);
      setWorkspaceMode("query");
      setLoading(true);
      setError(null);
      try {
        const ddl = await databaseApi.dbxGetTableDdl(connection.id, object.name, database, object.schema ?? null);
        setSql(ddl);
        setSqlResult(dbxQueryToExecuteResult({
          columns: ["ddl"],
          column_types: ["text"],
          column_sortables: [false],
          rows: [[ddl]],
          affected_rows: 0,
          execution_time_ms: 0,
          truncated: false,
          has_more: false,
        }));
        setQueryResult(null);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const showDbxObjectSource = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      if (!isSqlDbxConnection(connection)) return;
      const objectType = dbxObjectSourceKind(object);
      if (!objectType) return;
      const schema = object.schema ?? database ?? "";
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxObject(object);
      setWorkspaceMode("query");
      setLoading(true);
      setError(null);
      try {
        const source = await databaseApi.dbxGetObjectSource(connection.id, database, schema, object.name, objectType);
        setSql(source.source);
        setSqlResult(dbxQueryToExecuteResult({
          columns: ["source"],
          column_types: ["text"],
          column_sortables: [false],
          rows: [[source.source]],
          affected_rows: 0,
          execution_time_ms: 0,
          truncated: false,
          has_more: false,
        }));
        setQueryResult(null);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const writeDbxProcedureExecutionDraft = useCallback(
    (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxObject(object);
      setWorkspaceMode("query");
      setSql(`CALL ${dbxQualifiedSqlName(object)}();`);
      setSqlResult(null);
      setQueryResult(null);
      setError(null);
    },
    [],
  );

  const writeDbxObjectSqlDraft = useCallback(
    (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo, mode: "select" | "insert" | "update") => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(database);
      setActiveDbxObject(object);
      setWorkspaceMode("query");
      const name = dbxQualifiedSqlName(object);
      const draft =
        mode === "select"
          ? `SELECT * FROM ${name}\nLIMIT 100;`
          : mode === "insert"
            ? `INSERT INTO ${name} (\n  column_name\n) VALUES (\n  value\n);`
            : `UPDATE ${name}\nSET column_name = value\nWHERE condition;`;
      setSql(draft);
      setSqlResult(null);
      setQueryResult(null);
      setError(null);
    },
    [],
  );

  const exportDbxTableObject = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      format: TableExportFormat,
      requestOverrides: Partial<Pick<TableExportRequest, "columns" | "columnTypes" | "primaryKeys" | "whereInput" | "orderBy">> = {},
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      const extensionByFormat: Record<TableExportFormat, string> = {
        csv: "csv",
        json: "json",
        markdown: "md",
        insertSql: "sql",
        updateSql: "sql",
        xlsx: "xlsx",
      };
      const selectedPath = await saveDialog({
        defaultPath: `${object.name}.${extensionByFormat[format]}`,
        filters: [{ name: format.toUpperCase(), extensions: [extensionByFormat[format]] }],
      });
      if (typeof selectedPath !== "string" || !selectedPath.trim()) return;
      const request = {
        exportId: `export:${Date.now()}:${Math.random().toString(36).slice(2)}`,
        connectionId: connection.id,
        database: database ?? "",
        schema: object.schema ?? null,
        tableName: object.name,
        filePath: selectedPath.trim(),
        format,
        batchSize: 1000,
        ...requestOverrides,
      };
      setLoading(true);
      setExportProgress({ active: true, format, filePath: selectedPath.trim() });
      setError(null);
      try {
        if (format === "csv") await databaseApi.dbxExportTableCsv(request);
        else if (format === "json") await databaseApi.dbxExportTableJson(request);
        else if (format === "markdown") await databaseApi.dbxExportTableMarkdown(request);
        else if (format === "insertSql") await databaseApi.dbxExportTableInsertSql(request);
        else if (format === "updateSql") await databaseApi.dbxExportTableUpdateSql(request);
        else await databaseApi.dbxExportTableXlsx(request);
      } catch (err) {
        setError(String(err));
      } finally {
        setExportProgress(null);
        setLoading(false);
      }
    },
    [],
  );

  const exportActiveDbxGrid = useCallback(async () => {
    if (!activeDbxConnection || !activeDbxObject || !queryResult || visibleTableColumns.length === 0) return;
    const columns = visibleTableColumns.map(({ column }) => column);
    const columnTypes = columns.map((column) => {
      const metadata = activeObject?.columns.find((item) => item.name.toLowerCase() === column.toLowerCase());
      return metadata?.dataType ?? null;
    });
    await exportDbxTableObject(activeDbxConnection, activeDbxDatabase, activeDbxObject, dbxGridExportFormat, {
      columns,
      columnTypes,
      primaryKeys: activeObject?.primaryKeys ?? [],
      whereInput: dbxGridWhereInput.trim() || null,
      orderBy: dbxGridOrderByInput.trim() || null,
    });
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    activeObject?.columns,
    activeObject?.primaryKeys,
    dbxGridExportFormat,
    dbxGridOrderByInput,
    dbxGridWhereInput,
    exportDbxTableObject,
    queryResult,
    visibleTableColumns,
  ]);

  const exportSelectedDbxRows = useCallback(async () => {
    if (!activeDbxConnection || !activeDbxObject || !queryResult || dbxGridSelectedRows.size === 0) return;
    const primaryKeys = activeObject?.primaryKeys ?? queryResult.primaryKeys ?? [];
    if (primaryKeys.length === 0) return;
    const selectedRows = Array.from(dbxGridSelectedRows)
      .map((rowIndex) => queryResult.rows[rowIndex])
      .filter(Boolean);
    if (selectedRows.length === 0) return;
    const conditions = selectedRows.map((row) => {
      const parts = primaryKeys.map((pk) => {
        const colIndex = queryResult.columns.indexOf(pk);
        const value = colIndex >= 0 ? row.values[colIndex] : null;
        return value === null || value === undefined
          ? `${quoteSqlName(pk)} IS NULL`
          : `${quoteSqlName(pk)} = ${sqlLiteral(value)}`;
      });
      return parts.length === 1 ? parts[0] : `(${parts.join(" AND ")})`;
    });
    const whereClause = conditions.length === 1 ? conditions[0] : `(${conditions.join(" OR ")})`;
    await exportDbxTableObject(activeDbxConnection, activeDbxDatabase, activeDbxObject, dbxGridExportFormat, {
      whereInput: whereClause,
    });
  }, [activeDbxConnection, activeDbxDatabase, activeDbxObject, activeObject?.primaryKeys, dbxGridExportFormat, dbxGridSelectedRows, exportDbxTableObject, queryResult]);

  const copyDbxObjectStructure = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo, format: "markdown" | "tsv") => {
      setLoading(true);
      setError(null);
      try {
        const columns = await databaseApi.dbxGetColumns(connection.id, object.name, database, object.schema ?? null);
        const text =
          format === "markdown"
            ? [
                "| Column | Type | Nullable | Primary key |",
                "| --- | --- | --- | --- |",
                ...columns.map((column) =>
                  `| ${column.name} | ${column.data_type ?? ""} | ${column.is_nullable ? "yes" : "no"} | ${column.is_primary_key ? "yes" : "no"} |`,
                ),
              ].join("\n")
            : [
                "Column\tType\tNullable\tPrimary key",
                ...columns.map((column) =>
                  `${column.name}\t${column.data_type ?? ""}\t${column.is_nullable ? "yes" : "no"}\t${column.is_primary_key ? "yes" : "no"}`,
                ),
              ].join("\n");
        copyNodeName(text);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [copyNodeName],
  );

  const exportDbxObjectStructure = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      if (!isSqlDbxConnection(connection)) return;
      const exportDatabase = database || activeDbxDatabase;
      if (!exportDatabase) return;
      await openDatabaseExportDialog(connection, exportDatabase, object.schema ?? null, [object.name]);
    },
    [activeDbxDatabase, openDatabaseExportDialog],
  );

  const copyDbxObjectStructureDdl = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      if (!isSqlDbxConnection(connection)) return;
      setLoading(true);
      setError(null);
      try {
        const ddl = await databaseApi.dbxGetTableDdl(connection.id, object.name, database, object.schema ?? null);
        copyNodeName(ddl);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [copyNodeName],
  );

  const dropDbxObject = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string | null, object: DbxObjectInfo) => {
      if (!isSqlDbxConnection(connection)) return;
      if (isDbxTableObject(object)) {
        await dropDbxTableObject(connection, database, object);
        return;
      }
      setLoading(true);
      setError(null);
      try {
        const title = t(dbxObjectDropLabelKey(object));
        const sql = await databaseApi.dbxBuildDropObjectSql({
          databaseType: connection.dbType,
          objectType: object.object_type.toUpperCase(),
          schema: object.schema ?? null,
          name: object.name,
        });
        const ok = await confirm(`${t(dbxObjectDropConfirmLabelKey(object), { name: dbxObjectKey(object) })}\n\n${sql}`, {
          title,
          kind: "warning",
          okLabel: title,
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database,
          sql,
        });
        await loadDbxConnection(connection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [dropDbxTableObject, loadDbxConnection, t],
  );

  const dropDbxTableChildObjectByName = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      childObjectType: TableChildObjectType,
      childObjectName: string,
    ) => {
      if (!isSqlDbxConnection(connection) || !isDbxTableObject(object)) return;
      const actionConfig: Record<TableChildObjectType, { title: string; message: string; okLabel: string }> = {
        COLUMN: {
          title: t("database.dropColumn"),
          message: t("database.confirmDropColumn", { name: childObjectName }),
          okLabel: t("database.dropColumn"),
        },
        INDEX: {
          title: t("database.dropIndex"),
          message: t("database.confirmDropIndex", { name: childObjectName }),
          okLabel: t("database.dropIndex"),
        },
        FOREIGN_KEY: {
          title: t("database.dropForeignKey"),
          message: t("database.confirmDropForeignKey", { name: childObjectName }),
          okLabel: t("database.dropForeignKey"),
        },
        TRIGGER: {
          title: t("database.dropTrigger"),
          message: t("database.confirmDropTrigger", { name: childObjectName }),
          okLabel: t("database.dropTrigger"),
        },
      };
      const config = actionConfig[childObjectType];
      setLoading(true);
      setError(null);
      try {
        const sql = await databaseApi.dbxBuildDropTableChildObjectSql({
          databaseType: connection.dbType,
          objectType: childObjectType,
          schema: object.schema ?? null,
          tableName: object.name,
          name: childObjectName,
        });
        const ok = await confirm(`${config.message}\n\n${sql}`, {
          title: config.title,
          kind: "warning",
          okLabel: config.okLabel,
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database,
          sql,
        });
        await loadDbxDatabase(connection, database);
        await loadDbxColumnsForTables([object], connection, database);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxColumnsForTables, loadDbxDatabase, t],
  );

  const dropDbxColumn = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      column: DbxColumnInfo,
    ) => {
      await dropDbxTableChildObjectByName(connection, database, object, "COLUMN", column.name);
    },
    [dropDbxTableChildObjectByName],
  );

  const dropDbxTableChildObject = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      childObject: DbxObjectInfo,
    ) => {
      const childObjectType = dbxTableChildObjectType(childObject);
      if (!childObjectType) return;
      await dropDbxTableChildObjectByName(connection, database, object, childObjectType, childObject.name);
    },
    [dropDbxTableChildObjectByName],
  );

  const runDbxObjectContextMenuAction = useCallback(
    async (action: DbxObjectContextMenuAction) => {
      const menu = contextMenu?.kind === "dbx-object" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection || !isSqlDbxConnection(connection)) return;

      if (action === "togglePin") {
        const nodeId = contextMenuPinnedNodeId(menu);
        if (nodeId) togglePinnedTreeNode(nodeId);
        return;
      }
      if (action === "copyName") {
        copyNodeName(dbxObjectKey(menu.object));
        return;
      }
      if (action === "viewData") {
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        await loadDbxObject(menu.object, 1, connection, menu.database);
        return;
      }
      if (action === "editView" || action === "viewSource") {
        await showDbxObjectSource(connection, menu.database, menu.object);
        return;
      }
      if (action === "executeProcedure") {
        if (isDbxProcedureObject(menu.object)) {
          writeDbxProcedureExecutionDraft(connection, menu.database, menu.object);
        }
        return;
      }
      if (action === "editStructure") {
        await openDbxObjectStructure(connection, menu.database, menu.object);
        return;
      }
      if (action === "renameObject") {
        const objectType = dbxObjectRenameType(menu.object);
        if (!objectType || !canRenameDbxObject(connection, menu.object)) return;
        const newName = window.prompt(t("database.renameObjectNamePrompt"), menu.object.name)?.trim();
        if (!newName || newName === menu.object.name) return;
        setLoading(true);
        setError(null);
        try {
          const sql = await databaseApi.dbxBuildRenameObjectSql({
            databaseType: connection.dbType,
            objectType,
            schema: menu.object.schema ?? null,
            oldName: menu.object.name,
            newName,
          });
          const ok = await confirm(`${t("database.confirmRenameObject", { oldName: dbxObjectKey(menu.object), newName })}\n\n${sql}`, {
            title: t("database.renameObject"),
            kind: "warning",
            okLabel: t("database.renameObject"),
            cancelLabel: t("common.cancel"),
          });
          if (!ok) return;
          await databaseApi.dbxExecuteQuery({
            connectionId: connection.id,
            database: menu.database,
            schema: menu.object.schema ?? null,
            sql,
          });
          await loadDbxConnection(connection);
          if (activeDbxObject?.name === menu.object.name && activeDbxObject?.schema === menu.object.schema) {
            setActiveDbxObject({ ...menu.object, name: newName });
          }
        } catch (err) {
          setError(String(err));
        } finally {
          setLoading(false);
        }
        return;
      }
      if (action === "viewDdl") {
        await showDbxObjectDdl(connection, menu.database, menu.object);
        return;
      }
      if (action === "tableInfo") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.object.schema ?? null);
        setActiveDbxObject(menu.object);
        setWorkspaceMode("table-info");
        await loadDbxColumnsForTables([menu.object], connection, menu.database);
        return;
      }
      if (action === "newQuery" || action === "newSqlSelect" || action === "newSqlInsert" || action === "newSqlUpdate") {
        writeDbxObjectSqlDraft(
          connection,
          menu.database,
          menu.object,
          action === "newQuery" || action === "newSqlSelect" ? "select" : action === "newSqlInsert" ? "insert" : "update",
        );
        return;
      }
      if (action === "queryHistory") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.object.schema ?? null);
        setActiveDbxObject(menu.object);
        openQueryHistory();
        return;
      }
      if (action === "openErDiagram") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setWorkspaceMode("er-diagram");
        await loadDbxColumnsForTables(dbxObjects, connection, menu.database);
        return;
      }
      if (action === "dataCompare") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxObject(menu.object);
        setWorkspaceMode("data-compare");
        return;
      }
      if (action === "importData") {
        await openTableImportDialog(connection, menu.database, menu.object);
        return;
      }
      if (action === "exportDatabase") {
        const exportDatabase = menu.database || activeDbxDatabase;
        if (!exportDatabase) return;
        await openDatabaseExportDialog(connection, exportDatabase, menu.object.schema ?? null, [menu.object.name]);
        return;
      }
      if (action.startsWith("export")) {
        const formatByAction: Partial<Record<typeof action, TableExportFormat>> = {
          exportCsv: "csv",
          exportJson: "json",
          exportMarkdown: "markdown",
          exportInsertSql: "insertSql",
          exportUpdateSql: "updateSql",
          exportXlsx: "xlsx",
        };
        const format = formatByAction[action];
        if (format) await exportDbxTableObject(connection, menu.database, menu.object, format);
        return;
      }
      if (action === "copyStructureTsv" || action === "copyStructureMarkdown") {
        await copyDbxObjectStructure(connection, menu.database, menu.object, action === "copyStructureTsv" ? "tsv" : "markdown");
        return;
      }
      if (action === "copyStructureDdl") {
        await copyDbxObjectStructureDdl(connection, menu.database, menu.object);
        return;
      }
      if (action === "exportStructure") {
        await exportDbxObjectStructure(connection, menu.database, menu.object);
        return;
      }
      if (action === "duplicateStructure") {
        if (!isDbxTableObject(menu.object) || connection.readOnly) return;
        const defaultName = uniqueDbxObjectName(`${menu.object.name}_copy`, menu.object.schema, dbxObjects);
        const targetName = window.prompt(t("database.duplicateStructureNamePrompt"), defaultName)?.trim();
        if (!targetName || targetName === menu.object.name) return;
        setLoading(true);
        setError(null);
        try {
          const sql = await databaseApi.dbxBuildDuplicateTableStructureSql({
            databaseType: connection.dbType,
            schema: menu.object.schema ?? null,
            sourceName: menu.object.name,
            targetName,
          });
          const ok = await confirm(`${t("database.confirmDuplicateStructure", { source: dbxObjectKey(menu.object), target: targetName })}\n\n${sql}`, {
            title: t("database.duplicateStructure"),
            kind: "warning",
            okLabel: t("database.duplicateStructure"),
            cancelLabel: t("common.cancel"),
          });
          if (!ok) return;
          await databaseApi.dbxExecuteQuery({
            connectionId: connection.id,
            database: menu.database,
            schema: menu.object.schema ?? null,
            sql,
          });
          await loadDbxConnection(connection);
        } catch (err) {
          setError(String(err));
        } finally {
          setLoading(false);
        }
        return;
      }
      if (action === "dropObject") {
        await dropDbxObject(connection, menu.database, menu.object);
        return;
      }
      if (action === "refresh") {
        await loadDbxConnection(connection);
        return;
      }
      if (!isDbxTableObject(menu.object)) return;
      if (action !== "emptyTable" && action !== "truncateTable" && action !== "dropTable") return;
      if (action === "truncateTable" && !supportsDbxTableTruncate(connection)) return;

      const tableOptions = {
        databaseType: connection.dbType,
        schema: menu.object.schema ?? null,
        tableName: menu.object.name,
      };
      const actionConfig = {
        emptyTable: {
          title: t("database.emptyTable"),
          message: t("database.confirmEmptyTable", { name: dbxObjectKey(menu.object) }),
          okLabel: t("database.emptyTable"),
          buildSql: () => databaseApi.dbxBuildEmptyTableSql(tableOptions),
        },
        truncateTable: {
          title: t("database.truncateTable"),
          message: t("database.confirmTruncateTable", { name: dbxObjectKey(menu.object) }),
          okLabel: t("database.truncateTable"),
          buildSql: () => databaseApi.dbxBuildTruncateTableSql(tableOptions),
        },
        dropTable: {
          title: t("database.dropTable"),
          message: t("database.confirmDropTable", { name: dbxObjectKey(menu.object) }),
          okLabel: t("database.dropTable"),
          buildSql: () => databaseApi.dbxBuildDropTableSql(tableOptions),
        },
      }[action];
      if (!actionConfig) return;

      setLoading(true);
      setError(null);
      try {
        const sql = await actionConfig.buildSql();
        const ok = await confirm(`${actionConfig.message}\n\n${sql}`, {
          title: actionConfig.title,
          kind: "warning",
          okLabel: actionConfig.okLabel,
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database: menu.database,
          sql,
        });
        if (action === "dropTable") {
          await loadDbxConnection(connection);
        } else {
          setActiveDbxConnectionId(connection.id);
          setActiveDbxDatabase(menu.database);
          await loadDbxObject(menu.object, 1, connection, menu.database);
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [
      contextMenu,
      activeDbxDatabase,
      activeDbxObject,
      copyDbxObjectStructure,
      copyDbxObjectStructureDdl,
      copyNodeName,
      dbxConnections,
      dbxObjects,
      dropDbxObject,
      exportDbxObjectStructure,
      exportDbxTableObject,
      loadDbxColumnsForTables,
      loadDbxConnection,
      loadDbxObject,
      openDbxObjectStructure,
      openDatabaseExportDialog,
      openQueryHistory,
      openTableImportDialog,
      showDbxObjectDdl,
      showDbxObjectSource,
      t,
      togglePinnedTreeNode,
      writeDbxProcedureExecutionDraft,
      writeDbxObjectSqlDraft,
    ],
  );

  const contextMenuDbxDatabaseConnection =
    contextMenu?.kind === "dbx-database"
      ? dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null
      : null;
  const contextMenuDbxSchemaConnection =
    contextMenu?.kind === "dbx-schema"
      ? dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null
      : null;
  const contextMenuDbxObjectConnection =
    contextMenu?.kind === "dbx-object"
      ? dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null
      : null;
  const contextMenuDbxColumnConnection =
    contextMenu?.kind === "dbx-column"
      ? dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null
      : null;
  const contextMenuDbxTableChildConnection =
    contextMenu?.kind === "dbx-table-child"
      ? dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null
      : null;
  const contextMenuDbxObjectGroupConnection =
    contextMenu?.kind === "dbx-object-group"
      ? dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null
      : null;
  const contextMenuNoSqlConnection =
    contextMenu?.kind === "redis-database" ||
    contextMenu?.kind === "redis-key" ||
    contextMenu?.kind === "mongo-database" ||
    contextMenu?.kind === "mongo-collection" ||
    contextMenu?.kind === "mongo-document"
      ? dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null
      : null;

  const createSchemaConnection = useMemo(
    () => dbxConnections.find((connection) => connection.id === createSchemaTarget?.connectionId) ?? null,
    [createSchemaTarget, dbxConnections],
  );

  const closeCreateSchemaDialog = useCallback(() => {
    setCreateSchemaTarget(null);
    setCreateSchemaName("");
  }, []);

  const submitCreateSchema = useCallback(async () => {
    if (!createSchemaConnection || !createSchemaTarget || !createSchemaName.trim()) return;
    const name = createSchemaName.trim();
    setLoading(true);
    setError(null);
    try {
      const sql = await databaseApi.dbxBuildCreateSchemaSql({
        databaseType: createSchemaConnection.dbType,
        name,
      });
      await databaseApi.dbxExecuteQuery({
        connectionId: createSchemaConnection.id,
        database: createSchemaTarget.database,
        sql,
      });
      closeCreateSchemaDialog();
      await loadDbxDatabase(createSchemaConnection, createSchemaTarget.database);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [closeCreateSchemaDialog, createSchemaConnection, createSchemaName, createSchemaTarget, loadDbxDatabase]);

  const dropDbxDatabase = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string) => {
      if (!isSqlDbxConnection(connection)) return;
      setLoading(true);
      setError(null);
      try {
        const sql = await databaseApi.dbxBuildDropDatabaseSql({
          databaseType: connection.dbType,
          name: database,
        });
        const ok = await confirm(`${t("database.confirmDropDatabase", { name: database })}\n\n${sql}`, {
          title: t("database.dropDatabase"),
          kind: "warning",
          okLabel: t("database.dropDatabase"),
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database: "",
          sql,
        });
        await loadDbxConnection(connection);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxConnection, t],
  );

  const dropDbxSchema = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string, schemaName: string) => {
      if (!isSqlDbxConnection(connection)) return;
      setLoading(true);
      setError(null);
      try {
        const sql = await databaseApi.dbxBuildDropSchemaSql({
          databaseType: connection.dbType,
          name: schemaName,
        });
        const ok = await confirm(`${t("database.confirmDropSchema", { name: schemaName })}\n\n${sql}`, {
          title: t("database.dropSchema"),
          kind: "warning",
          okLabel: t("database.dropSchema"),
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxExecuteQuery({
          connectionId: connection.id,
          database,
          sql,
        });
        await loadDbxDatabase(connection, database);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [loadDbxDatabase, t],
  );

  const runDbxDatabaseContextMenuAction = useCallback(
    async (action: DbxDatabaseContextMenuAction) => {
      const menu = contextMenu?.kind === "dbx-database" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection || !isSqlDbxConnection(connection)) return;

      if (action === "togglePin") {
        const nodeId = contextMenuPinnedNodeId(menu);
        if (nodeId) togglePinnedTreeNode(nodeId);
        return;
      }
      if (action === "copyName") {
        copyNodeName(menu.database);
        return;
      }
      if (action === "openObjectBrowser") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode("object-browser");
        return;
      }
      if (action === "newQuery") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setWorkspaceMode("query");
        setSql("");
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "queryHistory") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        openQueryHistory();
        return;
      }
      if (action === "executeSqlFile") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setWorkspaceMode("sql-file");
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "createSchema") {
        setCreateSchemaTarget({ connectionId: connection.id, database: menu.database });
        setCreateSchemaName("");
        return;
      }
      if (action === "createTable") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode("query");
        setSql(dbxCreateTableDraft(null));
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "refresh") {
        await loadDbxDatabase(connection, menu.database);
        return;
      }
      if (action === "dataTransfer" || action === "schemaDiff" || action === "dataCompare") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode(action === "dataTransfer" ? "transfer" : action === "schemaDiff" ? "schema-diff" : "data-compare");
        return;
      }
      if (action === "openErDiagram" || action === "databaseSearch") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode(action === "openErDiagram" ? "er-diagram" : "database-search");
        try {
          const objects = await databaseApi.dbxListObjects(connection.id, menu.database, null);
          setDbxObjects(objects);
          if (action === "openErDiagram") {
            await loadDbxColumnsForTables(objects, connection, menu.database);
          }
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "exportDatabase") {
        await openDatabaseExportDialog(connection, menu.database, null);
        return;
      }
      if (action === "setDefaultDatabase") {
        await saveDbxDefaultDatabase(connection, menu.database);
        return;
      }
      if (action === "clearDefaultDatabase") {
        await saveDbxDefaultDatabase(connection, null);
        return;
      }
      if (action === "closeDatabaseConnection") {
        await databaseApi.dbxDisconnect(connection.id);
        await loadDbxConnection(connection);
        return;
      }

      await dropDbxDatabase(connection, menu.database);
    },
    [
      contextMenu,
      copyNodeName,
      dbxConnections,
      dropDbxDatabase,
      loadDbxColumnsForTables,
      loadDbxConnection,
      loadDbxDatabase,
      openDatabaseExportDialog,
      openQueryHistory,
      saveDbxDefaultDatabase,
      togglePinnedTreeNode,
    ],
  );

  const runDbxSchemaContextMenuAction = useCallback(
    async (action: DbxSchemaContextMenuAction) => {
      const menu = contextMenu?.kind === "dbx-schema" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection || !isSqlDbxConnection(connection)) return;

      if (action === "togglePin") {
        const nodeId = contextMenuPinnedNodeId(menu);
        if (nodeId) togglePinnedTreeNode(nodeId);
        return;
      }
      if (action === "copyName") {
        copyNodeName(menu.schema);
        return;
      }
      if (action === "openObjectBrowser") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("object-browser");
        return;
      }
      if (action === "newQuery") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("query");
        setSql("");
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "queryHistory") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        openQueryHistory();
        return;
      }
      if (action === "executeSqlFile") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("sql-file");
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "createTable") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("query");
        setSql(dbxCreateTableDraft(menu.schema));
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "refresh") {
        await loadDbxSchema(connection, menu.database, menu.schema);
        return;
      }
      if (action === "dataTransfer" || action === "schemaDiff" || action === "dataCompare") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode(action === "dataTransfer" ? "transfer" : action === "schemaDiff" ? "schema-diff" : "data-compare");
        return;
      }
      if (action === "openErDiagram" || action === "databaseSearch") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode(action === "openErDiagram" ? "er-diagram" : "database-search");
        try {
          const objects = await databaseApi.dbxListObjects(connection.id, menu.database, menu.schema);
          setDbxObjects(objects);
          if (action === "openErDiagram") {
            await loadDbxColumnsForTables(objects, connection, menu.database);
          }
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "exportDatabase") {
        await openDatabaseExportDialog(connection, menu.database, menu.schema);
        return;
      }
      await dropDbxSchema(connection, menu.database, menu.schema);
    },
    [contextMenu, copyNodeName, dbxConnections, dropDbxSchema, loadDbxColumnsForTables, loadDbxSchema, openDatabaseExportDialog, openQueryHistory, togglePinnedTreeNode],
  );

  const runDbxColumnContextMenuAction = useCallback(
    async (action: "copyName" | "openFieldLineage" | "dropColumn") => {
      const menu = contextMenu?.kind === "dbx-column" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection) return;
      if (action === "copyName") {
        copyNodeName(menu.column.name);
        return;
      }
      if (action === "openFieldLineage") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.object.schema ?? null);
        setActiveDbxObject(menu.object);
        setWorkspaceMode("field-lineage");
        return;
      }
      await dropDbxColumn(connection, menu.database, menu.object, menu.column);
    },
    [contextMenu, copyNodeName, dbxConnections, dropDbxColumn],
  );

  const runDbxTableChildContextMenuAction = useCallback(
    async (action: "copyName" | "dropTableChildObject") => {
      const menu = contextMenu?.kind === "dbx-table-child" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection) return;
      if (action === "copyName") {
        copyNodeName(menu.childObject.name);
        return;
      }
      await dropDbxTableChildObject(connection, menu.database, menu.object, menu.childObject);
    },
    [contextMenu, copyNodeName, dbxConnections, dropDbxTableChildObject],
  );

  const runDbxObjectGroupContextMenuAction = useCallback(
    async (action: "createTable" | "createView" | "refresh") => {
      const menu = contextMenu?.kind === "dbx-object-group" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection || !isSqlDbxConnection(connection)) return;

      if (action === "createTable" || action === "createView") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode("query");
        setSql(action === "createTable" ? dbxCreateTableDraft(menu.schema) : dbxCreateViewDraft(menu.schema));
        setSqlResult(null);
        setQueryResult(null);
        return;
      }

      if (menu.schema) {
        await loadDbxSchema(connection, menu.database, menu.schema);
      } else {
        await loadDbxDatabase(connection, menu.database);
      }
    },
    [contextMenu, dbxConnections, loadDbxDatabase, loadDbxSchema],
  );

  const runNoSqlContextMenuAction = useCallback(
    async (action: NoSqlContextMenuAction) => {
      const menu =
        contextMenu?.kind === "redis-database" ||
        contextMenu?.kind === "redis-key" ||
        contextMenu?.kind === "mongo-database" ||
        contextMenu?.kind === "mongo-collection" ||
        contextMenu?.kind === "mongo-document"
          ? contextMenu
          : null;
      setContextMenu(null);
      if (!menu) return;
      const connection = dbxConnections.find((item) => item.id === menu.connectionId) ?? null;
      if (!connection) return;
      if (action === "togglePin") {
        const nodeId = contextMenuPinnedNodeId(menu);
        if (nodeId) togglePinnedTreeNode(nodeId);
        return;
      }

      if (menu.kind === "redis-database") {
        if (action === "copyName") {
          copyNodeName(`db${menu.database}`);
          return;
        }
        if (action === "newQuery") {
          setActiveConnectionId(null);
          setActiveDbxConnectionId(connection.id);
          setActiveDbxDatabase(`db${menu.database}`);
          setActiveDbxSchema(null);
          setActiveMongoDocumentId(null);
          setActiveMongoWorkspaceDatabase(null);
          setWorkspaceMode("query");
          setSql("");
          setSqlResult(null);
          setQueryResult(null);
          return;
        }
        if (action === "openWorkspace") {
          selectRedisSidebarDatabase(connection, menu.database);
          return;
        }
        if (action === "setDefaultDatabase") {
          await saveDbxDefaultDatabase(connection, String(menu.database));
          return;
        }
        if (action === "clearDefaultDatabase") {
          await saveDbxDefaultDatabase(connection, null);
          return;
        }
        if (action === "flushRedisDb") {
          if (connection.readOnly) return;
          const ok = await confirm(t("database.confirmFlushRedisDb", { db: menu.database }), {
            title: t("database.redisFlushDb"),
            kind: "warning",
            okLabel: t("database.redisFlushDbConfirm"),
            cancelLabel: t("common.cancel"),
          });
          if (!ok) return;
          await databaseApi.dbxRedisExecuteCommand({
            connectionId: connection.id,
            db: menu.database,
            command: "FLUSHDB",
            skipSafetyCheck: true,
          });
          await loadRedisSidebarKeys(connection, menu.database);
          await loadRedisSidebarDatabases(connection);
          return;
        }
        await loadRedisSidebarDatabases(connection);
        return;
      }

      if (menu.kind === "redis-key") {
        if (action === "copyName") {
          copyNodeName(menu.keyRaw);
          return;
        }
        if (action === "openWorkspace") {
          selectRedisSidebarKey(connection, menu.database, menu.keyRaw);
          return;
        }
        if (action === "refresh") {
          await loadRedisSidebarKeys(connection, menu.database);
          return;
        }
        if (connection.readOnly) return;
        const ok = await confirm(t("database.confirmDeleteRedisKey", { name: menu.keyRaw }), {
          title: t("database.redisDeleteKey"),
          kind: "warning",
          okLabel: t("database.redisDeleteKey"),
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxRedisDeleteKey({
          connectionId: connection.id,
          db: menu.database,
          keyRaw: menu.keyRaw,
        });
        await loadRedisSidebarKeys(connection, menu.database);
        return;
      }

      if (menu.kind === "mongo-database") {
        if (action === "copyName") {
          copyNodeName(menu.database);
          return;
        }
        if (action === "newQuery") {
          setActiveConnectionId(null);
          setActiveDbxConnectionId(connection.id);
          setActiveDbxDatabase(menu.database);
          setActiveDbxSchema(null);
          setActiveMongoDocumentId(null);
          setActiveMongoWorkspaceDatabase(null);
          setWorkspaceMode("query");
          setSql("");
          setSqlResult(null);
          setQueryResult(null);
          return;
        }
        if (action === "openWorkspace") {
          await selectMongoSidebarDatabase(connection, menu.database);
          return;
        }
        if (action === "setDefaultDatabase") {
          await saveDbxDefaultDatabase(connection, menu.database);
          return;
        }
        if (action === "clearDefaultDatabase") {
          await saveDbxDefaultDatabase(connection, null);
          return;
        }
        await loadMongoSidebarDatabases(connection);
        await loadMongoSidebarCollections(connection, menu.database);
        return;
      }

      if (menu.kind === "mongo-document") {
        const documentId = mongoDocumentId(menu.document);
        if (action === "copyName") {
          copyNodeName(documentId);
          return;
        }
        if (action === "openWorkspace") {
          await selectMongoSidebarDocument(connection, menu.database, menu.collection, menu.document);
          return;
        }
        if (action === "refresh") {
          await loadMongoSidebarDocuments(connection, menu.database, menu.collection);
          return;
        }
        const rawId = mongoDocumentRawId(menu.document);
        if (connection.readOnly || rawId == null) return;
        const ok = await confirm(t("database.confirmDeleteMongoDocument", { collection: menu.collection, id: documentId }), {
          title: t("database.mongoDeleteDocument"),
          kind: "warning",
          okLabel: t("database.mongoDeleteDocument"),
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        await databaseApi.dbxMongoDeleteDocuments({
          connectionId: connection.id,
          database: menu.database,
          collection: menu.collection,
          filterJson: JSON.stringify({ _id: rawId }),
          many: false,
        });
        setActiveMongoDocumentId(null);
        await loadMongoSidebarDocuments(connection, menu.database, menu.collection);
        return;
      }

      if (action === "copyName") {
        copyNodeName(menu.collection);
        return;
      }
      if (action === "openWorkspace") {
        await selectMongoSidebarCollection(connection, menu.database, menu.collection);
        return;
      }
      await loadMongoSidebarCollections(connection, menu.database);
    },
    [
      contextMenu,
      copyNodeName,
      dbxConnections,
      loadMongoSidebarCollections,
      loadMongoSidebarDatabases,
      loadMongoSidebarDocuments,
      loadRedisSidebarDatabases,
      loadRedisSidebarKeys,
      selectMongoSidebarCollection,
      selectMongoSidebarDatabase,
      selectMongoSidebarDocument,
      selectRedisSidebarDatabase,
      selectRedisSidebarKey,
      saveDbxDefaultDatabase,
      t,
      togglePinnedTreeNode,
    ],
  );

  const runContextMenuAction = useCallback(
    async (
      action:
        | "open"
        | "close"
        | "newQuery"
        | "queryHistory"
        | "executeSqlFile"
        | "userAdmin"
        | "createDatabase"
        | "copyFinalProxyPort"
        | "selectVisibleDatabases"
        | "edit"
        | "revealDatabaseFile"
        | "backupSqliteDatabase"
        | "togglePin"
        | "moveToGroup"
        | "refresh"
        | "copy"
        | "delete",
    ) => {
      const menu = contextMenu;
      setContextMenu(null);
      if (!menu) return;
      if (menu.kind === "connection-group") return;
      const legacy = connections.find((connection) => connection.id === menu.connectionId) ?? null;
      const dbx = dbxConnections.find((connection) => connection.id === menu.connectionId) ?? null;

      if (action === "open") {
        if (legacy) await inspect(legacy);
        if (dbx) await loadDbxConnection(dbx);
        return;
      }
      if (action === "newQuery") {
        if (legacy) handleSelectConnection(legacy);
        if (dbx) await loadDbxConnection(dbx);
        handleNewQuery();
        return;
      }
      if (action === "queryHistory") {
        if (legacy) handleSelectConnection(legacy);
        if (dbx) await loadDbxConnection(dbx);
        openQueryHistory();
        return;
      }
      if (action === "executeSqlFile") {
        if (legacy) handleSelectConnection(legacy);
        if (dbx) await loadDbxConnection(dbx);
        handleExecuteSqlFile();
        return;
      }
      if (action === "userAdmin") {
        if (dbx) await loadDbxConnection(dbx);
        setWorkspaceMode("user-admin");
        setError(dbx && supportsDbxUserAdmin(dbx.dbType) ? null : t("database.selectUserAdminConnection"));
        setSqlResult(null);
        setQueryResult(null);
        return;
      }
      if (action === "createDatabase") {
        if (dbx?.dbType === "duckdb") {
          await createDuckDbAttachedDatabaseFile(dbx);
        } else if (dbx && canCreateDatabaseForConnection(dbx)) {
          openCreateDatabaseDialog(dbx);
        }
        return;
      }
      if (action === "copyFinalProxyPort") {
        const port = dbxConnectionFinalProxyPort(dbx);
        if (port != null) copyNodeName(String(port));
        return;
      }
      if (action === "selectVisibleDatabases") {
        if (dbx) await openVisibleDatabasesDialog(dbx);
        return;
      }
      if (action === "edit") {
        if (dbx) openEditDbxConnectionDialog(dbx);
        return;
      }
      if (action === "revealDatabaseFile") {
        const path = dbxConnectionLocalFilePath(dbx);
        if (!path) return;
        try {
          await invoke("open_in_system_file_manager", { path, projectPath: projectRoot ?? path });
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "backupSqliteDatabase") {
        if (!dbx || !sqliteBackupSourcePath(dbx)) return;
        const destinationPath = await saveDialog({
          defaultPath: defaultSqliteBackupFileName(dbx),
          filters: [{ name: "SQLite", extensions: ["db", "sqlite", "sqlite3"] }],
        });
        if (typeof destinationPath !== "string" || !destinationPath.trim()) return;
        setLoading(true);
        setError(null);
        try {
          await databaseApi.dbxBackupSqliteDatabase(dbx.id, destinationPath.trim());
        } catch (err) {
          setError(String(err));
        } finally {
          setLoading(false);
        }
        return;
      }
      if (action === "togglePin") {
        if (dbx) await toggleDbxConnectionPinned(dbx);
        return;
      }
      if (action === "moveToGroup") {
        if (dbx) await moveDbxConnectionToGroup(dbx);
        return;
      }
      if (action === "refresh") {
        if (legacy) await inspect(legacy);
        if (dbx) await loadDbxConnection(dbx);
        return;
      }
      if (action === "close") {
        if (dbx) await databaseApi.dbxDisconnect(dbx.id);
        if (activeDbxConnectionId === menu.connectionId) {
          setActiveDbxConnectionId(null);
          setDbxDatabases([]);
          setDbxObjects([]);
          setActiveDbxDatabase(null);
          setActiveDbxObject(null);
        }
        if (activeConnectionId === menu.connectionId) {
          setActiveConnectionId(null);
          setSchema(null);
          setActiveObject(null);
          setQueryResult(null);
        }
        return;
      }
      if (action === "copy") {
        if (legacy) copyLegacyConnection(legacy);
        if (dbx) await copyDbxConnection(dbx);
        return;
      }
      if (action === "delete") {
        if (legacy) handleDeleteConnection(legacy.id);
        if (dbx) await handleDeleteDbxConnection(dbx.id);
        return;
      }
    },
    [
      activeConnectionId,
      activeDbxConnectionId,
      connections,
      contextMenu,
      copyDbxConnection,
      copyLegacyConnection,
      copyNodeName,
      dbxConnections,
      handleDeleteConnection,
      handleDeleteDbxConnection,
      handleExecuteSqlFile,
      handleNewQuery,
      handleSelectConnection,
      inspect,
      loadDbxConnection,
      projectRoot,
      t,
      createDuckDbAttachedDatabaseFile,
      openCreateDatabaseDialog,
      openEditDbxConnectionDialog,
      openQueryHistory,
      openVisibleDatabasesDialog,
      moveDbxConnectionToGroup,
      toggleDbxConnectionPinned,
    ],
  );

  const runConnectionGroupContextMenuAction = useCallback(
    async (action: "copyName" | "newConnection" | "newGroup" | "renameGroup" | "deleteGroup") => {
      const menu = contextMenu?.kind === "connection-group" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      if (action === "copyName") {
        copyNodeName(menu.groupName);
        return;
      }
      if (action === "newConnection") {
        openNewConnectionDialog(menu.groupName);
        return;
      }
      if (action === "newGroup") {
        const childName = window.prompt(t("database.newConnectionGroupPrompt"), t("database.newConnectionGroupDefault"))?.trim();
        if (!childName) return;
        addExtraDbxConnectionGroup(`${menu.groupName}/${childName}`);
        return;
      }
      if (action === "renameGroup") {
        await renameDbxConnectionGroup(menu.groupName);
        return;
      }
      await deleteDbxConnectionGroup(menu.groupName);
    },
    [addExtraDbxConnectionGroup, contextMenu, copyNodeName, deleteDbxConnectionGroup, openNewConnectionDialog, renameDbxConnectionGroup, t],
  );

  const runSql = useCallback(async () => {
    if (!activeEndpoint && !activeDbxConnection) return;
    setLoading(true);
    setError(null);
    try {
      setWorkspaceMode("query");
      if (activeDbxConnection && dbxHasSqlObjectBrowser) {
        const result = await databaseApi.dbxExecuteQuery({
          connectionId: activeDbxConnection.id,
          database: activeDbxDatabase,
          schema: activeDbxSchema,
          sql,
          pageSize: PAGE_SIZE,
        });
        setSqlResult(dbxQueryToExecuteResult(result));
        setQueryResult(null);
        addQueryHistoryEntry({
          sql,
          connectionName: activeDbxConnection.name,
          database: activeDbxDatabase,
          schema: activeDbxSchema,
          rowsAffected: result.affected_rows,
          executionTimeMs: result.execution_time_ms,
        });
      } else if (activeEndpoint) {
        const result = await databaseApi.executeSql({
          endpoint: activeEndpoint,
          sql,
          page: 1,
          pageSize: PAGE_SIZE,
          readOnly: activeConnection?.readOnly ?? false,
          projectRoot,
        });
        setSqlResult(result);
        setQueryResult(null);
        addQueryHistoryEntry({
          sql,
          connectionName: activeConnection?.name ?? endpointLabel(activeEndpoint),
          database: null,
          schema: null,
          rowsAffected: result.rowsAffected,
          executionTimeMs: null,
        });
      }
      if (activeConnection) {
        const nextSchema = await databaseApi.inspect(activeConnection.endpoint, projectRoot);
        setSchema(nextSchema);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [
    activeConnection,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxSchema,
    activeEndpoint,
    addQueryHistoryEntry,
    dbxHasSqlObjectBrowser,
    projectRoot,
    sql,
  ]);

  const handleSqlDragOver = useCallback((event: DragEvent<HTMLTextAreaElement>) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  }, []);

  const handleSqlDrop = useCallback((event: DragEvent<HTMLTextAreaElement>) => {
    const structured = event.dataTransfer.getData("application/x-aeroric-database-object");
    let droppedText = "";
    if (structured) {
      try {
        const payload = JSON.parse(structured) as { reference?: unknown };
        if (typeof payload.reference === "string") droppedText = payload.reference;
      } catch {
        droppedText = "";
      }
    }
    if (!droppedText) droppedText = event.dataTransfer.getData("text/plain");
    if (!droppedText.trim()) return;

    event.preventDefault();
    const textarea = event.currentTarget;
    const selectionStart = textarea.selectionStart ?? textarea.value.length;
    const selectionEnd = textarea.selectionEnd ?? selectionStart;
    setSql((current) => {
      const start = Math.max(0, Math.min(selectionStart, current.length));
      const end = Math.max(start, Math.min(selectionEnd, current.length));
      return `${current.slice(0, start)}${droppedText}${current.slice(end)}`;
    });
    requestAnimationFrame(() => {
      const cursor = selectionStart + droppedText.length;
      textarea.focus();
      textarea.setSelectionRange(cursor, cursor);
    });
  }, []);

  const buildDbxGridSaveOptions = useCallback(
    (overrides: Pick<DataGridSaveStatementOptions, "dirtyRows" | "deletedRows" | "newRows">): DataGridSaveStatementOptions | null => {
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
      const rowIndexes =
        dbxGridSelectedRows.has(menu.rowIndex) && dbxGridSelectedRows.size > 0
          ? Array.from(dbxGridSelectedRows).sort((left, right) => left - right)
          : [menu.rowIndex];
      return rowIndexes
        .map((rowIndex) => queryResult.rows[rowIndex])
        .filter((row): row is DatabaseRow => Boolean(row));
    },
    [dbxGridSelectedRows, queryResult],
  );

  const buildDbxGridCopyOptions = useCallback(
    (rows: DatabaseRow[], excludePrimaryKeys = false): {
      insert: DataGridCopyInsertStatementOptions;
      update: DataGridCopyUpdateStatementOptions | null;
    } | null => {
      if (!activeDbxConnection || !activeDbxObject || !queryResult || visibleTableColumns.length === 0 || rows.length === 0) return null;
      const columns = visibleTableColumns.map(({ column }) => column);
      const rowValues = rows.map((row) => visibleTableColumns.map(({ index }) => row.values[index] ?? null));
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
        update: primaryKeys.length > 0
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
    [activeDbxConnection, activeDbxObject, activeObject?.primaryKeys, dbxColumnsByTable, queryResult, visibleTableColumns],
  );

  const buildDbxGridContextFilterOptions = useCallback(
    (menu: DbxGridCellContextMenuState, mode: DataGridContextFilterMode): DataGridContextFilterConditionOptions | null => {
      if (!activeDbxConnection || !activeDbxObject || !queryResult) return null;
      const metadataColumns = dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? [];
      const columnInfo = metadataColumns.find((column) => column.name.toLowerCase() === menu.column.toLowerCase()) ?? null;
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
    async (
      row: DatabaseRow,
      column: string,
      value: string,
      original: string,
    ) => {
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
          setDbxSqlPreviewStatements(preview.statements);
          setDbxSqlPreviewRollback(preview.rollbackStatements);
          setDbxSqlPreviewDescription(t("database.confirmUpdateCell", { column }));
          const rollback = preview.rollbackStatements.length
            ? `\n\n${t("database.gridRollbackSql")}\n${preview.rollbackStatements.join("\n")}`
            : "";
          const ok = await confirm(`${t("database.confirmUpdateCell", { column })}\n\n${preview.statements.join("\n")}${rollback}`, {
            title: t("database.updateCell"),
            kind: "warning",
            okLabel: t("database.updateCell"),
            cancelLabel: t("common.cancel"),
          });
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
          await loadDbxObject(activeDbxObject, page, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, dbxGridOrderByInput);
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
          projectRoot,
        });
        await loadTable(activeObject, page);
      } catch (err) {
        setError(String(err));
      }
    },
    [
      activeConnection?.readOnly,
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      activeEndpoint,
      activeObject,
      buildDbxGridSaveOptions,
      dbxGridOrderByInput,
      dbxGridWhereInput,
      loadDbxObject,
      loadTable,
      page,
      projectRoot,
      queryResult,
      t,
    ],
  );

  const insertRow = useCallback(async () => {
    if (activeDbxConnection && activeDbxObject && queryResult) {
      if (!activeObject || activeDbxConnection.readOnly || !queryResult.editable) return;
      const metadataColumns = dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? [];
      const metadataByName = new Map(metadataColumns.map((column) => [column.name.toLowerCase(), column]));
      const sample = Object.fromEntries(
        queryResult.columns
          .filter((column) => !metadataByName.get(column.toLowerCase())?.column_default)
          .map((column) => [column, null]),
      );
      const raw = window.prompt(t("database.insertJsonPrompt"), JSON.stringify(sample));
      if (!raw) return;
      try {
        const parsed = JSON.parse(raw) as Record<string, unknown>;
        const newRow = queryResult.columns.map((column) => (Object.prototype.hasOwnProperty.call(parsed, column) ? parsed[column] ?? null : null));
        const options = buildDbxGridSaveOptions({ newRows: [newRow] });
        if (!options) return;
        setError(null);
        const preview = await databaseApi.dbxInsertRow({
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
        setDbxSqlPreviewStatements(preview.statements);
        setDbxSqlPreviewRollback(preview.rollbackStatements);
        setDbxSqlPreviewDescription(t("database.confirmInsertRow"));
        const rollback = preview.rollbackStatements.length
          ? `\n\n${t("database.gridRollbackSql")}\n${preview.rollbackStatements.join("\n")}`
          : "";
        const ok = await confirm(`${t("database.confirmInsertRow")}\n\n${preview.statements.join("\n")}${rollback}`, {
          title: t("database.insertRow"),
          kind: "warning",
          okLabel: t("database.insert"),
          cancelLabel: t("common.cancel"),
        });
        if (!ok) return;
        const executed = await databaseApi.dbxInsertRow({
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
        await loadDbxObject(activeDbxObject, page, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, dbxGridOrderByInput);
      } catch (err) {
        setError(String(err));
      }
      return;
    }
    if (!activeEndpoint || !activeObject || activeConnection?.readOnly) return;
    setError(null);
    const sample = Object.fromEntries(
      activeObject.columns
        .filter((column) => !column.primaryKey && !column.defaultValue)
        .map((column) => [column.name, null]),
    );
    const raw = window.prompt(t("database.insertJsonPrompt"), JSON.stringify(sample));
    if (!raw) return;
    try {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      const values = Object.entries(parsed).map(([column, value]) => ({
        column,
        value: value === null || value === undefined ? null : String(value),
      }));
      await databaseApi.insertRow({
        endpoint: activeEndpoint,
        table: activeObject.name,
        values,
        readOnly: activeConnection?.readOnly ?? false,
        projectRoot,
      });
      await loadTable(activeObject, page);
    } catch (err) {
      setError(String(err));
    }
  }, [
    activeConnection?.readOnly,
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    activeEndpoint,
    activeObject,
    buildDbxGridSaveOptions,
    dbxColumnsByTable,
    dbxGridOrderByInput,
    dbxGridWhereInput,
    loadDbxObject,
    loadTable,
    page,
    projectRoot,
    queryResult,
    t,
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
        setDbxSqlPreviewStatements(preview.statements);
        setDbxSqlPreviewRollback(preview.rollbackStatements);
        setDbxSqlPreviewDescription(confirmMessage);
        const rollback = preview.rollbackStatements.length
          ? `\n\n${t("database.gridRollbackSql")}\n${preview.rollbackStatements.join("\n")}`
          : "";
        const ok = await confirm(`${confirmMessage}\n\n${preview.statements.join("\n")}${rollback}`, {
          title,
          kind: "warning",
          okLabel: t("file.delete"),
          cancelLabel: t("common.cancel"),
        });
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
        await loadDbxObject(activeDbxObject, page, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, dbxGridOrderByInput);
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
      loadDbxObject,
      page,
      queryResult,
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
  }, [dbxGridSelectedRows, queryResult, visibleTableColumns]);

  const handleDbxGridKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      if (isTextEditingShortcutTarget(event.target)) return;
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey || event.key.toLowerCase() !== "c") return;
      if (!queryResult || !activeDbxConnection || dbxGridSelectedRows.size === 0) return;
      event.preventDefault();
      void copySelectedDbxRows();
    },
    [activeDbxConnection, copySelectedDbxRows, dbxGridSelectedRows.size, queryResult],
  );

  const runDbxGridHeaderContextMenuAction = useCallback(
    async (action: DbxGridHeaderContextMenuAction) => {
      const menu: DbxGridHeaderContextMenuState | null = contextMenu?.kind === "dbx-grid-header" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      if (action === "copyColumnName") {
        await copyNodeName(menu.column);
        return;
      }
      if (action === "previewColumn") {
        setDbxColumnPreviewSearch("");
        setDbxColumnPreview({ column: menu.column, columnIndex: menu.columnIndex });
        return;
      }
      if (action === "sortAscending" || action === "sortDescending" || action === "clearSort") {
        if (!activeDbxConnection || !activeDbxObject || !queryResult) return;
        if (!dbxGridColumnSortable(queryResult, menu.columnIndex)) return;
        const nextOrderBy =
          action === "sortAscending"
            ? dbxOrderByForColumn(menu.column, "ASC")
            : action === "sortDescending"
            ? dbxOrderByForColumn(menu.column, "DESC")
            : dbxOrderByForColumn(menu.column, null);
        setDbxGridOrderByInput(nextOrderBy);
        await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, nextOrderBy);
      }
    },
    [activeDbxConnection, activeDbxDatabase, activeDbxObject, contextMenu, copyNodeName, dbxGridWhereInput, loadDbxObject, queryResult],
  );

  const runDbxGridCellContextMenuAction = useCallback(
    async (action: DbxGridCellContextMenuAction) => {
      const menu = contextMenu?.kind === "dbx-grid-cell" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      if (action === "copyValue") {
        try {
          await navigator.clipboard?.writeText(cellPreviewText(menu.value).text);
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "copyColumnName") {
        await copyNodeName(menu.column);
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
        if (!activeDbxConnection || !activeDbxObject || !queryResult) return;
        if (!dbxGridColumnSortable(queryResult, menu.columnIndex)) return;
        const nextOrderBy =
          action === "sortAscending"
            ? dbxOrderByForColumn(menu.column, "ASC")
            : action === "sortDescending"
            ? dbxOrderByForColumn(menu.column, "DESC")
            : dbxOrderByForColumn(menu.column, null);
        setDbxGridOrderByInput(nextOrderBy);
        await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, nextOrderBy);
        return;
      }
      if (action === "clearFilter") {
        setDbxGridWhereInput("");
        if (activeDbxConnection && activeDbxObject) {
          await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, "", dbxGridOrderByInput);
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
          await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, nextWhere, dbxGridOrderByInput);
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "copyAllTsv") {
        if (!queryResult || visibleTableColumns.length === 0) return;
        try {
          await navigator.clipboard?.writeText(dbxGridRowsToTsv(visibleTableColumns, queryResult.rows));
        } catch (err) {
          setError(String(err));
        }
        return;
      }
      if (action === "copyRowJson") {
        const rows = dbxGridContextRows(menu);
        if (rows.length === 0) return;
        try {
          await navigator.clipboard?.writeText(dbxGridRowsToJson(visibleTableColumns, rows));
        } catch (err) {
          setError(String(err));
        }
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
          setError(String(err));
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
          setError(String(err));
        }
      }
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      buildDbxGridContextFilterOptions,
      buildDbxGridCopyOptions,
      contextMenu,
      copyNodeName,
      dbxGridContextRows,
      dbxGridOrderByInput,
      dbxGridWhereInput,
      loadDbxObject,
      queryResult,
      visibleTableColumns,
    ],
  );

  const deleteRow = useCallback(
    async (row: DatabaseRow) => {
      if (activeDbxConnection && activeDbxObject && queryResult) {
        const rowIndex = queryResult.rows.indexOf(row);
        if (rowIndex >= 0) {
          await deleteDbxRowsByIndexes([rowIndex], t("database.confirmDeleteRow"), t("database.deleteRow"));
        }
        return;
      }
      if (!activeEndpoint || !activeObject || activeConnection?.readOnly) return;
      const ok = await confirm(t("database.confirmDeleteRow"), {
        title: t("database.deleteRow"),
        kind: "warning",
        okLabel: t("file.delete"),
        cancelLabel: t("common.cancel"),
      });
      if (!ok) return;
      setError(null);
      try {
        await databaseApi.deleteRow({
          endpoint: activeEndpoint,
          table: activeObject.name,
          rowKey: rowKeyFor(row),
          readOnly: activeConnection?.readOnly ?? false,
          projectRoot,
        });
        await loadTable(activeObject, page);
      } catch (err) {
        setError(String(err));
      }
    },
    [
      activeConnection?.readOnly,
      activeDbxConnection,
      activeDbxObject,
      activeEndpoint,
      activeObject,
      deleteDbxRowsByIndexes,
      loadTable,
      page,
      projectRoot,
      queryResult,
      t,
    ],
  );

  const contextMenuDbxConnection =
    contextMenu?.kind === "dbx" || contextMenu?.kind === "user-admin"
      ? dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null
      : null;
  const contextMenuDbxConnectionHasMoveTargets = contextMenuDbxConnection
    ? Boolean(contextMenuDbxConnection.connectionGroup?.trim()) ||
      extraDbxConnectionGroups.some((group) => group.trim().length > 0) ||
      dbxConnections.some((connection) => connection.id !== contextMenuDbxConnection.id && Boolean(connection.connectionGroup?.trim()))
    : false;
  const contextMenuConnectionActive =
    contextMenu?.kind === "legacy"
      ? activeConnectionId === contextMenu.connectionId
      : contextMenu?.kind === "dbx"
        ? activeDbxConnectionId === contextMenu.connectionId
        : false;
  const currentContextMenuPinnedNodeId = contextMenuPinnedNodeId(contextMenu);
  const contextMenuTreeNodePinned = currentContextMenuPinnedNodeId ? pinnedTreeNodeIds.has(currentContextMenuPinnedNodeId) : false;
  const activeDbxGridPrimaryKeys = activeObject?.primaryKeys ?? queryResult?.primaryKeys ?? [];
  const connectionDialogTitle = editingDbxConnectionId ? t("database.editConnection") : t("database.newConnection");

  return (
    <div style={s.databaseRoot}>
      <div style={s.databaseTopToolbar}>
        <DbxButton variant="ghost" size="sm" icon={Database} onClick={openNewConnectionDialog}>
          {t("database.newConnection")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={FilePlus} onClick={handleNewQuery} disabled={!activeSqlCapable}>
          {t("database.newQuery")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={FileCode} onClick={handleExecuteSqlFile} disabled={loading}>
          {t("database.executeSqlFile")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={Wrench} onClick={openDriverManager}>
          {t("database.driverManager")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={GitMerge} onClick={() => openAdvancedTool("transfer")}>
          {t("database.dataTransfer")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={GitCompare} onClick={() => openAdvancedTool("schema-diff")}>
          {t("database.schemaDiff")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={Network} onClick={() => openAdvancedTool("data-compare")}>
          {t("database.dataCompare")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={UsersRound} onClick={openUserAdmin}>
          {t("database.userAdmin")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={Table2} onClick={() => void openErDiagram()}>
          {t("database.erDiagram")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={Search} onClick={() => void openDatabaseSearch()}>
          {t("database.databaseSearch")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={SlidersHorizontal} onClick={() => void openTableStructure()}>
          {t("database.tableStructure")}
        </DbxButton>
      </div>
      <aside style={s.databaseSidebar}>
        <div style={s.databaseSidebarHeader}>
          <div style={s.databaseTitleRow}>
            <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
              <Database size={16} />
              <span style={s.databaseTitle}>{t("database.title")}</span>
            </div>
            <DbxButton
              variant="ghost"
              size="icon-sm"
              icon={RefreshCcw}
              onClick={refresh}
              disabled={(!activeConnection && !activeDbxConnection) || loading}
              title={t("common.refresh")}
            />
          </div>
          {activeConnection && (
            <DbxButton
              variant={activeConnection.readOnly ? "destructive" : "outline"}
              size="sm"
              onClick={toggleReadOnly}
              style={{ justifyContent: "flex-start" }}
            >
              {activeConnection.readOnly ? t("database.readOnlyOn") : t("database.readOnlyOff")}
            </DbxButton>
          )}
        </div>

        <DatabaseSidebarTree
          connections={connections}
          dbxConnections={dbxConnections}
          extraDbxConnectionGroups={extraDbxConnectionGroups}
          activeConnectionId={activeConnectionId}
          activeDbxConnectionId={activeDbxConnectionId}
          activeDbxConnection={activeDbxConnection}
          activeDbxDatabase={activeDbxDatabase}
          activeDbxSchema={activeDbxSchema}
          activeObject={activeObject}
          activeDbxObject={activeDbxObject}
          userAdminActive={workspaceMode === "user-admin"}
          dbxHasSqlObjectBrowser={dbxHasSqlObjectBrowser}
          visibleDbxDatabases={visibleDbxDatabases}
          dbxSchemas={dbxSchemas}
          legacyObjects={schema?.objects ?? []}
          dbxObjects={dbxObjects}
          dbxColumnsByTable={dbxColumnsByTable}
          redisDatabasesByConnection={redisDatabasesByConnection}
          redisKeysByDatabase={redisKeysByDatabase}
          redisScanStateByDatabase={redisScanStateByDatabase}
          mongoDatabasesByConnection={mongoDatabasesByConnection}
          mongoCollectionsByDatabase={mongoCollectionsByDatabase}
          mongoDocumentsByCollection={mongoDocumentsByCollection}
          mongoDocumentTotalsByCollection={mongoDocumentTotalsByCollection}
          activeMongoDocumentId={activeMongoDocumentId}
          pinnedTreeNodeIds={pinnedTreeNodeIds}
          onSelectConnection={handleSelectConnection}
          onSelectDbxConnection={handleSelectDbxConnection}
          onDeleteConnection={handleDeleteConnection}
          onDeleteDbxConnection={handleDeleteDbxConnection}
          onSelectDatabase={loadDbxDatabase}
          onSelectDbxSchema={(connection, database, schemaName) => {
            void loadDbxSchema(connection, database, schemaName);
          }}
          onSelectLegacyObject={(object) => loadTable(object, 1)}
          onSelectDbxObject={(object) => {
            if (isDbxRoutineLikeObject(object) && activeDbxConnection) {
              void showDbxObjectSource(activeDbxConnection, activeDbxDatabase, object);
            } else {
              void loadDbxObject(object, 1);
            }
          }}
          onOpenUserAdmin={(connection) => {
            void (async () => {
              await loadDbxConnection(connection);
              setWorkspaceMode("user-admin");
              setError(supportsDbxUserAdmin(connection.dbType) ? null : t("database.selectUserAdminConnection"));
              setSqlResult(null);
              setQueryResult(null);
            })();
          }}
          onOpenNoSqlWorkspace={() => {
            if (activeDbxConnection) setWorkspaceMode(activeDbxConnection.dbType === "redis" ? "redis" : "mongo");
          }}
          onSelectRedisDatabase={selectRedisSidebarDatabase}
          onExpandRedisDatabase={(connection, database) => {
            void loadRedisSidebarKeys(connection, database);
          }}
          onLoadMoreRedisKeys={(connection, database) => {
            void loadRedisSidebarKeys(connection, database, true);
          }}
          onSelectRedisKey={selectRedisSidebarKey}
          onSelectMongoDatabase={(connection, database) => {
            void selectMongoSidebarDatabase(connection, database);
          }}
          onExpandMongoDatabase={(connection, database) => {
            void loadMongoSidebarCollections(connection, database);
          }}
          onSelectMongoCollection={(connection, database, collection) => {
            void selectMongoSidebarCollection(connection, database, collection);
          }}
          onExpandMongoCollection={(connection, database, collection) => {
            void loadMongoSidebarDocuments(connection, database, collection);
          }}
          onLoadMoreMongoDocuments={(connection, database, collection) => {
            void loadMongoSidebarDocuments(connection, database, collection, true);
          }}
          onSelectMongoDocument={(connection, database, collection, document) => {
            void selectMongoSidebarDocument(connection, database, collection, document);
          }}
          onRenameConnection={renameLegacyConnection}
          onRenameDbxConnection={(connection) => {
            void renameDbxConnection(connection);
          }}
          onRefreshConnection={inspect}
          onRefreshDbxConnection={(connection) => {
            void loadDbxConnection(connection);
          }}
          onRefreshDatabase={(connection, database) => {
            void loadDbxDatabase(connection, database);
          }}
          onRefreshDbxSchema={(connection, database, schemaName) => {
            void loadDbxSchema(connection, database, schemaName);
          }}
          onCopyNodeName={copyNodeName}
          onDropDatabase={(connection, database) => {
            void dropDbxDatabase(connection, database);
          }}
          onDropDbxSchema={(connection, database, schemaName) => {
            void dropDbxSchema(connection, database, schemaName);
          }}
          onDropDbxObject={(connection, database, object) => {
            void dropDbxObject(connection, database, object);
          }}
          onDropDbxColumn={(connection, database, object, column) => {
            void dropDbxColumn(connection, database, object, column);
          }}
          onDropDbxTableChildObject={(connection, database, object, childObject) => {
            void dropDbxTableChildObject(connection, database, object, childObject);
          }}
          onConnectionContextMenu={(event, connectionId, kind) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              kind,
            });
          }}
          onConnectionGroupContextMenu={(event, groupName) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              groupName,
              kind: "connection-group",
            });
          }}
          onUserAdminContextMenu={(event, connectionId) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              kind: "user-admin",
            });
          }}
          onDbxDatabaseContextMenu={(event, connectionId, database) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              kind: "dbx-database",
            });
          }}
          onDbxSchemaContextMenu={(event, connectionId, database, schemaName) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              schema: schemaName,
              kind: "dbx-schema",
            });
          }}
          onDbxObjectContextMenu={(event, connectionId, database, object) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              object,
              kind: "dbx-object",
            });
          }}
          onDbxColumnContextMenu={(event, connectionId, database, object, column) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              object,
              column,
              kind: "dbx-column",
            });
          }}
          onDbxTableChildObjectContextMenu={(event, connectionId, database, object, childObject) => {
            event.preventDefault();
            const childObjectType = dbxTableChildObjectType(childObject);
            if (!childObjectType) return;
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              object,
              childObject,
              childObjectType,
              kind: "dbx-table-child",
            });
          }}
          onDbxObjectGroupContextMenu={(event, connectionId, database, schema, groupKey, label) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              schema,
              groupKey,
              label,
              kind: "dbx-object-group",
            });
          }}
          onRedisDatabaseContextMenu={(event, connectionId, database) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              kind: "redis-database",
            });
          }}
          onRedisKeyContextMenu={(event, connectionId, database, keyRaw) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              keyRaw,
              kind: "redis-key",
            });
          }}
          onMongoDatabaseContextMenu={(event, connectionId, database) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              kind: "mongo-database",
            });
          }}
          onMongoCollectionContextMenu={(event, connectionId, database, collection) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              collection,
              kind: "mongo-collection",
            });
          }}
          onMongoDocumentContextMenu={(event, connectionId, database, collection, document) => {
            event.preventDefault();
            setContextMenu({
              x: event.clientX,
              y: event.clientY,
              connectionId,
              database,
              collection,
              document,
              kind: "mongo-document",
            });
          }}
        />
      </aside>

      <main style={s.databaseMain}>
        <div style={s.databaseTopbar}>
          <div style={{ minWidth: 0 }}>
            <div style={s.databaseTitle}>
              {workspaceMode === "query"
                ? t("database.newQuery")
                : workspaceMode === "sql-file"
                  ? t("database.executeSqlFile")
                  : workspaceMode === "query-history"
                    ? t("database.queryHistory")
                    : workspaceMode === "drivers"
                      ? t("database.driverManager")
                      : workspaceMode === "redis"
                        ? "Redis"
                        : workspaceMode === "mongo"
                          ? "MongoDB"
                          : workspaceMode === "transfer"
                            ? t("database.dataTransfer")
                            : workspaceMode === "schema-diff"
                              ? t("database.schemaDiff")
                              : workspaceMode === "data-compare"
                                ? t("database.dataCompare")
                                : workspaceMode === "user-admin"
                                  ? t("database.userAdmin")
                                  : workspaceMode === "er-diagram"
                                    ? t("database.erDiagram")
                                    : workspaceMode === "database-search"
                                      ? t("database.databaseSearch")
                                      : workspaceMode === "table-structure"
                                        ? t("database.tableStructure")
                                        : workspaceMode === "table-info"
                                          ? t("database.tableInfo")
                                          : activeObject?.name ?? t("database.noSelection")}
            </div>
            <div style={s.databasePath}>
              {activeEndpoint
                ? endpointLabel(activeEndpoint)
                : activeDbxConnection
                  ? `${activeDbxConnection.dbType}: ${activeDbxConnection.name}`
                  : t("database.chooseConnection")}
            </div>
          </div>
          {error && <div style={s.databaseError} title={error}>{error}</div>}
        </div>

        {workspaceMode === "query-history" ? (
          <div style={s.databaseWorkspacePanel}>
            <div style={s.databaseWorkspaceHeader}>
              <div>
                <div style={s.databaseWorkspaceTitle}>{t("database.queryHistory")}</div>
                <div style={s.databaseDialogHint}>{t("database.queryHistoryHint")}</div>
              </div>
              <DbxButton variant="outline" size="sm" icon={Trash2} onClick={() => setQueryHistory([])} disabled={queryHistory.length === 0}>
                {t("database.clearQueryHistory")}
              </DbxButton>
            </div>
            {queryHistory.length === 0 ? (
              <div style={s.databaseEmpty}>{t("database.queryHistoryEmpty")}</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {queryHistory.map((entry) => (
                  <button
                    key={entry.id}
                    type="button"
                    style={{
                      ...s.databaseListButton,
                      alignItems: "stretch",
                      flexDirection: "column",
                      gap: 6,
                      minHeight: 0,
                      padding: "10px 12px",
                    }}
                    onClick={() => restoreQueryHistoryEntry(entry)}
                  >
                    <div style={{ display: "flex", gap: 8, width: "100%", alignItems: "center" }}>
                      <span style={{ color: "var(--text-secondary)", fontWeight: 600 }}>{entry.connectionName}</span>
                      {entry.database && <span style={{ color: "var(--text-hint)" }}>{entry.database}</span>}
                      {entry.schema && <span style={{ color: "var(--text-hint)" }}>{entry.schema}</span>}
                      <span style={{ marginLeft: "auto", color: "var(--text-hint)", fontSize: 11 }}>
                        {new Date(entry.executedAt).toLocaleString()}
                      </span>
                    </div>
                    <pre style={{ ...s.databaseSqlPreview, margin: 0, maxHeight: 86 }}>{entry.sql}</pre>
                    <div style={{ display: "flex", gap: 10, color: "var(--text-hint)", fontSize: 11 }}>
                      {entry.rowsAffected != null && <span>{t("database.historyRowsAffected", { rows: entry.rowsAffected })}</span>}
                      {entry.executionTimeMs != null && <span>{t("database.historyExecutionTime", { ms: entry.executionTimeMs })}</span>}
                      <span>{t("database.restoreQuery")}</span>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
        ) : workspaceMode === "sql-file" ? (
          <div style={s.databaseWorkspacePanel}>
            <div style={s.databaseWorkspaceHeader}>
              <div>
                <div style={s.databaseWorkspaceTitle}>{t("database.executeSqlFile")}</div>
                <div style={s.databaseDialogHint}>{t("database.sqlFileHint")}</div>
              </div>
              <DbxButton variant="outline" size="sm" icon={FileCode} onClick={chooseSqlFile} disabled={loading}>
                {t("database.chooseSqlFile")}
              </DbxButton>
            </div>
            <div style={s.databaseDialogFormGrid}>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.filePath")}</span>
                <input
                  style={s.databaseDialogInput}
                  value={sqlFilePath}
                  onChange={(event) => setSqlFilePath(event.target.value)}
                  placeholder="/path/to/script.sql"
                />
              </label>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.timeoutSecs")}</span>
                <input
                  style={s.databaseDialogInput}
                  value={sqlFileTimeoutSecs}
                  onChange={(event) => setSqlFileTimeoutSecs(event.target.value)}
                />
              </label>
            </div>
            <pre style={s.databaseSqlPreview}>
              {sqlFilePreview || t("database.sqlFilePreviewEmpty")}
            </pre>
            <div style={s.databaseDialogFooter}>
              <DbxButton
                variant="default"
                size="sm"
                icon={Play}
                onClick={executeSqlFileFromPanel}
                disabled={loading || !sqlFilePath.trim() || !activeSqlCapable}
              >
                {t("database.executeSqlFile")}
              </DbxButton>
            </div>
          </div>
        ) : workspaceMode === "drivers" ? (
          <div style={s.databaseWorkspacePanel}>
            <div style={s.databaseWorkspaceHeader}>
              <div>
                <div style={s.databaseWorkspaceTitle}>{t("database.driverManager")}</div>
                <div style={s.databaseDialogHint}>{t("database.driverManagerHint")}</div>
              </div>
              <DbxButton variant="outline" size="sm" icon={RefreshCcw} onClick={openDriverManager} disabled={loading}>
                {t("common.refresh")}
              </DbxButton>
            </div>
            <div style={s.databaseTableWrap}>
              <table style={s.databaseTable}>
                <thead>
                  <tr>
                    <th style={s.databaseTh}>{t("database.driver")}</th>
                    <th style={s.databaseTh}>{t("database.runtime")}</th>
                    <th style={s.databaseTh}>{t("database.defaultPort")}</th>
                    <th style={s.databaseTh}>{t("database.supportLevel")}</th>
                    <th style={s.databaseTh}>{t("database.capabilities")}</th>
                  </tr>
                </thead>
                <tbody>
                  {(driverManifest?.drivers ?? []).map((driver) => {
                    const enabledCapabilities = Object.entries(driver.capabilities)
                      .filter(([, enabled]) => enabled)
                      .map(([key]) => key);
                    return (
                      <tr key={driver.dbType}>
                        <td style={s.databaseTd}>{driver.label}</td>
                        <td style={s.databaseTd}>{driver.runtimeMode}</td>
                        <td style={s.databaseTd}>{driver.defaultPort ?? "-"}</td>
                        <td style={s.databaseTd}>{driver.supportLevel}</td>
                        <td style={s.databaseTd} title={enabledCapabilities.join(", ")}>
                          {enabledCapabilities.slice(0, 6).join(", ")}
                          {enabledCapabilities.length > 6 ? "..." : ""}
                        </td>
                      </tr>
                    );
                  })}
                  {!driverManifest && (
                    <tr>
                      <td style={s.databaseTd} colSpan={5}>
                        {loading ? t("database.loading") : t("database.empty")}
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        ) : workspaceMode === "redis" && activeDbxConnection ? (
          <RedisBrowser
            connectionId={activeDbxConnection.id}
            readOnly={activeDbxConnection.readOnly}
            initialDb={activeDbxDatabase?.startsWith("db") ? Number(activeDbxDatabase.slice(2)) : undefined}
            initialKey={activeDbxSchema ?? undefined}
            keySeparator={dbxString(dbxConfigRecord(activeDbxConnection), "redis_key_separator", ":")}
          />
        ) : workspaceMode === "mongo" && activeDbxConnection ? (
          <MongoBrowser
            connectionId={activeDbxConnection.id}
            readOnly={activeDbxConnection.readOnly}
            initialDatabase={activeMongoWorkspaceDatabase ?? undefined}
            initialCollection={activeMongoWorkspaceDatabase ? activeDbxSchema ?? undefined : undefined}
            initialDocumentId={activeMongoWorkspaceDatabase ? activeMongoDocumentId ?? undefined : undefined}
            onDocumentsQueryApplied={(database, collection, filter, sort) => {
              const query = { filter, sort };
              const key = `${activeDbxConnection.id}:${database}:${collection}`;
              setMongoDocumentQueriesByCollection((current) => ({ ...current, [key]: query }));
              void loadMongoSidebarDocuments(activeDbxConnection, database, collection, false, query);
            }}
          />
        ) : (workspaceMode === "transfer" || workspaceMode === "schema-diff" || workspaceMode === "data-compare") && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
          <GuidancePanel
            title={
              workspaceMode === "transfer"
                ? t("database.dataTransfer")
                : workspaceMode === "schema-diff"
                  ? t("database.schemaDiff")
                  : t("database.dataCompare")
            }
            message={t("database.selectDbxSqlConnection")}
          />
        ) : (workspaceMode === "transfer" || workspaceMode === "schema-diff" || workspaceMode === "data-compare") && activeDbxConnection ? (
          <DatabaseAdvancedTools
            connectionId={activeDbxConnection.id}
            mode={workspaceMode}
            database={activeDbxDatabase}
            schema={activeDbxSchema ?? selectedDbxTable?.schema ?? null}
            table={selectedDbxTable?.name ?? null}
            availableConnections={sqlDbxConnections}
            sourceObjects={dbxTableObjects}
            sourceColumnsByTable={dbxColumnsByTable}
            sourceDatabaseType={activeDbxConnection.dbType}
          />
        ) : workspaceMode === "user-admin" && (!activeDbxConnection || !supportsDbxUserAdmin(activeDbxConnection.dbType)) ? (
          <GuidancePanel title={t("database.userAdmin")} message={t("database.selectUserAdminConnection")} />
        ) : workspaceMode === "user-admin" && activeDbxConnection ? (
          <DatabaseUserAdminPanel
            connection={activeDbxConnection}
            database={activeDbxDatabase}
            schema={activeDbxSchema}
          />
        ) : workspaceMode === "er-diagram" && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
          <GuidancePanel title={t("database.erDiagram")} message={t("database.selectDbxSqlConnection")} />
        ) : workspaceMode === "er-diagram" && activeDbxConnection ? (
          <ErDiagramPanel
            tables={dbxTableObjects}
            columnsByTable={dbxColumnsByTable}
          />
        ) : workspaceMode === "database-search" && (!activeDbxConnection || !dbxHasSqlObjectBrowser || !activeDbxDatabase) ? (
          <GuidancePanel title={t("database.databaseSearch")} message={t("database.selectDbxSqlConnection")} />
        ) : workspaceMode === "database-search" && activeDbxConnection ? (
          <DatabaseSearchPanel
            connection={activeDbxConnection}
            database={activeDbxDatabase}
            schema={activeDbxSchema}
            objects={dbxObjects}
            onOpenResult={(object, whereInput) => {
              void loadDbxObject(object, 1, activeDbxConnection, activeDbxDatabase, whereInput);
            }}
          />
        ) : workspaceMode === "table-structure" && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
          <GuidancePanel title={t("database.tableStructure")} message={t("database.selectDbxTable")} />
        ) : workspaceMode === "table-structure" && !selectedDbxTable ? (
          <GuidancePanel title={t("database.tableStructure")} message={t("database.selectDbxTable")} />
        ) : workspaceMode === "table-structure" && selectedDbxTable ? (
          <TableStructurePanel
            connectionId={activeDbxConnection?.id}
            database={activeDbxDatabase}
            schema={selectedDbxTable.schema ?? null}
            databaseType={activeDbxConnection?.dbType ?? null}
            tableName={selectedDbxTable.name}
            columns={
              dbxColumnsByTable[
                selectedDbxTable.schema ? `${selectedDbxTable.schema}.${selectedDbxTable.name}` : selectedDbxTable.name
              ] ?? []
            }
            readOnly={activeDbxConnection?.readOnly ?? true}
          />
        ) : workspaceMode === "table-info" && (!activeDbxConnection || !dbxHasSqlObjectBrowser || !selectedDbxInfoObject) ? (
          <GuidancePanel title={t("database.tableInfo")} message={t("database.selectDbxTable")} />
        ) : workspaceMode === "table-info" && selectedDbxInfoObject ? (
          <div style={s.databaseTableInfoRoot}>
            <div style={s.databaseTableInfoHeader}>
              <div>
                <div style={s.databaseWorkspaceTitle}>{t("database.tableInfo")}</div>
                <div style={s.databaseDialogHint}>
                  {activeDbxDatabase ? `${activeDbxDatabase} / ` : ""}
                  {dbxObjectKey(selectedDbxInfoObject)}
                </div>
              </div>
              <DbxButton variant="outline" size="sm" icon={FileCode} onClick={() => void showDbxObjectDdl(activeDbxConnection!, activeDbxDatabase, selectedDbxInfoObject)}>
                {t("database.viewDdl")}
              </DbxButton>
            </div>
            <div role="tablist" aria-label="Table info sections" style={s.databaseTableInfoTabs}>
              {([
                { key: "columns" as const, label: t("database.columns"), count: selectedDbxInfoColumns.length },
                { key: "indexes" as const, label: t("database.indexes"), count: selectedDbxInfoIndexes.length },
                { key: "foreignKeys" as const, label: t("database.foreignKeys"), count: selectedDbxInfoForeignKeys.length },
                { key: "triggers" as const, label: t("database.triggers"), count: selectedDbxInfoTriggers.length },
                { key: "ddl" as const, label: t("database.ddl"), count: tableInfoDdl ? 1 : 0 },
              ]).map((tab) => {
                const active = tableInfoActiveTab === tab.key;
                return (
                  <button
                    key={tab.key}
                    type="button"
                    role="tab"
                    aria-label={`${tab.label} ${tab.count}`}
                    aria-selected={active}
                    style={{ ...s.databaseTableInfoTab, ...(active ? s.databaseTableInfoTabActive : null) }}
                    onClick={() => setTableInfoActiveTab(tab.key)}
                  >
                    <span>{tab.label}</span>
                    <span>{tab.count}</span>
                  </button>
                );
              })}
            </div>
            <div style={s.databaseTableInfoSearch}>
              <label style={{ ...s.databaseDialogField, flex: "1 1 260px", maxWidth: 420 }}>
                <span style={s.databaseDialogLabel}>{t("database.search")}</span>
                <span style={{ position: "relative", display: "flex", alignItems: "center" }}>
                  <Search aria-hidden="true" size={14} style={{ position: "absolute", left: 9, color: "var(--text-hint)" }} />
                  <input
                    style={{ ...s.databaseDialogInput, paddingLeft: 30 }}
                    value={tableInfoSearch}
                    onChange={(event) => setTableInfoSearch(event.target.value)}
                    placeholder={t("database.searchPlaceholder")}
                    aria-label="Search table info"
                  />
                </span>
              </label>
              <div style={{ display: "flex", gap: 12, flexWrap: "wrap", color: "var(--text-hint)", fontSize: 12 }}>
                <span>{selectedDbxInfoObject.name}</span>
                <span>{selectedDbxInfoObject.object_type}</span>
                <span>{selectedDbxInfoObject.schema || "-"}</span>
              </div>
            </div>
            <div style={s.databaseTableInfoContent} role="tabpanel">
              {tableInfoActiveTab === "ddl" ? (
                <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: 12 }}>
                  <div style={{ display: "flex", justifyContent: "space-between", gap: 8, alignItems: "center" }}>
                    <span style={s.databaseDialogHint}>
                      {tableInfoDdlLoading ? t("database.loading") : t("database.ddl")}
                    </span>
                    <DbxButton variant="outline" size="xs" icon={RefreshCcw} disabled={tableInfoDdlLoading} onClick={() => void loadTableInfoDdl()}>
                      {t("common.refresh")}
                    </DbxButton>
                  </div>
                  <pre style={{ ...s.databaseSqlPreview, margin: 0, minHeight: 180 }}>
                    {tableInfoDdlLoading ? t("database.loading") : tableInfoDdl || t("database.empty")}
                  </pre>
                  {tableInfoDdlError && <div style={s.databaseError}>{tableInfoDdlError}</div>}
                </div>
              ) : tableInfoActiveTab === "columns" ? (
                <table style={s.databaseTable}>
                  <thead>
                    <tr>
                      <th style={s.databaseTh}>{t("database.columnName")}</th>
                      <th style={s.databaseTh}>{t("database.dataType")}</th>
                      <th style={s.databaseTh}>{t("database.nullable")}</th>
                      <th style={s.databaseTh}>{t("database.primaryKey")}</th>
                      <th style={s.databaseTh}>{t("database.defaultValue")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {filteredDbxInfoColumns.map((column) => (
                      <tr key={column.name}>
                        <td style={s.databaseTd}>{column.name}</td>
                        <td style={s.databaseTd}>{column.data_type}</td>
                        <td style={s.databaseTd}>{column.is_nullable ? t("database.yes") : t("database.no")}</td>
                        <td style={s.databaseTd}>{column.is_primary_key ? t("database.yes") : t("database.no")}</td>
                        <td style={s.databaseTd}>{column.column_default ?? "-"}</td>
                      </tr>
                    ))}
                    {filteredDbxInfoColumns.length === 0 && (
                      <tr>
                        <td style={s.databaseTd} colSpan={5}>{t("database.empty")}</td>
                      </tr>
                    )}
                  </tbody>
                </table>
              ) : (
                <table style={s.databaseTable}>
                  <thead>
                    <tr>
                      <th style={s.databaseTh}>{t("database.objectName")}</th>
                      <th style={s.databaseTh}>{t("database.objectType")}</th>
                      <th style={s.databaseTh}>{t("database.schemaName")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(tableInfoActiveTab === "indexes"
                      ? filteredDbxInfoIndexes
                      : tableInfoActiveTab === "foreignKeys"
                        ? filteredDbxInfoForeignKeys
                        : filteredDbxInfoTriggers
                    ).map((object) => (
                      <tr key={`${object.object_type}:${object.name}`}>
                        <td style={s.databaseTd}>{object.name}</td>
                        <td style={s.databaseTd}>{object.object_type}</td>
                        <td style={s.databaseTd}>{object.schema || "-"}</td>
                      </tr>
                    ))}
                    {(tableInfoActiveTab === "indexes"
                      ? filteredDbxInfoIndexes
                      : tableInfoActiveTab === "foreignKeys"
                        ? filteredDbxInfoForeignKeys
                        : filteredDbxInfoTriggers
                    ).length === 0 && (
                      <tr>
                        <td style={s.databaseTd} colSpan={3}>{t("database.empty")}</td>
                      </tr>
                    )}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        ) : (
          <>
        {workspaceMode === "query" && (
          <div style={s.databaseSqlPanel}>
            <textarea
              style={s.databaseSqlInput}
              value={sql}
              onChange={(event) => setSql(event.target.value)}
              onDragOver={handleSqlDragOver}
              onDrop={handleSqlDrop}
              spellCheck={false}
              placeholder={t("database.sqlPlaceholder")}
            />
            <DbxButton variant="default" size="sm" icon={Play} onClick={runSql} disabled={!activeSqlCapable || loading} style={{ width: 86, height: "auto" }}>
              {t("database.run")}
            </DbxButton>
          </div>
        )}

        <div style={s.databaseToolbar}>
          <DbxButton
            variant="outline"
            size="sm"
            icon={RefreshCcw}
            disabled={!activeObject || loading}
            onClick={() => {
              if (activeDbxConnection && activeDbxObject) {
                loadDbxObject(activeDbxObject, page, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, dbxGridOrderByInput);
              } else if (activeObject) {
                loadTable(activeObject, page);
              }
            }}
          >
            {t("database.refresh")}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Plus}
            disabled={!activeObject || !queryResult?.editable || activeConnection?.readOnly || activeDbxConnection?.readOnly || loading}
            onClick={insertRow}
          >
            {t("database.insert")}
          </DbxButton>
          {dbxSqlPreviewStatements.length > 0 && (
            <DbxButton variant="outline" size="sm" icon={FileCode} onClick={() => setDbxSqlPreviewOpen(true)}>
              {t("database.previewSql")}
            </DbxButton>
          )}
          {queryResult && activeDbxConnection && activeDbxObject && (
            <DbxButton
              variant="outline"
              size="sm"
              icon={SlidersHorizontal}
              disabled={loading}
              onClick={() => {
                setWorkspaceMode("table-info");
                setTableInfoActiveTab("columns");
                void loadDbxColumnsForTables([activeDbxObject], activeDbxConnection, activeDbxDatabase);
              }}
            >
              {t("database.tableProperties")}
            </DbxButton>
          )}
          {queryResult && activeDbxConnection && (
            <>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                disabled={dbxGridSelectedRows.size === 0 || loading}
                onClick={() => void copySelectedDbxRows()}
              >
                {t("database.copySelectedRowsCount", { count: dbxGridSelectedRows.size })}
              </DbxButton>
            </>
          )}
          <DbxButton
            variant="ghost"
            size="icon-sm"
            icon={ChevronLeft}
            disabled={!activeObject || page <= 1 || loading}
            onClick={() => {
              if (activeDbxConnection && activeDbxObject) {
                loadDbxObject(activeDbxObject, Math.max(1, page - 1), activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, dbxGridOrderByInput);
              } else if (activeObject) {
                loadTable(activeObject, Math.max(1, page - 1));
              }
            }}
          />
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
            {t("database.page", {
              page,
              total: totalPages ?? "?",
            })}
          </span>
          {queryResult && activeDbxConnection && (
            <label style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 12, color: "var(--text-muted)" }}>
              <span>{t("database.gridRowsPerPage")}</span>
              <select
                style={{ ...s.databaseDialogInput, width: 82, height: 28, padding: "0 6px" }}
                value={(DBX_GRID_PAGE_SIZE_OPTIONS as readonly number[]).includes(dbxGridPageSize) ? dbxGridPageSize : "custom"}
                disabled={loading}
                aria-label={t("database.gridRowsPerPage")}
                onChange={(event) => {
                  const val = event.currentTarget.value;
                  if (val === "custom") {
                    const custom = window.prompt(t("database.gridCustomPageSize"), String(dbxGridPageSize));
                    if (custom) {
                      const num = Number(custom);
                      if (Number.isFinite(num) && num >= 1 && num <= 10000) void changeDbxGridPageSize(num);
                    }
                  } else {
                    void changeDbxGridPageSize(Number(val));
                  }
                }}
              >
                {DBX_GRID_PAGE_SIZE_OPTIONS.map((option) => (
                  <option key={option} value={option}>{option}</option>
                ))}
                {!(DBX_GRID_PAGE_SIZE_OPTIONS as readonly number[]).includes(dbxGridPageSize) && (
                  <option value="custom">{dbxGridPageSize}</option>
                )}
                <option value="custom">{t("database.gridCustomPageSize")}</option>
              </select>
            </label>
          )}
          <DbxButton
            variant="ghost"
            size="icon-sm"
            icon={ChevronRight}
            disabled={!activeObject || loading || (totalPages != null && page >= totalPages)}
            onClick={() => {
              if (activeDbxConnection && activeDbxObject) {
                loadDbxObject(activeDbxObject, page + 1, activeDbxConnection, activeDbxDatabase, dbxGridWhereInput, dbxGridOrderByInput);
              } else if (activeObject) {
                loadTable(activeObject, page + 1);
              }
            }}
          />
          <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
            {loading
              ? t("database.loading")
              : activeConnection?.readOnly
                ? t("database.readOnlyBadge")
                : sqlResult?.message}
          </span>
        </div>

        {queryResult && activeDbxConnection && activeDbxObject && (
          <div role="group" aria-label="Table filters" style={s.databaseGridFilterBar}>
            <label style={{ ...s.databaseDialogField, minWidth: 220, flex: "1 1 240px" }}>
              <span style={s.databaseDialogLabel}>{t("database.gridWhere")}</span>
              <input
                style={s.databaseDialogInput}
                value={dbxGridWhereInput}
                onChange={(event) => setDbxGridWhereInput(event.target.value)}
                placeholder={t("database.gridWherePlaceholder")}
                aria-label={t("database.gridWhere")}
              />
            </label>
            <label style={{ ...s.databaseDialogField, minWidth: 180, flex: "1 1 220px" }}>
              <span style={s.databaseDialogLabel}>{t("database.gridOrderBy")}</span>
              <input
                style={s.databaseDialogInput}
                value={dbxGridOrderByInput}
                onChange={(event) => setDbxGridOrderByInput(event.target.value)}
                placeholder={t("database.gridOrderByPlaceholder")}
                aria-label={t("database.gridOrderBy")}
              />
            </label>
            <label style={{ ...s.databaseDialogField, minWidth: 180, flex: "1 1 220px" }}>
              <span style={s.databaseDialogLabel}>{t("database.gridSearchCurrentPage")}</span>
              <input
                style={s.databaseDialogInput}
                value={dbxGridSearch}
                onChange={(event) => setDbxGridSearch(event.target.value)}
                placeholder={t("database.gridSearchPlaceholder")}
                aria-label={t("database.gridSearchCurrentPage")}
              />
            </label>
            <DbxButton variant="outline" size="sm" icon={Search} disabled={loading} onClick={() => void reloadActiveDbxGrid()} style={{ alignSelf: "end" }}>
              {t("database.applyFilter")}
            </DbxButton>
            <DbxButton variant="outline" size="sm" icon={RefreshCcw} disabled={loading} onClick={() => void resetActiveDbxGrid()} style={{ alignSelf: "end" }}>
              {t("database.gridReset")}
            </DbxButton>
            <label style={{ ...s.databaseDialogField, minWidth: 150, flex: "0 1 170px" }}>
              <span style={s.databaseDialogLabel}>{t("database.exportFormat")}</span>
              <select
                style={s.databaseDialogInput}
                value={dbxGridExportFormat}
                disabled={loading}
                aria-label={t("database.exportFormat")}
                onChange={(event) => setDbxGridExportFormat(event.currentTarget.value as TableExportFormat)}
              >
                <option value="csv">{t("database.exportCsv")}</option>
                <option value="json">{t("database.exportJson")}</option>
                <option value="markdown">{t("database.exportMarkdown")}</option>
                <option value="insertSql">{t("database.exportInsertSql")}</option>
                <option value="updateSql">{t("database.exportUpdateSql")}</option>
                <option value="xlsx">{t("database.exportXlsx")}</option>
              </select>
            </label>
            <DbxButton variant="outline" size="sm" icon={Download} disabled={loading || visibleTableColumns.length === 0} onClick={() => void exportActiveDbxGrid()} style={{ alignSelf: "end" }}>
              {t("database.exportGrid")}
            </DbxButton>
            {dbxGridSelectedRows.size > 0 && (
              <DbxButton variant="outline" size="sm" icon={Download} disabled={loading || visibleTableColumns.length === 0} onClick={() => void exportSelectedDbxRows()} style={{ alignSelf: "end" }}>
                {t("database.exportSelectedRows", { count: dbxGridSelectedRows.size })}
              </DbxButton>
            )}
            <div
              role="group"
              aria-label={t("database.gridColumnVisibility")}
              style={{
                flexBasis: "100%",
                display: "grid",
                gridTemplateColumns: "minmax(180px, 280px) minmax(0, 1fr)",
                gap: 8,
                alignItems: "end",
              }}
            >
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.gridColumnVisibility")}</span>
                <input
                  style={s.databaseDialogInput}
                  value={dbxGridColumnSearch}
                  onChange={(event) => setDbxGridColumnSearch(event.target.value)}
                  placeholder={t("database.gridSearchColumns")}
                  aria-label={t("database.gridSearchColumns")}
                />
              </label>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap", alignItems: "center" }}>
                <DbxButton variant="outline" size="xs" icon={Eye} disabled={dbxGridHiddenColumns.size === 0} onClick={showAllDbxGridColumns}>
                  {t("database.gridShowAllColumns")}
                </DbxButton>
                <DbxButton variant="outline" size="xs" icon={SlidersHorizontal} disabled={tableColumns.length <= 1} onClick={invertDbxGridColumnVisibility}>
                  {t("database.gridInvertColumnVisibility")}
                </DbxButton>
                <DbxButton variant="outline" size="xs" icon={Eraser} disabled={dbxGridNullColumns.size === 0} onClick={hideNullDbxGridColumns}>
                  {t("database.gridHideNullColumns")}
                </DbxButton>
                <DbxButton
                  variant="destructive"
                  size="xs"
                  icon={Trash2}
                  disabled={!queryResult.editable || activeDbxConnection.readOnly || dbxGridSelectedRows.size === 0 || loading}
                  onClick={() => void deleteSelectedDbxRows()}
                >
                  {t("database.deleteSelectedRowsCount", { count: dbxGridSelectedRows.size })}
                </DbxButton>
                <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
                  {visibleTableColumns.length}/{tableColumns.length} - {t("database.gridColumnVisibilityHint")}
                </span>
              </div>
              <div style={{ gridColumn: "1 / -1", display: "flex", gap: 6, flexWrap: "wrap", alignItems: "center" }}>
                {filteredDbxGridColumnOptions.length > 0 ? (
                  filteredDbxGridColumnOptions.map((column) => {
                    const hidden = dbxGridHiddenColumns.has(column);
                    const nullOnly = dbxGridNullColumns.has(column);
                    return (
                      <DbxButton
                        variant="ghost"
                        size="xs"
                        icon={hidden ? Square : CheckSquare}
                        key={column}
                        onClick={() => toggleDbxGridColumnVisibility(column)}
                        aria-pressed={!hidden}
                        title={nullOnly ? t("database.gridHideNullColumns") : column}
                        style={{ color: hidden ? "var(--text-hint)" : "var(--text-primary)" }}
                      >
                        {column}
                      </DbxButton>
                    );
                  })
                ) : (
                  <span style={{ color: "var(--text-hint)", fontSize: 12 }}>{t("database.gridNoSearchResults")}</span>
                )}
              </div>
            </div>
          </div>
        )}

        {tableColumns.length === 0 ? (
          <div style={s.databaseEmpty}>{t("database.empty")}</div>
        ) : (
          <div
            style={s.databaseTableWrap}
            tabIndex={queryResult && activeDbxConnection ? 0 : undefined}
            aria-label={queryResult && activeDbxConnection ? t("database.gridData") : undefined}
            onKeyDown={queryResult && activeDbxConnection ? handleDbxGridKeyDown : undefined}
          >
            <table style={{ ...s.databaseTable, minWidth: dbxGridTableMinWidth }}>
              <thead>
                <tr>
                  {queryResult && activeDbxConnection && (
                    <th style={{ ...s.databaseTh, ...s.databaseGridControlTh, width: 42 }}>
                      <input
                        type="checkbox"
                        aria-label={t("database.selectVisibleRows")}
                        checked={allVisibleDbxGridRowsSelected}
                        disabled={visibleDbxGridRowIndexes.length === 0 || loading}
                        onChange={toggleVisibleDbxGridRowsSelection}
                      />
                    </th>
                  )}
                  {showRowIdColumn && <th style={{ ...s.databaseTh, width: 86 }}>rowid</th>}
                  {queryResult && <th style={{ ...s.databaseTh, ...s.databaseGridControlTh, width: 74 }}>{t("database.actions")}</th>}
                  {visibleTableColumns.map(({ column }) => {
                    const columnWidth = dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH;
                    const columnIndex = tableColumns.indexOf(column);
                    const sortable = dbxGridColumnSortable(queryResult, columnIndex);
                    const dbxColumnInfo = activeDbxGridColumnsByName.get(column.toLowerCase());
                    const legacyColumnInfo = activeObject?.columns.find((item) => item.name.toLowerCase() === column.toLowerCase());
                    const columnType = dbxGridColumnType(queryResult, columnIndex) ?? dbxColumnInfo?.data_type ?? legacyColumnInfo?.dataType ?? "";
                    const columnComment = dbxColumnInfo?.comment?.trim() ?? "";
                    const nullableText = dbxColumnInfo
                      ? dbxColumnInfo.is_nullable ? t("database.yes") : t("database.no")
                      : legacyColumnInfo
                        ? legacyColumnInfo.nullable ? t("database.yes") : t("database.no")
                        : "-";
                    const primaryKeyText = dbxColumnInfo
                      ? dbxColumnInfo.is_primary_key ? t("database.yes") : t("database.no")
                      : legacyColumnInfo
                        ? legacyColumnInfo.primaryKey ? t("database.yes") : t("database.no")
                        : "-";
                    const defaultValue = dbxColumnInfo?.column_default ?? legacyColumnInfo?.defaultValue ?? "-";
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
                        <span style={s.databaseGridHeaderTypeLine} title={columnType ? t("database.gridColumnType", { type: columnType }) : undefined}>
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
                                setContextMenu({
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
                                  display: "inline-flex",
                                  alignItems: "center",
                                  gap: 5,
                                  width: "100%",
                                  minHeight: 50,
                                  padding: 0,
                                  paddingRight: 8,
                                  border: "none",
                                  background: "transparent",
                                  color: "inherit",
                                  font: "inherit",
                                  cursor: loading ? "default" : "pointer",
                                }}
                                aria-label={column}
                                disabled={loading}
                                onClick={() => void toggleDbxGridColumnSort(column)}
                              >
                                {columnHeaderContent}
                                {dbxGridOrderByInput.toLowerCase() === `${quoteSqlName(column)} asc`.toLowerCase() ? (
                                  <span aria-label={t("database.sortAscending")}>ASC</span>
                                ) : dbxGridOrderByInput.toLowerCase() === `${quoteSqlName(column)} desc`.toLowerCase() ? (
                                  <span aria-label={t("database.sortDescending")}>DESC</span>
                                ) : (
                                  <span aria-hidden="true" style={{ color: "var(--text-hint)" }}>--</span>
                                )}
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
                                borderRight: resizingDbxGridColumn === column ? "1px solid var(--accent)" : "1px solid transparent",
                                background: resizingDbxGridColumn === column ? "var(--bg-hover)" : "transparent",
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
                  const dbxRowIndex = queryResult && activeDbxConnection ? queryResult.rows.indexOf(row) : -1;
                  return (
                  <tr key={`${row.rowId ?? "sql"}:${rowIndex}`}>
                    {queryResult && activeDbxConnection && (
                      <td style={{ ...s.databaseTd, ...s.databaseGridControlTd }}>
                        <input
                          type="checkbox"
                          aria-label={t("database.selectRow", { row: rowIndex + 1 })}
                          checked={dbxRowIndex >= 0 && dbxGridSelectedRows.has(dbxRowIndex)}
                          disabled={dbxRowIndex < 0 || loading}
                          onChange={() => {
                            if (dbxRowIndex >= 0) toggleDbxGridRowSelection(dbxRowIndex);
                          }}
                        />
                      </td>
                    )}
                    {showRowIdColumn && <td style={{ ...s.databaseTd, color: "var(--text-hint)" }}>{row.rowId ?? "-"}</td>}
                    {queryResult && (
                      <td style={{ ...s.databaseTd, ...s.databaseGridControlTd }}>
                        <DbxButton
                          variant="destructive"
                          size="xs"
                          icon={Trash2}
                          disabled={!queryResult.editable || activeConnection?.readOnly || activeDbxConnection?.readOnly}
                          aria-label={t("database.deleteRow")}
                          title={t("database.deleteRow")}
                          onClick={() => deleteRow(row)}
                        />
                      </td>
                    )}
                    {visibleTableColumns.map(({ column, index: columnIndex }) => {
                      const original = valueToText(row.values[columnIndex]);
                      const previewable = Boolean(queryResult && activeDbxConnection);
                      const editable = Boolean(
                          queryResult &&
                          activeObject?.objectType === "table" &&
                          queryResult.editable &&
                          !activeConnection?.readOnly &&
                          !activeDbxConnection?.readOnly,
                      );
                      return (
                        <td
                          key={`${column}:${columnIndex}`}
                          style={{
                            ...s.databaseTd,
                            width: dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH,
                            minWidth: dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH,
                            maxWidth: dbxGridColumnWidths[column] ?? DBX_GRID_DEFAULT_COLUMN_WIDTH,
                          }}
                          title={original}
                          onContextMenu={
                            queryResult && activeDbxConnection
                              ? (event) => {
                                  event.preventDefault();
                                  setContextMenu({
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
                          <div style={{ display: "flex", alignItems: "center", gap: 4, minWidth: 0 }}>
                            {editable ? (
                              <input
                                style={{ ...s.databaseCellInput, minWidth: 0, flex: "1 1 auto" }}
                                defaultValue={original}
                                onFocus={(event) => {
                                  event.currentTarget.style.borderColor = "var(--border-focus)";
                                  event.currentTarget.style.background = "var(--bg-input)";
                                }}
                                onBlur={(event) => {
                                  event.currentTarget.style.borderColor = "transparent";
                                  event.currentTarget.style.background = "transparent";
                                  updateCell(row, column, event.currentTarget.value, original);
                                }}
                              />
                            ) : (
                              <span style={{ minWidth: 0, flex: "1 1 auto", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{original}</span>
                            )}
                            {previewable && (
                              <DbxButton
                                variant="ghost"
                                size="icon-xs"
                                icon={Eye}
                                aria-label={t("database.previewCellValue", { column })}
                                title={t("database.previewCellValue", { column })}
                                onClick={() => setDbxCellPreview({ column, value: row.values[columnIndex] })}
                              />
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
        )}
          </>
        )}
      </main>
      {exportProgress?.active && (
        <div style={s.databaseDialogOverlay}>
          <div style={{ ...s.databaseConnectionDialog, width: 400, maxWidth: "min(90vw, 400px)" }}>
            <div style={s.databaseDialogHeader}>{t("database.exportProgress")}</div>
            <div style={s.databaseDialogBody}>
              <div style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 8 }}>
                {t("database.exportProgressFormat", { format: exportProgress.format.toUpperCase() })}
              </div>
              <div style={{ fontSize: 12, color: "var(--text-muted)", wordBreak: "break-all" }}>
                {exportProgress.filePath}
              </div>
              <div style={{ marginTop: 16, display: "flex", alignItems: "center", gap: 8 }}>
                <div style={{
                  width: 16,
                  height: 16,
                  border: "2px solid var(--border-dim)",
                  borderTopColor: "var(--accent)",
                  borderRadius: "50%",
                  animation: "spin 1s linear infinite",
                }} />
                <span style={{ fontSize: 12, color: "var(--text-muted)" }}>{t("database.exportProgressRunning")}</span>
              </div>
            </div>
          </div>
        </div>
      )}
      {dbxSqlPreviewOpen && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setDbxSqlPreviewOpen(false);
          }}
        >
          <div style={{ ...s.databaseConnectionDialog, width: 560, maxWidth: "min(92vw, 560px)" }}>
            <div style={s.databaseDialogHeader}>{t("database.previewSql")}</div>
            <div style={s.databaseDialogBody}>
              {dbxSqlPreviewDescription && (
                <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>{dbxSqlPreviewDescription}</div>
              )}
              <div style={{ fontSize: 11, fontWeight: 650, color: "var(--text-muted)", marginBottom: 4 }}>{t("database.gridPreviewStatements")}</div>
              <pre style={{
                margin: 0,
                padding: "8px 10px",
                background: "var(--surface-alt)",
                borderRadius: 6,
                fontSize: 12,
                fontFamily: "var(--font-mono)",
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                maxHeight: "30vh",
                overflow: "auto",
                border: "1px solid var(--border-dim)",
              }}>
                {dbxSqlPreviewStatements.join("\n")}
              </pre>
              {dbxSqlPreviewRollback.length > 0 && (
                <>
                  <div style={{ fontSize: 11, fontWeight: 650, color: "var(--text-muted)", marginTop: 10, marginBottom: 4 }}>{t("database.gridRollbackSql")}</div>
                  <pre style={{
                    margin: 0,
                    padding: "8px 10px",
                    background: "var(--surface-alt)",
                    borderRadius: 6,
                    fontSize: 12,
                    fontFamily: "var(--font-mono)",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                    maxHeight: "20vh",
                    overflow: "auto",
                    border: "1px solid var(--border-dim)",
                    color: "var(--danger)",
                  }}>
                    {dbxSqlPreviewRollback.join("\n")}
                  </pre>
                </>
              )}
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxButton variant="outline" size="sm" onClick={() => setDbxSqlPreviewOpen(false)}>
                {t("common.close")}
              </DbxButton>
            </div>
          </div>
        </div>
      )}
      {dbxColumnPreview && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) {
              setDbxColumnPreview(null);
              setDbxColumnPreviewSearch("");
            }
          }}
        >
          <section
            role="dialog"
            aria-modal="true"
            aria-label={t("database.columnDetailsFor", { column: dbxColumnPreview.column })}
            style={{ ...s.databaseConnectionDialog, width: 720, maxWidth: "min(94vw, 720px)" }}
          >
            <div style={s.databaseDialogHeader}>{t("database.columnDetailsFor", { column: dbxColumnPreview.column })}</div>
            <div style={s.databaseDialogBody}>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 10 }}>
                <div style={s.databaseDialogField}>
                  <span style={s.databaseDialogLabel}>{t("database.columnName")}</span>
                  <div style={{ color: "var(--text-primary)", fontSize: 12 }}>{dbxColumnPreview.column}</div>
                </div>
                <div style={s.databaseDialogField}>
                  <span style={s.databaseDialogLabel}>{t("database.columnType")}</span>
                  <div style={{ color: "var(--text-primary)", fontSize: 12 }}>
                    {dbxGridColumnType(queryResult, dbxColumnPreview.columnIndex) ?? "-"}
                  </div>
                </div>
                <div style={s.databaseDialogField}>
                  <span style={s.databaseDialogLabel}>{t("database.rowCount")}</span>
                  <div style={{ color: "var(--text-primary)", fontSize: 12 }}>{dbxColumnPreviewFields.length}</div>
                </div>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 10, color: "var(--text-muted)", fontSize: 12 }}>
                <span style={{ whiteSpace: "nowrap" }}>{t("database.rowCount")}: {dbxColumnPreviewFields.length}</span>
                <label style={{ position: "relative", marginLeft: "auto", width: 220, maxWidth: "52%" }}>
                  <Search
                    size={13}
                    style={{
                      position: "absolute",
                      left: 9,
                      top: "50%",
                      transform: "translateY(-50%)",
                      color: "var(--text-hint)",
                      pointerEvents: "none",
                    }}
                  />
                  <input
                    style={{ ...s.databaseDialogInput, width: "100%", height: 28, paddingLeft: 28 }}
                    value={dbxColumnPreviewSearch}
                    onChange={(event) => setDbxColumnPreviewSearch(event.target.value)}
                    placeholder={t("database.detailSearchPlaceholder")}
                    aria-label={t("database.detailSearchPlaceholder")}
                  />
                </label>
              </div>
              <div style={{ ...s.databaseTableWrap, maxHeight: "52vh" }}>
                <table style={{ ...s.databaseTable, minWidth: 520 }}>
                  <thead>
                    <tr>
                      <th style={s.databaseTh}>{t("database.rowNumber")}</th>
                      <th style={s.databaseTh}>{t("database.cellValue")}</th>
                      <th style={s.databaseTh} />
                    </tr>
                  </thead>
                  <tbody>
                    {filteredDbxColumnPreviewFields.map((field) => (
                      <tr key={`${dbxColumnPreview.column}:${field.rowNumber}`}>
                        <td style={s.databaseTd}>{field.rowNumber}</td>
                        <td style={s.databaseTd}>
                          <pre style={{ margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-word", fontFamily: "var(--font-mono)" }}>{field.preview}</pre>
                        </td>
                        <td style={{ ...s.databaseTd, width: 44 }}>
                          <DbxButton
                            variant="ghost"
                            size="icon-sm"
                            icon={Copy}
                            aria-label={t("database.copyRowValue", { row: field.rowNumber })}
                            title={t("database.copyRowValue", { row: field.rowNumber })}
                            onClick={() => {
                              navigator.clipboard?.writeText(field.preview).catch((err) => setError(String(err)));
                            }}
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {dbxColumnPreviewSearch && filteredDbxColumnPreviewFields.length === 0 && (
                  <div style={s.databaseEmptyCompact}>{t("database.detailSearchNoMatch")}</div>
                )}
              </div>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                onClick={() => {
                  navigator.clipboard
                    ?.writeText(JSON.stringify(dbxColumnPreviewFields.map((field) => ({ row: field.rowNumber, value: field.value })), null, 2))
                    .catch((err) => setError(String(err)));
                }}
              >
                {t("database.copyColumnValues")}
              </DbxButton>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                onClick={() => {
                  navigator.clipboard
                    ?.writeText(dbxColumnPreviewFields.map((field) => valueToText(field.value)).join("\n"))
                    .catch((err) => setError(String(err)));
                }}
              >
                {t("database.copyColumnTsv")}
              </DbxButton>
              <DbxButton variant="outline" size="sm" icon={Copy} onClick={() => void copyNodeName(dbxColumnPreview.column)}>
                {t("database.copyColumnName")}
              </DbxButton>
              <DbxButton
                variant="outline"
                size="sm"
                onClick={() => {
                  setDbxColumnPreview(null);
                  setDbxColumnPreviewSearch("");
                }}
              >
                {t("common.close")}
              </DbxButton>
            </div>
          </section>
        </div>
      )}
      {dbxRowPreview && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) {
              setDbxRowPreview(null);
              setDbxRowPreviewSearch("");
            }
          }}
        >
          <section
            role="dialog"
            aria-modal="true"
            aria-label={t("database.rowDetailsFor", { row: dbxRowPreview.rowIndex + 1 })}
            style={{ ...s.databaseConnectionDialog, width: 760, maxWidth: "min(94vw, 760px)" }}
          >
            <div style={s.databaseDialogHeader}>{t("database.rowDetailsFor", { row: dbxRowPreview.rowIndex + 1 })}</div>
            <div style={s.databaseDialogBody}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, color: "var(--text-muted)", fontSize: 12 }}>
                <span style={{ whiteSpace: "nowrap" }}>{t("database.columnsCount", { count: dbxRowPreviewFields.length })}</span>
                <label style={{ position: "relative", marginLeft: "auto", width: 220, maxWidth: "52%" }}>
                  <Search
                    size={13}
                    style={{
                      position: "absolute",
                      left: 9,
                      top: "50%",
                      transform: "translateY(-50%)",
                      color: "var(--text-hint)",
                      pointerEvents: "none",
                    }}
                  />
                  <input
                    style={{ ...s.databaseDialogInput, width: "100%", height: 28, paddingLeft: 28 }}
                    value={dbxRowPreviewSearch}
                    onChange={(event) => setDbxRowPreviewSearch(event.target.value)}
                    placeholder={t("database.detailSearchPlaceholder")}
                    aria-label={t("database.detailSearchPlaceholder")}
                  />
                </label>
              </div>
              <div style={{ ...s.databaseTableWrap, maxHeight: "52vh" }}>
                <table style={{ ...s.databaseTable, minWidth: 620 }}>
                  <thead>
                    <tr>
                      <th style={s.databaseTh}>{t("database.fieldIndex")}</th>
                      <th style={s.databaseTh}>{t("database.columnName")}</th>
                      <th style={s.databaseTh}>{t("database.cellValue")}</th>
                      <th style={s.databaseTh} />
                    </tr>
                  </thead>
                  <tbody>
                    {filteredDbxRowPreviewFields.map((field, index) => (
                      <tr key={`${index}:${field.column}`}>
                        <td style={s.databaseTd}>{index + 1}</td>
                        <td style={s.databaseTd}>
                          <div style={{ color: "var(--text-primary)" }}>{field.column}</div>
                          <div style={{ color: "var(--text-hint)", fontSize: 11 }}>{field.type ?? "-"}</div>
                        </td>
                        <td style={s.databaseTd}>
                          <pre style={{ margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-word", fontFamily: "var(--font-mono)" }}>{field.preview}</pre>
                        </td>
                        <td style={{ ...s.databaseTd, width: 44 }}>
                          <DbxButton
                            variant="ghost"
                            size="icon-sm"
                            icon={Copy}
                            aria-label={t("database.copyFieldValue", { column: field.column })}
                            title={t("database.copyFieldValue", { column: field.column })}
                            onClick={() => {
                              navigator.clipboard?.writeText(field.preview).catch((err) => setError(String(err)));
                            }}
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {dbxRowPreviewSearch && filteredDbxRowPreviewFields.length === 0 && (
                  <div style={s.databaseEmptyCompact}>{t("database.detailSearchNoMatch")}</div>
                )}
              </div>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                onClick={() => {
                  navigator.clipboard
                    ?.writeText(dbxGridRowsToJson(visibleTableColumns, [dbxRowPreview.row]))
                    .catch((err) => setError(String(err)));
                }}
              >
                {t("database.copyRow")}
              </DbxButton>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                onClick={() => {
                  navigator.clipboard
                    ?.writeText(dbxGridRowsToTsv(visibleTableColumns, [dbxRowPreview.row]))
                    .catch((err) => setError(String(err)));
                }}
              >
                {t("database.copyRowTsv")}
              </DbxButton>
              <DbxButton
                variant="outline"
                size="sm"
                onClick={() => {
                  setDbxRowPreview(null);
                  setDbxRowPreviewSearch("");
                }}
              >
                {t("common.close")}
              </DbxButton>
            </div>
          </section>
        </div>
      )}
      {dbxCellPreview && formattedDbxCellPreview && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setDbxCellPreview(null);
          }}
        >
          <section
            role="dialog"
            aria-modal="true"
            aria-label={t("database.cellValuePreview")}
            style={{ ...s.databaseConnectionDialog, width: 640, maxWidth: "min(92vw, 640px)" }}
          >
            <div style={s.databaseDialogHeader}>{t("database.cellValuePreview")}</div>
            <div style={s.databaseDialogBody}>
              <div style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.columnName")}</span>
                <div style={{ color: "var(--text-primary)", fontSize: 12 }}>{dbxCellPreview.column}</div>
              </div>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>
                  {formattedDbxCellPreview.json ? t("database.documentJson") : t("database.cellValue")}
                </span>
                <textarea
                  style={{ ...s.databaseSqlInput, minHeight: 280, resize: "vertical" }}
                  readOnly
                  value={formattedDbxCellPreview.text}
                  aria-label={formattedDbxCellPreview.json ? t("database.documentJson") : t("database.cellValue")}
                />
              </label>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                onClick={() => {
                  navigator.clipboard?.writeText(formattedDbxCellPreview.text).catch((err) => setError(String(err)));
                }}
              >
                {t("database.copyValue")}
              </DbxButton>
              <DbxButton variant="outline" size="sm" icon={Copy} onClick={() => copyNodeName(dbxCellPreview.column)}>
                {t("database.copyColumnName")}
              </DbxButton>
              <DbxButton variant="outline" size="sm" onClick={() => setDbxCellPreview(null)}>
                {t("common.close")}
              </DbxButton>
            </div>
          </section>
        </div>
      )}
      {connectionDialogOpen && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) closeConnectionDialog();
          }}
        >
          <form
            role="dialog"
            aria-modal="true"
            aria-label={connectionDialogTitle}
            style={s.databaseConnectionDialog}
            onSubmit={(event) => {
              event.preventDefault();
              if (wizardStep === "type") {
                setWizardStep("config");
              } else {
                submitConnection();
              }
            }}
          >
            <div style={s.databaseDialogHeader}>{connectionDialogTitle}</div>
            <div style={s.databaseDialogBody}>
              {wizardStep === "type" ? (
                <>
                  <div style={s.databaseTypeChooserHeader}>
                    <div style={s.databaseDialogIntro}>{t("database.chooseDatabaseType")}</div>
                    <DbxButtonGroup style={s.databaseTypeViewToggle} aria-label={t("database.typeViewMode")}>
                      {(["icon", "list"] as const).map((mode) => (
                        <DbxSegmentedButton
                          key={mode}
                          active={profileViewMode === mode}
                          onClick={() => setProfileViewMode(mode)}
                        >
                          {t(mode === "icon" ? "database.typeIconView" : "database.typeListView")}
                        </DbxSegmentedButton>
                      ))}
                    </DbxButtonGroup>
                  </div>
                  <label style={s.databaseSearchBox}>
                    <Search size={13} />
                    <input
                      aria-label={t("database.typeSearch")}
                      style={s.databaseSearchInput}
                      value={profileSearch}
                      placeholder={t("database.typeSearch")}
                      onChange={(event) => setProfileSearch(event.target.value)}
                    />
                  </label>
                  {profileViewMode === "icon" ? (
                    <div style={s.databaseTypeGrid}>
                      {filteredProfiles.map((profile) => (
                        <button
                          key={profile.key}
                          type="button"
                          style={{
                            ...s.databaseTypeCard,
                            ...(selectedProfileKey === profile.key ? s.databaseTypeCardSelected : {}),
                          }}
                          onClick={() => handleProfileSelect(profile.key)}
                          onDoubleClick={() => handleProfileDoubleClick(profile.key)}
                        >
                          <span style={{ ...s.databaseTypeIcon, background: `${profile.accent}1f`, color: profile.accent }}>
                            <DatabaseProfileIcon profile={profile} size={25} />
                          </span>
                          <span style={s.databaseTypeLabel}>{profile.label}</span>
                        </button>
                      ))}
                    </div>
                  ) : (
                    <div style={s.databaseTypeList}>
                      {filteredProfiles.map((profile) => (
                        <button
                          key={profile.key}
                          type="button"
                          style={{
                            ...s.databaseTypeListItem,
                            ...(selectedProfileKey === profile.key ? s.databaseTypeCardSelected : {}),
                          }}
                          onClick={() => handleProfileSelect(profile.key)}
                          onDoubleClick={() => handleProfileDoubleClick(profile.key)}
                        >
                          <span style={{ ...s.databaseTypeIconSmall, background: `${profile.accent}1f`, color: profile.accent }}>
                            <DatabaseProfileIcon profile={profile} size={16} />
                          </span>
                          <span style={s.databaseTypeLabel}>{profile.label}</span>
                          <span style={s.databaseTypeMeta}>{profile.localFile ? t("database.filePath") : `${t("database.port")} ${profile.port}`}</span>
                        </button>
                      ))}
                    </div>
                  )}
                  {filteredProfiles.length === 0 && <div style={s.databaseEmpty}>{t("database.typeSearchEmpty")}</div>}
                </>
              ) : (
                <>
                  <div style={s.databaseConfigHeader}>
                    <button type="button" style={s.databaseTypeMiniCard} onClick={() => setWizardStep("type")}>
                      <span style={{ ...s.databaseTypeIconSmall, background: `${selectedProfile.accent}1f`, color: selectedProfile.accent }}>
                        <DatabaseProfileIcon profile={selectedProfile} size={16} />
                      </span>
                      <span>{selectedProfile.label}</span>
                    </button>
                  </div>
                  <div style={s.databaseConfigTabs}>
                    {[
                      ["connection", t("database.connectionInfo"), Database],
                      ["tls", t("database.tlsSsl"), Shield],
                      ["transport", t("database.sshTunnelProxy"), Server],
                      ["advanced", t("database.advanced"), SlidersHorizontal],
                    ].map(([key, label, Icon]) => (
                      <button
                        key={key as string}
                        type="button"
                        style={{
                          ...s.databaseConfigTab,
                          ...(configTab === key ? s.databaseConfigTabActive : {}),
                        }}
                        onClick={() => setConfigTab(key as DbConfigTab)}
                      >
                        <Icon size={13} />
                        <span>{label as string}</span>
                      </button>
                    ))}
                  </div>
                  {configTab === "connection" ? (
                    <div style={s.databaseDialogFormGrid}>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.connectionName")}</span>
                        <input style={s.databaseDialogInput} value={draftName} onChange={(event) => setDraftName(event.target.value)} autoFocus />
                      </label>
                      <div style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.connectionColor")}</span>
                        <div style={s.databaseColorPickerRow}>
                          {CONNECTION_COLOR_OPTIONS.map((color) => {
                            const selected = draftColor === color.value;
                            return (
                              <button
                                key={color.value || "none"}
                                type="button"
                                aria-label={t(color.labelKey)}
                                title={t(color.labelKey)}
                                style={{
                                  ...s.databaseColorSwatch,
                                  ...(color.value ? { background: color.value } : s.databaseColorSwatchEmpty),
                                  ...(selected ? s.databaseColorSwatchSelected : {}),
                                }}
                                onClick={() => setDraftColor(color.value)}
                              />
                            );
                          })}
                          <label style={s.databaseColorCustom}>
                            <span>{t("database.colorCustom")}</span>
                            <input
                              aria-label={t("database.colorCustom")}
                              type="color"
                              value={draftColor || "#3b82f6"}
                              onChange={(event) => setDraftColor(event.target.value)}
                            />
                          </label>
                        </div>
                      </div>
                      {selectedProfile.localFile ? (
                        <label style={s.databaseDialogField}>
                          <span style={s.databaseDialogLabel}>{t("database.filePath")}</span>
                          <div style={s.databaseInputButtonRow}>
                            <input style={s.databaseDialogInput} value={draftFilePath} onChange={(event) => setDraftFilePath(event.target.value)} placeholder="/path/to/database.db" />
                            <DbxButton variant="ghost" size="icon-sm" icon={FilePlus} onClick={chooseLocalDbFile} />
                          </div>
                        </label>
                      ) : (
                        <>
                          {selectedProfile.key === "mongodb" && (
                            <div style={s.databaseDialogField}>
                              <span style={s.databaseDialogLabel}>{t("database.mongoConnectionMode")}</span>
                              <DbxButtonGroup style={s.databaseTypeViewToggle} aria-label={t("database.mongoConnectionMode")}>
                                {([
                                  [false, t("database.mongoModeForm")],
                                  [true, "URL"],
                                ] as const).map(([useUrl, label]) => (
                                  <DbxSegmentedButton
                                    key={String(useUrl)}
                                    active={draftMongoUseUrl === useUrl}
                                    onClick={() => setDraftMongoUseUrl(useUrl)}
                                  >
                                    {label}
                                  </DbxSegmentedButton>
                                ))}
                              </DbxButtonGroup>
                            </div>
                          )}
                          {!(selectedProfile.key === "mongodb" && draftMongoUseUrl) && (
                            <>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.host")}</span>
                                <input style={s.databaseDialogInput} value={draftHost} onChange={(event) => setDraftHost(event.target.value)} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.port")}</span>
                                <input style={s.databaseDialogInput} value={draftPort} onChange={(event) => setDraftPort(event.target.value)} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.user")}</span>
                                <input style={s.databaseDialogInput} value={draftUser} onChange={(event) => setDraftUser(event.target.value)} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.password")}</span>
                                <input style={s.databaseDialogInput} type="password" value={draftPassword} onChange={(event) => setDraftPassword(event.target.value)} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.databaseName")}</span>
                                <input style={s.databaseDialogInput} value={draftDatabase} onChange={(event) => setDraftDatabase(event.target.value)} />
                              </label>
                              {selectedProfile.key === "oracle" && (
                                <>
                                  <div style={s.databaseDialogField}>
                                    <span style={s.databaseDialogLabel}>{t("database.oracleConnectionType")}</span>
                                    <DbxButtonGroup style={s.databaseTypeViewToggle} aria-label={t("database.oracleConnectionType")}>
                                      {(["service_name", "sid"] as const).map((type) => (
                                        <DbxSegmentedButton
                                          key={type}
                                          active={draftOracleConnectionType === type}
                                          onClick={() => setDraftOracleConnectionType(type)}
                                        >
                                          {t(`database.oracleConnectionType.${type}`)}
                                        </DbxSegmentedButton>
                                      ))}
                                    </DbxButtonGroup>
                                  </div>
                                  <label style={s.databaseSwitchRow}>
                                    <input type="checkbox" checked={draftOracleSysdba} onChange={(event) => setDraftOracleSysdba(event.target.checked)} />
                                    <span>{t("database.oracleSysdba")}</span>
                                  </label>
                                </>
                              )}
                            </>
                          )}
                          {(selectedProfile.key !== "mongodb" || draftMongoUseUrl) && (
                            <label style={s.databaseDialogField}>
                              <span style={s.databaseDialogLabel}>{selectedProfile.key === "mongodb" ? "URL" : t("database.connectionString")}</span>
                              <div style={s.databaseInputButtonRow}>
                                <input
                                  style={s.databaseDialogInput}
                                  value={draftConnectionString}
                                  onChange={(event) => setDraftConnectionString(event.target.value)}
                                  placeholder={selectedProfile.key === "mongodb" ? "mongodb+srv://user:pass@cluster.mongodb.net/mydb" : "jdbc:postgresql://localhost:5432/postgres"}
                                />
                                <DbxButton variant="outline" size="sm" onClick={applyConnectionUrl}>
                                  {t("database.parseConnectionUrl")}
                                </DbxButton>
                              </div>
                            </label>
                          )}
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.urlParams")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={draftUrlParams}
                              onChange={(event) => setDraftUrlParams(event.target.value)}
                              placeholder="sslmode=require&connectTimeout=15"
                            />
                          </label>
                          {selectedProfile.key === "redis" && (
                            <>
                              <div style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.redisConnectionMode")}</span>
                                <DbxButtonGroup style={s.databaseTypeViewToggle} aria-label={t("database.redisConnectionMode")}>
                                  {(["standalone", "sentinel", "cluster"] as const).map((mode) => (
                                    <DbxSegmentedButton
                                      key={mode}
                                      active={draftRedisConnectionMode === mode}
                                      onClick={() => setDraftRedisConnectionMode(mode)}
                                    >
                                      {t(`database.redisMode.${mode}`)}
                                    </DbxSegmentedButton>
                                  ))}
                                </DbxButtonGroup>
                              </div>
                              {draftRedisConnectionMode === "sentinel" && (
                                <>
                                  <label style={s.databaseDialogField}>
                                    <span style={s.databaseDialogLabel}>{t("database.redisSentinelNodes")}</span>
                                    <textarea
                                      style={{ ...s.databaseDialogInput, minHeight: 64, resize: "vertical" }}
                                      value={draftRedisSentinelNodes}
                                      onChange={(event) => setDraftRedisSentinelNodes(event.target.value)}
                                      placeholder={"sentinel-1:26379\nsentinel-2:26379"}
                                    />
                                  </label>
                                  <label style={s.databaseDialogField}>
                                    <span style={s.databaseDialogLabel}>{t("database.redisSentinelMaster")}</span>
                                    <input style={s.databaseDialogInput} value={draftRedisSentinelMaster} onChange={(event) => setDraftRedisSentinelMaster(event.target.value)} placeholder="mymaster" />
                                  </label>
                                  <label style={s.databaseDialogField}>
                                    <span style={s.databaseDialogLabel}>{t("database.redisSentinelUser")}</span>
                                    <input style={s.databaseDialogInput} value={draftRedisSentinelUsername} onChange={(event) => setDraftRedisSentinelUsername(event.target.value)} />
                                  </label>
                                  <label style={s.databaseDialogField}>
                                    <span style={s.databaseDialogLabel}>{t("database.redisSentinelPassword")}</span>
                                    <input style={s.databaseDialogInput} type="password" value={draftRedisSentinelPassword} onChange={(event) => setDraftRedisSentinelPassword(event.target.value)} />
                                  </label>
                                  <label style={s.databaseSwitchRow}>
                                    <input type="checkbox" checked={draftRedisSentinelTls} onChange={(event) => setDraftRedisSentinelTls(event.target.checked)} />
                                    <span>{t("database.redisSentinelTls")}</span>
                                  </label>
                                </>
                              )}
                              {draftRedisConnectionMode === "cluster" && (
                                <label style={s.databaseDialogField}>
                                  <span style={s.databaseDialogLabel}>{t("database.redisClusterNodes")}</span>
                                  <textarea
                                    style={{ ...s.databaseDialogInput, minHeight: 64, resize: "vertical" }}
                                    value={draftRedisClusterNodes}
                                    onChange={(event) => setDraftRedisClusterNodes(event.target.value)}
                                    placeholder={"redis-1:6379\nredis-2:6379"}
                                  />
                                </label>
                              )}
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.redisKeySeparator")}</span>
                                <input style={s.databaseDialogInput} value={draftRedisKeySeparator} onChange={(event) => setDraftRedisKeySeparator(event.target.value)} placeholder=":" />
                              </label>
                            </>
                          )}
                        </>
                      )}
                    </div>
                  ) : configTab === "tls" ? (
                    <div style={s.databaseDialogPanel}>
                      <label style={s.databaseSwitchRow}>
                        <input type="checkbox" checked={tlsEnabled} onChange={(event) => setTlsEnabled(event.target.checked)} />
                        <span>{t("database.enableTls")}</span>
                      </label>
                      {selectedProfile.key === "postgres" && (
                        <label style={s.databaseDialogField}>
                          <span style={s.databaseDialogLabel}>{t("connection.postgresSslMode")}</span>
                          <select
                            style={{ ...s.databaseDialogInput, height: 32 }}
                            value={draftTlsMode || "prefer"}
                            onChange={(event) => setDraftTlsMode(event.target.value)}
                          >
                            <option value="disable">{t("connection.postgresSslModeDisable")}</option>
                            <option value="prefer">{t("connection.postgresSslModePrefer")}</option>
                            <option value="require">{t("connection.postgresSslModeRequire")}</option>
                            <option value="verify-ca">{t("connection.postgresSslModeVerifyCa")}</option>
                            <option value="verify-full">{t("connection.postgresSslModeVerifyFull")}</option>
                          </select>
                        </label>
                      )}
                      {selectedProfile.key === "mysql" && (
                        <label style={s.databaseDialogField}>
                          <span style={s.databaseDialogLabel}>{t("connection.mysqlTlsMode")}</span>
                          <select
                            style={{ ...s.databaseDialogInput, height: 32 }}
                            value={draftTlsMode || "preferred"}
                            onChange={(event) => setDraftTlsMode(event.target.value)}
                          >
                            <option value="preferred">{t("connection.mysqlTlsModePreferred")}</option>
                            <option value="disabled">{t("connection.mysqlTlsModeDisabled")}</option>
                            <option value="required">{t("connection.mysqlTlsModeRequired")}</option>
                            <option value="verify_ca">{t("connection.mysqlTlsModeVerifyCa")}</option>
                            <option value="verify_identity">{t("connection.mysqlTlsModeVerifyIdentity")}</option>
                          </select>
                        </label>
                      )}
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.caCertPath")}</span>
                        <div style={s.databaseInputButtonRow}>
                          <input style={s.databaseDialogInput} value={draftCaCertPath} onChange={(event) => setDraftCaCertPath(event.target.value)} />
                          <DbxButton
                            variant="ghost"
                            size="icon-sm"
                            icon={FilePlus}
                            aria-label={t("database.chooseCaCertPath")}
                            title={t("database.chooseCaCertPath")}
                            onClick={() => {
                              void chooseTlsCertificatePath(setDraftCaCertPath);
                            }}
                          />
                        </div>
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.clientCertPath")}</span>
                        <div style={s.databaseInputButtonRow}>
                          <input style={s.databaseDialogInput} value={draftClientCertPath} onChange={(event) => setDraftClientCertPath(event.target.value)} />
                          <DbxButton
                            variant="ghost"
                            size="icon-sm"
                            icon={FilePlus}
                            aria-label={t("database.chooseClientCertPath")}
                            title={t("database.chooseClientCertPath")}
                            onClick={() => {
                              void chooseTlsCertificatePath(setDraftClientCertPath);
                            }}
                          />
                        </div>
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.clientKeyPath")}</span>
                        <div style={s.databaseInputButtonRow}>
                          <input style={s.databaseDialogInput} value={draftClientKeyPath} onChange={(event) => setDraftClientKeyPath(event.target.value)} />
                          <DbxButton
                            variant="ghost"
                            size="icon-sm"
                            icon={FilePlus}
                            aria-label={t("database.chooseClientKeyPath")}
                            title={t("database.chooseClientKeyPath")}
                            onClick={() => {
                              void chooseTlsCertificatePath(setDraftClientKeyPath);
                            }}
                          />
                        </div>
                      </label>
                      <div style={s.databaseDialogHint}>{t("database.tlsHint")}</div>
                    </div>
                  ) : configTab === "transport" ? (
                    <div style={s.databaseDialogPanel}>
                      <label style={s.databaseSwitchRow}>
                        <input type="checkbox" checked={transportEnabled} onChange={(event) => setTransportEnabled(event.target.checked)} />
                        <span>{t("database.enableTransport")}</span>
                      </label>
                      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                        <DbxButton variant="outline" size="sm" icon={Plus} onClick={() => addTransportLayer("ssh")}>
                          {t("database.addSshHop")}
                        </DbxButton>
                        <DbxButton variant="outline" size="sm" icon={Plus} onClick={() => addTransportLayer("proxy")}>
                          {t("database.addProxyLayer")}
                        </DbxButton>
                      </div>
                      {draftTransportLayers.length > 0 && (
                        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                          {draftTransportLayers.map((layer, index) => (
                            <button
                              key={layer.id}
                              type="button"
                              style={{
                                ...s.databaseListButton,
                                ...(selectedTransportLayer?.id === layer.id ? s.databaseListButtonActive : {}),
                              }}
                              onClick={() => setSelectedTransportLayerId(layer.id)}
                            >
                              <input
                                type="checkbox"
                                checked={layer.enabled}
                                onClick={(event) => event.stopPropagation()}
                                onChange={(event) => updateTransportLayer(layer.id, { enabled: event.target.checked })}
                              />
                              <span style={{ color: "var(--text-hint)", width: 18 }}>{index + 1}</span>
                              <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                                {layer.name || layer.host || (layer.type === "ssh" ? t("database.addSshHop") : t("database.addProxyLayer"))}
                              </span>
                              <span style={{ color: "var(--text-hint)", fontSize: 10, textTransform: "uppercase" }}>{layer.type}</span>
                              <DbxButton
                                variant="ghost"
                                size="icon-xs"
                                icon={Copy}
                                aria-label={t("database.copyTransportLayer")}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  copyTransportLayer(layer.id);
                                }}
                              />
                              {index > 0 && (
                                <DbxButton
                                  variant="ghost"
                                  size="icon-xs"
                                  icon={ArrowUp}
                                  aria-label={t("database.moveTransportLayerUp")}
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    moveTransportLayer(layer.id, -1);
                                  }}
                                />
                              )}
                              {index < draftTransportLayers.length - 1 && (
                                <DbxButton
                                  variant="ghost"
                                  size="icon-xs"
                                  icon={ArrowDown}
                                  aria-label={t("database.moveTransportLayerDown")}
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    moveTransportLayer(layer.id, 1);
                                  }}
                                />
                              )}
                              <DbxButton
                                variant="ghost"
                                size="icon-xs"
                                icon={Trash2}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  removeTransportLayer(layer.id);
                                }}
                              />
                            </button>
                          ))}
                        </div>
                      )}
                      {selectedTransportLayer ? (
                        <div style={s.databaseDialogFormGrid}>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.transportLayerName")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.name}
                              onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { name: event.target.value })}
                            />
                          </label>
                          <div style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.transportLayerType")}</span>
                            <DbxButtonGroup style={s.databaseTypeViewToggle} aria-label={t("database.transportLayerType")}>
                              {(["ssh", "proxy"] as const).map((type) => (
                                <DbxSegmentedButton
                                  key={type}
                                  active={selectedTransportLayer.type === type}
                                  onClick={() => updateTransportLayer(selectedTransportLayer.id, { type, port: type === "ssh" ? "22" : "1080" })}
                                >
                                  {type === "ssh" ? "SSH" : "Proxy"}
                                </DbxSegmentedButton>
                              ))}
                            </DbxButtonGroup>
                          </div>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{selectedTransportLayer.type === "ssh" ? t("database.sshHost") : t("database.proxyHost")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.host}
                              onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { host: event.target.value })}
                              placeholder={selectedTransportLayer.type === "ssh" ? "ssh.example.com" : "proxy.example.com"}
                            />
                          </label>
                          <label style={s.databaseDialogField}>
                            <span style={s.databaseDialogLabel}>{t("database.port")}</span>
                            <input
                              style={s.databaseDialogInput}
                              value={selectedTransportLayer.port}
                              onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { port: event.target.value })}
                            />
                          </label>
                          {selectedTransportLayer.type === "ssh" ? (
                            <>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.sshUser")}</span>
                                <input style={s.databaseDialogInput} value={selectedTransportLayer.user} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { user: event.target.value })} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.sshPassword")}</span>
                                <input style={s.databaseDialogInput} type="password" value={selectedTransportLayer.password} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { password: event.target.value })} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.sshKeyPath")}</span>
                                <input style={s.databaseDialogInput} value={selectedTransportLayer.keyPath} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { keyPath: event.target.value })} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.sshKeyPassphrase")}</span>
                                <input style={s.databaseDialogInput} type="password" value={selectedTransportLayer.keyPassphrase} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { keyPassphrase: event.target.value })} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.connectTimeoutSecs")}</span>
                                <input style={s.databaseDialogInput} value={selectedTransportLayer.connectTimeoutSecs} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { connectTimeoutSecs: event.target.value })} />
                              </label>
                              <label style={s.databaseSwitchRow}>
                                <input type="checkbox" checked={selectedTransportLayer.useSshAgent} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { useSshAgent: event.target.checked })} />
                                <span>{t("database.sshUseAgent")}</span>
                              </label>
                              <label style={s.databaseSwitchRow}>
                                <input type="checkbox" checked={selectedTransportLayer.exposeLan} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { exposeLan: event.target.checked })} />
                                <span>{t("database.sshExposeLan")}</span>
                              </label>
                            </>
                          ) : (
                            <>
                              <div style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.proxyType")}</span>
                                <DbxButtonGroup style={s.databaseTypeViewToggle} aria-label={t("database.proxyType")}>
                                  {(["socks5", "http"] as const).map((proxyType) => (
                                    <DbxSegmentedButton
                                      key={proxyType}
                                      active={selectedTransportLayer.proxyType === proxyType}
                                      onClick={() => updateTransportLayer(selectedTransportLayer.id, { proxyType })}
                                    >
                                      {proxyType.toUpperCase()}
                                    </DbxSegmentedButton>
                                  ))}
                                </DbxButtonGroup>
                              </div>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.proxyUsername")}</span>
                                <input style={s.databaseDialogInput} value={selectedTransportLayer.username} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { username: event.target.value })} />
                              </label>
                              <label style={s.databaseDialogField}>
                                <span style={s.databaseDialogLabel}>{t("database.proxyPassword")}</span>
                                <input style={s.databaseDialogInput} type="password" value={selectedTransportLayer.password} onChange={(event) => updateTransportLayer(selectedTransportLayer.id, { password: event.target.value })} />
                              </label>
                            </>
                          )}
                        </div>
                      ) : (
                        <div style={s.databaseDialogHint}>{t("database.transportHint")}</div>
                      )}
                    </div>
                  ) : (
                    <div style={s.databaseDialogPanel}>
                      <label style={s.databaseSwitchRow}>
                        <input type="checkbox" checked={draftReadOnly} onChange={(event) => setDraftReadOnly(event.target.checked)} />
                        <span>{t("database.openReadOnly")}</span>
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.connectTimeoutSecs")}</span>
                        <input style={s.databaseDialogInput} type="number" min={1} max={300} value={draftConnectTimeoutSecs} onChange={(event) => setDraftConnectTimeoutSecs(event.target.value)} />
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.queryTimeoutSecs")}</span>
                        <input style={s.databaseDialogInput} type="number" min={0} max={300} value={draftQueryTimeoutSecs} onChange={(event) => setDraftQueryTimeoutSecs(event.target.value)} />
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.idleTimeoutSecs")}</span>
                        <input style={s.databaseDialogInput} type="number" min={0} max={600} value={draftIdleTimeoutSecs} onChange={(event) => setDraftIdleTimeoutSecs(event.target.value)} />
                      </label>
                      <label style={s.databaseSwitchRow}>
                        <input
                          type="checkbox"
                          checked={Number.parseInt(draftKeepaliveIntervalSecs, 10) > 0}
                          onChange={(event) => setDraftKeepaliveIntervalSecs(event.target.checked ? "30" : "0")}
                        />
                        <span>{t("database.enableKeepalive")}</span>
                      </label>
                      <label style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{t("database.keepaliveIntervalSecs")}</span>
                        <input
                          style={s.databaseDialogInput}
                          type="number"
                          min={1}
                          max={3600}
                          value={draftKeepaliveIntervalSecs}
                          disabled={Number.parseInt(draftKeepaliveIntervalSecs, 10) <= 0}
                          onChange={(event) => setDraftKeepaliveIntervalSecs(event.target.value)}
                        />
                      </label>
                      <div style={s.databaseDialogHint}>{t("database.advancedHint")}</div>
                    </div>
                  )}
                </>
              )}
            </div>
            <div style={s.databaseDialogFooter}>
              {connectionTestResult && (
                <span style={{
                  fontSize: 12,
                  color: connectionTestResult.success ? "var(--success)" : "var(--danger)",
                  flex: 1,
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}>
                  {connectionTestResult.message}
                  <DbxButton
                    variant="ghost"
                    size="icon-xs"
                    icon={Copy}
                    aria-label={t("database.copyTestResult")}
                    onClick={() => {
                      void navigator.clipboard.writeText(connectionTestResult.message);
                    }}
                  />
                </span>
              )}
              <DbxButton variant="outline" size="sm" onClick={closeConnectionDialog}>
                {t("common.cancel")}
              </DbxButton>
              {wizardStep === "config" && selectedProfile.key !== "sqlite" && (
                <DbxButton
                  variant="outline"
                  size="sm"
                  icon={Plug}
                  onClick={testDbxConnectionDraft}
                  disabled={loading || (selectedProfile.localFile ? !draftFilePath.trim() : !draftHost.trim())}
                >
                  {t("database.testConnection")}
                </DbxButton>
              )}
              <DbxButton
                variant="default"
                size="sm"
                icon={wizardStep === "type" ? ChevronRight : Plus}
                type="submit"
                disabled={
                  loading ||
                  (wizardStep === "config" &&
                    (selectedProfile.localFile ? !draftFilePath.trim() : !draftHost.trim()))
                }
              >
                {wizardStep === "type" ? t("database.next") : editingDbxConnectionId ? t("common.save") : t("database.addConnection")}
              </DbxButton>
            </div>
          </form>
        </div>
      )}
      {createDatabaseConnection && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) closeCreateDatabaseDialog();
          }}
        >
          <form
            role="dialog"
            aria-modal="true"
            aria-label={t("database.createDatabase")}
            style={s.databaseDialog}
            onSubmit={(event) => {
              event.preventDefault();
              void submitCreateDatabase();
            }}
          >
            <div style={s.databaseDialogHeader}>{t("database.createDatabase")}</div>
            <div style={s.databaseDialogBody}>
              <div style={s.databaseDialogHint}>
                {t("database.createDatabaseHint")}
              </div>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.createDatabaseName")}</span>
                <input
                  aria-label={t("database.createDatabaseName")}
                  style={s.databaseDialogInput}
                  value={createDatabaseName}
                  onChange={(event) => setCreateDatabaseName(event.target.value)}
                  autoFocus
                />
              </label>
              {canSetCreateDatabaseCharset(createDatabaseConnection) && (
                <div style={s.databaseDialogFormGrid}>
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.charset")}</span>
                    <input
                      aria-label={t("database.charset")}
                      style={s.databaseDialogInput}
                      value={createDatabaseCharset}
                      onChange={(event) => setCreateDatabaseCharset(event.target.value)}
                    />
                  </label>
                  <label style={s.databaseDialogField}>
                    <span style={s.databaseDialogLabel}>{t("database.collation")}</span>
                    <input
                      aria-label={t("database.collation")}
                      style={s.databaseDialogInput}
                      value={createDatabaseCollation}
                      onChange={(event) => setCreateDatabaseCollation(event.target.value)}
                    />
                  </label>
                </div>
              )}
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxDialogFooterButton type="button" onClick={closeCreateDatabaseDialog} disabled={loading}>
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton type="submit" variant="default" icon={Plus} disabled={loading || !createDatabaseName.trim()}>
                {t("database.createDatabase")}
              </DbxDialogFooterButton>
            </div>
          </form>
        </div>
      )}
      {createSchemaTarget && createSchemaConnection && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) closeCreateSchemaDialog();
          }}
        >
          <form
            role="dialog"
            aria-modal="true"
            aria-label={t("database.createSchema")}
            style={s.databaseDialog}
            onSubmit={(event) => {
              event.preventDefault();
              void submitCreateSchema();
            }}
          >
            <div style={s.databaseDialogHeader}>{t("database.createSchema")}</div>
            <div style={s.databaseDialogBody}>
              <div style={s.databaseDialogHint}>
                {t("database.createSchemaHint", { database: createSchemaTarget.database })}
              </div>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.schemaName")}</span>
                <input
                  aria-label={t("database.schemaName")}
                  style={s.databaseDialogInput}
                  value={createSchemaName}
                  onChange={(event) => setCreateSchemaName(event.target.value)}
                  autoFocus
                />
              </label>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxDialogFooterButton type="button" onClick={closeCreateSchemaDialog} disabled={loading}>
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton type="submit" variant="default" icon={Plus} disabled={loading || !createSchemaName.trim()}>
                {t("database.createSchema")}
              </DbxDialogFooterButton>
            </div>
          </form>
        </div>
      )}
      {visibleDatabaseConnection && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) closeVisibleDatabasesDialog();
          }}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label={t("database.visibleDatabasesTitle")}
            style={{ ...s.databaseDialog, width: 460 }}
          >
            <div style={s.databaseDialogHeader}>{t("database.visibleDatabasesTitle")}</div>
            <div style={s.databaseDialogBody}>
              <div style={s.databaseDialogHint}>
                {t("database.visibleDatabasesDescription", { connection: visibleDatabaseConnection.name })}
              </div>
              <label style={s.databaseSearchBox}>
                <Search size={13} />
                <input
                  aria-label={t("database.visibleDatabasesSearch")}
                  style={s.databaseSearchInput}
                  value={visibleDatabaseSearch}
                  onChange={(event) => setVisibleDatabaseSearch(event.target.value)}
                  placeholder={t("database.visibleDatabasesSearch")}
                  disabled={visibleDatabaseLoading || Boolean(visibleDatabaseError)}
                />
              </label>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10, fontSize: 11.5, color: "var(--text-muted)" }}>
                <span>
                  {t("database.visibleDatabasesSelectedCount", {
                    selected: visibleDatabaseSelection.size,
                    total: listedVisibleDatabaseNames.length,
                  })}
                </span>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <button
                    type="button"
                    style={{ border: "none", background: "transparent", color: "var(--text-secondary)", fontSize: 11.5, cursor: "pointer", padding: "0 2px" }}
                    onClick={() => setVisibleDatabaseSelection(new Set(listedVisibleDatabaseNames))}
                    disabled={visibleDatabaseLoading}
                  >
                    {t("database.visibleDatabasesSelectAll")}
                  </button>
                  {visibleDatabaseSearch.trim() && (
                    <button
                      type="button"
                      style={{ border: "none", background: "transparent", color: "var(--text-secondary)", fontSize: 11.5, cursor: "pointer", padding: "0 2px" }}
                      onClick={() => setVisibleDatabaseSelection(new Set(filteredVisibleDatabaseNames))}
                      disabled={visibleDatabaseLoading}
                    >
                      {t("database.visibleDatabasesSelectFiltered")}
                    </button>
                  )}
                  <button
                    type="button"
                    style={{ border: "none", background: "transparent", color: "var(--text-secondary)", fontSize: 11.5, cursor: "pointer", padding: "0 2px" }}
                    onClick={() => setVisibleDatabaseSelection(new Set())}
                    disabled={visibleDatabaseLoading}
                  >
                    {t("database.visibleDatabasesClear")}
                  </button>
                  <button
                    type="button"
                    style={{ border: "none", background: "transparent", color: "var(--text-secondary)", fontSize: 11.5, cursor: "pointer", padding: "0 2px" }}
                    onClick={() => {
                      void showAllVisibleDatabases();
                    }}
                    disabled={visibleDatabaseLoading || !configuredVisibleDatabases(visibleDatabaseConnection)}
                  >
                    {t("database.visibleDatabasesShowAll")}
                  </button>
                </div>
              </div>
              {!visibleDatabaseLoading && !visibleDatabaseError && !visibleDatabaseCanSave && (
                <div style={{ color: "var(--danger)", fontSize: 12 }}>{t("database.visibleDatabasesEmptySelection")}</div>
              )}
              {visibleDatabaseHasSystemNames && (
                <label style={s.databaseSwitchRow}>
                  <input
                    type="checkbox"
                    checked={visibleDatabaseShowSystem}
                    onChange={(event) => {
                      const nextShowSystem = event.target.checked;
                      setVisibleDatabaseShowSystem(nextShowSystem);
                      if (!nextShowSystem) {
                        setVisibleDatabaseSelection((current) => {
                          const next = new Set([...current].filter((name) => !isSystemDatabaseName(visibleDatabaseConnection.dbType, name)));
                          return next;
                        });
                      }
                    }}
                    disabled={visibleDatabaseLoading || Boolean(visibleDatabaseError)}
                  />
                  <span>{t("database.visibleDatabasesShowSystem")}</span>
                </label>
              )}
              <div
                style={{
                  height: 288,
                  overflowY: "auto",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 8,
                  background: "var(--bg-subtle)",
                  padding: 4,
                }}
              >
                {visibleDatabaseLoading ? (
                  <div style={s.databaseEmptyCompact}>{t("common.loading")}</div>
                ) : visibleDatabaseError ? (
                  <div style={{ ...s.databaseEmptyCompact, color: "var(--danger)" }}>
                    {t("database.visibleDatabasesLoadFailed", { message: visibleDatabaseError })}
                  </div>
                ) : filteredVisibleDatabaseNames.length === 0 ? (
                  <div style={s.databaseEmptyCompact}>{t("database.sidebarSearchNoResults")}</div>
                ) : (
                  filteredVisibleDatabaseNames.map((database) => {
                    const selected = visibleDatabaseSelection.has(database);
                    return (
                      <button
                        key={database}
                        type="button"
                        style={{
                          ...s.databaseListButton,
                          minHeight: 30,
                          padding: "5px 8px",
                          color: selected ? "var(--text-primary)" : "var(--text-secondary)",
                        }}
                        onClick={() => toggleVisibleDatabaseSelection(database)}
                      >
                        {selected ? <CheckSquare size={14} /> : <Square size={14} />}
                        <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                          {database}
                        </span>
                      </button>
                    );
                  })
                )}
              </div>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxDialogFooterButton type="button" onClick={closeVisibleDatabasesDialog} disabled={visibleDatabaseLoading}>
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton
                type="button"
                variant="default"
                onClick={() => {
                  void saveVisibleDatabaseSelection();
                }}
                disabled={visibleDatabaseLoading || Boolean(visibleDatabaseError) || !visibleDatabaseCanSave}
              >
                {t("database.visibleDatabasesSave")}
              </DbxDialogFooterButton>
            </div>
          </div>
        </div>
      )}
      {databaseExportTarget && databaseExportConnection && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) closeDatabaseExportDialog();
          }}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label={t("database.databaseExport")}
            style={{ ...s.databaseDialog, width: 500 }}
          >
            <div style={s.databaseDialogHeader}>{t("database.databaseExport")}</div>
            <div style={s.databaseDialogBody}>
              <div style={s.databaseDialogHint}>
                {t("database.databaseExportHint", {
                  database: databaseExportTarget.schema
                    ? `${databaseExportTarget.database}.${databaseExportTarget.schema}`
                    : databaseExportTarget.database,
                })}
              </div>
              <div style={s.databaseDialogFormGrid}>
                <label style={s.databaseSwitchRow}>
                  <input
                    type="checkbox"
                    checked={databaseExportIncludeStructure}
                    onChange={(event) => setDatabaseExportIncludeStructure(event.target.checked)}
                  />
                  <span>{t("database.exportIncludeStructure")}</span>
                </label>
                <label style={s.databaseSwitchRow}>
                  <input
                    type="checkbox"
                    checked={databaseExportIncludeData}
                    onChange={(event) => setDatabaseExportIncludeData(event.target.checked)}
                  />
                  <span>{t("database.exportIncludeData")}</span>
                </label>
                <label style={s.databaseSwitchRow}>
                  <input
                    type="checkbox"
                    checked={databaseExportIncludeObjects}
                    onChange={(event) => setDatabaseExportIncludeObjects(event.target.checked)}
                  />
                  <span>{t("database.exportIncludeObjects")}</span>
                </label>
                <label style={s.databaseSwitchRow}>
                  <input
                    type="checkbox"
                    checked={databaseExportDropTableIfExists}
                    onChange={(event) => setDatabaseExportDropTableIfExists(event.target.checked)}
                  />
                  <span>{t("database.exportDropTableIfExists")}</span>
                </label>
              </div>
              <label style={s.databaseSearchBox}>
                <Search size={13} />
                <input
                  aria-label={t("database.exportSearchTables")}
                  style={s.databaseSearchInput}
                  value={databaseExportSearch}
                  onChange={(event) => setDatabaseExportSearch(event.target.value)}
                  placeholder={t("database.exportSearchTables")}
                  disabled={databaseExportLoading || Boolean(databaseExportError)}
                />
              </label>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10, fontSize: 11.5, color: "var(--text-muted)" }}>
                <span>
                  {t("database.exportSelectedTables", {
                    selected: databaseExportSelection.size,
                    total: databaseExportTables.length,
                  })}
                </span>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <button
                    type="button"
                    style={{ border: "none", background: "transparent", color: "var(--text-secondary)", fontSize: 11.5, cursor: "pointer", padding: "0 2px" }}
                    onClick={() => setDatabaseExportSelection(new Set(filteredDatabaseExportTables))}
                    disabled={databaseExportLoading}
                  >
                    {t("database.visibleDatabasesSelectAll")}
                  </button>
                  <button
                    type="button"
                    style={{ border: "none", background: "transparent", color: "var(--text-secondary)", fontSize: 11.5, cursor: "pointer", padding: "0 2px" }}
                    onClick={() => {
                      const removing = new Set(filteredDatabaseExportTables);
                      setDatabaseExportSelection((current) => new Set([...current].filter((table) => !removing.has(table))));
                    }}
                    disabled={databaseExportLoading}
                  >
                    {t("database.visibleDatabasesClear")}
                  </button>
                </div>
              </div>
              {!databaseExportLoading && !databaseExportError && databaseExportSelection.size === 0 && (
                <div style={{ color: "var(--danger)", fontSize: 12 }}>{t("database.exportEmptySelection")}</div>
              )}
              {!databaseExportLoading &&
                !databaseExportError &&
                !databaseExportIncludeStructure &&
                !databaseExportIncludeData &&
                !databaseExportIncludeObjects && (
                  <div style={{ color: "var(--danger)", fontSize: 12 }}>{t("database.exportEmptyOptions")}</div>
                )}
              <div
                style={{
                  height: 240,
                  overflowY: "auto",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 8,
                  background: "var(--bg-subtle)",
                  padding: 4,
                }}
              >
                {databaseExportLoading ? (
                  <div style={s.databaseEmptyCompact}>{t("common.loading")}</div>
                ) : databaseExportError ? (
                  <div style={{ ...s.databaseEmptyCompact, color: "var(--danger)" }}>
                    {t("database.exportLoadFailed", { message: databaseExportError })}
                  </div>
                ) : filteredDatabaseExportTables.length === 0 ? (
                  <div style={s.databaseEmptyCompact}>{t("database.sidebarSearchNoResults")}</div>
                ) : (
                  filteredDatabaseExportTables.map((table) => {
                    const selected = databaseExportSelection.has(table);
                    return (
                      <button
                        key={table}
                        type="button"
                        style={{
                          ...s.databaseListButton,
                          minHeight: 30,
                          padding: "5px 8px",
                          color: selected ? "var(--text-primary)" : "var(--text-secondary)",
                        }}
                        onClick={() => toggleDatabaseExportTable(table)}
                      >
                        {selected ? <CheckSquare size={14} /> : <Square size={14} />}
                        <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                          {table}
                        </span>
                      </button>
                    );
                  })
                )}
              </div>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxDialogFooterButton type="button" onClick={closeDatabaseExportDialog} disabled={databaseExportLoading}>
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton
                type="button"
                variant="default"
                onClick={() => {
                  void submitDatabaseExport();
                }}
                disabled={databaseExportLoading || Boolean(databaseExportError) || !databaseExportCanRun}
              >
                {t("database.databaseExport")}
              </DbxDialogFooterButton>
            </div>
          </div>
        </div>
      )}
      {tableImportTarget && tableImportConnection && (
        <div
          style={s.databaseDialogOverlay}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) closeTableImportDialog();
          }}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label={t("database.tableImport")}
            style={{ ...s.databaseDialog, width: 720 }}
          >
            <div style={s.databaseDialogHeader}>{t("database.tableImport")}</div>
            <div style={{ ...s.databaseDialogBody, maxHeight: "calc(100vh - 180px)", overflowY: "auto" }}>
              <div style={s.databaseDialogHint}>
                {t("database.tableImportHint", {
                  table: tableImportTarget.object.schema
                    ? `${tableImportTarget.object.schema}.${tableImportTarget.object.name}`
                    : tableImportTarget.object.name,
                })}
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <DbxButton
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    void chooseTableImportFile();
                  }}
                  disabled={tableImportLoading}
                >
                  {t("database.tableImportChooseFile")}
                </DbxButton>
                <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 12, color: "var(--text-muted)" }}>
                  {tableImportPreview ? tableImportPreview.fileName || tableImportPreview.filePath : t("database.tableImportNoFile")}
                </span>
              </div>
              <div style={s.databaseDialogFormGrid}>
                <label style={s.databaseDialogField}>
                  <span style={s.databaseDialogLabel}>{t("database.tableImportMode")}</span>
                  <select
                    aria-label={t("database.tableImportMode")}
                    style={s.databaseDialogInput}
                    value={tableImportMode}
                    onChange={(event) => setTableImportMode(event.target.value as TableImportMode)}
                  >
                    <option value="append">{t("database.tableImportAppend")}</option>
                    <option value="truncate">{t("database.tableImportTruncate")}</option>
                  </select>
                </label>
                <label style={s.databaseDialogField}>
                  <span style={s.databaseDialogLabel}>{t("database.tableImportBatchSize")}</span>
                  <input
                    aria-label={t("database.tableImportBatchSize")}
                    style={s.databaseDialogInput}
                    value={tableImportBatchSize}
                    onChange={(event) => setTableImportBatchSize(event.target.value)}
                    inputMode="numeric"
                  />
                </label>
              </div>
              {tableImportError && <div style={{ color: "var(--danger)", fontSize: 12 }}>{tableImportError}</div>}
              {tableImportLoading ? (
                <div style={s.databaseEmptyCompact}>{t("common.loading")}</div>
              ) : tableImportPreview ? (
                <>
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10 }}>
                    <div style={s.databaseDialogLabel}>
                      {t("database.tableImportMappedColumns", {
                        mapped: tableImportMappedColumns.length,
                        total: tableImportPreview.columns.length,
                      })}
                    </div>
                    <button
                      type="button"
                      style={{ border: "none", background: "transparent", color: "var(--text-secondary)", fontSize: 11.5, cursor: "pointer", padding: "0 2px" }}
                      onClick={() => setTableImportMappings(autoMapImportColumns(tableImportPreview.columns, tableImportTargetColumnNames))}
                    >
                      {t("database.tableImportAutoMap")}
                    </button>
                  </div>
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 8 }}>
                    {tableImportPreview.columns.map((sourceColumn) => (
                      <label key={sourceColumn} style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{sourceColumn}</span>
                        <select
                          aria-label={t("database.tableImportTargetColumn", { column: sourceColumn })}
                          style={s.databaseDialogInput}
                          value={tableImportMappings[sourceColumn] ?? ""}
                          onChange={(event) => updateTableImportMapping(sourceColumn, event.target.value)}
                        >
                          <option value="">{t("database.tableImportSkipColumn")}</option>
                          {tableImportTargetColumnNames.map((targetColumn) => (
                            <option key={targetColumn} value={targetColumn}>
                              {targetColumn}
                            </option>
                          ))}
                        </select>
                      </label>
                    ))}
                  </div>
                  <div style={s.databaseDialogLabel}>
                    {t("database.tableImportPreviewRows", { rows: tableImportPreview.totalRows })}
                  </div>
                  <div style={{ overflow: "auto", border: "1px solid var(--border-dim)", borderRadius: 8, background: "var(--bg-subtle)" }}>
                    <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                      <thead>
                        <tr>
                          {tableImportPreview.columns.map((column) => (
                            <th key={column} style={{ padding: "6px 8px", textAlign: "left", borderBottom: "1px solid var(--border-dim)", color: "var(--text-muted)" }}>
                              {column}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {tableImportPreview.rows.map((row, rowIndex) => (
                          <tr key={rowIndex}>
                            {tableImportPreview.columns.map((column, columnIndex) => (
                              <td key={column} style={{ padding: "6px 8px", borderBottom: "1px solid var(--border-dim)", color: "var(--text-secondary)" }}>
                                {row[columnIndex] === null || row[columnIndex] === undefined ? "NULL" : String(row[columnIndex])}
                              </td>
                            ))}
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </>
              ) : (
                <div style={s.databaseEmptyCompact}>{t("database.tableImportSelectFileHint")}</div>
              )}
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxDialogFooterButton type="button" onClick={closeTableImportDialog} disabled={tableImportLoading}>
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton
                type="button"
                variant="default"
                onClick={() => {
                  void submitTableImport();
                }}
                disabled={tableImportLoading || Boolean(tableImportError) || !tableImportCanRun}
              >
                {t("database.tableImport")}
              </DbxDialogFooterButton>
            </div>
          </div>
        </div>
      )}
      {contextMenu && (
        <>
          <div style={s.fileCtxBackdrop} onClick={() => setContextMenu(null)} />
          <div
            role="menu"
            style={{
              ...s.fileCtxMenu,
              left: contextMenu.x,
              top: contextMenu.y,
              minWidth: 190,
            }}
          >
            {contextMenu.kind === "dbx-grid-header"
              ? ([
                  ["copyColumnName", "database.copyColumnName"],
                  ["previewColumn", "database.openColumnDetailsDialog"],
                  ["sortAscending", "database.sortAscending"],
                  ["sortDescending", "database.sortDescending"],
                  ...(dbxGridOrderByInput.trim() ? ([["clearSort", "database.clearSort"]] as const) : []),
                ] as const).map(([action, labelKey]) => {
                  const disabled =
                    (action === "sortAscending" || action === "sortDescending" || action === "clearSort") &&
                    !dbxGridColumnSortable(queryResult, contextMenu.columnIndex);
                  return (
                  <button
                    key={action}
                    type="button"
                    role="menuitem"
                    disabled={disabled}
                    style={{
                      ...s.fileCtxMenuItem,
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                    }}
                    onClick={() => {
                      void runDbxGridHeaderContextMenuAction(action);
                    }}
                  >
                    {action === "copyColumnName" && <Copy size={13} />}
                    {action === "previewColumn" && <Eye size={13} />}
                    {action === "sortAscending" && <ArrowUp size={13} />}
                    {action === "sortDescending" && <ArrowDown size={13} />}
                    {action === "clearSort" && <ArrowUpDown size={13} />}
                    <span>{t(labelKey)}</span>
                  </button>
                  );
                })
              : contextMenu.kind === "dbx-grid-cell"
              ? ([
                  ["copyValue", "database.copyValue"],
                  ["copyColumnName", "database.copyColumnName"],
                  ["previewValue", "database.previewValue"],
                  ["previewRow", "database.openRowDetailsDialog"],
                  ["previewColumn", "database.openColumnDetailsDialog"],
                  ["sortAscending", "database.sortAscending"],
                  ["sortDescending", "database.sortDescending"],
                  ...(dbxGridOrderByInput.trim() ? ([["clearSort", "database.clearSort"]] as const) : []),
                  ["filterEquals", "database.filterByValue"],
                  ["filterNotEquals", "database.filterExcludeValue"],
                  ["filterLike", "database.filterLike"],
                  ["filterNotLike", "database.filterNotLike"],
                  ["filterLessThan", "database.filterLessThan"],
                  ["filterGreaterThan", "database.filterGreaterThan"],
                  ["filterIsNull", "database.filterIsNull"],
                  ["filterIsNotNull", "database.filterIsNotNull"],
                  ["clearFilter", "database.clearFilter"],
                  ["copyRowJson", "database.copyRow"],
                  ["copyRowInsert", "database.copyRowInsert"],
                  ...(activeDbxGridPrimaryKeys.length
                    ? ([["copyRowInsertWithoutPrimaryKeys", "database.copyRowInsertWithoutPrimaryKeys"]] as const)
                    : []),
                  ["copyRowUpdate", "database.copyRowUpdate"],
                  ["copyAllTsv", "database.copyAllTsv"],
                ] as const).map(([action, labelKey]) => {
                  const multiRowLabelKey =
                    dbxGridCellContextRowCount > 1 && action === "copyRowJson"
                      ? "database.copyRows"
                      : dbxGridCellContextRowCount > 1 && action === "copyRowInsert"
                      ? "database.copyRowsInsert"
                      : dbxGridCellContextRowCount > 1 && action === "copyRowInsertWithoutPrimaryKeys"
                      ? "database.copyRowsInsertWithoutPrimaryKeys"
                      : dbxGridCellContextRowCount > 1 && action === "copyRowUpdate"
                      ? "database.copyRowsUpdate"
                      : labelKey;
                  const disabled =
                    (action === "copyRowUpdate" && activeDbxGridPrimaryKeys.length === 0) ||
                    ((action === "sortAscending" || action === "sortDescending" || action === "clearSort") &&
                      !dbxGridColumnSortable(queryResult, contextMenu.columnIndex));
                  return (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      disabled={disabled}
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                      }}
                      onClick={() => {
                        void runDbxGridCellContextMenuAction(action);
                      }}
                    >
                      {(action === "copyValue" ||
                        action === "copyColumnName" ||
                        action === "copyRowJson" ||
                        action === "copyRowInsert" ||
                        action === "copyRowInsertWithoutPrimaryKeys" ||
                        action === "copyRowUpdate" ||
                        action === "copyAllTsv") && <Copy size={13} />}
                      {(action === "previewValue" || action === "previewRow" || action === "previewColumn") && <Eye size={13} />}
                      {action === "sortAscending" && <ArrowUp size={13} />}
                      {action === "sortDescending" && <ArrowDown size={13} />}
                      {action === "clearSort" && <ArrowUpDown size={13} />}
                      {(action === "filterEquals" ||
                        action === "filterNotEquals" ||
                        action === "filterLike" ||
                        action === "filterNotLike" ||
                        action === "filterLessThan" ||
                        action === "filterGreaterThan" ||
                        action === "filterIsNull" ||
                        action === "filterIsNotNull") && <Search size={13} />}
                      {action === "clearFilter" && <Eraser size={13} />}
                      <span>{t(multiRowLabelKey, { count: dbxGridCellContextRowCount })}</span>
                    </button>
                  );
                })
              : contextMenu.kind === "dbx-table-child" && contextMenuDbxTableChildConnection
              ? ([
                  ["copyName", "database.copyName"],
                  ["dropTableChildObject", dbxTableChildDropLabelKey(contextMenu.childObjectType)],
                ] as const).map(([action, labelKey]) => (
                  <button
                    key={action}
                    type="button"
                    role="menuitem"
                    style={{
                      ...s.fileCtxMenuItem,
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      ...(action === "dropTableChildObject" ? { color: "var(--danger)" } : {}),
                    }}
                    onClick={() => {
                      void runDbxTableChildContextMenuAction(action);
                    }}
                  >
                    {action === "copyName" && <Copy size={13} />}
                      <span>{t(labelKey)}</span>
                    </button>
                  ))
              : contextMenu.kind === "dbx-object-group" && contextMenuDbxObjectGroupConnection
                ? ([
                    ...(contextMenu.groupKey === "tables" ? ([["createTable", "database.createTable"]] as const) : []),
                    ...(contextMenu.groupKey === "views" ? ([["createView", "database.createView"]] as const) : []),
                    ["refresh", "database.refresh"],
                  ] as const).map(([action, labelKey]) => (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                      }}
                      onClick={() => {
                        void runDbxObjectGroupContextMenuAction(action);
                      }}
                    >
                      {(action === "createTable" || action === "createView") && <Plus size={13} />}
                      {action === "refresh" && <RefreshCcw size={13} />}
                      <span>{t(labelKey)}</span>
                    </button>
                  ))
              : contextMenu.kind === "connection-group"
                ? ([
                    ["copyName", "database.copyName"],
                    ["newConnection", "database.newConnection"],
                    ["newGroup", "database.newConnectionGroup"],
                    ["renameGroup", "database.renameConnectionGroup"],
                    ["deleteGroup", "database.deleteConnectionGroup"],
                  ] as const).map(([action, labelKey]) => (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        ...(action === "deleteGroup" ? { color: "var(--danger)" } : {}),
                      }}
                      onClick={() => {
                        void runConnectionGroupContextMenuAction(action);
                      }}
                    >
                      {action === "copyName" && <Copy size={13} />}
                      {(action === "newConnection" || action === "newGroup") && <Plus size={13} />}
                      <span>{t(labelKey)}</span>
                    </button>
                  ))
              : contextMenu.kind === "redis-key" && contextMenuNoSqlConnection
                ? ([
                    ["copyName", "database.copyName"],
                    ["openWorkspace", "database.openWorkspace"],
                    ["refresh", "database.refresh"],
                    ["deleteRedisKey", "database.redisDeleteKey"],
                  ] as const).map(([action, labelKey]) => (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      disabled={action === "deleteRedisKey" && contextMenuNoSqlConnection.readOnly}
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        ...(action === "deleteRedisKey" ? { color: "var(--danger)" } : {}),
                      }}
                      onClick={() => {
                        void runNoSqlContextMenuAction(action);
                      }}
                    >
                      {action === "copyName" && <Copy size={13} />}
                      {action === "openWorkspace" && <Play size={13} />}
                      {action === "refresh" && <RefreshCcw size={13} />}
                      {action === "deleteRedisKey" && <Trash2 size={13} />}
                      <span>{t(labelKey)}</span>
                    </button>
                  ))
              : contextMenu.kind === "mongo-document" && contextMenuNoSqlConnection
                ? ([
                    ["copyName", "database.copyName"],
                    ["openWorkspace", "database.openWorkspace"],
                    ["refresh", "database.refresh"],
                    ["deleteDocument", "database.mongoDeleteDocument"],
                  ] as const).map(([action, labelKey]) => (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      disabled={
                        action === "deleteDocument" &&
                        (contextMenuNoSqlConnection.readOnly || mongoDocumentRawId(contextMenu.document) == null)
                      }
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        ...(action === "deleteDocument" ? { color: "var(--danger)" } : {}),
                      }}
                      onClick={() => {
                        void runNoSqlContextMenuAction(action);
                      }}
                    >
                      {action === "copyName" && <Copy size={13} />}
                      {action === "openWorkspace" && <Play size={13} />}
                      {action === "refresh" && <RefreshCcw size={13} />}
                      {action === "deleteDocument" && <Trash2 size={13} />}
                      <span>{t(labelKey)}</span>
                    </button>
                  ))
              : (contextMenu.kind === "redis-database" || contextMenu.kind === "mongo-database" || contextMenu.kind === "mongo-collection") &&
                  contextMenuNoSqlConnection
                ? (contextMenu.kind === "mongo-collection"
                    ? noSqlCollectionContextMenuItems(contextMenuTreeNodePinned)
                    : noSqlDatabaseContextMenuItems(contextMenu, contextMenuNoSqlConnection, contextMenuTreeNodePinned)
                  ).map(([action, labelKey]) => (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      disabled={action === "flushRedisDb" && contextMenuNoSqlConnection.readOnly}
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        ...(action === "flushRedisDb" ? { color: "var(--danger)" } : {}),
                      }}
                      onClick={() => {
                        void runNoSqlContextMenuAction(action);
                      }}
                    >
                      {action === "togglePin" && <Pin size={13} />}
                      {action === "copyName" && <Copy size={13} />}
                      {action === "newQuery" && <FilePlus size={13} />}
                      {action === "openWorkspace" && <Play size={13} />}
                      {(action === "setDefaultDatabase" || action === "clearDefaultDatabase") && <Database size={13} />}
                      {action === "refresh" && <RefreshCcw size={13} />}
                      {action === "flushRedisDb" && <Eraser size={13} />}
                      <span>{t(labelKey)}</span>
                    </button>
                  ))
              : contextMenu.kind === "dbx-column" && contextMenuDbxColumnConnection
              ? ([
                  ["copyName", "database.copyName"],
                  ["openFieldLineage", "database.openFieldLineage"],
                  ["dropColumn", "database.dropColumn"],
                ] as const).map(([action, labelKey]) => (
                  <button
                    key={action}
                    type="button"
                    role="menuitem"
                    style={{
                      ...s.fileCtxMenuItem,
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      ...(action === "dropColumn" ? { color: "var(--danger)" } : {}),
                    }}
                    onClick={() => {
                      void runDbxColumnContextMenuAction(action);
                    }}
                  >
                    {action === "copyName" && <Copy size={13} />}
                    <span>{t(labelKey)}</span>
                  </button>
                ))
              : contextMenu.kind === "dbx-object"
              ? dbxObjectContextMenuItems(
                  contextMenu.object,
                  contextMenuDbxObjectConnection,
                  contextMenuTreeNodePinned,
                ).map(([action, labelKey]) => (
                  <button
                    key={action}
                    type="button"
                    role="menuitem"
                    style={{
                      ...s.fileCtxMenuItem,
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      ...(action === "dropTable" || action === "dropObject" ? { color: "var(--danger)" } : {}),
                    }}
                    onClick={() => {
                      void runDbxObjectContextMenuAction(action);
                    }}
                  >
                    {action === "togglePin" && <Pin size={13} />}
                    {action === "copyName" && <Copy size={13} />}
                    <span>{t(labelKey)}</span>
                  </button>
	                ))
              : contextMenu.kind === "dbx-schema" && contextMenuDbxSchemaConnection
                ? dbxSchemaContextMenuItems(contextMenuDbxSchemaConnection, contextMenuTreeNodePinned).map(([action, labelKey]) => (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        ...(action === "dropSchema" ? { color: "var(--danger)" } : {}),
                      }}
                      onClick={() => {
                        void runDbxSchemaContextMenuAction(action);
                      }}
                    >
                      {action === "togglePin" && <Pin size={13} />}
                      {action === "copyName" && <Copy size={13} />}
                      <span>{t(labelKey)}</span>
                    </button>
                  ))
              : contextMenu.kind === "dbx-database" && contextMenuDbxDatabaseConnection
                ? dbxDatabaseContextMenuItems(
                    contextMenuDbxDatabaseConnection,
                    contextMenu.database,
                    contextMenuTreeNodePinned,
                  ).map(([action, labelKey]) => (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      style={{
                        ...s.fileCtxMenuItem,
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        ...(action === "dropDatabase" ? { color: "var(--danger)" } : {}),
                      }}
                      onClick={() => {
                        void runDbxDatabaseContextMenuAction(action);
                      }}
                    >
                      {action === "togglePin" && <Pin size={13} />}
                      {action === "copyName" && <Copy size={13} />}
                      <span>{t(labelKey)}</span>
	                    </button>
	                  ))
              : contextMenu.kind === "user-admin"
                ? ([["userAdmin", "database.openUserAdmin"]] as const).map(([action, labelKey]) => (
                    <button
                      key={action}
                      type="button"
                      role="menuitem"
                      style={{ ...s.fileCtxMenuItem, display: "flex", alignItems: "center", gap: 8 }}
                      onClick={() => {
                        void runContextMenuAction(action);
                      }}
                    >
                      <UsersRound size={13} />
                      <span>{t(labelKey)}</span>
                    </button>
                  ))
              : ([
                  ...(contextMenuDbxConnection
                    ? ([
                        [
                          "togglePin",
                          contextMenuDbxConnection.pinned ? "database.unpinConnection" : "database.pinConnection",
                        ],
                      ] as const)
                    : []),
                  [
                    contextMenuConnectionActive ? "close" : "open",
                    contextMenuConnectionActive ? "database.closeConnection" : "database.openConnection",
                  ],
                  ["newQuery", "database.newQuery"],
                  ["queryHistory", "database.queryHistory"],
                  ...(supportsDbxUserAdmin(contextMenuDbxConnection?.dbType)
                    ? ([["userAdmin", "database.userAdmin"]] as const)
                    : []),
                  ...(contextMenuDbxConnection &&
                  hasEnabledDbxTransportLayers(contextMenuDbxConnection) &&
                  dbxConnectionFinalProxyPort(contextMenuDbxConnection) != null
                    ? ([["copyFinalProxyPort", "database.copyFinalProxyPort"]] as const)
                    : []),
                  ["executeSqlFile", "database.executeSqlFile"],
                  ...(canCreateDatabaseForConnection(contextMenuDbxConnection)
                    ? ([
                        [
                          "createDatabase",
                          contextMenuDbxConnection?.dbType === "duckdb" ? "database.createDuckDbFile" : "database.createDatabase",
                        ],
                      ] as const)
                    : []),
                  ...(contextMenuDbxConnection
                    ? ([
                        [
                          "moveToGroup",
                          contextMenuDbxConnectionHasMoveTargets ? "database.moveToGroup" : "database.moveToNewGroup",
                        ],
                      ] as const)
                    : []),
                  ["refresh", "database.refresh"],
                  ...(contextMenuDbxConnection ? ([["selectVisibleDatabases", "database.selectVisibleDatabases"]] as const) : []),
                  ...(contextMenuDbxConnection ? ([["edit", "database.editConnection"]] as const) : []),
                  ...(dbxConnectionLocalFilePath(contextMenuDbxConnection)
                    ? ([["revealDatabaseFile", "database.revealDatabaseFile"]] as const)
                    : []),
                  ...(sqliteBackupSourcePath(contextMenuDbxConnection)
                    ? ([["backupSqliteDatabase", "database.backupSqliteDatabase"]] as const)
                    : []),
                  ["copy", "database.duplicateConnection"],
                  ["delete", "database.deleteConnection"],
                ].map(([action, labelKey]) => (
                  <button
                    key={action}
                    type="button"
                    role="menuitem"
                    style={{ ...s.fileCtxMenuItem, display: "flex", alignItems: "center", gap: 8 }}
                    onClick={() => {
                      void runContextMenuAction(action as Parameters<typeof runContextMenuAction>[0]);
                    }}
                  >
                    {action === "copy" && <Copy size={13} />}
                    <span>{t(labelKey)}</span>
                  </button>
                )))}
          </div>
        </>
      )}
    </div>
  );
}
