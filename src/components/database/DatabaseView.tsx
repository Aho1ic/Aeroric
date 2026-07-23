import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  CSSProperties,
  DragEvent,
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  ChevronLeft,
  ChevronRight,
  CheckSquare,
  Columns3,
  Database,
  FileCode,
  FilePlus,
  Hash,
  KeyRound,
  Plus,
  Play,
  SlidersHorizontal,
  RefreshCcw,
  Trash2,
  Wrench,
  GitCompare,
  GitMerge,
  Network,
  Pin,
  Table2,
  Copy,
  Eraser,
  Eye,
  Search,
  Square,
  UsersRound,
  Zap,
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
  DbxListObjectsOptions,
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
import { DbxButton, DbxDialogFooterButton } from "./DbxButton";
import { ConnectionDialog } from "./ConnectionDialog";
import {
  dbxGridRowsToTsv,
  dbxGridRowsToJson,
  isTextEditingShortcutTarget,
  valueToText,
  quoteSqlName,
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
import { GuidancePanel, renderSqlTokens } from "./DatabaseViewPrimitives";
import { dbxConfigRecord, dbxString } from "./databaseConnectionDraft";
import { confirmDbxProductionOperation, hasProductionProtection } from "./databaseProductionSafety";
import { DataGridView } from "./DataGridView";
import {
  combineDbxGridWhereCondition,
  dbxFilterModeForCellAction,
  dbxGridContextRowIndexes,
  dbxOrderByForColumn,
  dbxPendingCellEditsToDirtyRows,
  nextDbxOrderByForColumn,
  type DatabaseRow,
  type DbxGridCellContextMenuAction,
  type DbxGridCellContextMenuState,
  type DbxGridHeaderContextMenuAction,
  type DbxGridHeaderContextMenuState,
  type TableExportFormat,
} from "./databaseGridState";
import { DBX_GRID_PAGE_SIZE_OPTIONS, useDbxDataGrid } from "./useDbxDataGrid";

interface Props {
  projectRoot?: string;
  initialSqliteFilePath?: string;
  remoteConnection?: SshConnection;
  remoteProjectPath?: string;
  sshConnections?: SshConnection[];
}

const PAGE_SIZE = 100;
const MONGO_SIDEBAR_DOCUMENT_PREVIEW_LIMIT = 20;
const DATABASE_SIDEBAR_DEFAULT_WIDTH = 284;
const DATABASE_SIDEBAR_MIN_WIDTH = 220;
const DATABASE_SIDEBAR_MAX_WIDTH = 520;
const EMPTY_DBX_COLUMNS: DbxColumnInfo[] = [];
const DBX_OBJECT_PAGE_SIZE = 200;
const PRODUCTION_SQL_PREVIEW_LIMIT = 2000;
const DBX_TABLE_LIKE_OBJECT_TYPES = ["TABLE", "VIEW", "MATERIALIZED_VIEW"];
const DBX_KEYLESS_GRID_EDIT_DB_TYPES = new Set<DbxDatabaseType>([
  "sqlite",
  "mysql",
  "postgres",
  "duckdb",
  "sqlserver",
  "oracle",
]);
type RedisSidebarScanState = { cursor: number; totalKeys: number };
type MongoSidebarDocumentQuery = { filter: string; sort: string; projection: string };
type DbxObjectGroupKey = "tables" | "views" | "procedures" | "functions" | "sequences" | "packages";
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

function productionSqlPreview(sql: string): string {
  const trimmed = sql.trim();
  if (trimmed.length <= PRODUCTION_SQL_PREVIEW_LIMIT) return trimmed;
  return `${trimmed.slice(0, PRODUCTION_SQL_PREVIEW_LIMIT)}\n...`;
}

async function listAllDbxObjects(
  connectionId: string,
  database: string | null,
  schema: string | null,
  options: DbxListObjectsOptions = {},
): Promise<DbxObjectInfo[]> {
  if (options.filter) {
    return databaseApi.dbxListObjects(connectionId, database, schema, options);
  }

  const objects: DbxObjectInfo[] = [];
  let offset = 0;
  while (true) {
    const page = await databaseApi.dbxListObjects(connectionId, database, schema, {
      ...options,
      limit: DBX_OBJECT_PAGE_SIZE + 1,
      offset,
    });
    const hasMore = page.length > DBX_OBJECT_PAGE_SIZE;
    const visiblePage = hasMore ? page.slice(0, DBX_OBJECT_PAGE_SIZE) : page;
    objects.push(...visiblePage);
    if (!hasMore || visiblePage.length === 0) return objects;
    offset += visiblePage.length;
  }
}
type DatabaseContextMenuState =
  | {
      x: number;
      y: number;
      connectionId: string;
      kind: "legacy" | "dbx";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string | null;
      object: DbxObjectInfo;
      kind: "dbx-object";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string;
      kind: "dbx-database";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string;
      schema: string;
      kind: "dbx-schema";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string | null;
      object: DbxObjectInfo;
      column: DbxColumnInfo;
      kind: "dbx-column";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string | null;
      object: DbxObjectInfo;
      childObject: DbxObjectInfo;
      childObjectType: Exclude<TableChildObjectType, "COLUMN">;
      kind: "dbx-table-child";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string;
      schema: string | null;
      groupKey: DbxObjectGroupKey;
      label: string;
      kind: "dbx-object-group";
    }
  | {
      x: number;
      y: number;
      groupName: string;
      kind: "connection-group";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      kind: "user-admin";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: number;
      kind: "redis-database";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: number;
      keyRaw: string;
      kind: "redis-key";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string;
      kind: "mongo-database";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string;
      collection: string;
      kind: "mongo-collection";
    }
  | {
      x: number;
      y: number;
      connectionId: string;
      database: string;
      collection: string;
      document: unknown;
      kind: "mongo-document";
    }
  | DbxGridHeaderContextMenuState
  | DbxGridCellContextMenuState
  | {
      x: number;
      y: number;
      tabId: string;
      kind: "workspace-tab";
    }
  | null;
type DbxContextMenuItem<Action extends string> = [action: Action, labelKey: string];
type WorkspaceTabContextMenuAction =
  | "toggleShortTitle"
  | "pinTab"
  | "closeTab"
  | "closeOtherTabs"
  | "closeAllTabs";
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
    const parsed = JSON.parse(
      window.localStorage.getItem(PINNED_TREE_NODE_IDS_STORAGE_KEY) ?? "[]",
    );
    return new Set(
      Array.isArray(parsed)
        ? parsed.filter((value): value is string => typeof value === "string")
        : [],
    );
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
    const parsed = JSON.parse(
      window.localStorage.getItem(EXTRA_DBX_CONNECTION_GROUPS_STORAGE_KEY) ?? "[]",
    );
    return Array.isArray(parsed)
      ? parsed.filter(
          (value): value is string => typeof value === "string" && value.trim().length > 0,
        )
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
  if (
    menu.kind === "dbx-object" &&
    (isDbxTableObject(menu.object) || isDbxViewObject(menu.object))
  ) {
    return `dbx-object:${normalizeDbxObjectType(menu.object.object_type)}:${menu.object.schema ?? ""}:${menu.object.name}`;
  }
  if (menu.kind === "redis-database") return `redis-database:${menu.connectionId}:${menu.database}`;
  if (menu.kind === "mongo-database") return `mongo-database:${menu.connectionId}:${menu.database}`;
  if (menu.kind === "mongo-collection")
    return `mongo-collection:${menu.connectionId}:${menu.database}:${menu.collection}`;
  return null;
}

function contextMenuConnectionId(menu: DatabaseContextMenuState): string | null {
  return menu && "connectionId" in menu ? menu.connectionId : null;
}

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

const MYSQL_COMPATIBLE_CREATE_DATABASE_PROFILES = new Set([
  "mysql",
  "mariadb",
  "tidb",
  "oceanbase",
  "doris",
  "starrocks",
  "custom_mysql",
]);
const CREATE_DATABASE_DB_TYPES = new Set([
  "mysql",
  "postgres",
  "sqlserver",
  "oracle",
  "clickhouse",
  "duckdb",
]);
const DBX_DATABASE_CREATE_NODE_DB_TYPES = new Set<DbxDatabaseType>([
  "mysql",
  "postgres",
  "sqlserver",
  "oracle",
  "clickhouse",
]);
const DBX_TABLE_STRUCTURE_DB_TYPES = new Set<DbxDatabaseType>([
  "sqlite",
  "mysql",
  "postgres",
  "duckdb",
  "sqlserver",
  "oracle",
  "clickhouse",
]);
const DBX_SCHEMA_AWARE_DB_TYPES = new Set<DbxDatabaseType>([
  "postgres",
  "sqlserver",
  "oracle",
  "duckdb",
]);
const DBX_TREE_SCHEMA_DB_TYPES = new Set<DbxDatabaseType>(["postgres", "sqlserver", "duckdb"]);
const DBX_DIAGRAM_DB_TYPES = new Set<DbxDatabaseType>([
  "sqlite",
  "mysql",
  "postgres",
  "sqlserver",
  "oracle",
]);
const DBX_NO_TRUNCATE_DB_TYPES = new Set<string>([
  "sqlite",
  "rqlite",
  "turso",
  "duckdb",
  "influxdb",
  "manticoresearch",
]);
const SYSTEM_DATABASE_NAMES: Partial<Record<DbxDatabaseType, ReadonlySet<string>>> = {
  mysql: new Set(["information_schema", "mysql", "performance_schema", "sys"]),
  postgres: new Set(["template0", "template1"]),
  clickhouse: new Set(["information_schema", "system"]),
  sqlserver: new Set(["master", "model", "msdb", "tempdb"]),
  mongodb: new Set(["admin", "config", "local"]),
};
const DATA_TOOL_EXPORT_FORMATS: Array<{
  format: TableExportFormat;
  labelKey: string;
  label: string;
}> = [
  { format: "csv", labelKey: "database.exportCsv", label: "CSV" },
  { format: "json", labelKey: "database.exportJson", label: "JSON" },
  { format: "insertSql", labelKey: "database.exportInsertSql", label: "SQL INSERT" },
  { format: "xlsx", labelKey: "database.exportXlsx", label: "XLSX" },
];
function endpointLabel(endpoint: DbEndpoint): string {
  if (endpoint.kind === "local") return endpoint.path;
  return `${endpoint.connection.name}: ${endpoint.path}`;
}

export function dbxColumnInfoToEditableStructureColumn(
  column: DbxColumnInfo,
  originalPosition: number,
): EditableStructureColumn {
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

function isSqlDbxConnection(connection: AeroricDbConnectionConfig | null | undefined) {
  return Boolean(connection && !["redis", "mongodb"].includes(connection.dbType));
}

function configuredTargetDatabase(
  connection: AeroricDbConnectionConfig | null | undefined,
): string | null {
  if (!connection?.dbx || typeof connection.dbx !== "object") return null;
  const database = (connection.dbx as { database?: unknown }).database;
  return typeof database === "string" && database.trim() ? database.trim() : null;
}

function isDbxDefaultDatabase(
  connection: AeroricDbConnectionConfig | null | undefined,
  database: string,
): boolean {
  return configuredTargetDatabase(connection) === database;
}

function configuredVisibleDatabases(
  connection: AeroricDbConnectionConfig | null | undefined,
): string[] | undefined {
  if (!connection?.dbx || typeof connection.dbx !== "object") return undefined;
  const configured = (connection.dbx as { visible_databases?: unknown }).visible_databases;
  if (!Array.isArray(configured)) return undefined;
  return configured.filter(
    (name): name is string => typeof name === "string" && name.trim().length > 0,
  );
}

function isSystemDatabaseName(databaseType: DbxDatabaseType | undefined, name: string) {
  if (!databaseType) return false;
  return SYSTEM_DATABASE_NAMES[databaseType]?.has(name.toLowerCase()) ?? false;
}

function filterDbxDatabasesForConnection(
  databases: DbxDatabaseInfo[],
  connection: AeroricDbConnectionConfig,
): DbxDatabaseInfo[] {
  const targetDatabase = configuredTargetDatabase(connection);
  if (targetDatabase) return databases.filter((database) => database.name === targetDatabase);

  const visibleDatabases = configuredVisibleDatabases(connection);
  if (visibleDatabases) {
    const visible = new Set(visibleDatabases);
    return databases.filter((database) => visible.has(database.name));
  }

  return databases.filter((database) => !isSystemDatabaseName(connection.dbType, database.name));
}

function normalizeVisibleDatabaseSelection(
  selectedNames: string[],
  databaseNames: string[],
): string[] {
  const available = new Set(databaseNames);
  const seen = new Set<string>();
  return selectedNames.filter((name) => {
    if (!available.has(name) || seen.has(name)) return false;
    seen.add(name);
    return true;
  });
}

function autoMapImportColumns(
  sourceColumns: string[],
  targetColumns: string[],
): Record<string, string> {
  const targetByLower = new Map(targetColumns.map((column) => [column.toLowerCase(), column]));
  return Object.fromEntries(
    sourceColumns.map((sourceColumn) => [
      sourceColumn,
      targetByLower.get(sourceColumn.toLowerCase()) ?? "",
    ]),
  );
}

function hasEnabledDbxTransportLayers(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  const layers = dbxConfigRecord(connection).transport_layers;
  return (
    Array.isArray(layers) &&
    layers.some((layer) => {
      if (!layer || typeof layer !== "object") return false;
      return (layer as { enabled?: unknown }).enabled !== false;
    })
  );
}

function dbxConnectionFinalProxyPort(
  connection: AeroricDbConnectionConfig | null | undefined,
): number | null {
  if (!connection) return null;
  const config = dbxConfigRecord(connection);
  const value = config.final_proxy_port ?? config.finalProxyPort;
  const port =
    typeof value === "number"
      ? value
      : typeof value === "string"
        ? Number.parseInt(value, 10)
        : NaN;
  return Number.isFinite(port) && port > 0 ? port : null;
}

function dbxConnectionLocalFilePath(
  connection: AeroricDbConnectionConfig | null | undefined,
): string | null {
  if (!connection || (connection.dbType !== "sqlite" && connection.dbType !== "duckdb"))
    return null;
  const path = dbxString(dbxConfigRecord(connection), "host").trim();
  if (!path || path === ":memory:") return null;
  return path;
}

function sqliteBackupSourcePath(
  connection: AeroricDbConnectionConfig | null | undefined,
): string | null {
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
  return dotIndex <= 0
    ? `${fileName}.backup.db`
    : `${fileName.slice(0, dotIndex)}.backup${fileName.slice(dotIndex)}`;
}

function sqliteEndpointKey(endpoint: DbEndpoint): string {
  return endpoint.kind === "local"
    ? `local:${endpoint.path}`
    : `ssh:${endpoint.connection.id}:${endpoint.path}`;
}

function createSqliteFileConnection(endpoint: DbEndpoint): DbConnectionConfig {
  const now = Date.now();
  return {
    id: `sqlite-file:${now}:${Math.random().toString(36).slice(2)}`,
    name: createConnectionName(endpoint),
    endpoint,
    readOnly: false,
    createdAt: now,
    lastOpenedAt: now,
  };
}

function dbxDriverProfile(connection: AeroricDbConnectionConfig | null | undefined): string | null {
  if (!connection?.dbx || typeof connection.dbx !== "object") return null;
  const profile = (connection.dbx as { driver_profile?: unknown }).driver_profile;
  return typeof profile === "string" && profile.trim() ? profile.trim() : null;
}

function canCreateDatabaseForConnection(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && CREATE_DATABASE_DB_TYPES.has(connection.dbType));
}

function canSetCreateDatabaseCharset(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  if (!connection) return false;
  return (
    connection.dbType === "mysql" ||
    MYSQL_COMPATIBLE_CREATE_DATABASE_PROFILES.has(dbxDriverProfile(connection) ?? "")
  );
}

function ensureDuckDbFileExtension(path: string): string {
  return /\.(duckdb|db)$/i.test(path) ? path : `${path}.duckdb`;
}

function duckDbAttachedDatabaseNameFromPath(path: string): string {
  const fileName = path.split(/[\\/]/).pop() ?? path;
  const withoutExtension = fileName.replace(/\.(duckdb|db)$/i, "");
  const normalized = withoutExtension
    .trim()
    .replace(/[^A-Za-z0-9_]+/g, "_")
    .replace(/^_+|_+$/g, "");
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

function dbxAttachedDatabaseRecords(
  connection: AeroricDbConnectionConfig,
): Array<Record<string, unknown>> {
  if (!connection.dbx || typeof connection.dbx !== "object") return [];
  const attachedDatabases = (connection.dbx as { attached_databases?: unknown }).attached_databases;
  return Array.isArray(attachedDatabases)
    ? attachedDatabases.filter((database): database is Record<string, unknown> =>
        Boolean(database && typeof database === "object"),
      )
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
  if (document && typeof document === "object" && "_id" in document)
    return String((document as { _id: unknown })._id);
  return `#${fallback + 1}`;
}

function mongoDocumentRawId(document: unknown): unknown | null {
  if (!document || typeof document !== "object" || !("_id" in document)) return null;
  return (document as { _id: unknown })._id;
}

function deriveDbxSchemas(objects: DbxObjectInfo[]): string[] {
  return Array.from(
    new Set(objects.map((object) => object.schema?.trim() ?? "").filter(Boolean)),
  ).sort((left, right) =>
    left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }),
  );
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

function dbxTableChildObjectType(
  object: DbxObjectInfo,
): Exclude<TableChildObjectType, "COLUMN"> | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType.includes("FOREIGN_KEY")) return "FOREIGN_KEY";
  if (objectType.includes("TRIGGER")) return "TRIGGER";
  if (objectType.includes("INDEX")) return "INDEX";
  return null;
}

function sameDbxName(left: string | null | undefined, right: string | null | undefined) {
  return (left ?? "").trim().toLowerCase() === (right ?? "").trim().toLowerCase();
}

