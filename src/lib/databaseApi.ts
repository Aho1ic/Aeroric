import { invoke } from "@tauri-apps/api/core";
import type {
  AeroricDbConnectionConfig,
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DbQueryResult,
  DbSchema,
  DbxColumnInfo,
  DbxDatabaseInfo,
  DbxObjectInfo,
  DbxQueryResult,
  ExecuteQueryRequest,
  DataGridSaveStatementOptions,
  DatabaseDriverManifest,
  DatabaseExportRequest,
  ExecuteSqlFileRequest,
  GridSaveRequest,
  MongoDeleteDocumentsRequest,
  MongoDocumentResult,
  MongoFindDocumentsRequest,
  MongoInsertDocumentRequest,
  MongoUpdateDocumentRequest,
  RedisDatabaseInfo,
  RedisCommandRequest,
  RedisCommandResult,
  RedisCreateKeyRequest,
  RedisKeyRequest,
  RedisScanKeysRequest,
  RedisScanResult,
  RedisSetTtlRequest,
  RedisSetValueRequest,
  RedisValue,
  SqlPreviewResponse,
  TableExportRequest,
  TableImportPreview,
  TableImportRequest,
  TableImportSummary,
  TableDataRequest,
  TableDataResponse,
  TableStructureSqlOptions,
  TableStructureSqlResult,
} from "../types/database";

