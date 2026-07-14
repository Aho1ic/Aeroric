export interface DbSshConnection {
  id: string;
  name: string;
  group?: string;
  host: string;
  port: number;
  username: string;
  identityFile?: string;
  password?: string;
  remotePath?: string;
  createdAt: number;
  lastConnectedAt?: number;
}

export type DbEndpoint =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connection: DbSshConnection; path: string; projectPath?: string };

export type DbxDatabaseType =
  | "sqlite"
  | "mysql"
  | "postgres"
  | "duckdb"
  | "redis"
  | "mongodb"
  | "sqlserver"
  | "oracle"
  | "clickhouse";

export interface DbConnectionConfig {
  id: string;
  name: string;
  endpoint: DbEndpoint;
  readOnly?: boolean;
  createdAt: number;
  lastOpenedAt?: number | null;
}

export interface AeroricDbConnectionConfig extends Omit<
  DbConnectionConfig,
  "endpoint" | "readOnly"
> {
  endpoint?: DbEndpoint;
  projectScope?: {
    kind: "local" | "ssh" | string;
    projectRoot?: string | null;
    remoteProjectPath?: string | null;
    sshConnectionId?: string | null;
  } | null;
  dbType: DbxDatabaseType;
  readOnly: boolean;
  dbx?: unknown;
  migratedFromLegacy?: boolean;
  connectionGroup?: string | null;
  pinned?: boolean;
}

export interface DbColumn {
  name: string;
  dataType: string;
  nullable: boolean;
  notNull: boolean;
  primaryKey: boolean;
  primaryKeyOrdinal: number;
  defaultValue?: string | null;
}

export interface DbIndex {
  name: string;
  unique: boolean;
  columns: string[];
}

export interface DbForeignKey {
  table: string;
  from: string;
  to: string;
  onUpdate: string;
  onDelete: string;
}

export interface DbTrigger {
  name: string;
  sql?: string | null;
}

export interface DbObject {
  name: string;
  objectType: "table" | "view" | string;
  columns: DbColumn[];
  indexes: DbIndex[];
  foreignKeys: DbForeignKey[];
  triggers: DbTrigger[];
  ddl?: string | null;
  rowCount?: number | null;
  editable: boolean;
  primaryKeys: string[];
  hasRowId: boolean;
}

export interface DbSchema {
  objects: DbObject[];
}

export interface DbRow {
  rowId?: number | null;
  keyValues: Array<{ column: string; value: unknown }>;
  values: unknown[];
}

export type DbCellValue = string | null;

export interface DbQueryResult {
  columns: string[];
  columnTypes?: string[];
  columnSortables?: boolean[];
  rows: DbRow[];
  page: number;
  pageSize: number;
  totalRows?: number | null;
  editable: boolean;
  primaryKeys: string[];
  hasRowId: boolean;
}

export interface DbExecuteResult {
  columns: string[];
  rows: DbRow[];
  rowsAffected: number;
  message: string;
}

export interface DbxDatabaseInfo {
  name: string;
}

export interface DbxObjectInfo {
  name: string;
  object_type: string;
  schema?: string | null;
  signature?: string | null;
  comment?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
  parent_schema?: string | null;
  parent_name?: string | null;
}

export type DbxObjectSourceKind =
  | "VIEW"
  | "PROCEDURE"
  | "FUNCTION"
  | "SEQUENCE"
  | "PACKAGE"
  | "PACKAGE_BODY";

export interface DbxObjectSource {
  name: string;
  object_type: DbxObjectSourceKind;
  schema?: string | null;
  source: string;
}

export interface DbxColumnInfo {
  name: string;
  data_type: string;
  is_nullable: boolean;
  column_default?: string | null;
  comment?: string | null;
  is_primary_key: boolean;
  extra?: string | null;
  numeric_precision?: number | null;
  numeric_scale?: number | null;
  character_maximum_length?: number | null;
}

export interface DatabaseSearchColumn {
  name: string;
  data_type: string;
  is_primary_key?: boolean;
}

export interface DatabaseSearchSqlOptions {
  databaseType?: DbxDatabaseType | null;
  schema?: string | null;
  tableName: string;
  columns: DatabaseSearchColumn[];
  term: string;
  limit?: number | null;
}

export interface DatabaseSearchSql {
  sql: string;
  searchableColumns: string[];
}

export interface SearchResultWhereOptions {
  databaseType?: DbxDatabaseType | null;
  columns: DatabaseSearchColumn[];
  resultColumns: string[];
  row: unknown[];
  matchedColumns?: string[];
}

