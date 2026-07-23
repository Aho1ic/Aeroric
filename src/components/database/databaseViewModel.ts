import type { CSSProperties } from "react";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DbxColumnInfo,
  DbxDatabaseType,
  DbxDatabaseInfo,
  DbxListObjectsOptions,
  DbxObjectInfo,
  DbxObjectSourceKind,
  DbxQueryResult,
  EditableStructureColumn,
  TableChildObjectType,
} from "../../types";
import { databaseApi } from "../../lib/databaseApi";
import { createConnectionName, quoteSqlName } from "../../lib/databaseUtils";
import s from "../../styles";
import { dbxConfigRecord, dbxString } from "./databaseConnectionDraft";
import type {
  DbxGridCellContextMenuState,
  DbxGridHeaderContextMenuState,
  TableExportFormat,
} from "./databaseGridState";

export const PAGE_SIZE = 100;
export const MONGO_SIDEBAR_DOCUMENT_PREVIEW_LIMIT = 20;
export const DATABASE_SIDEBAR_DEFAULT_WIDTH = 284;
export const DATABASE_SIDEBAR_MIN_WIDTH = 220;
export const DATABASE_SIDEBAR_MAX_WIDTH = 520;
export const EMPTY_DBX_COLUMNS: DbxColumnInfo[] = [];
export const DBX_OBJECT_PAGE_SIZE = 200;
export const PRODUCTION_SQL_PREVIEW_LIMIT = 2000;
export const DBX_TABLE_LIKE_OBJECT_TYPES = ["TABLE", "VIEW", "MATERIALIZED_VIEW"];
export const DBX_KEYLESS_GRID_EDIT_DB_TYPES = new Set<DbxDatabaseType>([
  "sqlite",
  "mysql",
  "postgres",
  "duckdb",
  "sqlserver",
  "oracle",
]);
export type RedisSidebarScanState = { cursor: number; totalKeys: number };
export type MongoSidebarDocumentQuery = { filter: string; sort: string; projection: string };
export type DbxObjectGroupKey =
  | "tables"
  | "views"
  | "procedures"
  | "functions"
  | "sequences"
  | "packages";
export type DbWorkspaceMode =
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

export function productionSqlPreview(sql: string): string {
  const trimmed = sql.trim();
  if (trimmed.length <= PRODUCTION_SQL_PREVIEW_LIMIT) return trimmed;
  return `${trimmed.slice(0, PRODUCTION_SQL_PREVIEW_LIMIT)}\n...`;
}