export const databaseApi = {
  loadConnections: () => invoke<DbConnectionConfig[]>("db_load_connections"),
  saveConnections: (connections: DbConnectionConfig[]) =>
    invoke<void>("db_save_connections", { connections }),
  inspect: (endpoint: DbEndpoint, projectRoot?: string) =>
    invoke<DbSchema>("db_inspect", { endpoint, projectRoot }),
  queryTable: (
    endpoint: DbEndpoint,
    table: string,
    page: number,
    pageSize: number,
    projectRoot?: string,
  ) =>
    invoke<DbQueryResult>("db_query_table", {
      endpoint,
      table,
      page,
      pageSize,
      projectRoot,
    }),
  executeSql: (params: {
    endpoint: DbEndpoint;
    sql: string;
    page: number;
    pageSize: number;
    readOnly: boolean;
    projectRoot?: string;
  }) => invoke<DbExecuteResult>("db_execute_sql", params),
  updateCell: (params: {
    endpoint: DbEndpoint;
    table: string;
    rowKey: { rowId?: number | null; keyValues: Array<{ column: string; value: unknown }> };
    column: string;
    value: string | null;
    readOnly: boolean;
    projectRoot?: string;
  }) => invoke<void>("db_update_cell", params),
  insertRow: (params: {
    endpoint: DbEndpoint;
    table: string;
    values: Array<{ column: string; value: string | null }>;
    readOnly: boolean;
    projectRoot?: string;
  }) => invoke<void>("db_insert_row", params),
  deleteRow: (params: {
    endpoint: DbEndpoint;
    table: string;
    rowKey: { rowId?: number | null; keyValues: Array<{ column: string; value: unknown }> };
    readOnly: boolean;
    projectRoot?: string;
  }) => invoke<void>("db_delete_row", params),
  readSqlFile: (path: string) => invoke<string>("db_read_sql_file", { path }),
  dbxListConnections: () => invoke<AeroricDbConnectionConfig[]>("dbx_list_connections"),
  dbxSaveConnection: (connection: AeroricDbConnectionConfig) =>
    invoke<void>("dbx_save_connection", { connection }),
  dbxDeleteConnection: (connectionId: string) =>
    invoke<void>("dbx_delete_connection", { connectionId }),
  dbxTestConnection: (connection: AeroricDbConnectionConfig) =>
    invoke<void>("dbx_test_connection", { connection }),
  dbxConnect: (connectionId: string) => invoke<void>("dbx_connect", { connectionId }),
  dbxDisconnect: (connectionId: string) => invoke<void>("dbx_disconnect", { connectionId }),
  dbxListDatabases: (connectionId: string) =>
    invoke<DbxDatabaseInfo[]>("dbx_list_databases", { connectionId }),
  dbxListSchemas: (connectionId: string, database?: string | null) =>
    invoke<string[]>("dbx_list_schemas", { connectionId, database }),
  dbxListObjects: (connectionId: string, database?: string | null, schema?: string | null) =>
    invoke<DbxObjectInfo[]>("dbx_list_objects", { connectionId, database, schema }),
  dbxGetColumns: (
    connectionId: string,
    table: string,
    database?: string | null,
    schema?: string | null,
  ) => invoke<DbxColumnInfo[]>("dbx_get_columns", { connectionId, database, schema, table }),
  dbxGetTableDdl: (
    connectionId: string,
    table: string,
    database?: string | null,
    schema?: string | null,
  ) => invoke<string>("dbx_get_table_ddl", { connectionId, database, schema, table }),
  dbxExecuteQuery: (request: ExecuteQueryRequest) =>
    invoke<DbxQueryResult>("dbx_execute_query", { request }),
  dbxExecuteMulti: (request: ExecuteQueryRequest) =>
    invoke<DbxQueryResult[]>("dbx_execute_multi", { request }),
  dbxCancelQuery: (executionId: string) =>
    invoke<void>("dbx_cancel_query", { executionId }),
  dbxCloseResultSession: (params: {
    connectionId: string;
    sessionId: string;
    database?: string | null;
    clientSessionId?: string | null;
  }) => invoke<void>("dbx_close_result_session", params),
  dbxQueryTableData: (request: TableDataRequest) =>
    invoke<TableDataResponse>("dbx_query_table_data", { request }),
  dbxPreviewGridSql: (options: DataGridSaveStatementOptions) =>
    invoke<SqlPreviewResponse>("dbx_preview_grid_sql", { options }),
  dbxUpdateCell: (request: GridSaveRequest) =>
    invoke<SqlPreviewResponse>("dbx_update_cell", { request }),
  dbxInsertRow: (request: GridSaveRequest) =>
    invoke<SqlPreviewResponse>("dbx_insert_row", { request }),
  dbxDeleteRows: (request: GridSaveRequest) =>
    invoke<SqlPreviewResponse>("dbx_delete_rows", { request }),
  dbxExportTableCsv: (request: TableExportRequest) =>
    invoke<void>("dbx_export_table_csv", { request }),
  dbxExportTableJson: (request: TableExportRequest) =>
    invoke<void>("dbx_export_table_json", { request }),
  dbxExportTableMarkdown: (request: TableExportRequest) =>
    invoke<void>("dbx_export_table_markdown", { request }),
  dbxExportTableInsertSql: (request: TableExportRequest) =>
    invoke<void>("dbx_export_table_insert_sql", { request }),
  dbxExportTableUpdateSql: (request: TableExportRequest) =>
    invoke<void>("dbx_export_table_update_sql", { request }),
  dbxExportTableXlsx: (request: TableExportRequest) =>
    invoke<void>("dbx_export_table_xlsx", { request }),
  dbxPreviewTableImportFile: (filePath: string) =>
    invoke<TableImportPreview>("dbx_preview_table_import_file", { filePath }),
  dbxImportTableFile: (request: TableImportRequest) =>
    invoke<TableImportSummary>("dbx_import_table_file", { request }),
  dbxExportDatabase: (request: DatabaseExportRequest) =>
    invoke<void>("dbx_export_database", { request }),
  dbxExecuteSqlFile: (request: ExecuteSqlFileRequest) =>
    invoke<DbxQueryResult[]>("dbx_execute_sql_file", { request }),
  dbxRedisListDatabases: (connectionId: string) =>
    invoke<RedisDatabaseInfo[]>("dbx_redis_list_databases", { connectionId }),
  dbxRedisScanKeys: (request: RedisScanKeysRequest) =>
    invoke<RedisScanResult>("dbx_redis_scan_keys", {
      connectionId: request.connectionId,
      db: request.db,
      cursor: request.cursor ?? 0,
      pattern: request.pattern ?? null,
      count: request.count ?? null,
    }),
  dbxRedisGetValue: (request: RedisKeyRequest) =>
    invoke<RedisValue>("dbx_redis_get_value", { ...request }),
  dbxRedisSetValue: (request: RedisSetValueRequest) =>
    invoke<void>("dbx_redis_set_value", { ...request }),
  dbxRedisDeleteKey: (request: RedisKeyRequest) =>
    invoke<void>("dbx_redis_delete_key", { ...request }),
  dbxRedisSetTtl: (request: RedisSetTtlRequest) =>
    invoke<void>("dbx_redis_set_ttl", { ...request }),
  dbxRedisCreateKey: (request: RedisCreateKeyRequest) =>
    invoke<void>("dbx_redis_create_key", { request }),
  dbxRedisExecuteCommand: (request: RedisCommandRequest) =>
    invoke<RedisCommandResult>("dbx_redis_execute_command", {
      connectionId: request.connectionId,
      db: request.db,
      command: request.command,
      skipSafetyCheck: request.skipSafetyCheck ?? false,
    }),
  dbxMongoListDatabases: (connectionId: string) =>
    invoke<string[]>("dbx_mongo_list_databases", { connectionId }),
  dbxMongoListCollections: (connectionId: string, database: string) =>
    invoke<string[]>("dbx_mongo_list_collections", { connectionId, database }),
  dbxMongoFindDocuments: (request: MongoFindDocumentsRequest) =>
    invoke<MongoDocumentResult>("dbx_mongo_find_documents", { ...request }),
  dbxMongoInsertDocument: (request: MongoInsertDocumentRequest) =>
    invoke<string>("dbx_mongo_insert_document", { ...request }),
  dbxMongoUpdateDocument: (request: MongoUpdateDocumentRequest) =>
    invoke<number>("dbx_mongo_update_document", { ...request }),
  dbxMongoDeleteDocuments: (request: MongoDeleteDocumentsRequest) =>
    invoke<number>("dbx_mongo_delete_documents", { ...request }),
  dbxDriverManifest: () => invoke<DatabaseDriverManifest>("dbx_driver_manifest"),
  dbxStartTransfer: (request: unknown) =>
    invoke<void>("dbx_start_transfer", { request }),
  dbxCancelTransfer: (transferId: string) =>
    invoke<void>("dbx_cancel_transfer", { transferId }),
  dbxPrepareSchemaDiff: (options: unknown) =>
    invoke<unknown>("dbx_prepare_schema_diff", { options }),
  dbxGenerateSchemaSyncSql: (params: {
    diffs: unknown[];
    functionDiffs?: unknown[] | null;
    sequenceDiffs?: unknown[] | null;
    ruleDiffs?: unknown[] | null;
    ownerDiffs?: unknown[] | null;
    databaseType: string;
    targetSchema?: string | null;
    cascadeDelete?: boolean | null;
  }) => invoke<string>("dbx_generate_schema_sync_sql", params),
  dbxPrepareDataCompare: (options: unknown) =>
    invoke<unknown>("dbx_prepare_data_compare", { options }),
  dbxBuildDataCompareSyncPlan: (options: unknown) =>
    invoke<unknown>("dbx_build_data_compare_sync_plan", { options }),
  dbxPrepareDataCompareFromTables: (options: unknown) =>
    invoke<unknown>("dbx_prepare_data_compare_from_tables", { options }),
  dbxBuildTableStructureChangeSql: (options: TableStructureSqlOptions) =>
    invoke<TableStructureSqlResult>("dbx_build_table_structure_change_sql", { options }),
};