export interface DbxQueryResult {
  columns: string[];
  column_types: string[];
  column_sortables: boolean[];
  rows: unknown[][];
  affected_rows: number;
  execution_time_ms: number;
  truncated: boolean;
  session_id?: string | null;
  has_more: boolean;
}

export interface ExecuteQueryRequest {
  connectionId: string;
  database?: string | null;
  sql: string;
  schema?: string | null;
  maxRows?: number;
  fetchSize?: number;
  pageSize?: number;
  resultSessionId?: string | null;
  clientSessionId?: string | null;
  timeoutSecs?: number;
  executionId?: string | null;
}

export interface ExecuteMultiRequest extends ExecuteQueryRequest {
  useTransaction?: boolean | null;
}

export interface AssessProductionSqlRequest {
  connectionId: string;
  database?: string | null;
  sql: string;
}

export interface AssessProductionTargetRequest {
  connectionId: string;
  database?: string | null;
}

export interface ProductionSqlAssessment {
  requiresConfirmation: boolean;
  isMutation: boolean;
  productionDatabases: string[];
}

export interface ProductionTargetAssessment {
  requiresConfirmation: boolean;
  productionDatabases: string[];
}

export interface DbxTransferRequest {
  transferId: string;
  sourceConnectionId: string;
  sourceDatabase: string;
  sourceSchema: string;
  targetConnectionId: string;
  targetDatabase: string;
  targetSchema: string;
  tables: string[];
  createTable: boolean;
  mode?: "append" | "overwrite" | "upsert";
  targetTableNameCase?: "preserve" | "lower" | "upper";
  ownershipPolicy?: "preserve" | "skip" | "reassignMissing";
  batchSize: number;
}

export interface DbxTransferProgress {
  transferId: string;
  table: string;
  tableIndex: number;
  totalTables: number;
  rowsTransferred: number;
  totalRows?: number | null;
  status: "running" | "tableDone" | "done" | "error" | "cancelled";
  error?: string | null;
  terminal: boolean;
}

export interface TableDataRequest {
  connectionId: string;
  catalog?: string | null;
  database?: string | null;
  schema?: string | null;
  table: string;
  page?: number;
  pageSize?: number;
  orderBy?: string | null;
  whereInput?: string | null;
}

export interface TableDataResponse {
  result: DbxQueryResult;
  totalRows?: number | null;
  sql: string;
  countSql: string;
}

export interface DataGridColumnInfo {
  name: string;
  data_type?: string;
  is_nullable?: boolean;
  is_primary_key?: boolean;
  column_default?: string | null;
  extra?: string | null;
}

export interface DataGridTableMeta {
  schema?: string | null;
  tableName: string;
  primaryKeys?: string[];
  columns?: DataGridColumnInfo[];
}

export interface DataGridSaveStatementOptions {
  databaseType?: string | null;
  tableMeta: DataGridTableMeta;
  columns: string[];
  sourceColumns?: Array<string | null>;
  rows?: unknown[][];
  dirtyRows?: Array<[number, Array<[number, unknown]>]>;
  deletedRows?: number[];
  newRows?: unknown[][];
}

export interface DataGridCopyUpdateStatementOptions {
  databaseType?: string | null;
  tableMeta: DataGridTableMeta;
  columns: string[];
  sourceColumns?: Array<string | null>;
  rows?: unknown[][];
}

export interface DataGridCopyInsertStatementOptions {
  databaseType?: string | null;
  tableMeta?: DataGridTableMeta | null;
  columns: string[];
  sourceColumns?: Array<string | null>;
  rows?: unknown[][];
  excludePrimaryKeys?: boolean;
}

export type DataGridContextFilterMode =
  | "equals"
  | "not-equals"
  | "is-null"
  | "is-not-null"
  | "like"
  | "not-like"
  | "less-than"
  | "greater-than";

export interface DataGridContextFilterConditionOptions {
  databaseType?: string | null;
  columnName: string;
  mode: DataGridContextFilterMode;
  value: unknown;
  columnInfo?: DataGridColumnInfo | null;
}

export interface GridSaveRequest {
  connectionId: string;
  database?: string | null;
  schema?: string | null;
  options: DataGridSaveStatementOptions;
  execute?: boolean;
}

export interface SqlPreviewResponse {
  statements: string[];
  rollbackStatements: string[];
  validationError?: string | null;
  executionSchema?: string | null;
  executed: boolean;
  rowsAffected: number;
}

export interface TableExportRequest {
  exportId: string;
  connectionId: string;
  database: string;
  schema?: string | null;
  tableName: string;
  filePath: string;
  format: string;
  columns?: string[] | null;
  columnTypes?: Array<string | null> | null;
  primaryKeys?: string[] | null;
  whereInput?: string | null;
  orderBy?: string | null;
  skipCount?: boolean;
  batchSize?: number | null;
}