export async function listAllDbxObjects(
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
export type DatabaseContextMenuState =
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
export type DbxContextMenuItem<Action extends string> = [action: Action, labelKey: string];
export type WorkspaceTabContextMenuAction =
  | "toggleShortTitle"
  | "pinTab"
  | "closeTab"
  | "closeOtherTabs"
  | "closeAllTabs";
export type DbxDatabaseContextMenuAction =
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
export type DbxSchemaContextMenuAction =
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
export type NoSqlContextMenuAction =
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
export type NoSqlDatabaseContextMenuState = Extract<
  NonNullable<DatabaseContextMenuState>,
  { kind: "redis-database" | "mongo-database" }
>;
export type TableInfoTab = "columns" | "indexes" | "foreignKeys" | "triggers" | "ddl";
export type DbxObjectContextMenuAction =
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
export type DbxObjectContextMenuItem = [action: DbxObjectContextMenuAction, labelKey: string];

export const PINNED_TREE_NODE_IDS_STORAGE_KEY = "aeroric:database:pinned-nosql-tree-nodes";
export const EXTRA_DBX_CONNECTION_GROUPS_STORAGE_KEY =
  "aeroric:database:extra-dbx-connection-groups";

export function loadPinnedTreeNodeIds() {
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

export function savePinnedTreeNodeIds(ids: Set<string>) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(PINNED_TREE_NODE_IDS_STORAGE_KEY, JSON.stringify([...ids]));
}

export function loadExtraDbxConnectionGroups() {
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

export function saveExtraDbxConnectionGroups(groups: string[]) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(EXTRA_DBX_CONNECTION_GROUPS_STORAGE_KEY, JSON.stringify(groups));
}

export function contextMenuPinnedNodeId(menu: DatabaseContextMenuState): string | null {
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

export function contextMenuConnectionId(menu: DatabaseContextMenuState): string | null {
  return menu && "connectionId" in menu ? menu.connectionId : null;
}

export type QueryHistoryEntry = {
  id: string;
  sql: string;
  connectionName: string;
  database: string | null;
  schema: string | null;
  executedAt: number;
  rowsAffected?: number | null;
  executionTimeMs?: number | null;
};

export const MYSQL_COMPATIBLE_CREATE_DATABASE_PROFILES = new Set([
  "mysql",
  "mariadb",
  "tidb",
  "oceanbase",
  "doris",
  "starrocks",
  "custom_mysql",
]);
export const CREATE_DATABASE_DB_TYPES = new Set([
  "mysql",
  "postgres",
  "sqlserver",
  "oracle",
  "clickhouse",
  "duckdb",
]);
export const DBX_DATABASE_CREATE_NODE_DB_TYPES = new Set<DbxDatabaseType>([
  "mysql",
  "postgres",
  "sqlserver",
  "oracle",
  "clickhouse",
]);
export const DBX_TABLE_STRUCTURE_DB_TYPES = new Set<DbxDatabaseType>([
  "sqlite",
  "mysql",
  "postgres",
  "duckdb",
  "sqlserver",
  "oracle",
  "clickhouse",
]);
export const DBX_SCHEMA_AWARE_DB_TYPES = new Set<DbxDatabaseType>([
  "postgres",
  "sqlserver",
  "oracle",
  "duckdb",
]);
export const DBX_TREE_SCHEMA_DB_TYPES = new Set<DbxDatabaseType>([
  "postgres",
  "sqlserver",
  "duckdb",
]);
export const DBX_DIAGRAM_DB_TYPES = new Set<DbxDatabaseType>([
  "sqlite",
  "mysql",
  "postgres",
  "sqlserver",
  "oracle",
]);
export const DBX_NO_TRUNCATE_DB_TYPES = new Set<string>([
  "sqlite",
  "rqlite",
  "turso",
  "duckdb",
  "influxdb",
  "manticoresearch",
]);
export const SYSTEM_DATABASE_NAMES: Partial<Record<DbxDatabaseType, ReadonlySet<string>>> = {
  mysql: new Set(["information_schema", "mysql", "performance_schema", "sys"]),
  postgres: new Set(["template0", "template1"]),
  clickhouse: new Set(["information_schema", "system"]),
  sqlserver: new Set(["master", "model", "msdb", "tempdb"]),
  mongodb: new Set(["admin", "config", "local"]),
};
export const DATA_TOOL_EXPORT_FORMATS: Array<{
  format: TableExportFormat;
  labelKey: string;
  label: string;
}> = [
  { format: "csv", labelKey: "database.exportCsv", label: "CSV" },
  { format: "json", labelKey: "database.exportJson", label: "JSON" },
  { format: "insertSql", labelKey: "database.exportInsertSql", label: "SQL INSERT" },
  { format: "xlsx", labelKey: "database.exportXlsx", label: "XLSX" },
];
export function endpointLabel(endpoint: DbEndpoint): string {
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

export function dbxRowsToDatabaseRows(rows: unknown[][]) {
  return rows.map((row) => ({ rowId: null, keyValues: [], values: row }));
}

export function dbxColumnsToDbColumns(columns: DbxColumnInfo[]) {
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

export function dbxQueryToExecuteResult(result: DbxQueryResult, message?: string): DbExecuteResult {
  return {
    columns: result.columns,
    rows: dbxRowsToDatabaseRows(result.rows),
    rowsAffected: result.affected_rows,
    message: message ?? `${result.affected_rows} affected · ${result.execution_time_ms} ms`,
  };
}

export function dbxSqlFileResultsToExecuteResult(results: DbxQueryResult[]): DbExecuteResult {
  const last = results[results.length - 1] ?? null;
  const statementCount = results.length;
  return {
    columns: last?.columns ?? [],
    rows: dbxRowsToDatabaseRows(last?.rows ?? []),
    rowsAffected: results.reduce((sum, item) => sum + (item.affected_rows ?? 0), 0),
    message: `${statementCount} statement${statementCount === 1 ? "" : "s"} executed`,
  };
}

export function dbxDataTypeStyle(dataType: string): CSSProperties {
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

export function isSqlDbxConnection(connection: AeroricDbConnectionConfig | null | undefined) {
  return Boolean(connection && !["redis", "mongodb"].includes(connection.dbType));
}

export function configuredTargetDatabase(
  connection: AeroricDbConnectionConfig | null | undefined,
): string | null {
  if (!connection?.dbx || typeof connection.dbx !== "object") return null;
  const database = (connection.dbx as { database?: unknown }).database;
  return typeof database === "string" && database.trim() ? database.trim() : null;
}

export function isDbxDefaultDatabase(
  connection: AeroricDbConnectionConfig | null | undefined,
  database: string,
): boolean {
  return configuredTargetDatabase(connection) === database;
}

export function configuredVisibleDatabases(
  connection: AeroricDbConnectionConfig | null | undefined,
): string[] | undefined {
  if (!connection?.dbx || typeof connection.dbx !== "object") return undefined;
  const configured = (connection.dbx as { visible_databases?: unknown }).visible_databases;
  if (!Array.isArray(configured)) return undefined;
  return configured.filter(
    (name): name is string => typeof name === "string" && name.trim().length > 0,
  );
}

export function isSystemDatabaseName(databaseType: DbxDatabaseType | undefined, name: string) {
  if (!databaseType) return false;
  return SYSTEM_DATABASE_NAMES[databaseType]?.has(name.toLowerCase()) ?? false;
}

export function filterDbxDatabasesForConnection(
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

export function normalizeVisibleDatabaseSelection(
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

export function autoMapImportColumns(
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

export function hasEnabledDbxTransportLayers(
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

export function dbxConnectionFinalProxyPort(
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

export function dbxConnectionLocalFilePath(
  connection: AeroricDbConnectionConfig | null | undefined,
): string | null {
  if (!connection || (connection.dbType !== "sqlite" && connection.dbType !== "duckdb"))
    return null;
  const path = dbxString(dbxConfigRecord(connection), "host").trim();
  if (!path || path === ":memory:") return null;
  return path;
}

export function sqliteBackupSourcePath(
  connection: AeroricDbConnectionConfig | null | undefined,
): string | null {
  if (!connection || connection.dbType !== "sqlite") return null;
  return dbxConnectionLocalFilePath(connection);
}

export function defaultSqliteBackupFileName(connection: AeroricDbConnectionConfig): string {
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

export function sqliteEndpointKey(endpoint: DbEndpoint): string {
  return endpoint.kind === "local"
    ? `local:${endpoint.path}`
    : `ssh:${endpoint.connection.id}:${endpoint.path}`;
}

export function createSqliteFileConnection(endpoint: DbEndpoint): DbConnectionConfig {
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

export function dbxDriverProfile(
  connection: AeroricDbConnectionConfig | null | undefined,
): string | null {
  if (!connection?.dbx || typeof connection.dbx !== "object") return null;
  const profile = (connection.dbx as { driver_profile?: unknown }).driver_profile;
  return typeof profile === "string" && profile.trim() ? profile.trim() : null;
}

export function canCreateDatabaseForConnection(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && CREATE_DATABASE_DB_TYPES.has(connection.dbType));
}

export function canSetCreateDatabaseCharset(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  if (!connection) return false;
  return (
    connection.dbType === "mysql" ||
    MYSQL_COMPATIBLE_CREATE_DATABASE_PROFILES.has(dbxDriverProfile(connection) ?? "")
  );
}

export function ensureDuckDbFileExtension(path: string): string {
  return /\.(duckdb|db)$/i.test(path) ? path : `${path}.duckdb`;
}

export function duckDbAttachedDatabaseNameFromPath(path: string): string {
  const fileName = path.split(/[\\/]/).pop() ?? path;
  const withoutExtension = fileName.replace(/\.(duckdb|db)$/i, "");
  const normalized = withoutExtension
    .trim()
    .replace(/[^A-Za-z0-9_]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return normalized || "duckdb_database";
}

export function uniqueDuckDbAttachedDatabaseName(
  baseName: string,
  existingNames: string[],
): string {
  const normalizedExisting = new Set(existingNames.map((name) => name.toLowerCase()));
  if (!normalizedExisting.has(baseName.toLowerCase())) return baseName;
  for (let index = 2; ; index += 1) {
    const candidate = `${baseName}_${index}`;
    if (!normalizedExisting.has(candidate.toLowerCase())) return candidate;
  }
}

export function dbxAttachedDatabaseRecords(
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

export function dbxObjectKey(object: DbxObjectInfo) {
  return object.schema ? `${object.schema}.${object.name}` : object.name;
}

export function dbxCreateTableDraft(schema: string | null): string {
  const prefix = schema ? `${schema}.` : "";
  return `CREATE TABLE ${prefix}table_name (\n  id INTEGER PRIMARY KEY,\n  name TEXT NOT NULL\n);`;
}

export function dbxCreateViewDraft(schema: string | null): string {
  const viewName = schema ? `${schema}.new_view` : "new_view";
  return `CREATE VIEW ${viewName} AS\nSELECT\n  *\nFROM table_name;\n`;
}

export function mongoDocumentId(document: unknown, fallback = 0) {
  if (document && typeof document === "object" && "_id" in document)
    return String((document as { _id: unknown })._id);
  return `#${fallback + 1}`;
}

export function mongoDocumentRawId(document: unknown): unknown | null {
  if (!document || typeof document !== "object" || !("_id" in document)) return null;
  return (document as { _id: unknown })._id;
}

export function deriveDbxSchemas(objects: DbxObjectInfo[]): string[] {
  return Array.from(
    new Set(objects.map((object) => object.schema?.trim() ?? "").filter(Boolean)),
  ).sort((left, right) =>
    left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }),
  );
}

export function isDbxTableObject(object: DbxObjectInfo | null | undefined): boolean {
  return object?.object_type?.toLowerCase() === "table";
}

export function isDbxViewObject(object: DbxObjectInfo | null | undefined): boolean {
  return object?.object_type?.toLowerCase() === "view";
}

export function normalizeDbxObjectType(objectType: string) {
  return objectType.toUpperCase().replace(/[\s-]+/g, "_");
}

export function dbxTableChildObjectType(
  object: DbxObjectInfo,
): Exclude<TableChildObjectType, "COLUMN"> | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType.includes("FOREIGN_KEY")) return "FOREIGN_KEY";
  if (objectType.includes("TRIGGER")) return "TRIGGER";
  if (objectType.includes("INDEX")) return "INDEX";
  return null;
}

export function sameDbxName(left: string | null | undefined, right: string | null | undefined) {
  return (left ?? "").trim().toLowerCase() === (right ?? "").trim().toLowerCase();
}

export function uniqueDbxObjectName(
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

export function dbxChildObjectBelongsToTable(
  childObject: DbxObjectInfo,
  tableObject: DbxObjectInfo,
) {
  if (!childObject.parent_name || !sameDbxName(childObject.parent_name, tableObject.name))
    return false;
  if (!tableObject.schema) return true;
  return !childObject.parent_schema || sameDbxName(childObject.parent_schema, tableObject.schema);
}

export function dbxTableChildDropLabelKey(objectType: TableChildObjectType) {
  if (objectType === "INDEX") return "database.dropIndex";
  if (objectType === "FOREIGN_KEY") return "database.dropForeignKey";
  if (objectType === "TRIGGER") return "database.dropTrigger";
  return "database.dropColumn";
}

export function dbxObjectSourceKind(object: DbxObjectInfo): DbxObjectSourceKind | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType === "VIEW" || objectType.includes("VIEW")) return "VIEW";
  if (objectType.includes("PROCEDURE")) return "PROCEDURE";
  if (objectType.includes("FUNCTION")) return "FUNCTION";
  if (objectType.includes("SEQUENCE")) return "SEQUENCE";
  if (objectType === "PACKAGE_BODY" || objectType.includes("PACKAGE_BODY")) return "PACKAGE_BODY";
  if (objectType.includes("PACKAGE")) return "PACKAGE";
  return null;
}

export type DbxRenameObjectType = "TABLE" | "VIEW" | "PROCEDURE" | "FUNCTION";

export function dbxObjectRenameType(object: DbxObjectInfo): DbxRenameObjectType | null {
  const objectType = normalizeDbxObjectType(object.object_type);
  if (objectType === "TABLE" || objectType.includes("TABLE")) return "TABLE";
  if (objectType === "VIEW" || objectType.includes("VIEW")) return "VIEW";
  if (objectType.includes("PROCEDURE")) return "PROCEDURE";
  if (objectType.includes("FUNCTION")) return "FUNCTION";
  return null;
}

export function canRenameDbxObject(
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

export function supportsDbxObjectBrowserTreeNode(
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

export function supportsDbxTableStructureEditing(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && DBX_TABLE_STRUCTURE_DB_TYPES.has(connection.dbType));
}

export function supportsDbxSqlFileExecution(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && isSqlDbxConnection(connection));
}

export function supportsDbxDiagram(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && DBX_DIAGRAM_DB_TYPES.has(connection.dbType));
}

export function supportsDbxDatabaseSearch(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && isSqlDbxConnection(connection));
}

export function canCreateDbxSchema(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && DBX_TREE_SCHEMA_DB_TYPES.has(connection.dbType));
}

export function canDropDbxDatabaseNode(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && DBX_DATABASE_CREATE_NODE_DB_TYPES.has(connection.dbType));
}

export function canDropDbxSchemaNode(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return canCreateDbxSchema(connection);
}

export function supportsDbxTableTruncate(
  connection: AeroricDbConnectionConfig | null | undefined,
): boolean {
  return Boolean(connection && !DBX_NO_TRUNCATE_DB_TYPES.has(connection.dbType));
}

export function isDbxRoutineLikeObject(object: DbxObjectInfo | null | undefined): boolean {
  if (!object) return false;
  const sourceKind = dbxObjectSourceKind(object);
  return sourceKind !== null && sourceKind !== "VIEW";
}

export function isDbxProcedureObject(object: DbxObjectInfo | null | undefined): boolean {
  return object ? dbxObjectSourceKind(object) === "PROCEDURE" : false;
}

export function isDbxDroppableRoutineObject(object: DbxObjectInfo | null | undefined): boolean {
  const sourceKind = object ? dbxObjectSourceKind(object) : null;
  return sourceKind === "PROCEDURE" || sourceKind === "FUNCTION";
}

export function dbxDatabaseContextMenuItems(
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

export function dbxSchemaContextMenuItems(
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

export function noSqlDatabaseContextMenuItems(
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

export function noSqlCollectionContextMenuItems(
  pinned: boolean,
): DbxContextMenuItem<NoSqlContextMenuAction>[] {
  return [["togglePin", pinned ? "database.unpin" : "database.pin"]];
}

export function dbxObjectDropLabelKey(object: DbxObjectInfo) {
  const sourceKind = dbxObjectSourceKind(object);
  if (sourceKind === "PROCEDURE") return "database.dropProcedure";
  if (sourceKind === "FUNCTION") return "database.dropFunction";
  if (sourceKind === "VIEW") return "database.dropView";
  return "database.dropObject";
}

export function dbxObjectContextMenuItems(
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

export function dbxObjectDropConfirmLabelKey(object: DbxObjectInfo) {
  const sourceKind = dbxObjectSourceKind(object);
  if (sourceKind === "PROCEDURE") return "database.confirmDropProcedure";
  if (sourceKind === "FUNCTION") return "database.confirmDropFunction";
  if (sourceKind === "VIEW") return "database.confirmDropView";
  return "database.confirmDropObject";
}

export function dbxQualifiedSqlName(object: DbxObjectInfo): string {
  return object.schema
    ? `${quoteSqlName(object.schema)}.${quoteSqlName(object.name)}`
    : quoteSqlName(object.name);
}