function uniqueDbxObjectName(
  baseName: string,
  schema: string | null | undefined,
  objects: DbxObjectInfo[],
) {
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
  if (!childObject.parent_name || !sameDbxName(childObject.parent_name, tableObject.name))
    return false;
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

function canRenameDbxObject(
  connection: AeroricDbConnectionConfig | null | undefined,
  object: DbxObjectInfo | null | undefined,
): boolean {
  if (!connection || !object || connection.readOnly) return false;
  const objectType = dbxObjectRenameType(object);
  if (!objectType) return false;
  if (connection.dbType === "sqlserver") return true;
  if (connection.dbType === "sqlite") return objectType === "TABLE";
  if (
    connection.dbType === "mysql" ||
    connection.dbType === "postgres" ||
    connection.dbType === "oracle"
  ) {
    return objectType === "TABLE" || objectType === "VIEW";
  }
  return false;
}

function supportsDbxObjectBrowserTreeNode(
  connection: AeroricDbConnectionConfig | null | undefined,
  nodeType: "database" | "schema",
): boolean {
  if (!connection || !isSqlDbxConnection(connection)) return false;
  if (
    nodeType === "database" &&
    DBX_SCHEMA_AWARE_DB_TYPES.has(connection.dbType) &&
    connection.dbType !== "sqlserver"
  ) {
    return false;
  }
  return true;
}

function supportsDbxTableStructureEditing(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && DBX_TABLE_STRUCTURE_DB_TYPES.has(connection.dbType));
}

function supportsDbxSqlFileExecution(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && isSqlDbxConnection(connection));
}

function supportsDbxDiagram(connection: AeroricDbConnectionConfig | null | undefined): boolean {
  return Boolean(connection && DBX_DIAGRAM_DB_TYPES.has(connection.dbType));
}