export interface DatabaseExportRequest {
  exportId: string;
  connectionId: string;
  database: string;
  schema: string;
  filePath: string;
  selectedTables?: string[];
  includeStructure: boolean;
  includeData: boolean;
  includeObjects: boolean;
  dropTableIfExists?: boolean;
  batchSize: number;
}

export interface ExecuteSqlFileRequest {
  connectionId: string;
  database?: string | null;
  schema?: string | null;
  path: string;
  timeoutSecs?: number;
}

export interface TableImportColumnMapping {
  sourceColumn: string;
  targetColumn: string;
}

export type TableImportMode = "append" | "truncate";

export interface TableImportRequest {
  importId: string;
  connectionId: string;
  database: string;
  schema: string;
  table: string;
  filePath: string;
  mappings: TableImportColumnMapping[];
  mode: TableImportMode;
  batchSize: number;
}

export interface TableImportPreview {
  fileName: string;
  filePath: string;
  fileType: string;
  sizeBytes: number;
  columns: string[];
  rows: unknown[][];
  totalRows: number;
}

export interface TableImportSummary {
  importId: string;
  rowsImported: number;
  totalRows: number;
}

export interface RedisDatabaseInfo {
  db: number;
  keys: number;
}

export interface RedisKeyInfo {
  key_display: string;
  key_raw: string;
  key_type: string;
  ttl: number;
  size: number;
  value_preview: string;
}

export interface RedisScanResult {
  cursor: number;
  keys: RedisKeyInfo[];
  total_keys: number;
}

export interface RedisValue {
  key_display: string;
  key_raw: string;
  key_type: string;
  ttl: number;
  value_is_binary: boolean;
  value: unknown;
  total?: number | null;
  scan_cursor?: number | null;
}

export interface RedisScanKeysRequest {
  connectionId: string;
  db: number;
  cursor?: number;
  pattern?: string | null;
  count?: number | null;
}

export interface RedisLoadMoreRequest {
  connectionId: string;
  db: number;
  keyRaw: string;
  keyType: string;
  cursor: number;
  count?: number | null;
  filter?: string | null;
}

export type RedisCollectionPage =
  | { kind: "list"; items: unknown[]; scan_cursor?: number | null }
  | { kind: "set"; items: unknown[]; scan_cursor?: number | null }
  | { kind: "hash"; items: unknown[]; scan_cursor?: number | null }
  | { kind: "zset"; items: unknown[]; scan_cursor?: number | null };

export interface RedisHashFieldRequest {
  connectionId: string;
  db: number;
  keyRaw: string;
  field: string;
}

export interface RedisHashSetRequest extends RedisHashFieldRequest {
  value: string;
  ttl?: number | null;
}

export interface RedisListIndexRequest {
  connectionId: string;
  db: number;
  keyRaw: string;
  index: number;
}

export interface RedisListPushRequest {
  connectionId: string;
  db: number;
  keyRaw: string;
  value: string;
  ttl?: number | null;
}

export interface RedisListSetRequest extends RedisListIndexRequest {
  value: string;
}

export interface RedisSetMemberRequest {
  connectionId: string;
  db: number;
  keyRaw: string;
  member: string;
}

export interface RedisSetAddRequest extends RedisSetMemberRequest {
  ttl?: number | null;
}

export interface RedisZaddRequest extends RedisSetAddRequest {
  score: number;
}

export interface RedisKeyRequest {
  connectionId: string;
  db: number;
  keyRaw: string;
}

export interface RedisSetValueRequest extends RedisKeyRequest {
  value: string;
  ttl?: number | null;
}

export interface RedisSetTtlRequest extends RedisKeyRequest {
  ttl: number;
}

export type RedisCreateKeyType = "string" | "hash" | "list" | "set" | "zset" | "stream" | "json";

export interface RedisCreateKeyRequest {
  connectionId: string;
  db: number;
  keyRaw: string;
  keyType: RedisCreateKeyType;
  value: string;
  field?: string | null;
  score?: number | null;
  entryId?: string | null;
  ttl?: number | null;
}

export interface RedisCommandRequest {
  connectionId: string;
  db: number;
  command: string;
  skipSafetyCheck?: boolean;
}

export interface RedisCommandResult {
  command: string;
  safety: "allowed" | "confirm" | "blocked" | string;
  value: unknown;
}

export interface MongoDocumentResult {
  documents: unknown[];
  total: number;
}

export interface MongoFindDocumentsRequest {
  connectionId: string;
  database: string;
  collection: string;
  skip?: number;
  limit?: number;
  filter?: string | null;
  projection?: string | null;
  sort?: string | null;
  executionId?: string | null;
}