function supportsDbxDatabaseSearch(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
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

function supportsDbxTableTruncate(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
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
  const add = (action: DbxDatabaseContextMenuAction, labelKey: string) =>
    items.push([action, labelKey]);
  add("togglePin", pinned ? "database.unpin" : "database.pin");
  add("copyName", "database.copyName");
  if (supportsDbxObjectBrowserTreeNode(connection, "database"))
    add("openObjectBrowser", "database.openObjectBrowser");
  add("newQuery", "database.newQuery");
  add("queryHistory", "database.queryHistory");
  add(
    connection && isDbxDefaultDatabase(connection, database)
      ? "clearDefaultDatabase"
      : "setDefaultDatabase",
    connection && isDbxDefaultDatabase(connection, database)
      ? "database.clearDefaultDatabase"
      : "database.setDefaultDatabase",
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
  const add = (action: DbxSchemaContextMenuAction, labelKey: string) =>
    items.push([action, labelKey]);
  add("togglePin", pinned ? "database.unpin" : "database.pin");
  add("copyName", "database.copyName");
  if (supportsDbxObjectBrowserTreeNode(connection, "schema"))
    add("openObjectBrowser", "database.openObjectBrowser");
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
  add(
    isDefault ? "clearDefaultDatabase" : "setDefaultDatabase",
    isDefault ? "database.clearDefaultDatabase" : "database.setDefaultDatabase",
  );
  if (menu.kind === "redis-database") add("flushRedisDb", "database.redisFlushDb");
  return items;
}

function noSqlCollectionContextMenuItems(
  pinned: boolean,
): DbxContextMenuItem<NoSqlContextMenuAction>[] {
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
  const add = (action: DbxObjectContextMenuAction, labelKey: string) =>
    items.push([action, labelKey]);
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
  return object.schema
    ? `${quoteSqlName(object.schema)}.${quoteSqlName(object.name)}`
    : quoteSqlName(object.name);
}

export function DatabaseView({
  projectRoot,
  initialSqliteFilePath,
  remoteConnection,
  remoteProjectPath,
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
  const [exportProgress, setExportProgress] = useState<{
    active: boolean;
    format: string;
    filePath: string;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [workspaceMode, setWorkspaceMode] = useState<DbWorkspaceMode>("table");
  type WorkspaceTab = { id: string; mode: DbWorkspaceMode; label: string; closable: boolean };
  const [workspaceTabs, setWorkspaceTabs] = useState<WorkspaceTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string>("");
  const [shortWorkspaceTabIds, setShortWorkspaceTabIds] = useState<Set<string>>(new Set());

  const closeWorkspaceTab = useCallback(
    (tabId: string) => {
      setWorkspaceTabs((prev) => {
        const next = prev.filter((t) => t.id !== tabId);
        if (activeTabId === tabId && next.length > 0) {
          const last = next[next.length - 1];
          setActiveTabId(last.id);
          setWorkspaceMode(last.mode);
        } else if (activeTabId === tabId) {
          setActiveTabId("");
        }
        return next;
      });
      setShortWorkspaceTabIds((current) => {
        const next = new Set(current);
        next.delete(tabId);
        return next;
      });
    },
    [activeTabId],
  );

  const activateWorkspaceTab = useCallback((tab: WorkspaceTab | undefined) => {
    if (!tab) {
      setActiveTabId("");
      return;
    }
    setActiveTabId(tab.id);
    setWorkspaceMode(tab.mode);
  }, []);

  const closeWorkspaceTabs = useCallback(
    (tabIds: Set<string>) => {
      setWorkspaceTabs((prev) => {
        const next = prev.filter((tab) => !tabIds.has(tab.id));
        if (tabIds.has(activeTabId)) {
          activateWorkspaceTab(next[next.length - 1]);
        }
        return next;
      });
      setShortWorkspaceTabIds((current) => {
        const next = new Set(current);
        tabIds.forEach((tabId) => next.delete(tabId));
        return next;
      });
    },
    [activeTabId, activateWorkspaceTab],
  );

  const [connectionDialogOpen, setConnectionDialogOpen] = useState(false);
  const [editingDbxConnectionId, setEditingDbxConnectionId] = useState<string | null>(null);
  const [newConnectionGroup, setNewConnectionGroup] = useState<string | null>(null);
  const [driverManifest, setDriverManifest] = useState<DatabaseDriverManifest | null>(null);
  const [createDatabaseConnectionId, setCreateDatabaseConnectionId] = useState<string | null>(null);
  const [createDatabaseName, setCreateDatabaseName] = useState("");
  const [createDatabaseCharset, setCreateDatabaseCharset] = useState("utf8mb4");
  const [createDatabaseCollation, setCreateDatabaseCollation] = useState("utf8mb4_unicode_ci");
  const [createSchemaTarget, setCreateSchemaTarget] = useState<{
    connectionId: string;
    database: string;
  } | null>(null);
  const [createSchemaName, setCreateSchemaName] = useState("");
  const [sqlFilePath, setSqlFilePath] = useState("");
  const [sqlFilePreview, setSqlFilePreview] = useState("");
  const [sqlFileTimeoutSecs, setSqlFileTimeoutSecs] = useState("60");
  const [dbxColumnsByTable, setDbxColumnsByTable] = useState<Record<string, DbxColumnInfo[]>>({});
  const [redisDatabasesByConnection, setRedisDatabasesByConnection] = useState<
    Record<string, RedisDatabaseInfo[]>
  >({});
  const [redisKeysByDatabase, setRedisKeysByDatabase] = useState<Record<string, RedisKeyInfo[]>>(
    {},
  );
  const [redisScanStateByDatabase, setRedisScanStateByDatabase] = useState<
    Record<string, RedisSidebarScanState>
  >({});
  const [mongoDatabasesByConnection, setMongoDatabasesByConnection] = useState<
    Record<string, string[]>
  >({});
  const [mongoCollectionsByDatabase, setMongoCollectionsByDatabase] = useState<
    Record<string, string[]>
  >({});
  const [mongoDocumentsByCollection, setMongoDocumentsByCollection] = useState<
    Record<string, unknown[]>
  >({});
  const [mongoDocumentTotalsByCollection, setMongoDocumentTotalsByCollection] = useState<
    Record<string, number>
  >({});
  const [mongoDocumentQueriesByCollection, setMongoDocumentQueriesByCollection] = useState<
    Record<string, MongoSidebarDocumentQuery>
  >({});
  const [activeMongoDocumentId, setActiveMongoDocumentId] = useState<string | null>(null);
  const [activeMongoWorkspaceDatabase, setActiveMongoWorkspaceDatabase] = useState<string | null>(
    null,
  );
  const [dbxSqlPreviewOpen, setDbxSqlPreviewOpen] = useState(false);
  const [dbxSqlPreviewStatements, setDbxSqlPreviewStatements] = useState<string[]>([]);
  const [dbxSqlPreviewRollback, setDbxSqlPreviewRollback] = useState<string[]>([]);
  const [dbxSqlPreviewDescription, setDbxSqlPreviewDescription] = useState("");
  const [visibleDatabaseConnectionId, setVisibleDatabaseConnectionId] = useState<string | null>(
    null,
  );
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
  const runWorkspaceTabContextMenuAction = useCallback(
    (action: WorkspaceTabContextMenuAction) => {
      const menu = contextMenu?.kind === "workspace-tab" ? contextMenu : null;
      setContextMenu(null);
      if (!menu) return;
      if (action === "toggleShortTitle") {
        setShortWorkspaceTabIds((current) => {
          const next = new Set(current);
          if (next.has(menu.tabId)) next.delete(menu.tabId);
          else next.add(menu.tabId);
          return next;
        });
        return;
      }
      if (action === "pinTab") {
        setWorkspaceTabs((prev) => {
          const tab = prev.find((item) => item.id === menu.tabId);
          if (!tab) return prev;
          return [tab, ...prev.filter((item) => item.id !== menu.tabId)];
        });
        return;
      }
      if (action === "closeTab") {
        closeWorkspaceTab(menu.tabId);
        return;
      }
      if (action === "closeOtherTabs") {
        closeWorkspaceTabs(
          new Set(workspaceTabs.filter((tab) => tab.id !== menu.tabId).map((tab) => tab.id)),
        );
        return;
      }
      if (action === "closeAllTabs") {
        closeWorkspaceTabs(new Set(workspaceTabs.map((tab) => tab.id)));
      }
    },
    [closeWorkspaceTab, closeWorkspaceTabs, contextMenu, workspaceTabs],
  );
  const [tableInfoActiveTab, setTableInfoActiveTab] = useState<TableInfoTab>("columns");
  const [tableInfoSearch, setTableInfoSearch] = useState("");
  const [tableInfoDdl, setTableInfoDdl] = useState("");
  const [tableInfoDdlLoading, setTableInfoDdlLoading] = useState(false);
  const [tableInfoDdlError, setTableInfoDdlError] = useState("");
  const databaseSidebarResizeStartRef = useRef({ x: 0, width: DATABASE_SIDEBAR_DEFAULT_WIDTH });
  const openedInitialSqliteFilePathRef = useRef<string | null>(null);
  const [databaseSidebarWidth, setDatabaseSidebarWidth] = useState(DATABASE_SIDEBAR_DEFAULT_WIDTH);
  const [resizingDatabaseSidebar, setResizingDatabaseSidebar] = useState(false);
  const [pinnedTreeNodeIds, setPinnedTreeNodeIds] = useState<Set<string>>(loadPinnedTreeNodeIds);
  const [extraDbxConnectionGroups, setExtraDbxConnectionGroups] = useState<string[]>(
    loadExtraDbxConnectionGroups,
  );

  const createInitialSqliteEndpoint = useCallback((): DbEndpoint | null => {
    const path = initialSqliteFilePath?.trim();
    if (!path) return null;
    if (remoteConnection) {
      return {
        kind: "ssh",
        connection: remoteConnection,
        path,
        projectPath: remoteProjectPath,
      };
    }
    return { kind: "local", path };
  }, [initialSqliteFilePath, remoteConnection, remoteProjectPath]);

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
  const dbxHasSqlObjectBrowser = isSqlDbxConnection(activeDbxConnection);
  const sqlDbxConnections = useMemo(
    () => dbxConnections.filter((connection) => isSqlDbxConnection(connection)),
    [dbxConnections],
  );
  const dbxTableObjects = useMemo(
    () => dbxObjects.filter((object) => isDbxTableObject(object)),
    [dbxObjects],
  );
  const selectedDbxTable = useMemo(
    () =>
      activeDbxObject && isDbxTableObject(activeDbxObject)
        ? activeDbxObject
        : (dbxTableObjects[0] ?? null),
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
  const selectedDbxInfoColumns = selectedDbxInfoObject
    ? (dbxColumnsByTable[selectedDbxInfoObjectKey] ?? EMPTY_DBX_COLUMNS)
    : EMPTY_DBX_COLUMNS;
  const selectedDbxInfoChildObjects = useMemo(
    () =>
      selectedDbxInfoObject
        ? dbxObjects.filter(
            (object) =>
              Boolean(dbxTableChildObjectType(object)) &&
              dbxChildObjectBelongsToTable(object, selectedDbxInfoObject),
          )
        : [],
    [dbxObjects, selectedDbxInfoObject],
  );
  const selectedDbxInfoIndexes = useMemo(
    () =>
      selectedDbxInfoChildObjects.filter((object) => dbxTableChildObjectType(object) === "INDEX"),
    [selectedDbxInfoChildObjects],
  );
  const selectedDbxInfoForeignKeys = useMemo(
    () =>
      selectedDbxInfoChildObjects.filter(
        (object) => dbxTableChildObjectType(object) === "FOREIGN_KEY",
      ),
    [selectedDbxInfoChildObjects],
  );
  const selectedDbxInfoTriggers = useMemo(
    () =>
      selectedDbxInfoChildObjects.filter((object) => dbxTableChildObjectType(object) === "TRIGGER"),
    [selectedDbxInfoChildObjects],
  );
  const tableInfoQuery = tableInfoSearch.trim().toLowerCase();
  const filteredDbxInfoColumns = useMemo(() => {
    if (!tableInfoQuery) return selectedDbxInfoColumns;
    return selectedDbxInfoColumns.filter((column) =>
      [column.name, column.data_type, column.column_default ?? ""].some((value) =>
        value.toLowerCase().includes(tableInfoQuery),
      ),
    );
  }, [selectedDbxInfoColumns, tableInfoQuery]);
  const filterTableInfoObjects = useCallback(
    (objects: DbxObjectInfo[]) => {
      if (!tableInfoQuery) return objects;
      return objects.filter((object) =>
        [object.name, object.schema ?? "", object.object_type].some((value) =>
          value.toLowerCase().includes(tableInfoQuery),
        ),
      );
    },
    [tableInfoQuery],
  );
  const filteredDbxInfoIndexes = useMemo(
    () => filterTableInfoObjects(selectedDbxInfoIndexes),
    [filterTableInfoObjects, selectedDbxInfoIndexes],
  );
  const filteredDbxInfoForeignKeys = useMemo(
    () => filterTableInfoObjects(selectedDbxInfoForeignKeys),
    [filterTableInfoObjects, selectedDbxInfoForeignKeys],
  );
  const filteredDbxInfoTriggers = useMemo(
    () => filterTableInfoObjects(selectedDbxInfoTriggers),
    [filterTableInfoObjects, selectedDbxInfoTriggers],
  );
  useEffect(() => {
    setTableInfoActiveTab("columns");
    setTableInfoSearch("");
    setTableInfoDdl("");
    setTableInfoDdlError("");
  }, [selectedDbxInfoObjectKey]);
  const loadTableInfoDdlForObject = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      setTableInfoDdlLoading(true);
      setTableInfoDdlError("");
      try {
        const ddl = await databaseApi.dbxGetTableDdl(
          connection.id,
          object.name,
          database,
          object.schema ?? null,
        );
        setTableInfoDdl(ddl);
      } catch (err) {
        setTableInfoDdlError(String(err));
      } finally {
        setTableInfoDdlLoading(false);
      }
    },
    [],
  );
  const loadTableInfoDdl = useCallback(async () => {
    if (!activeDbxConnection || !selectedDbxInfoObject || tableInfoDdlLoading) return;
    await loadTableInfoDdlForObject(activeDbxConnection, activeDbxDatabase, selectedDbxInfoObject);
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    loadTableInfoDdlForObject,
    selectedDbxInfoObject,
    tableInfoDdlLoading,
  ]);
  useEffect(() => {
    if (
      tableInfoActiveTab === "ddl" &&
      !tableInfoDdl &&
      !tableInfoDdlLoading &&
      !tableInfoDdlError
    ) {
      void loadTableInfoDdl();
    }
  }, [loadTableInfoDdl, tableInfoActiveTab, tableInfoDdl, tableInfoDdlError, tableInfoDdlLoading]);
  const activeSqlCapable = Boolean(
    activeEndpoint || (activeDbxConnection && dbxHasSqlObjectBrowser),
  );
  const rawTableRows = useMemo(
    () => queryResult?.rows ?? sqlResult?.rows ?? [],
    [queryResult, sqlResult],
  );
  const tableColumns = useMemo(
    () => queryResult?.columns ?? sqlResult?.columns ?? [],
    [queryResult, sqlResult],
  );
  const showRowIdColumn = Boolean(queryResult && !activeDbxConnection && queryResult.hasRowId);
  const activeDbxGridColumns = useMemo(() => {
    if (!activeDbxObject) return EMPTY_DBX_COLUMNS;
    return dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? EMPTY_DBX_COLUMNS;
  }, [activeDbxObject, dbxColumnsByTable]);
  const dbxGrid = useDbxDataGrid({
    initialPageSize: PAGE_SIZE,
    tableColumns,
    rawTableRows,
    queryResult,
    activeDbxConnection,
    activeDbxGridColumns,
    activeObject,
    showRowIdColumn,
  });
  const {
    dbxGridWhereInput,
    setDbxGridWhereInput,
    dbxGridOrderByInput,
    setDbxGridOrderByInput,
    dbxGridSearch,
    setDbxGridSearch,
    dbxGridColumnSearch,
    setDbxGridColumnSearch,
    dbxGridHiddenColumns,
    dbxDataToolsOpen,
    setDbxDataToolsOpen,
    dbxDataToolsMode,
    setDbxDataToolsMode,
    dbxFieldFilterOpen,
    setDbxFieldFilterOpen,
    dbxGridPageSize,
    setDbxGridPageSize,
    dbxGridSelectedRows,
    setDbxGridSelectedRows,
    dbxGridExportFormat,
    setDbxGridExportFormat,
    dbxCellPreview,
    setDbxCellPreview,
    dbxCellDetail,
    setDbxCellDetail,
    dbxSelectedCell,
    dbxPendingCellEdits,
    setDbxPendingCellEdits,
    dbxRowPreview,
    setDbxRowPreview,
    dbxRowPreviewSearch,
    setDbxRowPreviewSearch,
    dbxColumnPreview,
    setDbxColumnPreview,
    dbxColumnPreviewSearch,
    setDbxColumnPreviewSearch,
  } = dbxGrid.state;
  const {
    visibleTableColumns,
    filteredDbxGridColumnOptions,
    formattedDbxCellPreview,
    dbxRowPreviewFields,
    filteredDbxRowPreviewFields,
    dbxColumnPreviewFields,
    filteredDbxColumnPreviewFields,
    dbxPendingCellEditCount,
  } = dbxGrid.derived;
  const {
    initializeLoadedGrid,
    resetGridPresentation,
    toggleDbxGridColumnVisibility,
    showAllDbxGridColumns,
    invertDbxGridColumnVisibility,
  } = dbxGrid.actions;
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
      const whereInput = dbxGridWhereInput.trim();
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
    dbxGridWhereInput,
    page,
    queryResult,
    sql,
  ]);
  const activeDbxTargetDatabase = configuredTargetDatabase(activeDbxConnection);
  const visibleDbxDatabases = activeDbxTargetDatabase
    ? dbxDatabases.filter((database) => database.name === activeDbxTargetDatabase)
    : dbxDatabases;
  const visibleDatabaseConnection = useMemo(
    () =>
      dbxConnections.find((connection) => connection.id === visibleDatabaseConnectionId) ?? null,
    [dbxConnections, visibleDatabaseConnectionId],
  );
  const listedVisibleDatabaseNames = useMemo(
    () =>
      visibleDatabaseShowSystem
        ? visibleDatabaseNames
        : visibleDatabaseNames.filter(
            (name) => !isSystemDatabaseName(visibleDatabaseConnection?.dbType, name),
          ),
    [visibleDatabaseConnection?.dbType, visibleDatabaseNames, visibleDatabaseShowSystem],
  );
  const filteredVisibleDatabaseNames = useMemo(() => {
    const query = visibleDatabaseSearch.trim().toLowerCase();
    if (!query) return listedVisibleDatabaseNames;
    return listedVisibleDatabaseNames.filter((name) => name.toLowerCase().includes(query));
  }, [listedVisibleDatabaseNames, visibleDatabaseSearch]);
  const visibleDatabaseHasSystemNames = useMemo(
    () =>
      visibleDatabaseNames.some((name) =>
        isSystemDatabaseName(visibleDatabaseConnection?.dbType, name),
      ),
    [visibleDatabaseConnection?.dbType, visibleDatabaseNames],
  );
  const visibleDatabaseCanSave = visibleDatabaseSelection.size > 0;
  const databaseExportConnection = useMemo(
    () =>
      dbxConnections.find((connection) => connection.id === databaseExportTarget?.connectionId) ??
      null,
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
    () =>
      dbxConnections.find((connection) => connection.id === tableImportTarget?.connectionId) ??
      null,
    [dbxConnections, tableImportTarget?.connectionId],
  );
  const tableImportTargetColumnNames = useMemo(
    () => tableImportColumns.map((column) => column.name),
    [tableImportColumns],
  );
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
  const tableImportCanRun = Boolean(
    tableImportConnection &&
    tableImportTarget &&
    tableImportPreview &&
    tableImportMappedColumns.length > 0,
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
    databaseApi
      .loadConnections()
      .then((items) => {
        const initialEndpoint = createInitialSqliteEndpoint();
        if (initialEndpoint) {
          const initialEndpointKey = sqliteEndpointKey(initialEndpoint);
          const initialConnection = createSqliteFileConnection(initialEndpoint);
          openedInitialSqliteFilePathRef.current = initialEndpointKey;
          setConnections([
            initialConnection,
            ...items.filter((item) => sqliteEndpointKey(item.endpoint) !== initialEndpointKey),
          ]);
          setActiveConnectionId(initialConnection.id);
          inspect(initialConnection);
        } else {
          setConnections(items);
        }
        if (!initialEndpoint && items[0]) {
          setActiveConnectionId(items[0].id);
          inspect(items[0]);
        }
      })
      .catch((err) => setError(String(err)));
    databaseApi
      .dbxListConnections()
      .then((items) => {
        setDbxConnections(items);
        if (!activeConnectionId && !items[0]) return;
      })
      .catch((err) => setError(String(err)));
    // Load once; inspect is intentionally not a dependency here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const endpoint = createInitialSqliteEndpoint();
    if (!endpoint) return;
    const endpointKey = sqliteEndpointKey(endpoint);
    if (openedInitialSqliteFilePathRef.current === endpointKey) return;
    const connection = createSqliteFileConnection(endpoint);
    openedInitialSqliteFilePathRef.current = endpointKey;
    setConnections((current) => [
      connection,
      ...current.filter((item) => sqliteEndpointKey(item.endpoint) !== endpointKey),
    ]);
    setActiveDbxConnectionId(null);
    setActiveConnectionId(connection.id);
    setWorkspaceMode("table");
    inspect(connection);
  }, [createInitialSqliteEndpoint, inspect]);

  useEffect(() => {
    if (!resizingDatabaseSidebar) return undefined;
    const originalCursor = document.body.style.cursor;
    const originalUserSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const handlePointerMove = (event: PointerEvent) => {
      const { width, x } = databaseSidebarResizeStartRef.current;
      const nextWidth = Math.min(
        DATABASE_SIDEBAR_MAX_WIDTH,
        Math.max(DATABASE_SIDEBAR_MIN_WIDTH, Math.round(width + event.clientX - x)),
      );
      setDatabaseSidebarWidth((current) => (current === nextWidth ? current : nextWidth));
    };
    const handlePointerUp = () => {
      setResizingDatabaseSidebar(false);
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    return () => {
      document.body.style.cursor = originalCursor;
      document.body.style.userSelect = originalUserSelect;
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [resizingDatabaseSidebar]);

  const startDatabaseSidebarResize = useCallback(
    (event: ReactPointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.stopPropagation();
      databaseSidebarResizeStartRef.current = {
        x: event.clientX,
        width: databaseSidebarWidth,
      };
      setResizingDatabaseSidebar(true);
    },
    [databaseSidebarWidth],
  );

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

  const openNewConnectionDialog = useCallback((connectionGroup: unknown = null) => {
    setEditingDbxConnectionId(null);
    setNewConnectionGroup(typeof connectionGroup === "string" ? connectionGroup.trim() : null);
    setError(null);
    setConnectionDialogOpen(true);
  }, []);

  const openEditDbxConnectionDialog = useCallback((connection: AeroricDbConnectionConfig) => {
    setEditingDbxConnectionId(connection.id);
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

  const addQueryHistoryEntry = useCallback(
    (entry: Omit<QueryHistoryEntry, "id" | "executedAt">) => {
      const statement = entry.sql.trim();
      if (!statement) return;
      setQueryHistory((current) => [
        {
          ...entry,
          sql: statement,
          id: `history:${Date.now()}:${Math.random().toString(36).slice(2)}`,
          executedAt: Date.now(),
        },
        ...current
          .filter((item) => item.sql !== statement || item.connectionName !== entry.connectionName)
          .slice(0, 49),
      ]);
    },
    [],
  );

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

  const openAdvancedTool = useCallback(
    (mode: DatabaseAdvancedToolMode) => {
      setWorkspaceMode(mode);
      setError(
        activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"),
      );
      setSqlResult(null);
      setQueryResult(null);
    },
    [activeDbxConnection, dbxHasSqlObjectBrowser, t],
  );

  const openUserAdmin = useCallback(() => {
    setWorkspaceMode("user-admin");
    setError(
      activeDbxConnection && supportsDbxUserAdmin(activeDbxConnection.dbType)
        ? null
        : t("database.selectUserAdminConnection"),
    );
    setSqlResult(null);
    setQueryResult(null);
  }, [activeDbxConnection, t]);

  const loadDbxColumnsForTables = useCallback(
    async (
      objects: DbxObjectInfo[],
      connection = activeDbxConnection,
      database = activeDbxDatabase,
    ) => {
      if (!connection || !isSqlDbxConnection(connection)) return;
      const nextColumns: Record<string, DbxColumnInfo[]> = {};
      for (const object of objects
        .filter((item) => isDbxTableObject(item) || isDbxViewObject(item))
        .slice(0, 12)) {
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
    setError(
      activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"),
    );
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser) return;
    let objects = dbxObjects;
    if (objects.length === 0) {
      try {
        objects = await listAllDbxObjects(activeDbxConnection.id, activeDbxDatabase, null);
        setDbxObjects(objects);
      } catch (err) {
        setError(String(err));
        return;
      }
    }
    await loadDbxColumnsForTables(objects, activeDbxConnection, activeDbxDatabase);
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    dbxHasSqlObjectBrowser,
    dbxObjects,
    loadDbxColumnsForTables,
    t,
  ]);

  const openDatabaseSearch = useCallback(async () => {
    setWorkspaceMode("database-search");
    setError(
      activeDbxConnection && dbxHasSqlObjectBrowser ? null : t("database.selectDbxSqlConnection"),
    );
    if (!activeDbxConnection || !dbxHasSqlObjectBrowser || !activeDbxDatabase) return;
    if (dbxObjects.length === 0) {
      try {
        setDbxObjects(await listAllDbxObjects(activeDbxConnection.id, activeDbxDatabase, null));
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
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    dbxHasSqlObjectBrowser,
    loadDbxColumnsForTables,
    selectedDbxTable,
    t,
  ]);

  const openDbxObjectStructure = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
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

  const confirmDbxProductionSql = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      sqlText: string,
    ): Promise<boolean> => {
      if (!hasProductionProtection(connection)) return true;
      const assessment = await databaseApi.dbxAssessProductionSql({
        connectionId: connection.id,
        database,
        sql: sqlText,
      });
      if (!assessment.requiresConfirmation) return true;
      const productionScope =
        assessment.productionDatabases.length > 0
          ? assessment.productionDatabases.join(", ")
          : t("database.productionEntireConnection");
      return confirm(
        t("database.productionSqlWarning", {
          connection: connection.name,
          databases: productionScope,
          sql: productionSqlPreview(sqlText),
        }),
        {
          title: t("database.productionWarningTitle"),
          kind: "warning",
          okLabel: t("database.execute"),
          cancelLabel: t("common.cancel"),
        },
      );
    },
    [t],
  );

  const executeSqlFileFromPanel = useCallback(async () => {
    if (!sqlFilePath.trim()) return;
    setLoading(true);
    setError(null);
    try {
      if (activeDbxConnection && dbxHasSqlObjectBrowser) {
        const fileSql = await databaseApi.readSqlFile(sqlFilePath.trim());
        const approved = await confirmDbxProductionSql(
          activeDbxConnection,
          activeDbxDatabase,
          fileSql,
        );
        if (!approved) return;
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
    confirmDbxProductionSql,
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
          listAllDbxObjects(connection.id, database, null),
          databaseApi
            .dbxListSchemas(connection.id, database)
            .then((value) => (Array.isArray(value) ? value : []))
            .catch(() => [] as string[]),
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
        const objects = await listAllDbxObjects(connection.id, database, schemaName);
        setActiveDbxDatabase(database);
        setActiveDbxSchema(schemaName);
        setDbxSchemas((current) =>
          current.includes(schemaName) ? current : [...current, schemaName].sort(),
        );
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

  const loadRedisSidebarKeys = useCallback(
    async (connection: AeroricDbConnectionConfig, database: number, append = false) => {
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
    },
    [redisScanStateByDatabase],
  );

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

  const loadMongoSidebarCollections = useCallback(
    async (connection: AeroricDbConnectionConfig, database: string) => {
      try {
        const collections = await databaseApi.dbxMongoListCollections(connection.id, database);
        setMongoCollectionsByDatabase((current) => ({
          ...current,
          [`${connection.id}:${database}`]: collections,
        }));
        return collections;
      } catch (err) {
        setError(String(err));
        return [] as string[];
      }
    },
    [],
  );

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
        const query = queryOverride ??
          mongoDocumentQueriesByCollection[key] ?? {
            filter: "{}",
            sort: "{}",
            projection: "{}",
          };
        const skip = append ? (mongoDocumentsByCollection[key]?.length ?? 0) : 0;
        const result = await databaseApi.dbxMongoFindDocuments({
          connectionId: connection.id,
          database,
          collection,
          filter: query.filter,
          projection: query.projection,
          sort: query.sort,
          skip,
          limit: MONGO_SIDEBAR_DOCUMENT_PREVIEW_LIMIT,
        });
        const nextDocuments = append
          ? [...(mongoDocumentsByCollection[key] ?? []), ...result.documents]
          : result.documents;
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
          listAllDbxObjects(connection.id, database, null),
          databaseApi
            .dbxListSchemas(connection.id, database)
            .then((value) => (Array.isArray(value) ? value : []))
            .catch(() => [] as string[]),
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

  const selectRedisSidebarDatabase = useCallback(
    (connection: AeroricDbConnectionConfig, database: number) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(`db${database}`);
      setActiveDbxSchema(null);
      setActiveMongoDocumentId(null);
      setActiveMongoWorkspaceDatabase(null);
      setWorkspaceMode("redis");
    },
    [],
  );

  const selectRedisSidebarKey = useCallback(
    (connection: AeroricDbConnectionConfig, database: number, keyRaw: string) => {
      setActiveConnectionId(null);
      setActiveDbxConnectionId(connection.id);
      setActiveDbxDatabase(`db${database}`);
      setActiveDbxSchema(keyRaw);
      setActiveMongoDocumentId(null);
      setActiveMongoWorkspaceDatabase(null);
      setWorkspaceMode("redis");
    },
    [],
  );

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
    async (
      connection: AeroricDbConnectionConfig,
      database: string,
      collection: string,
      document: unknown,
    ) => {
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
      const sameDbxObject =
        activeDbxObject?.name === object.name && activeDbxObject?.schema === object.schema;
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
            objectColumns = await databaseApi.dbxGetColumns(
              connection.id,
              object.name,
              database,
              object.schema ?? null,
            );
            setDbxColumnsByTable((current) => ({
              ...current,
              [dbxObjectKey(object)]: objectColumns,
            }));
          } catch {
            objectColumns = [];
          }
        }
        const primaryKeys = objectColumns
          .filter((column) => column.is_primary_key)
          .map((column) => column.name);
        const editable =
          isDbxTableObject(object) &&
          !connection.readOnly &&
          (primaryKeys.length > 0 || DBX_KEYLESS_GRID_EDIT_DB_TYPES.has(connection.dbType));
        const resultRows = dbxRowsToDatabaseRows(result.result.rows);
        const headerColumnTypes = result.result.columns.map((column, index) => {
          const metadataColumn = objectColumns.find(
            (item) => item.name.toLowerCase() === column.toLowerCase(),
          );
          return metadataColumn?.data_type ?? result.result.column_types?.[index] ?? "";
        });
        setActiveDbxObject(object);
        setActiveDbxSchema(object.schema ?? null);
        setWorkspaceMode("table");
        const tabId = `table:${object.name}`;
        setWorkspaceTabs((prev) =>
          prev.some((t) => t.id === tabId)
            ? prev
            : [...prev, { id: tabId, mode: "table", label: object.name, closable: true }],
        );
        setActiveTabId(tabId);
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
        initializeLoadedGrid({
          sameDbxObject,
          columns: result.result.columns,
          rows: resultRows,
          columnTypes: headerColumnTypes,
          whereInput: normalizedWhereInput,
          orderByInput: normalizedOrderBy,
        });
        setSql(result.sql);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      dbxGridPageSize,
      initializeLoadedGrid,
    ],
  );

  const handleConnectionSaved = useCallback(
    (next: AeroricDbConnectionConfig[], connection: AeroricDbConnectionConfig) => {
      setDbxConnections(next);
      setEditingDbxConnectionId(null);
      return loadDbxConnection(connection);
    },
    [loadDbxConnection],
  );

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
    await loadDbxObject(activeDbxObject, 1, activeDbxConnection, activeDbxDatabase, "", "");
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
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

  const loadVisibleDatabaseNames = useCallback(
    async (connection: AeroricDbConnectionConfig): Promise<string[]> => {
      await databaseApi.dbxConnect(connection.id);
      if (connection.dbType === "redis") {
        return (await databaseApi.dbxRedisListDatabases(connection.id)).map((database) =>
          String(database.db),
        );
      }
      if (connection.dbType === "mongodb") {
        return databaseApi.dbxMongoListDatabases(connection.id);
      }
      return (await databaseApi.dbxListDatabases(connection.id)).map((database) => database.name);
    },
    [],
  );

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
        setVisibleDatabaseShowSystem(
          initialSelection.some((name) => isSystemDatabaseName(connection.dbType, name)),
        );
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
      const currentDbx =
        connection.dbx && typeof connection.dbx === "object"
          ? (connection.dbx as Record<string, unknown>)
          : {};
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
      const currentDbx =
        connection.dbx && typeof connection.dbx === "object"
          ? (connection.dbx as Record<string, unknown>)
          : {};
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
    const normalized = normalizeVisibleDatabaseSelection(
      [...visibleDatabaseSelection],
      visibleDatabaseNames,
    );
    await saveVisibleDatabaseConfig(visibleDatabaseConnection, normalized);
  }, [
    saveVisibleDatabaseConfig,
    visibleDatabaseCanSave,
    visibleDatabaseConnection,
    visibleDatabaseNames,
    visibleDatabaseSelection,
  ]);

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
        const objects = await listAllDbxObjects(connection.id, database, schema, {
          objectTypes: DBX_TABLE_LIKE_OBJECT_TYPES,
        });
        const tableNames = Array.from(
          new Set(
            objects
              .filter((object) => isDbxTableObject(object) || isDbxViewObject(object))
              .map((object) => object.name)
              .filter(Boolean),
          ),
        ).sort((left, right) =>
          left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }),
        );
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
    const safeName =
      (databaseExportTarget.database || "database").replace(/[\\/:*?"<>|]+/g, "_").trim() ||
      "database";
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
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
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
        const columns = await databaseApi.dbxGetColumns(
          connection.id,
          object.name,
          database,
          object.schema ?? null,
        );
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
    if (
      !tableImportConnection ||
      !tableImportTarget ||
      !tableImportPreview ||
      tableImportMappedColumns.length === 0
    )
      return;
    setLoading(true);
    setTableImportLoading(true);
    setError(null);
    setTableImportError("");
    try {
      const tableName = tableImportTarget.object.schema
        ? `${tableImportTarget.object.schema}.${tableImportTarget.object.name}`
        : tableImportTarget.object.name;
      const approved = await confirmDbxProductionOperation({
        connection: tableImportConnection,
        database: tableImportTarget.database,
        operation: t("database.productionTableImportOperation", {
          table: tableName,
          mode:
            tableImportMode === "truncate"
              ? t("database.tableImportTruncate")
              : t("database.tableImportAppend"),
        }),
        okLabel: t("database.tableImport"),
        t,
      });
      if (!approved) return;
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
    t,
  ]);

  const renameLegacyConnection = useCallback(
    (connection: DbConnectionConfig) => {
      const nextName = window.prompt(t("database.renameConnectionPrompt"), connection.name)?.trim();
      if (!nextName || nextName === connection.name) return;
      saveConnections(
        connections.map((item) => (item.id === connection.id ? { ...item, name: nextName } : item)),
      );
    },
    [connections, saveConnections, t],
  );

  const renameDbxConnection = useCallback(
    async (connection: AeroricDbConnectionConfig) => {
      const nextName = window.prompt(t("database.renameConnectionPrompt"), connection.name)?.trim();
      if (!nextName || nextName === connection.name) return;
      const currentDbx =
        connection.dbx && typeof connection.dbx === "object"
          ? (connection.dbx as Record<string, unknown>)
          : {};
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

  const saveDbxConnectionMetadata = useCallback(
    async (connection: AeroricDbConnectionConfig, patch: Partial<AeroricDbConnectionConfig>) => {
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
    },
    [],
  );

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
      const next = [...current, normalized].sort((left, right) =>
        left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }),
      );
      saveExtraDbxConnectionGroups(next);
      return next;
    });
  }, []);

  const renameExtraDbxConnectionGroup = useCallback((oldName: string, newName: string) => {
    setExtraDbxConnectionGroups((current) => {
      const next = Array.from(
        new Set(
          current
            .map((group) => (group.trim() === oldName ? newName : group))
            .filter((group) => group.trim().length > 0),
        ),
      ).sort((left, right) =>
        left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }),
      );
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
      const nextGroup = window
        .prompt(t("database.connectionGroupPrompt"), connection.connectionGroup ?? "")
        ?.trim();
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
            .map((connection) =>
              databaseApi.dbxSaveConnection({ ...connection, connectionGroup: nextGroup }),
            ),
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
            .map((connection) =>
              databaseApi.dbxSaveConnection({ ...connection, connectionGroup: null }),
            ),
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
          connection.dbx && typeof connection.dbx === "object"
            ? (connection.dbx as Record<string, unknown>)
            : {};
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
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
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
        const ok = await confirm(
          `${t("database.confirmDropTable", { name: dbxObjectKey(object) })}\n\n${sql}`,
          {
            title: t("database.dropTable"),
            kind: "warning",
            okLabel: t("database.dropTable"),
            cancelLabel: t("common.cancel"),
          },
        );
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
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
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
        const ddl = await databaseApi.dbxGetTableDdl(
          connection.id,
          object.name,
          database,
          object.schema ?? null,
        );
        setSql(ddl);
        setSqlResult(
          dbxQueryToExecuteResult({
            columns: ["ddl"],
            column_types: ["text"],
            column_sortables: [false],
            rows: [[ddl]],
            affected_rows: 0,
            execution_time_ms: 0,
            truncated: false,
            has_more: false,
          }),
        );
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
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
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
        const source = await databaseApi.dbxGetObjectSource(
          connection.id,
          database,
          schema,
          object.name,
          objectType,
          object.signature ?? null,
        );
        setSql(source.source);
        setSqlResult(
          dbxQueryToExecuteResult({
            columns: ["source"],
            column_types: ["text"],
            column_sortables: [false],
            rows: [[source.source]],
            affected_rows: 0,
            execution_time_ms: 0,
            truncated: false,
            has_more: false,
          }),
        );
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
    (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      mode: "select" | "insert" | "update",
    ) => {
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
      requestOverrides: Partial<
        Pick<
          TableExportRequest,
          "columns" | "columnTypes" | "primaryKeys" | "whereInput" | "orderBy"
        >
      > = {},
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

  const exportActiveDbxGrid = useCallback(
    async (format: TableExportFormat = dbxGridExportFormat) => {
      if (
        !activeDbxConnection ||
        !activeDbxObject ||
        !queryResult ||
        visibleTableColumns.length === 0
      )
        return;
      const columns = visibleTableColumns.map(({ column }) => column);
      const columnTypes = columns.map((column) => {
        const metadata = activeObject?.columns.find(
          (item) => item.name.toLowerCase() === column.toLowerCase(),
        );
        return metadata?.dataType ?? null;
      });
      await exportDbxTableObject(activeDbxConnection, activeDbxDatabase, activeDbxObject, format, {
        columns,
        columnTypes,
        primaryKeys: activeObject?.primaryKeys ?? [],
        whereInput: dbxGridWhereInput.trim() || null,
        orderBy: dbxGridOrderByInput.trim() || null,
      });
    },
    [
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
    ],
  );

  const copyDbxObjectStructure = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
      format: "markdown" | "tsv",
    ) => {
      setLoading(true);
      setError(null);
      try {
        const columns = await databaseApi.dbxGetColumns(
          connection.id,
          object.name,
          database,
          object.schema ?? null,
        );
        const text =
          format === "markdown"
            ? [
                "| Column | Type | Nullable | Primary key |",
                "| --- | --- | --- | --- |",
                ...columns.map(
                  (column) =>
                    `| ${column.name} | ${column.data_type ?? ""} | ${column.is_nullable ? "yes" : "no"} | ${column.is_primary_key ? "yes" : "no"} |`,
                ),
              ].join("\n")
            : [
                "Column\tType\tNullable\tPrimary key",
                ...columns.map(
                  (column) =>
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
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      const exportDatabase = database || activeDbxDatabase;
      if (!exportDatabase) return;
      await openDatabaseExportDialog(connection, exportDatabase, object.schema ?? null, [
        object.name,
      ]);
    },
    [activeDbxDatabase, openDatabaseExportDialog],
  );

  const copyDbxObjectStructureDdl = useCallback(
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
      if (!isSqlDbxConnection(connection)) return;
      setLoading(true);
      setError(null);
      try {
        const ddl = await databaseApi.dbxGetTableDdl(
          connection.id,
          object.name,
          database,
          object.schema ?? null,
        );
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
    async (
      connection: AeroricDbConnectionConfig,
      database: string | null,
      object: DbxObjectInfo,
    ) => {
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
        const ok = await confirm(
          `${t(dbxObjectDropConfirmLabelKey(object), { name: dbxObjectKey(object) })}\n\n${sql}`,
          {
            title,
            kind: "warning",
            okLabel: title,
            cancelLabel: t("common.cancel"),
          },
        );
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
      const actionConfig: Record<
        TableChildObjectType,
        { title: string; message: string; okLabel: string }
      > = {
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
      await dropDbxTableChildObjectByName(
        connection,
        database,
        object,
        childObjectType,
        childObject.name,
      );
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
        const newName = window
          .prompt(t("database.renameObjectNamePrompt"), menu.object.name)
          ?.trim();
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
          const ok = await confirm(
            `${t("database.confirmRenameObject", { oldName: dbxObjectKey(menu.object), newName })}\n\n${sql}`,
            {
              title: t("database.renameObject"),
              kind: "warning",
              okLabel: t("database.renameObject"),
              cancelLabel: t("common.cancel"),
            },
          );
          if (!ok) return;
          await databaseApi.dbxExecuteQuery({
            connectionId: connection.id,
            database: menu.database,
            schema: menu.object.schema ?? null,
            sql,
          });
          await loadDbxConnection(connection);
          if (
            activeDbxObject?.name === menu.object.name &&
            activeDbxObject?.schema === menu.object.schema
          ) {
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
        const tabId = `table-info:${menu.object.name}`;
        setWorkspaceTabs((prev) =>
          prev.some((t) => t.id === tabId)
            ? prev
            : [
                ...prev,
                {
                  id: tabId,
                  mode: "table-info",
                  label: `${t("database.tableProperties")}: ${menu.object.name}`,
                  closable: true,
                },
              ],
        );
        setActiveTabId(tabId);
        await loadDbxColumnsForTables([menu.object], connection, menu.database);
        await loadTableInfoDdlForObject(connection, menu.database, menu.object);
        return;
      }
      if (
        action === "newQuery" ||
        action === "newSqlSelect" ||
        action === "newSqlInsert" ||
        action === "newSqlUpdate"
      ) {
        writeDbxObjectSqlDraft(
          connection,
          menu.database,
          menu.object,
          action === "newQuery" || action === "newSqlSelect"
            ? "select"
            : action === "newSqlInsert"
              ? "insert"
              : "update",
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
        await openDatabaseExportDialog(connection, exportDatabase, menu.object.schema ?? null, [
          menu.object.name,
        ]);
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
        await copyDbxObjectStructure(
          connection,
          menu.database,
          menu.object,
          action === "copyStructureTsv" ? "tsv" : "markdown",
        );
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
        const defaultName = uniqueDbxObjectName(
          `${menu.object.name}_copy`,
          menu.object.schema,
          dbxObjects,
        );
        const targetName = window
          .prompt(t("database.duplicateStructureNamePrompt"), defaultName)
          ?.trim();
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
          const ok = await confirm(
            `${t("database.confirmDuplicateStructure", { source: dbxObjectKey(menu.object), target: targetName })}\n\n${sql}`,
            {
              title: t("database.duplicateStructure"),
              kind: "warning",
              okLabel: t("database.duplicateStructure"),
              cancelLabel: t("common.cancel"),
            },
          );
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
      loadTableInfoDdlForObject,
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
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxSchemaConnection =
    contextMenu?.kind === "dbx-schema"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxObjectConnection =
    contextMenu?.kind === "dbx-object"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxColumnConnection =
    contextMenu?.kind === "dbx-column"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxTableChildConnection =
    contextMenu?.kind === "dbx-table-child"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxObjectGroupConnection =
    contextMenu?.kind === "dbx-object-group"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuNoSqlConnection =
    contextMenu?.kind === "redis-database" ||
    contextMenu?.kind === "redis-key" ||
    contextMenu?.kind === "mongo-database" ||
    contextMenu?.kind === "mongo-collection" ||
    contextMenu?.kind === "mongo-document"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;

  const createSchemaConnection = useMemo(
    () =>
      dbxConnections.find((connection) => connection.id === createSchemaTarget?.connectionId) ??
      null,
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
  }, [
    closeCreateSchemaDialog,
    createSchemaConnection,
    createSchemaName,
    createSchemaTarget,
    loadDbxDatabase,
  ]);

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
        const ok = await confirm(
          `${t("database.confirmDropDatabase", { name: database })}\n\n${sql}`,
          {
            title: t("database.dropDatabase"),
            kind: "warning",
            okLabel: t("database.dropDatabase"),
            cancelLabel: t("common.cancel"),
          },
        );
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
        const ok = await confirm(
          `${t("database.confirmDropSchema", { name: schemaName })}\n\n${sql}`,
          {
            title: t("database.dropSchema"),
            kind: "warning",
            okLabel: t("database.dropSchema"),
            cancelLabel: t("common.cancel"),
          },
        );
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
        setWorkspaceMode(
          action === "dataTransfer"
            ? "transfer"
            : action === "schemaDiff"
              ? "schema-diff"
              : "data-compare",
        );
        return;
      }
      if (action === "openErDiagram" || action === "databaseSearch") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(null);
        setWorkspaceMode(action === "openErDiagram" ? "er-diagram" : "database-search");
        try {
          const objects = await listAllDbxObjects(connection.id, menu.database, null);
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
        setWorkspaceMode(
          action === "dataTransfer"
            ? "transfer"
            : action === "schemaDiff"
              ? "schema-diff"
              : "data-compare",
        );
        return;
      }
      if (action === "openErDiagram" || action === "databaseSearch") {
        setActiveConnectionId(null);
        setActiveDbxConnectionId(connection.id);
        setActiveDbxDatabase(menu.database);
        setActiveDbxSchema(menu.schema);
        setWorkspaceMode(action === "openErDiagram" ? "er-diagram" : "database-search");
        try {
          const objects = await listAllDbxObjects(connection.id, menu.database, menu.schema);
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
    [
      contextMenu,
      copyNodeName,
      dbxConnections,
      dropDbxSchema,
      loadDbxColumnsForTables,
      loadDbxSchema,
      openDatabaseExportDialog,
      openQueryHistory,
      togglePinnedTreeNode,
    ],
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
        setSql(
          action === "createTable"
            ? dbxCreateTableDraft(menu.schema)
            : dbxCreateViewDraft(menu.schema),
        );
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
          await selectMongoSidebarDocument(
            connection,
            menu.database,
            menu.collection,
            menu.document,
          );
          return;
        }
        if (action === "refresh") {
          await loadMongoSidebarDocuments(connection, menu.database, menu.collection);
          return;
        }
        const rawId = mongoDocumentRawId(menu.document);
        if (connection.readOnly || rawId == null) return;
        const ok = await confirm(
          t("database.confirmDeleteMongoDocument", { collection: menu.collection, id: documentId }),
          {
            title: t("database.mongoDeleteDocument"),
            kind: "warning",
            okLabel: t("database.mongoDeleteDocument"),
            cancelLabel: t("common.cancel"),
          },
        );
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
      const menuConnectionId = contextMenuConnectionId(menu);
      if (!menuConnectionId) return;
      const legacy = connections.find((connection) => connection.id === menuConnectionId) ?? null;
      const dbx = dbxConnections.find((connection) => connection.id === menuConnectionId) ?? null;

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
        setError(
          dbx && supportsDbxUserAdmin(dbx.dbType) ? null : t("database.selectUserAdminConnection"),
        );
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
        if (activeDbxConnectionId === menuConnectionId) {
          setActiveDbxConnectionId(null);
          setDbxDatabases([]);
          setDbxObjects([]);
          setActiveDbxDatabase(null);
          setActiveDbxObject(null);
        }
        if (activeConnectionId === menuConnectionId) {
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
        const childName = window
          .prompt(t("database.newConnectionGroupPrompt"), t("database.newConnectionGroupDefault"))
          ?.trim();
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
    [
      addExtraDbxConnectionGroup,
      contextMenu,
      copyNodeName,
      deleteDbxConnectionGroup,
      openNewConnectionDialog,
      renameDbxConnectionGroup,
      t,
    ],
  );

  const runSql = useCallback(async () => {
    if (!activeEndpoint && !activeDbxConnection) return;
    setLoading(true);
    setError(null);
    try {
      setWorkspaceMode("query");
      if (activeDbxConnection && dbxHasSqlObjectBrowser) {
        const approved = await confirmDbxProductionSql(activeDbxConnection, activeDbxDatabase, sql);
        if (!approved) return;
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
    confirmDbxProductionSql,
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
          setDbxSqlPreviewStatements(preview.statements);
          setDbxSqlPreviewRollback(preview.rollbackStatements);
          setDbxSqlPreviewDescription(t("database.confirmUpdateCell", { column }));
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

  const saveDbxPendingCellEdits = useCallback(async () => {
    if (
      !activeDbxConnection ||
      !activeDbxObject ||
      !queryResult ||
      !activeObject ||
      activeDbxConnection.readOnly ||
      !queryResult.editable
    )
      return;
    if (Object.keys(dbxPendingCellEdits).length === 0) return;
    const options = buildDbxGridSaveOptions({
      dirtyRows: dbxPendingCellEditsToDirtyRows(dbxPendingCellEdits, textToCellValue),
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
      setDbxSqlPreviewDescription(t("database.updateCell"));
      const rollback = preview.rollbackStatements.length
        ? `\n\n${t("database.gridRollbackSql")}\n${preview.rollbackStatements.join("\n")}`
        : "";
      const ok = await confirm(
        `${t("database.updateCell")}\n\n${preview.statements.join("\n")}${rollback}`,
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
      setDbxPendingCellEdits({});
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
  }, [
    activeDbxConnection,
    activeDbxDatabase,
    activeDbxObject,
    activeObject,
    buildDbxGridSaveOptions,
    dbxGridOrderByInput,
    dbxGridWhereInput,
    dbxPendingCellEdits,
    loadDbxObject,
    page,
    queryResult,
    setDbxPendingCellEdits,
    t,
  ]);

  const insertRow = useCallback(async () => {
    if (activeDbxConnection && activeDbxObject && queryResult) {
      if (!activeObject || !isDbxTableObject(activeDbxObject) || activeDbxConnection.readOnly)
        return;
      const metadataColumns = dbxColumnsByTable[dbxObjectKey(activeDbxObject)] ?? [];
      const metadataByName = new Map(
        metadataColumns.map((column) => [column.name.toLowerCase(), column]),
      );
      const sample = Object.fromEntries(
        queryResult.columns
          .filter((column) => !metadataByName.get(column.toLowerCase())?.column_default)
          .map((column) => [column, null]),
      );
      const raw = window.prompt(t("database.insertJsonPrompt"), JSON.stringify(sample));
      if (!raw) return;
      try {
        const parsed = JSON.parse(raw) as Record<string, unknown>;
        const newRow = queryResult.columns.map((column) =>
          Object.prototype.hasOwnProperty.call(parsed, column) ? (parsed[column] ?? null) : null,
        );
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
        const ok = await confirm(
          `${t("database.confirmInsertRow")}\n\n${preview.statements.join("\n")}${rollback}`,
          {
            title: t("database.insertRow"),
            kind: "warning",
            okLabel: t("database.insert"),
            cancelLabel: t("common.cancel"),
          },
        );
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
      loadDbxObject,
      page,
      queryResult,
      setDbxGridSelectedRows,
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

  const runDbxGridHeaderContextMenuAction = useCallback(
    async (action: DbxGridHeaderContextMenuAction) => {
      const menu: DbxGridHeaderContextMenuState | null =
        contextMenu?.kind === "dbx-grid-header" ? contextMenu : null;
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
        await loadDbxObject(
          activeDbxObject,
          1,
          activeDbxConnection,
          activeDbxDatabase,
          dbxGridWhereInput,
          nextOrderBy,
        );
      }
    },
    [
      activeDbxConnection,
      activeDbxDatabase,
      activeDbxObject,
      contextMenu,
      copyNodeName,
      dbxGridWhereInput,
      loadDbxObject,
      queryResult,
      setDbxColumnPreview,
      setDbxColumnPreviewSearch,
      setDbxGridOrderByInput,
    ],
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
        await loadDbxObject(
          activeDbxObject,
          1,
          activeDbxConnection,
          activeDbxDatabase,
          dbxGridWhereInput,
          nextOrderBy,
        );
        return;
      }
      if (action === "clearFilter") {
        setDbxGridWhereInput("");
        if (activeDbxConnection && activeDbxObject) {
          await loadDbxObject(
            activeDbxObject,
            1,
            activeDbxConnection,
            activeDbxDatabase,
            "",
            dbxGridOrderByInput,
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
          setError(String(err));
        }
        return;
      }
      if (action === "copyAllTsv") {
        if (!queryResult || visibleTableColumns.length === 0) return;
        try {
          await navigator.clipboard?.writeText(
            dbxGridRowsToTsv(visibleTableColumns, queryResult.rows),
          );
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
      setDbxCellPreview,
      setDbxColumnPreview,
      setDbxColumnPreviewSearch,
      setDbxGridOrderByInput,
      setDbxGridWhereInput,
      setDbxRowPreview,
      setDbxRowPreviewSearch,
      visibleTableColumns,
    ],
  );

  const contextMenuDbxConnection =
    contextMenu?.kind === "dbx" || contextMenu?.kind === "user-admin"
      ? (dbxConnections.find((connection) => connection.id === contextMenu.connectionId) ?? null)
      : null;
  const contextMenuDbxConnectionHasMoveTargets = contextMenuDbxConnection
    ? Boolean(contextMenuDbxConnection.connectionGroup?.trim()) ||
      extraDbxConnectionGroups.some((group) => group.trim().length > 0) ||
      dbxConnections.some(
        (connection) =>
          connection.id !== contextMenuDbxConnection.id &&
          Boolean(connection.connectionGroup?.trim()),
      )
    : false;
  const contextMenuConnectionActive =
    contextMenu?.kind === "legacy"
      ? activeConnectionId === contextMenu.connectionId
      : contextMenu?.kind === "dbx"
        ? activeDbxConnectionId === contextMenu.connectionId
        : false;
  const currentContextMenuPinnedNodeId = contextMenuPinnedNodeId(contextMenu);
  const contextMenuTreeNodePinned = currentContextMenuPinnedNodeId
    ? pinnedTreeNodeIds.has(currentContextMenuPinnedNodeId)
    : false;
  const activeDbxGridPrimaryKeys = activeObject?.primaryKeys ?? queryResult?.primaryKeys ?? [];
  const canInsertActiveTable =
    workspaceMode === "table" &&
    Boolean(queryResult) &&
    (activeDbxConnection
      ? Boolean(
          activeDbxObject &&
          isDbxTableObject(activeDbxObject) &&
          activeObject &&
          !activeDbxConnection.readOnly,
        )
      : Boolean(activeObject?.objectType === "table" && !activeConnection?.readOnly));
  const hasActiveDatabaseWorkspace =
    Boolean(activeObject || activeDbxObject || queryResult || sqlResult) ||
    (workspaceMode !== "table" && workspaceMode !== "query");
  const hideDatabaseWorkspaceTopbar =
    !hasActiveDatabaseWorkspace ||
    workspaceMode === "drivers" ||
    workspaceMode === "transfer" ||
    workspaceMode === "schema-diff" ||
    workspaceMode === "data-compare";
  const databaseWorkspaceTitle = (mode: DbWorkspaceMode) => {
    switch (mode) {
      case "query":
        return t("database.newQuery");
      case "sql-file":
        return t("database.executeSqlFile");
      case "query-history":
        return t("database.queryHistory");
      case "drivers":
        return t("database.driverManager");
      case "redis":
        return "Redis";
      case "mongo":
        return "MongoDB";
      case "transfer":
        return t("database.dataTransfer");
      case "schema-diff":
        return t("database.schemaDiff");
      case "data-compare":
        return t("database.dataCompare");
      case "user-admin":
        return t("database.userAdmin");
      case "er-diagram":
        return t("database.erDiagram");
      case "database-search":
        return t("database.databaseSearch");
      case "table-structure":
        return t("database.tableStructure");
      case "table-info":
        return t("database.tableInfo");
      default:
        return activeObject?.name ?? t("database.noSelection");
    }
  };

  return (
    <div
      style={{ ...s.databaseRoot, gridTemplateColumns: `${databaseSidebarWidth}px minmax(0, 1fr)` }}
    >
      <div style={s.databaseTopToolbar}>
        <DbxButton variant="ghost" size="sm" icon={Database} onClick={openNewConnectionDialog}>
          {t("database.newConnection")}
        </DbxButton>
        <DbxButton
          variant="ghost"
          size="sm"
          icon={FilePlus}
          onClick={handleNewQuery}
          disabled={!activeSqlCapable}
        >
          {t("database.newQuery")}
        </DbxButton>
        <DbxButton
          variant="ghost"
          size="sm"
          icon={FileCode}
          onClick={handleExecuteSqlFile}
          disabled={loading}
        >
          {t("database.executeSqlFile")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={Wrench} onClick={openDriverManager}>
          {t("database.driverManager")}
        </DbxButton>
        <DbxButton
          variant="ghost"
          size="sm"
          icon={GitMerge}
          onClick={() => openAdvancedTool("transfer")}
        >
          {t("database.dataTransfer")}
        </DbxButton>
        <DbxButton
          variant="ghost"
          size="sm"
          icon={GitCompare}
          onClick={() => openAdvancedTool("schema-diff")}
        >
          {t("database.schemaDiff")}
        </DbxButton>
        <DbxButton
          variant="ghost"
          size="sm"
          icon={Network}
          onClick={() => openAdvancedTool("data-compare")}
        >
          {t("database.dataCompare")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={UsersRound} onClick={openUserAdmin}>
          {t("database.userAdmin")}
        </DbxButton>
        <DbxButton variant="ghost" size="sm" icon={Table2} onClick={() => void openErDiagram()}>
          {t("database.erDiagram")}
        </DbxButton>
        <DbxButton
          variant="ghost"
          size="sm"
          icon={Search}
          onClick={() => void openDatabaseSearch()}
        >
          {t("database.databaseSearch")}
        </DbxButton>
        <DbxButton
          variant="ghost"
          size="sm"
          icon={SlidersHorizontal}
          onClick={() => void openTableStructure()}
        >
          {t("database.tableStructure")}
        </DbxButton>
      </div>
      <aside style={{ ...s.databaseSidebar, width: databaseSidebarWidth }}>
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
              setError(
                supportsDbxUserAdmin(connection.dbType)
                  ? null
                  : t("database.selectUserAdminConnection"),
              );
              setSqlResult(null);
              setQueryResult(null);
            })();
          }}
          onOpenNoSqlWorkspace={() => {
            if (activeDbxConnection)
              setWorkspaceMode(activeDbxConnection.dbType === "redis" ? "redis" : "mongo");
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
          onDbxTableChildObjectContextMenu={(
            event,
            connectionId,
            database,
            object,
            childObject,
          ) => {
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
        <button
          type="button"
          role="separator"
          aria-label={t("database.resizeSidebar")}
          aria-orientation="vertical"
          title={t("database.resizeSidebar")}
          onPointerDown={startDatabaseSidebarResize}
          style={{
            ...s.databaseSidebarResizeHandle,
            ...(resizingDatabaseSidebar ? s.databaseSidebarResizeHandleActive : undefined),
          }}
        />
      </aside>

      <main style={s.databaseMain}>
        {workspaceTabs.length > 0 && (
          <div style={s.databaseTabBar}>
            {workspaceTabs.map((tab) => (
              <button
                key={tab.id}
                role="tab"
                aria-selected={activeTabId === tab.id}
                type="button"
                style={{
                  ...s.databaseTab,
                  ...(activeTabId === tab.id ? s.databaseTabActive : undefined),
                }}
                title={tab.label}
                onClick={() => {
                  activateWorkspaceTab(tab);
                }}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setContextMenu({
                    x: event.clientX,
                    y: event.clientY,
                    tabId: tab.id,
                    kind: "workspace-tab",
                  });
                }}
              >
                <span
                  style={{
                    maxWidth: shortWorkspaceTabIds.has(tab.id) ? 72 : 160,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {tab.label}
                </span>
                {tab.closable && (
                  <span
                    role="button"
                    tabIndex={-1}
                    style={s.databaseTabClose}
                    onClick={(e) => {
                      e.stopPropagation();
                      closeWorkspaceTab(tab.id);
                    }}
                  >
                    ×
                  </span>
                )}
              </button>
            ))}
          </div>
        )}

        {workspaceTabs.length === 0 && !hideDatabaseWorkspaceTopbar && (
          <div style={s.databaseTopbar}>
            <div style={{ minWidth: 0 }}>
              <div style={s.databaseTitle}>{databaseWorkspaceTitle(workspaceMode)}</div>
              <div style={s.databasePath}>
                {activeEndpoint
                  ? endpointLabel(activeEndpoint)
                  : activeDbxConnection
                    ? `${activeDbxConnection.dbType}: ${activeDbxConnection.name}`
                    : t("database.chooseConnection")}
              </div>
            </div>
            {error && (
              <div style={s.databaseError} title={error}>
                {error}
              </div>
            )}
          </div>
        )}

        {workspaceMode === "query-history" ? (
          <div style={s.databaseWorkspacePanel}>
            <div style={s.databaseWorkspaceHeader}>
              <div>
                <div style={s.databaseWorkspaceTitle}>{t("database.queryHistory")}</div>
                <div style={s.databaseDialogHint}>{t("database.queryHistoryHint")}</div>
              </div>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Trash2}
                onClick={() => setQueryHistory([])}
                disabled={queryHistory.length === 0}
              >
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
                      <span style={{ color: "var(--text-secondary)", fontWeight: 600 }}>
                        {entry.connectionName}
                      </span>
                      {entry.database && (
                        <span style={{ color: "var(--text-hint)" }}>{entry.database}</span>
                      )}
                      {entry.schema && (
                        <span style={{ color: "var(--text-hint)" }}>{entry.schema}</span>
                      )}
                      <span style={{ marginLeft: "auto", color: "var(--text-hint)", fontSize: 11 }}>
                        {new Date(entry.executedAt).toLocaleString()}
                      </span>
                    </div>
                    <pre style={{ ...s.databaseSqlPreview, margin: 0, maxHeight: 86 }}>
                      {entry.sql}
                    </pre>
                    <div
                      style={{ display: "flex", gap: 10, color: "var(--text-hint)", fontSize: 11 }}
                    >
                      {entry.rowsAffected != null && (
                        <span>
                          {t("database.historyRowsAffected", { rows: entry.rowsAffected })}
                        </span>
                      )}
                      {entry.executionTimeMs != null && (
                        <span>
                          {t("database.historyExecutionTime", { ms: entry.executionTimeMs })}
                        </span>
                      )}
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
              <DbxButton
                variant="outline"
                size="sm"
                icon={FileCode}
                onClick={chooseSqlFile}
                disabled={loading}
              >
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
              <DbxButton
                variant="outline"
                size="sm"
                icon={RefreshCcw}
                onClick={openDriverManager}
                disabled={loading}
              >
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
            connection={activeDbxConnection}
            readOnly={activeDbxConnection.readOnly}
            initialDb={
              activeDbxDatabase?.startsWith("db") ? Number(activeDbxDatabase.slice(2)) : undefined
            }
            initialKey={activeDbxSchema ?? undefined}
            keySeparator={dbxString(
              dbxConfigRecord(activeDbxConnection),
              "redis_key_separator",
              ":",
            )}
          />
        ) : workspaceMode === "mongo" && activeDbxConnection ? (
          <MongoBrowser
            connectionId={activeDbxConnection.id}
            connection={activeDbxConnection}
            readOnly={activeDbxConnection.readOnly}
            initialDatabase={activeMongoWorkspaceDatabase ?? undefined}
            initialCollection={
              activeMongoWorkspaceDatabase ? (activeDbxSchema ?? undefined) : undefined
            }
            initialDocumentId={
              activeMongoWorkspaceDatabase ? (activeMongoDocumentId ?? undefined) : undefined
            }
            onDocumentsQueryApplied={(database, collection, filter, sort, projection) => {
              const query = { filter, sort, projection };
              const key = `${activeDbxConnection.id}:${database}:${collection}`;
              setMongoDocumentQueriesByCollection((current) => ({ ...current, [key]: query }));
              void loadMongoSidebarDocuments(
                activeDbxConnection,
                database,
                collection,
                false,
                query,
              );
            }}
          />
        ) : (workspaceMode === "transfer" ||
            workspaceMode === "schema-diff" ||
            workspaceMode === "data-compare") &&
          (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
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
        ) : (workspaceMode === "transfer" ||
            workspaceMode === "schema-diff" ||
            workspaceMode === "data-compare") &&
          activeDbxConnection ? (
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
        ) : workspaceMode === "user-admin" &&
          (!activeDbxConnection || !supportsDbxUserAdmin(activeDbxConnection.dbType)) ? (
          <GuidancePanel
            title={t("database.userAdmin")}
            message={t("database.selectUserAdminConnection")}
          />
        ) : workspaceMode === "user-admin" && activeDbxConnection ? (
          <DatabaseUserAdminPanel
            connection={activeDbxConnection}
            database={activeDbxDatabase}
            schema={activeDbxSchema}
          />
        ) : workspaceMode === "er-diagram" && (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
          <GuidancePanel
            title={t("database.erDiagram")}
            message={t("database.selectDbxSqlConnection")}
          />
        ) : workspaceMode === "er-diagram" && activeDbxConnection ? (
          <ErDiagramPanel tables={dbxTableObjects} columnsByTable={dbxColumnsByTable} />
        ) : workspaceMode === "database-search" &&
          (!activeDbxConnection || !dbxHasSqlObjectBrowser || !activeDbxDatabase) ? (
          <GuidancePanel
            title={t("database.databaseSearch")}
            message={t("database.selectDbxSqlConnection")}
          />
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
        ) : workspaceMode === "table-structure" &&
          (!activeDbxConnection || !dbxHasSqlObjectBrowser) ? (
          <GuidancePanel
            title={t("database.tableStructure")}
            message={t("database.selectDbxTable")}
          />
        ) : workspaceMode === "table-structure" && !selectedDbxTable ? (
          <GuidancePanel
            title={t("database.tableStructure")}
            message={t("database.selectDbxTable")}
          />
        ) : workspaceMode === "table-structure" && selectedDbxTable ? (
          <TableStructurePanel
            connectionId={activeDbxConnection?.id}
            database={activeDbxDatabase}
            schema={selectedDbxTable.schema ?? null}
            databaseType={activeDbxConnection?.dbType ?? null}
            tableName={selectedDbxTable.name}
            columns={
              dbxColumnsByTable[
                selectedDbxTable.schema
                  ? `${selectedDbxTable.schema}.${selectedDbxTable.name}`
                  : selectedDbxTable.name
              ] ?? []
            }
            readOnly={activeDbxConnection?.readOnly ?? true}
          />
        ) : workspaceMode === "table-info" &&
          (!activeDbxConnection || !dbxHasSqlObjectBrowser || !selectedDbxInfoObject) ? (
          <GuidancePanel title={t("database.tableInfo")} message={t("database.selectDbxTable")} />
        ) : workspaceMode === "table-info" && selectedDbxInfoObject ? (
          <div style={s.databaseTableInfoRoot}>
            <div style={s.databaseTableInfoHeader}>
              <span
                style={{ position: "relative", display: "flex", alignItems: "center", flex: 1 }}
              >
                <Search
                  aria-hidden="true"
                  size={14}
                  style={{ position: "absolute", left: 9, color: "var(--text-hint)" }}
                />
                <input
                  style={{ ...s.databaseDialogInput, paddingLeft: 30, minWidth: 220 }}
                  value={tableInfoSearch}
                  onChange={(event) => setTableInfoSearch(event.target.value)}
                  placeholder={t("database.searchPlaceholder")}
                  aria-label="Search table info"
                />
              </span>
              <DbxButton
                variant="outline"
                size="sm"
                icon={FileCode}
                onClick={() =>
                  void showDbxObjectDdl(
                    activeDbxConnection!,
                    activeDbxDatabase,
                    selectedDbxInfoObject,
                  )
                }
              >
                {t("database.viewDdl")}
              </DbxButton>
            </div>
            <div role="tablist" aria-label="Table info sections" style={s.databaseTableInfoTabs}>
              {[
                {
                  key: "columns" as const,
                  label: t("database.columns"),
                  count: selectedDbxInfoColumns.length,
                  icon: <Columns3 size={14} aria-hidden="true" />,
                },
                {
                  key: "indexes" as const,
                  label: t("database.indexes"),
                  count: selectedDbxInfoIndexes.length,
                  icon: <Hash size={14} aria-hidden="true" />,
                },
                {
                  key: "foreignKeys" as const,
                  label: t("database.foreignKeys"),
                  count: selectedDbxInfoForeignKeys.length,
                  icon: <KeyRound size={14} aria-hidden="true" />,
                },
                {
                  key: "triggers" as const,
                  label: t("database.triggers"),
                  count: selectedDbxInfoTriggers.length,
                  icon: <Zap size={14} aria-hidden="true" />,
                },
                {
                  key: "ddl" as const,
                  label: t("database.ddl"),
                  count: tableInfoDdl ? 1 : 0,
                  icon: <FileCode size={14} aria-hidden="true" />,
                },
              ].map((tab) => {
                const active = tableInfoActiveTab === tab.key;
                return (
                  <button
                    key={tab.key}
                    type="button"
                    role="tab"
                    aria-label={`${tab.label} ${tab.count}`}
                    aria-selected={active}
                    style={{
                      ...s.databaseTableInfoTab,
                      ...(active ? s.databaseTableInfoTabActive : null),
                      border: "none",
                    }}
                    onClick={() => setTableInfoActiveTab(tab.key)}
                  >
                    {tab.icon}
                    <span>{tab.label}</span>
                    <span>{tab.count}</span>
                  </button>
                );
              })}
            </div>
            <div style={s.databaseTableInfoContent} role="tabpanel">
              {tableInfoActiveTab === "ddl" ? (
                <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: 12 }}>
                  <div
                    style={{
                      display: "flex",
                      justifyContent: "space-between",
                      gap: 8,
                      alignItems: "center",
                    }}
                  >
                    <span style={s.databaseDialogHint}>
                      {tableInfoDdlLoading ? t("database.loading") : t("database.ddl")}
                    </span>
                    <DbxButton
                      variant="outline"
                      size="xs"
                      icon={RefreshCcw}
                      disabled={tableInfoDdlLoading}
                      onClick={() => void loadTableInfoDdl()}
                    >
                      {t("common.refresh")}
                    </DbxButton>
                  </div>
                  <pre
                    data-testid={
                      !tableInfoDdlLoading && tableInfoDdl ? "database-ddl-highlight" : undefined
                    }
                    style={{ ...s.databaseSqlPreview, margin: 0, minHeight: 180 }}
                  >
                    {tableInfoDdlLoading
                      ? t("database.loading")
                      : tableInfoDdl
                        ? renderSqlTokens(tableInfoDdl)
                        : t("database.empty")}
                  </pre>
                  {tableInfoDdlError && <div style={s.databaseError}>{tableInfoDdlError}</div>}
                </div>
              ) : tableInfoActiveTab === "columns" ? (
                <table style={s.databaseTable}>
                  <thead>
                    <tr>
                      <th style={s.databaseTh}>{t("database.columnName")}</th>
                      <th style={s.databaseTh}>{t("database.columnType")}</th>
                      <th style={s.databaseTh}>{t("database.defaultValue")}</th>
                      <th style={s.databaseTh}>{t("database.columnComment")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {filteredDbxInfoColumns.map((column) => (
                      <tr key={column.name}>
                        <td style={s.databaseTd}>
                          {column.is_primary_key && (
                            <span title={t("database.primaryKey")} style={{ marginRight: 4 }}>
                              🔑
                            </span>
                          )}
                          <span style={{ fontWeight: 700 }}>{column.name}</span>
                        </td>
                        <td style={s.databaseTd}>
                          {column.data_type}
                          {column.is_nullable ? " NULL" : " NOT NULL"}
                        </td>
                        <td style={s.databaseTd}>{column.column_default ?? "-"}</td>
                        <td style={s.databaseTd}>{column.comment ?? "-"}</td>
                      </tr>
                    ))}
                    {filteredDbxInfoColumns.length === 0 && (
                      <tr>
                        <td style={s.databaseTd} colSpan={4}>
                          {t("database.empty")}
                        </td>
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
                        <td style={s.databaseTd} colSpan={3}>
                          {t("database.empty")}
                        </td>
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
                <DbxButton
                  variant="default"
                  size="sm"
                  icon={Play}
                  onClick={runSql}
                  disabled={!activeSqlCapable || loading}
                  style={{ width: 86, height: "auto" }}
                >
                  {t("database.run")}
                </DbxButton>
              </div>
            )}

            <div style={s.databaseToolbar}>
              {hideDatabaseWorkspaceTopbar && error && (
                <div style={s.databaseError} title={error}>
                  {error}
                </div>
              )}
              <DbxButton
                variant="outline"
                size="sm"
                icon={RefreshCcw}
                disabled={!activeObject || loading}
                onClick={() => {
                  if (activeDbxConnection && activeDbxObject) {
                    loadDbxObject(
                      activeDbxObject,
                      page,
                      activeDbxConnection,
                      activeDbxDatabase,
                      dbxGridWhereInput,
                      dbxGridOrderByInput,
                    );
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
                disabled={!canInsertActiveTable || loading}
                onClick={insertRow}
              >
                {t("database.insert")}
              </DbxButton>
              {dbxSqlPreviewStatements.length > 0 && (
                <DbxButton
                  variant="outline"
                  size="sm"
                  icon={FileCode}
                  onClick={() => setDbxSqlPreviewOpen(true)}
                >
                  {t("database.previewSql")}
                </DbxButton>
              )}
              {queryResult && activeDbxConnection && activeDbxObject && (
                <>
                  <DbxButton
                    variant="outline"
                    size="sm"
                    icon={SlidersHorizontal}
                    disabled={loading}
                    onClick={() => {
                      setWorkspaceMode("table-info");
                      setTableInfoActiveTab("columns");
                      const tabId = `table-info:${activeDbxObject?.name ?? ""}`;
                      setWorkspaceTabs((prev) =>
                        prev.some((t) => t.id === tabId)
                          ? prev
                          : [
                              ...prev,
                              {
                                id: tabId,
                                mode: "table-info",
                                label: `${t("database.tableProperties")}: ${activeDbxObject?.name ?? ""}`,
                                closable: true,
                              },
                            ],
                      );
                      setActiveTabId(tabId);
                      void loadDbxColumnsForTables(
                        [activeDbxObject],
                        activeDbxConnection,
                        activeDbxDatabase,
                      );
                      void loadTableInfoDdlForObject(
                        activeDbxConnection,
                        activeDbxDatabase,
                        activeDbxObject,
                      );
                    }}
                  >
                    {t("database.tableProperties")}
                  </DbxButton>
                  <div style={s.databaseToolbarMenuAnchor}>
                    <DbxButton
                      variant="outline"
                      size="sm"
                      icon={Wrench}
                      disabled={loading}
                      onClick={() => {
                        setDbxDataToolsOpen((open) => !open);
                        setDbxFieldFilterOpen(false);
                        setDbxDataToolsMode("root");
                      }}
                    >
                      {t("database.dataTools")}
                    </DbxButton>
                    {dbxDataToolsOpen && (
                      <div
                        role="menu"
                        aria-label={t("database.dataTools")}
                        style={s.databaseToolbarMenu}
                      >
                        {dbxDataToolsMode === "root" ? (
                          <>
                            <button
                              type="button"
                              role="menuitem"
                              style={s.databaseToolbarMenuButton}
                              disabled={!canInsertActiveTable || loading}
                              onClick={() => {
                                setDbxDataToolsOpen(false);
                                insertRow();
                              }}
                            >
                              {t("database.generateData")}
                            </button>
                            <button
                              type="button"
                              role="menuitem"
                              style={s.databaseToolbarMenuButton}
                              disabled={activeDbxConnection.readOnly || loading}
                              onClick={() => {
                                setDbxDataToolsOpen(false);
                                void openTableImportDialog(
                                  activeDbxConnection,
                                  activeDbxDatabase,
                                  activeDbxObject,
                                );
                              }}
                            >
                              {t("database.importData")}
                            </button>
                            <button
                              type="button"
                              role="menuitem"
                              style={s.databaseToolbarMenuButton}
                              disabled={visibleTableColumns.length === 0 || loading}
                              onClick={() => setDbxDataToolsMode("export")}
                            >
                              {t("database.exportData")}
                              <ChevronRight size={14} aria-hidden="true" />
                            </button>
                          </>
                        ) : (
                          <>
                            <button
                              type="button"
                              style={s.databaseToolbarMenuButton}
                              onClick={() => setDbxDataToolsMode("root")}
                            >
                              <ChevronLeft size={14} aria-hidden="true" />
                              {t("common.back")}
                            </button>
                            {DATA_TOOL_EXPORT_FORMATS.map((item) => (
                              <button
                                key={item.format}
                                type="button"
                                role="menuitem"
                                style={s.databaseToolbarMenuButton}
                                disabled={visibleTableColumns.length === 0 || loading}
                                onClick={() => {
                                  setDbxGridExportFormat(item.format);
                                  setDbxDataToolsOpen(false);
                                  void exportActiveDbxGrid(item.format);
                                }}
                              >
                                {item.label}
                              </button>
                            ))}
                          </>
                        )}
                      </div>
                    )}
                  </div>
                  <div style={s.databaseToolbarMenuAnchor}>
                    <DbxButton
                      variant="outline"
                      size="sm"
                      icon={Eye}
                      disabled={tableColumns.length === 0}
                      onClick={() => {
                        setDbxFieldFilterOpen((open) => !open);
                        setDbxDataToolsOpen(false);
                      }}
                    >
                      {t("database.fieldFilter")}
                    </DbxButton>
                    {dbxFieldFilterOpen && (
                      <div
                        role="menu"
                        aria-label={t("database.fieldFilter")}
                        style={{ ...s.databaseToolbarMenu, ...s.databaseToolbarMenuWide }}
                      >
                        <input
                          style={s.databaseDialogInput}
                          value={dbxGridColumnSearch}
                          onChange={(event) => setDbxGridColumnSearch(event.target.value)}
                          placeholder={t("database.gridSearchColumns")}
                          aria-label={t("database.gridSearchColumns")}
                        />
                        <div style={s.databaseFieldFilterList}>
                          {filteredDbxGridColumnOptions.length > 0 ? (
                            filteredDbxGridColumnOptions.map((column) => {
                              const hidden = dbxGridHiddenColumns.has(column);
                              const visibleCount = tableColumns.filter(
                                (item) => !dbxGridHiddenColumns.has(item),
                              ).length;
                              return (
                                <label key={column} style={s.databaseFieldFilterItem}>
                                  <input
                                    type="checkbox"
                                    checked={!hidden}
                                    disabled={!hidden && visibleCount <= 1}
                                    onChange={() => toggleDbxGridColumnVisibility(column)}
                                  />
                                  <span
                                    style={{
                                      overflow: "hidden",
                                      textOverflow: "ellipsis",
                                      whiteSpace: "nowrap",
                                    }}
                                  >
                                    {column}
                                  </span>
                                </label>
                              );
                            })
                          ) : (
                            <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
                              {t("database.gridNoSearchResults")}
                            </span>
                          )}
                        </div>
                        <div style={s.databaseFieldFilterFooter}>
                          <DbxButton
                            variant="outline"
                            size="xs"
                            disabled={tableColumns.length <= 1}
                            onClick={invertDbxGridColumnVisibility}
                          >
                            {t("database.gridInvertColumnVisibility")}
                          </DbxButton>
                          <DbxButton
                            variant="outline"
                            size="xs"
                            disabled={dbxGridHiddenColumns.size === 0}
                            onClick={showAllDbxGridColumns}
                          >
                            {t("database.gridShowAllColumns")}
                          </DbxButton>
                        </div>
                      </div>
                    )}
                  </div>
                </>
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
                  <DbxButton
                    variant="destructive"
                    size="sm"
                    icon={Trash2}
                    disabled={
                      !queryResult.editable ||
                      activeDbxConnection.readOnly ||
                      dbxGridSelectedRows.size === 0 ||
                      loading
                    }
                    onClick={() => void deleteSelectedDbxRows()}
                  >
                    {t("database.deleteSelectedRowsCount", { count: dbxGridSelectedRows.size })}
                  </DbxButton>
                  <DbxButton
                    variant="outline"
                    size="sm"
                    icon={RefreshCcw}
                    disabled={loading}
                    onClick={() => void resetActiveDbxGrid()}
                  >
                    {t("database.gridReset")}
                  </DbxButton>
                  <DbxButton
                    variant="outline"
                    size="sm"
                    icon={CheckSquare}
                    disabled={loading || dbxPendingCellEditCount === 0}
                    onClick={() => void saveDbxPendingCellEdits()}
                  >
                    {t("common.save")}
                  </DbxButton>
                </>
              )}
              <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
                {loading
                  ? t("database.loading")
                  : activeConnection?.readOnly
                    ? t("database.readOnlyBadge")
                    : sqlResult?.message}
              </span>
            </div>

            {queryResult && activeDbxConnection && activeDbxObject && (
              <div
                role="group"
                aria-label="Table filters"
                style={s.databaseGridFilterBar}
                onKeyDown={(event) => {
                  if (
                    event.key === "Enter" &&
                    !event.shiftKey &&
                    !event.metaKey &&
                    !event.ctrlKey &&
                    !event.altKey
                  ) {
                    event.preventDefault();
                    void reloadActiveDbxGrid();
                  }
                }}
              >
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
              </div>
            )}

            <DataGridView
              variant={workspaceMode === "query" ? "query" : "table"}
              queryResult={queryResult}
              activeDbxConnection={activeDbxConnection}
              activeConnectionReadOnly={Boolean(activeConnection?.readOnly)}
              activeObject={activeObject}
              tableColumns={tableColumns}
              showRowIdColumn={showRowIdColumn}
              loading={loading}
              grid={dbxGrid}
              onKeyDown={handleDbxGridKeyDown}
              onSortColumn={toggleDbxGridColumnSort}
              onOpenContextMenu={setContextMenu}
              onUpdateCell={updateCell}
            />
            {queryResult && (
              <div style={s.databaseGridFooter}>
                <div
                  role="status"
                  aria-label={t("database.tableRowCount")}
                  style={s.databaseGridFooterRows}
                >
                  {tableFooterRowCountText}
                </div>
                <div
                  role="status"
                  aria-label={t("database.currentSql")}
                  style={s.databaseGridFooterSql}
                  title={tableFooterSqlText}
                >
                  {tableFooterSqlText || "-"}
                </div>
                <div
                  role="group"
                  aria-label={t("database.tablePagination")}
                  style={s.databaseGridFooterPager}
                >
                  <DbxButton
                    variant="ghost"
                    size="icon-sm"
                    icon={ChevronLeft}
                    disabled={!activeObject || page <= 1 || loading}
                    onClick={() => {
                      if (activeDbxConnection && activeDbxObject) {
                        loadDbxObject(
                          activeDbxObject,
                          Math.max(1, page - 1),
                          activeDbxConnection,
                          activeDbxDatabase,
                          dbxGridWhereInput,
                          dbxGridOrderByInput,
                        );
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
                  {activeDbxConnection && (
                    <label
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 6,
                        fontSize: 12,
                        color: "var(--text-muted)",
                        whiteSpace: "nowrap",
                      }}
                    >
                      <span>{t("database.gridRowsPerPage")}</span>
                      <select
                        style={{
                          ...s.databaseDialogInput,
                          width: 82,
                          height: 28,
                          padding: "0 6px",
                        }}
                        value={
                          (DBX_GRID_PAGE_SIZE_OPTIONS as readonly number[]).includes(
                            dbxGridPageSize,
                          )
                            ? dbxGridPageSize
                            : "custom"
                        }
                        disabled={loading}
                        aria-label={t("database.gridRowsPerPage")}
                        onChange={(event) => {
                          const val = event.currentTarget.value;
                          if (val === "custom") {
                            const custom = window.prompt(
                              t("database.gridCustomPageSize"),
                              String(dbxGridPageSize),
                            );
                            if (custom) {
                              const num = Number(custom);
                              if (Number.isFinite(num) && num >= 1 && num <= 10000)
                                void changeDbxGridPageSize(num);
                            }
                          } else {
                            void changeDbxGridPageSize(Number(val));
                          }
                        }}
                      >
                        {DBX_GRID_PAGE_SIZE_OPTIONS.map((option) => (
                          <option key={option} value={option}>
                            {option}
                          </option>
                        ))}
                        {!(DBX_GRID_PAGE_SIZE_OPTIONS as readonly number[]).includes(
                          dbxGridPageSize,
                        ) && <option value="custom">{dbxGridPageSize}</option>}
                        <option value="custom">{t("database.gridCustomPageSize")}</option>
                      </select>
                    </label>
                  )}
                  <DbxButton
                    variant="ghost"
                    size="icon-sm"
                    icon={ChevronRight}
                    disabled={
                      !activeObject || loading || (totalPages != null && page >= totalPages)
                    }
                    onClick={() => {
                      if (activeDbxConnection && activeDbxObject) {
                        loadDbxObject(
                          activeDbxObject,
                          page + 1,
                          activeDbxConnection,
                          activeDbxDatabase,
                          dbxGridWhereInput,
                          dbxGridOrderByInput,
                        );
                      } else if (activeObject) {
                        loadTable(activeObject, page + 1);
                      }
                    }}
                  />
                </div>
              </div>
            )}
          </>
        )}
        {dbxCellDetail && (
          <div style={s.databaseCellDetailPanel}>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                marginBottom: 4,
              }}
            >
              <div style={{ fontWeight: 700, fontSize: 13 }}>{t("database.cellDetail")}</div>
              <button
                type="button"
                onClick={() => setDbxCellDetail(null)}
                style={{
                  background: "transparent",
                  border: "none",
                  cursor: "pointer",
                  color: "var(--text-hint)",
                  fontSize: 18,
                  padding: "0 4px",
                }}
              >
                ×
              </button>
            </div>
            <div style={s.databaseCellDetailGrid}>
              <div style={s.databaseCellDetailField}>
                <span style={s.databaseCellDetailLabel}>{t("database.columnName")}</span>
                <span style={{ ...s.databaseCellDetailValue, fontWeight: 700 }}>
                  {dbxCellDetail.column}
                </span>
              </div>
              <div style={s.databaseCellDetailField}>
                <span style={s.databaseCellDetailLabel}>{t("database.rowNumber")}</span>
                <span style={s.databaseCellDetailValue}>{dbxCellDetail.rowIndex + 1}</span>
              </div>
              <div style={s.databaseCellDetailField}>
                <span style={s.databaseCellDetailLabel}>{t("database.columnType")}</span>
                <span
                  style={{
                    ...s.databaseCellDetailValue,
                    ...dbxDataTypeStyle(dbxCellDetail.columnInfo?.data_type ?? ""),
                  }}
                >
                  {dbxCellDetail.columnInfo?.data_type ?? "-"}
                </span>
              </div>
              <div style={s.databaseCellDetailField}>
                <span style={s.databaseCellDetailLabel}>NULL</span>
                <span style={s.databaseCellDetailValue}>
                  {dbxCellDetail.columnInfo?.is_nullable ? "YES" : "NO"}
                </span>
              </div>
              <div style={s.databaseCellDetailField}>
                <span style={s.databaseCellDetailLabel}>{t("database.length")}</span>
                <span style={s.databaseCellDetailValue}>
                  {dbxCellDetail.columnInfo?.character_maximum_length ?? "-"}
                </span>
              </div>
              <div style={s.databaseCellDetailField}>
                <span style={s.databaseCellDetailLabel}>{t("database.columnComment")}</span>
                <span style={s.databaseCellDetailValue}>
                  {dbxCellDetail.columnInfo?.comment ?? "-"}
                </span>
              </div>
            </div>
            <div style={{ marginTop: 4 }}>
              <span style={s.databaseCellDetailLabel}>{t("database.cellValue")}</span>
              <textarea
                style={{
                  width: "100%",
                  minHeight: 60,
                  marginTop: 4,
                  fontFamily: "var(--font-mono)",
                  fontSize: 12,
                  padding: 6,
                  background: "var(--bg-input)",
                  color: "var(--text-primary)",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 4,
                  resize: "vertical",
                }}
                defaultValue={valueToText(dbxCellDetail.value)}
                readOnly
              />
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
              <DbxButton
                variant="outline"
                size="sm"
                onClick={() => {
                  navigator.clipboard?.writeText(valueToText(dbxCellDetail.value));
                }}
              >
                {t("common.copy")}
              </DbxButton>
              <DbxButton
                variant="outline"
                size="sm"
                onClick={() => {
                  const rows = queryResult?.rows;
                  if (rows && dbxCellDetail.rowIndex >= 0 && dbxCellDetail.rowIndex < rows.length) {
                    const row = rows[dbxCellDetail.rowIndex];
                    if (row) {
                      updateCell(row, dbxCellDetail.column, "", valueToText(dbxCellDetail.value));
                    }
                  }
                }}
              >
                {t("database.setNull")}
              </DbxButton>
              <DbxButton
                variant="outline"
                size="sm"
                onClick={() => {
                  const rows = queryResult?.rows;
                  if (rows && dbxCellDetail.rowIndex >= 0 && dbxCellDetail.rowIndex < rows.length) {
                    const row = rows[dbxCellDetail.rowIndex];
                    if (row) {
                      const original = valueToText(row.values[dbxCellDetail.columnIndex]);
                      updateCell(row, dbxCellDetail.column, original, original);
                    }
                  }
                }}
              >
                {t("database.restore")}
              </DbxButton>
            </div>
          </div>
        )}
      </main>
      {exportProgress?.active && (
        <div style={s.databaseDialogOverlay}>
          <div style={{ ...s.databaseConnectionDialog, width: 400, maxWidth: "min(90vw, 400px)" }}>
            <div style={s.databaseDialogHeader}>{t("database.exportProgress")}</div>
            <div style={s.databaseDialogBody}>
              <div style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 8 }}>
                {t("database.exportProgressFormat", {
                  format: exportProgress.format.toUpperCase(),
                })}
              </div>
              <div style={{ fontSize: 12, color: "var(--text-muted)", wordBreak: "break-all" }}>
                {exportProgress.filePath}
              </div>
              <div style={{ marginTop: 16, display: "flex", alignItems: "center", gap: 8 }}>
                <div
                  style={{
                    width: 16,
                    height: 16,
                    border: "2px solid var(--border-dim)",
                    borderTopColor: "var(--accent)",
                    borderRadius: "50%",
                    animation: "spin 1s linear infinite",
                  }}
                />
                <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
                  {t("database.exportProgressRunning")}
                </span>
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
                <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 10 }}>
                  {dbxSqlPreviewDescription}
                </div>
              )}
              <div
                style={{
                  fontSize: 11,
                  fontWeight: 650,
                  color: "var(--text-muted)",
                  marginBottom: 4,
                }}
              >
                {t("database.gridPreviewStatements")}
              </div>
              <pre
                style={{
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
                }}
              >
                {dbxSqlPreviewStatements.join("\n")}
              </pre>
              {dbxSqlPreviewRollback.length > 0 && (
                <>
                  <div
                    style={{
                      fontSize: 11,
                      fontWeight: 650,
                      color: "var(--text-muted)",
                      marginTop: 10,
                      marginBottom: 4,
                    }}
                  >
                    {t("database.gridRollbackSql")}
                  </div>
                  <pre
                    style={{
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
                    }}
                  >
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
            <div style={s.databaseDialogHeader}>
              {t("database.columnDetailsFor", { column: dbxColumnPreview.column })}
            </div>
            <div style={s.databaseDialogBody}>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 10 }}>
                <div style={s.databaseDialogField}>
                  <span style={s.databaseDialogLabel}>{t("database.columnName")}</span>
                  <div style={{ color: "var(--text-primary)", fontSize: 12 }}>
                    {dbxColumnPreview.column}
                  </div>
                </div>
                <div style={s.databaseDialogField}>
                  <span style={s.databaseDialogLabel}>{t("database.columnType")}</span>
                  <div style={{ color: "var(--text-primary)", fontSize: 12 }}>
                    {dbxGridColumnType(queryResult, dbxColumnPreview.columnIndex) ?? "-"}
                  </div>
                </div>
                <div style={s.databaseDialogField}>
                  <span style={s.databaseDialogLabel}>{t("database.rowCount")}</span>
                  <div style={{ color: "var(--text-primary)", fontSize: 12 }}>
                    {dbxColumnPreviewFields.length}
                  </div>
                </div>
              </div>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  color: "var(--text-muted)",
                  fontSize: 12,
                }}
              >
                <span style={{ whiteSpace: "nowrap" }}>
                  {t("database.rowCount")}: {dbxColumnPreviewFields.length}
                </span>
                <label
                  style={{ position: "relative", marginLeft: "auto", width: 220, maxWidth: "52%" }}
                >
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
                          <pre
                            style={{
                              margin: 0,
                              whiteSpace: "pre-wrap",
                              wordBreak: "break-word",
                              fontFamily: "var(--font-mono)",
                            }}
                          >
                            {field.preview}
                          </pre>
                        </td>
                        <td style={{ ...s.databaseTd, width: 44 }}>
                          <DbxButton
                            variant="ghost"
                            size="icon-sm"
                            icon={Copy}
                            aria-label={t("database.copyRowValue", { row: field.rowNumber })}
                            title={t("database.copyRowValue", { row: field.rowNumber })}
                            onClick={() => {
                              navigator.clipboard
                                ?.writeText(field.preview)
                                .catch((err) => setError(String(err)));
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
                    ?.writeText(
                      JSON.stringify(
                        dbxColumnPreviewFields.map((field) => ({
                          row: field.rowNumber,
                          value: field.value,
                        })),
                        null,
                        2,
                      ),
                    )
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
                    ?.writeText(
                      dbxColumnPreviewFields.map((field) => valueToText(field.value)).join("\n"),
                    )
                    .catch((err) => setError(String(err)));
                }}
              >
                {t("database.copyColumnTsv")}
              </DbxButton>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                onClick={() => void copyNodeName(dbxColumnPreview.column)}
              >
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
            <div style={s.databaseDialogHeader}>
              {t("database.rowDetailsFor", { row: dbxRowPreview.rowIndex + 1 })}
            </div>
            <div style={s.databaseDialogBody}>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  color: "var(--text-muted)",
                  fontSize: 12,
                }}
              >
                <span style={{ whiteSpace: "nowrap" }}>
                  {t("database.columnsCount", { count: dbxRowPreviewFields.length })}
                </span>
                <label
                  style={{ position: "relative", marginLeft: "auto", width: 220, maxWidth: "52%" }}
                >
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
                          <div style={{ color: "var(--text-hint)", fontSize: 11 }}>
                            {field.type ?? "-"}
                          </div>
                        </td>
                        <td style={s.databaseTd}>
                          <pre
                            style={{
                              margin: 0,
                              whiteSpace: "pre-wrap",
                              wordBreak: "break-word",
                              fontFamily: "var(--font-mono)",
                            }}
                          >
                            {field.preview}
                          </pre>
                        </td>
                        <td style={{ ...s.databaseTd, width: 44 }}>
                          <DbxButton
                            variant="ghost"
                            size="icon-sm"
                            icon={Copy}
                            aria-label={t("database.copyFieldValue", { column: field.column })}
                            title={t("database.copyFieldValue", { column: field.column })}
                            onClick={() => {
                              navigator.clipboard
                                ?.writeText(field.preview)
                                .catch((err) => setError(String(err)));
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
                <div style={{ color: "var(--text-primary)", fontSize: 12 }}>
                  {dbxCellPreview.column}
                </div>
              </div>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>
                  {formattedDbxCellPreview.json
                    ? t("database.documentJson")
                    : t("database.cellValue")}
                </span>
                <textarea
                  style={{ ...s.databaseSqlInput, minHeight: 280, resize: "vertical" }}
                  readOnly
                  value={formattedDbxCellPreview.text}
                  aria-label={
                    formattedDbxCellPreview.json
                      ? t("database.documentJson")
                      : t("database.cellValue")
                  }
                />
              </label>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                onClick={() => {
                  navigator.clipboard
                    ?.writeText(formattedDbxCellPreview.text)
                    .catch((err) => setError(String(err)));
                }}
              >
                {t("database.copyValue")}
              </DbxButton>
              <DbxButton
                variant="outline"
                size="sm"
                icon={Copy}
                onClick={() => copyNodeName(dbxCellPreview.column)}
              >
                {t("database.copyColumnName")}
              </DbxButton>
              <DbxButton variant="outline" size="sm" onClick={() => setDbxCellPreview(null)}>
                {t("common.close")}
              </DbxButton>
            </div>
          </section>
        </div>
      )}
      <ConnectionDialog
        open={connectionDialogOpen}
        editingConnection={editingDbxConnection}
        initialConnectionGroup={newConnectionGroup}
        projectRoot={projectRoot}
        onAddLocalConnection={(endpoint) => addConnection(endpoint)}
        onSaved={handleConnectionSaved}
        onClose={closeConnectionDialog}
      />
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
              <div style={s.databaseDialogHint}>{t("database.createDatabaseHint")}</div>
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
              <DbxDialogFooterButton
                type="button"
                onClick={closeCreateDatabaseDialog}
                disabled={loading}
              >
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton
                type="submit"
                variant="default"
                icon={Plus}
                disabled={loading || !createDatabaseName.trim()}
              >
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
              <DbxDialogFooterButton
                type="button"
                onClick={closeCreateSchemaDialog}
                disabled={loading}
              >
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton
                type="submit"
                variant="default"
                icon={Plus}
                disabled={loading || !createSchemaName.trim()}
              >
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
                {t("database.visibleDatabasesDescription", {
                  connection: visibleDatabaseConnection.name,
                })}
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
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 10,
                  fontSize: 11.5,
                  color: "var(--text-muted)",
                }}
              >
                <span>
                  {t("database.visibleDatabasesSelectedCount", {
                    selected: visibleDatabaseSelection.size,
                    total: listedVisibleDatabaseNames.length,
                  })}
                </span>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <button
                    type="button"
                    style={{
                      border: "none",
                      background: "transparent",
                      color: "var(--text-secondary)",
                      fontSize: 11.5,
                      cursor: "pointer",
                      padding: "0 2px",
                    }}
                    onClick={() => setVisibleDatabaseSelection(new Set(listedVisibleDatabaseNames))}
                    disabled={visibleDatabaseLoading}
                  >
                    {t("database.visibleDatabasesSelectAll")}
                  </button>
                  {visibleDatabaseSearch.trim() && (
                    <button
                      type="button"
                      style={{
                        border: "none",
                        background: "transparent",
                        color: "var(--text-secondary)",
                        fontSize: 11.5,
                        cursor: "pointer",
                        padding: "0 2px",
                      }}
                      onClick={() =>
                        setVisibleDatabaseSelection(new Set(filteredVisibleDatabaseNames))
                      }
                      disabled={visibleDatabaseLoading}
                    >
                      {t("database.visibleDatabasesSelectFiltered")}
                    </button>
                  )}
                  <button
                    type="button"
                    style={{
                      border: "none",
                      background: "transparent",
                      color: "var(--text-secondary)",
                      fontSize: 11.5,
                      cursor: "pointer",
                      padding: "0 2px",
                    }}
                    onClick={() => setVisibleDatabaseSelection(new Set())}
                    disabled={visibleDatabaseLoading}
                  >
                    {t("database.visibleDatabasesClear")}
                  </button>
                  <button
                    type="button"
                    style={{
                      border: "none",
                      background: "transparent",
                      color: "var(--text-secondary)",
                      fontSize: 11.5,
                      cursor: "pointer",
                      padding: "0 2px",
                    }}
                    onClick={() => {
                      void showAllVisibleDatabases();
                    }}
                    disabled={
                      visibleDatabaseLoading ||
                      !configuredVisibleDatabases(visibleDatabaseConnection)
                    }
                  >
                    {t("database.visibleDatabasesShowAll")}
                  </button>
                </div>
              </div>
              {!visibleDatabaseLoading && !visibleDatabaseError && !visibleDatabaseCanSave && (
                <div style={{ color: "var(--danger)", fontSize: 12 }}>
                  {t("database.visibleDatabasesEmptySelection")}
                </div>
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
                          const next = new Set(
                            [...current].filter(
                              (name) =>
                                !isSystemDatabaseName(visibleDatabaseConnection.dbType, name),
                            ),
                          );
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
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {database}
                        </span>
                      </button>
                    );
                  })
                )}
              </div>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxDialogFooterButton
                type="button"
                onClick={closeVisibleDatabasesDialog}
                disabled={visibleDatabaseLoading}
              >
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton
                type="button"
                variant="default"
                onClick={() => {
                  void saveVisibleDatabaseSelection();
                }}
                disabled={
                  visibleDatabaseLoading || Boolean(visibleDatabaseError) || !visibleDatabaseCanSave
                }
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
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 10,
                  fontSize: 11.5,
                  color: "var(--text-muted)",
                }}
              >
                <span>
                  {t("database.exportSelectedTables", {
                    selected: databaseExportSelection.size,
                    total: databaseExportTables.length,
                  })}
                </span>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <button
                    type="button"
                    style={{
                      border: "none",
                      background: "transparent",
                      color: "var(--text-secondary)",
                      fontSize: 11.5,
                      cursor: "pointer",
                      padding: "0 2px",
                    }}
                    onClick={() =>
                      setDatabaseExportSelection(new Set(filteredDatabaseExportTables))
                    }
                    disabled={databaseExportLoading}
                  >
                    {t("database.visibleDatabasesSelectAll")}
                  </button>
                  <button
                    type="button"
                    style={{
                      border: "none",
                      background: "transparent",
                      color: "var(--text-secondary)",
                      fontSize: 11.5,
                      cursor: "pointer",
                      padding: "0 2px",
                    }}
                    onClick={() => {
                      const removing = new Set(filteredDatabaseExportTables);
                      setDatabaseExportSelection(
                        (current) => new Set([...current].filter((table) => !removing.has(table))),
                      );
                    }}
                    disabled={databaseExportLoading}
                  >
                    {t("database.visibleDatabasesClear")}
                  </button>
                </div>
              </div>
              {!databaseExportLoading &&
                !databaseExportError &&
                databaseExportSelection.size === 0 && (
                  <div style={{ color: "var(--danger)", fontSize: 12 }}>
                    {t("database.exportEmptySelection")}
                  </div>
                )}
              {!databaseExportLoading &&
                !databaseExportError &&
                !databaseExportIncludeStructure &&
                !databaseExportIncludeData &&
                !databaseExportIncludeObjects && (
                  <div style={{ color: "var(--danger)", fontSize: 12 }}>
                    {t("database.exportEmptyOptions")}
                  </div>
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
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {table}
                        </span>
                      </button>
                    );
                  })
                )}
              </div>
            </div>
            <div style={s.databaseDialogFooter}>
              <DbxDialogFooterButton
                type="button"
                onClick={closeDatabaseExportDialog}
                disabled={databaseExportLoading}
              >
                {t("common.cancel")}
              </DbxDialogFooterButton>
              <DbxDialogFooterButton
                type="button"
                variant="default"
                onClick={() => {
                  void submitDatabaseExport();
                }}
                disabled={
                  databaseExportLoading || Boolean(databaseExportError) || !databaseExportCanRun
                }
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
            <div
              style={{
                ...s.databaseDialogBody,
                maxHeight: "calc(100vh - 180px)",
                overflowY: "auto",
              }}
            >
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
                <span
                  style={{
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontSize: 12,
                    color: "var(--text-muted)",
                  }}
                >
                  {tableImportPreview
                    ? tableImportPreview.fileName || tableImportPreview.filePath
                    : t("database.tableImportNoFile")}
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
              {tableImportError && (
                <div style={{ color: "var(--danger)", fontSize: 12 }}>{tableImportError}</div>
              )}
              {tableImportLoading ? (
                <div style={s.databaseEmptyCompact}>{t("common.loading")}</div>
              ) : tableImportPreview ? (
                <>
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 10,
                    }}
                  >
                    <div style={s.databaseDialogLabel}>
                      {t("database.tableImportMappedColumns", {
                        mapped: tableImportMappedColumns.length,
                        total: tableImportPreview.columns.length,
                      })}
                    </div>
                    <button
                      type="button"
                      style={{
                        border: "none",
                        background: "transparent",
                        color: "var(--text-secondary)",
                        fontSize: 11.5,
                        cursor: "pointer",
                        padding: "0 2px",
                      }}
                      onClick={() =>
                        setTableImportMappings(
                          autoMapImportColumns(
                            tableImportPreview.columns,
                            tableImportTargetColumnNames,
                          ),
                        )
                      }
                    >
                      {t("database.tableImportAutoMap")}
                    </button>
                  </div>
                  <div
                    style={{
                      display: "grid",
                      gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
                      gap: 8,
                    }}
                  >
                    {tableImportPreview.columns.map((sourceColumn) => (
                      <label key={sourceColumn} style={s.databaseDialogField}>
                        <span style={s.databaseDialogLabel}>{sourceColumn}</span>
                        <select
                          aria-label={t("database.tableImportTargetColumn", {
                            column: sourceColumn,
                          })}
                          style={s.databaseDialogInput}
                          value={tableImportMappings[sourceColumn] ?? ""}
                          onChange={(event) =>
                            updateTableImportMapping(sourceColumn, event.target.value)
                          }
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
                  <div
                    style={{
                      overflow: "auto",
                      border: "1px solid var(--border-dim)",
                      borderRadius: 8,
                      background: "var(--bg-subtle)",
                    }}
                  >
                    <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                      <thead>
                        <tr>
                          {tableImportPreview.columns.map((column) => (
                            <th
                              key={column}
                              style={{
                                padding: "6px 8px",
                                textAlign: "left",
                                borderBottom: "1px solid var(--border-dim)",
                                color: "var(--text-muted)",
                              }}
                            >
                              {column}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {tableImportPreview.rows.map((row, rowIndex) => (
                          <tr key={rowIndex}>
                            {tableImportPreview.columns.map((column, columnIndex) => (
                              <td
                                key={column}
                                style={{
                                  padding: "6px 8px",
                                  borderBottom: "1px solid var(--border-dim)",
                                  color: "var(--text-secondary)",
                                }}
                              >
                                {row[columnIndex] === null || row[columnIndex] === undefined
                                  ? "NULL"
                                  : String(row[columnIndex])}
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
              <DbxDialogFooterButton
                type="button"
                onClick={closeTableImportDialog}
                disabled={tableImportLoading}
              >
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
            aria-label={contextMenu.kind === "workspace-tab" ? t("database.tabActions") : undefined}
            style={{
              ...s.fileCtxMenu,
              left: contextMenu.x,
              top: contextMenu.y,
              minWidth: 190,
            }}
          >
            {contextMenu.kind === "dbx-grid-header"
              ? (
                  [
                    ["copyColumnName", "database.copyColumnName"],
                    ["previewColumn", "database.openColumnDetailsDialog"],
                    ["sortAscending", "database.sortAscending"],
                    ["sortDescending", "database.sortDescending"],
                    ...(dbxGridOrderByInput.trim()
                      ? ([["clearSort", "database.clearSort"]] as const)
                      : []),
                  ] as const
                ).map(([action, labelKey]) => {
                  const disabled =
                    (action === "sortAscending" ||
                      action === "sortDescending" ||
                      action === "clearSort") &&
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
              : contextMenu.kind === "workspace-tab"
                ? (
                    [
                      ["toggleShortTitle", "database.shortenTabTitle"],
                      ["pinTab", "database.pinTab"],
                      ["closeTab", "database.closeTab"],
                      ["closeOtherTabs", "database.closeOtherTabs"],
                      ["closeAllTabs", "database.closeAllTabs"],
                    ] as const
                  ).map(([action, labelKey]) => {
                    const checked =
                      action === "toggleShortTitle" && shortWorkspaceTabIds.has(contextMenu.tabId);
                    return (
                      <button
                        key={action}
                        type="button"
                        role={action === "toggleShortTitle" ? "menuitemcheckbox" : "menuitem"}
                        aria-checked={action === "toggleShortTitle" ? checked : undefined}
                        style={{
                          ...s.fileCtxMenuItem,
                          display: "grid",
                          gridTemplateColumns: "16px minmax(0, 1fr)",
                          alignItems: "center",
                          gap: 8,
                        }}
                        onClick={() => {
                          runWorkspaceTabContextMenuAction(action);
                        }}
                      >
                        <span
                          style={{
                            width: 16,
                            display: "inline-flex",
                            alignItems: "center",
                            justifyContent: "center",
                          }}
                        >
                          {action === "toggleShortTitle" && checked ? (
                            "✓"
                          ) : action === "pinTab" ? (
                            <Pin size={13} />
                          ) : (
                            ""
                          )}
                        </span>
                        <span>{t(labelKey)}</span>
                      </button>
                    );
                  })
                : contextMenu.kind === "dbx-grid-cell"
                  ? (
                      [
                        ["copyValue", "database.copyValue"],
                        ["copyColumnName", "database.copyColumnName"],
                        ["previewValue", "database.previewValue"],
                        ["previewRow", "database.openRowDetailsDialog"],
                        ["previewColumn", "database.openColumnDetailsDialog"],
                        ["sortAscending", "database.sortAscending"],
                        ["sortDescending", "database.sortDescending"],
                        ...(dbxGridOrderByInput.trim()
                          ? ([["clearSort", "database.clearSort"]] as const)
                          : []),
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
                          ? ([
                              [
                                "copyRowInsertWithoutPrimaryKeys",
                                "database.copyRowInsertWithoutPrimaryKeys",
                              ],
                            ] as const)
                          : []),
                        ["copyRowUpdate", "database.copyRowUpdate"],
                        ["copyAllTsv", "database.copyAllTsv"],
                      ] as const
                    ).map(([action, labelKey]) => {
                      const multiRowLabelKey =
                        dbxGridCellContextRowCount > 1 && action === "copyRowJson"
                          ? "database.copyRows"
                          : dbxGridCellContextRowCount > 1 && action === "copyRowInsert"
                            ? "database.copyRowsInsert"
                            : dbxGridCellContextRowCount > 1 &&
                                action === "copyRowInsertWithoutPrimaryKeys"
                              ? "database.copyRowsInsertWithoutPrimaryKeys"
                              : dbxGridCellContextRowCount > 1 && action === "copyRowUpdate"
                                ? "database.copyRowsUpdate"
                                : labelKey;
                      const disabled =
                        (action === "copyRowUpdate" && activeDbxGridPrimaryKeys.length === 0) ||
                        ((action === "sortAscending" ||
                          action === "sortDescending" ||
                          action === "clearSort") &&
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
                          {(action === "previewValue" ||
                            action === "previewRow" ||
                            action === "previewColumn") && <Eye size={13} />}
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
                    ? (
                        [
                          ["copyName", "database.copyName"],
                          [
                            "dropTableChildObject",
                            dbxTableChildDropLabelKey(contextMenu.childObjectType),
                          ],
                        ] as const
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
                            ...(action === "dropTableChildObject"
                              ? { color: "var(--danger)" }
                              : {}),
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
                      ? (
                          [
                            ...(contextMenu.groupKey === "tables"
                              ? ([["createTable", "database.createTable"]] as const)
                              : []),
                            ...(contextMenu.groupKey === "views"
                              ? ([["createView", "database.createView"]] as const)
                              : []),
                            ["refresh", "database.refresh"],
                          ] as const
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
                            }}
                            onClick={() => {
                              void runDbxObjectGroupContextMenuAction(action);
                            }}
                          >
                            {(action === "createTable" || action === "createView") && (
                              <Plus size={13} />
                            )}
                            {action === "refresh" && <RefreshCcw size={13} />}
                            <span>{t(labelKey)}</span>
                          </button>
                        ))
                      : contextMenu.kind === "connection-group"
                        ? (
                            [
                              ["copyName", "database.copyName"],
                              ["newConnection", "database.newConnection"],
                              ["newGroup", "database.newConnectionGroup"],
                              ["renameGroup", "database.renameConnectionGroup"],
                              ["deleteGroup", "database.deleteConnectionGroup"],
                            ] as const
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
                                ...(action === "deleteGroup" ? { color: "var(--danger)" } : {}),
                              }}
                              onClick={() => {
                                void runConnectionGroupContextMenuAction(action);
                              }}
                            >
                              {action === "copyName" && <Copy size={13} />}
                              {(action === "newConnection" || action === "newGroup") && (
                                <Plus size={13} />
                              )}
                              <span>{t(labelKey)}</span>
                            </button>
                          ))
                        : contextMenu.kind === "redis-key" && contextMenuNoSqlConnection
                          ? (
                              [
                                ["copyName", "database.copyName"],
                                ["openWorkspace", "database.openWorkspace"],
                                ["refresh", "database.refresh"],
                                ["deleteRedisKey", "database.redisDeleteKey"],
                              ] as const
                            ).map(([action, labelKey]) => (
                              <button
                                key={action}
                                type="button"
                                role="menuitem"
                                disabled={
                                  action === "deleteRedisKey" && contextMenuNoSqlConnection.readOnly
                                }
                                style={{
                                  ...s.fileCtxMenuItem,
                                  display: "flex",
                                  alignItems: "center",
                                  gap: 8,
                                  ...(action === "deleteRedisKey"
                                    ? { color: "var(--danger)" }
                                    : {}),
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
                            ? (
                                [
                                  ["copyName", "database.copyName"],
                                  ["openWorkspace", "database.openWorkspace"],
                                  ["refresh", "database.refresh"],
                                  ["deleteDocument", "database.mongoDeleteDocument"],
                                ] as const
                              ).map(([action, labelKey]) => (
                                <button
                                  key={action}
                                  type="button"
                                  role="menuitem"
                                  disabled={
                                    action === "deleteDocument" &&
                                    (contextMenuNoSqlConnection.readOnly ||
                                      mongoDocumentRawId(contextMenu.document) == null)
                                  }
                                  style={{
                                    ...s.fileCtxMenuItem,
                                    display: "flex",
                                    alignItems: "center",
                                    gap: 8,
                                    ...(action === "deleteDocument"
                                      ? { color: "var(--danger)" }
                                      : {}),
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
                            : (contextMenu.kind === "redis-database" ||
                                  contextMenu.kind === "mongo-database" ||
                                  contextMenu.kind === "mongo-collection") &&
                                contextMenuNoSqlConnection
                              ? (contextMenu.kind === "mongo-collection"
                                  ? noSqlCollectionContextMenuItems(contextMenuTreeNodePinned)
                                  : noSqlDatabaseContextMenuItems(
                                      contextMenu,
                                      contextMenuNoSqlConnection,
                                      contextMenuTreeNodePinned,
                                    )
                                ).map(([action, labelKey]) => (
                                  <button
                                    key={action}
                                    type="button"
                                    role="menuitem"
                                    disabled={
                                      action === "flushRedisDb" &&
                                      contextMenuNoSqlConnection.readOnly
                                    }
                                    style={{
                                      ...s.fileCtxMenuItem,
                                      display: "flex",
                                      alignItems: "center",
                                      gap: 8,
                                      ...(action === "flushRedisDb"
                                        ? { color: "var(--danger)" }
                                        : {}),
                                    }}
                                    onClick={() => {
                                      void runNoSqlContextMenuAction(action);
                                    }}
                                  >
                                    {action === "togglePin" && <Pin size={13} />}
                                    {action === "copyName" && <Copy size={13} />}
                                    {action === "newQuery" && <FilePlus size={13} />}
                                    {action === "openWorkspace" && <Play size={13} />}
                                    {(action === "setDefaultDatabase" ||
                                      action === "clearDefaultDatabase") && <Database size={13} />}
                                    {action === "refresh" && <RefreshCcw size={13} />}
                                    {action === "flushRedisDb" && <Eraser size={13} />}
                                    <span>{t(labelKey)}</span>
                                  </button>
                                ))
                              : contextMenu.kind === "dbx-column" && contextMenuDbxColumnConnection
                                ? (
                                    [
                                      ["copyName", "database.copyName"],
                                      ["openFieldLineage", "database.openFieldLineage"],
                                      ["dropColumn", "database.dropColumn"],
                                    ] as const
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
                                        ...(action === "dropColumn"
                                          ? { color: "var(--danger)" }
                                          : {}),
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
                                          ...(action === "dropTable" || action === "dropObject"
                                            ? { color: "var(--danger)" }
                                            : {}),
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
                                  : contextMenu.kind === "dbx-schema" &&
                                      contextMenuDbxSchemaConnection
                                    ? dbxSchemaContextMenuItems(
                                        contextMenuDbxSchemaConnection,
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
                                            ...(action === "dropSchema"
                                              ? { color: "var(--danger)" }
                                              : {}),
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
                                    : contextMenu.kind === "dbx-database" &&
                                        contextMenuDbxDatabaseConnection
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
                                              ...(action === "dropDatabase"
                                                ? { color: "var(--danger)" }
                                                : {}),
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
                                        ? ([["userAdmin", "database.openUserAdmin"]] as const).map(
                                            ([action, labelKey]) => (
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
                                                  void runContextMenuAction(action);
                                                }}
                                              >
                                                <UsersRound size={13} />
                                                <span>{t(labelKey)}</span>
                                              </button>
                                            ),
                                          )
                                        : [
                                            ...(contextMenuDbxConnection
                                              ? ([
                                                  [
                                                    "togglePin",
                                                    contextMenuDbxConnection.pinned
                                                      ? "database.unpinConnection"
                                                      : "database.pinConnection",
                                                  ],
                                                ] as const)
                                              : []),
                                            [
                                              contextMenuConnectionActive ? "close" : "open",
                                              contextMenuConnectionActive
                                                ? "database.closeConnection"
                                                : "database.openConnection",
                                            ],
                                            ["newQuery", "database.newQuery"],
                                            ["queryHistory", "database.queryHistory"],
                                            ...(supportsDbxUserAdmin(
                                              contextMenuDbxConnection?.dbType,
                                            )
                                              ? ([["userAdmin", "database.userAdmin"]] as const)
                                              : []),
                                            ...(contextMenuDbxConnection &&
                                            hasEnabledDbxTransportLayers(
                                              contextMenuDbxConnection,
                                            ) &&
                                            dbxConnectionFinalProxyPort(contextMenuDbxConnection) !=
                                              null
                                              ? ([
                                                  [
                                                    "copyFinalProxyPort",
                                                    "database.copyFinalProxyPort",
                                                  ],
                                                ] as const)
                                              : []),
                                            ["executeSqlFile", "database.executeSqlFile"],
                                            ...(canCreateDatabaseForConnection(
                                              contextMenuDbxConnection,
                                            )
                                              ? ([
                                                  [
                                                    "createDatabase",
                                                    contextMenuDbxConnection?.dbType === "duckdb"
                                                      ? "database.createDuckDbFile"
                                                      : "database.createDatabase",
                                                  ],
                                                ] as const)
                                              : []),
                                            ...(contextMenuDbxConnection
                                              ? ([
                                                  [
                                                    "moveToGroup",
                                                    contextMenuDbxConnectionHasMoveTargets
                                                      ? "database.moveToGroup"
                                                      : "database.moveToNewGroup",
                                                  ],
                                                ] as const)
                                              : []),
                                            ["refresh", "database.refresh"],
                                            ...(contextMenuDbxConnection
                                              ? ([
                                                  [
                                                    "selectVisibleDatabases",
                                                    "database.selectVisibleDatabases",
                                                  ],
                                                ] as const)
                                              : []),
                                            ...(contextMenuDbxConnection
                                              ? ([["edit", "database.editConnection"]] as const)
                                              : []),
                                            ...(dbxConnectionLocalFilePath(contextMenuDbxConnection)
                                              ? ([
                                                  [
                                                    "revealDatabaseFile",
                                                    "database.revealDatabaseFile",
                                                  ],
                                                ] as const)
                                              : []),
                                            ...(sqliteBackupSourcePath(contextMenuDbxConnection)
                                              ? ([
                                                  [
                                                    "backupSqliteDatabase",
                                                    "database.backupSqliteDatabase",
                                                  ],
                                                ] as const)
                                              : []),
                                            ["copy", "database.duplicateConnection"],
                                            ["delete", "database.deleteConnection"],
                                          ].map(([action, labelKey]) => (
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
                                                void runContextMenuAction(
                                                  action as Parameters<
                                                    typeof runContextMenuAction
                                                  >[0],
                                                );
                                              }}
                                            >
                                              {action === "copy" && <Copy size={13} />}
                                              <span>{t(labelKey)}</span>
                                            </button>
                                          ))}
          </div>
        </>
      )}
    </div>
  );
}