export interface MongoInsertDocumentRequest {
  connectionId: string;
  database: string;
  collection: string;
  docJson: string;
}

export interface MongoUpdateDocumentRequest extends MongoInsertDocumentRequest {
  id: string;
  routing?: string | null;
}

export interface DbxListObjectsOptions {
  filter?: string | null;
  limit?: number | null;
  offset?: number | null;
  objectTypes?: string[] | null;
}

export interface MongoDeleteDocumentsRequest {
  connectionId: string;
  database: string;
  collection: string;
  filterJson: string;
  many?: boolean;
}

export interface EditableStructureColumn {
  id: string;
  name: string;
  dataType: string;
  isNullable: boolean;
  defaultValue?: string;
  comment?: string;
  isPrimaryKey?: boolean;
  original?: {
    name: string;
    data_type: string;
    is_nullable: boolean;
    column_default?: string | null;
    is_primary_key?: boolean;
    extra?: string | null;
    comment?: string | null;
  } | null;
  originalPosition?: number | null;
  markedForDrop?: boolean;
}

export interface TableStructureSqlOptions {
  databaseType?: string | null;
  schema?: string | null;
  tableName: string;
  columns: EditableStructureColumn[];
  indexes?: unknown[];
  foreignKeys?: unknown[];
  triggers?: unknown[];
  tableComment?: string | null;
  originalTableComment?: string | null;
}

export interface SingleColumnAlterSqlOptions {
  databaseType?: string | null;
  schema?: string | null;
  tableName: string;
  column: EditableStructureColumn;
}

export interface TableStructureSqlResult {
  statements: string[];
  warnings: string[];
}

export type DatabaseObjectType =
  | "TABLE"
  | "VIEW"
  | "PROCEDURE"
  | "FUNCTION"
  | "SEQUENCE"
  | "PACKAGE"
  | "PACKAGE_BODY"
  | string;
export type TableChildObjectType = "COLUMN" | "INDEX" | "FOREIGN_KEY" | "TRIGGER";

export interface CreateDatabaseSqlOptions {
  databaseType?: string | null;
  driverProfile?: string | null;
  name: string;
  charset?: string | null;
  collation?: string | null;
}

export interface DuckDbAttachDatabaseSqlOptions {
  path: string;
  name: string;
}

export interface RenameObjectSqlOptions {
  databaseType?: string | null;
  objectType: DatabaseObjectType;
  schema?: string | null;
  oldName: string;
  newName: string;
}

export interface DatabaseNameSqlOptions {
  databaseType?: string | null;
  name: string;
}

export interface SchemaNameSqlOptions {
  databaseType?: string | null;
  name: string;
}

export interface TableAdminSqlOptions {
  databaseType?: string | null;
  schema?: string | null;
  tableName: string;
}

export interface DropObjectSqlOptions {
  databaseType?: string | null;
  objectType: DatabaseObjectType;
  schema?: string | null;
  name: string;
}

export interface DropTableChildObjectSqlOptions {
  databaseType?: string | null;
  objectType: TableChildObjectType;
  schema?: string | null;
  tableName: string;
  name: string;
}

export interface DuplicateTableStructureSqlOptions {
  databaseType?: string | null;
  schema?: string | null;
  sourceName: string;
  targetName: string;
}

export type DriverRuntimeMode = "native" | "file" | "jdbc" | "agent" | string;
export type DriverSupportLevel = "operate" | "connect" | "experimental" | "optional" | string;

export interface DatabaseDriverCapabilities {
  queryExecution?: boolean;
  metadataBrowse?: boolean;
  objectBrowser?: boolean;
  objectSource?: boolean;
  schemaSearch?: boolean;
  diagram?: boolean;
  tableDataEdit?: boolean;
  tableStructureEdit?: boolean;
  tableImport?: boolean;
  dataTransfer?: boolean;
  sqlFileExecution?: boolean;
  databaseCreate?: boolean;
  fieldLineage?: boolean;
  sqlExplain?: boolean;
  userAdmin?: boolean;
  driverManagement?: boolean;
  [key: string]: boolean | undefined;
}

export interface DatabaseDriverManifestEntry {
  dbType: string;
  label: string;
  runtimeMode: DriverRuntimeMode;
  mcpMode?: string;
  singleConnectionPool?: boolean;
  metadataConnectionScoped?: boolean;
  skipTcpProbe?: boolean;
  defaultPort?: number;
  supportLevel: DriverSupportLevel;
  capabilities: DatabaseDriverCapabilities;
}

export interface DatabaseDriverManifest {
  schemaVersion: number;
  drivers: DatabaseDriverManifestEntry[];
}
