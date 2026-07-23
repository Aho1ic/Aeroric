import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AeroricDbConnectionConfig,
  AssessProductionSqlRequest,
  AssessProductionTargetRequest,
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DbQueryResult,
  DbSchema,
  DbxColumnInfo,
  DbxDatabaseInfo,
  DbxObjectInfo,
  DbxObjectSource,
  DbxObjectSourceKind,
  DbxQueryResult,
  DbxListObjectsOptions,
  DbxTransferProgress,
  DbxTransferRequest,
  ExecuteMultiRequest,
  ExecuteQueryRequest,
  DataGridContextFilterConditionOptions,
  DataGridCopyInsertStatementOptions,
  DataGridCopyUpdateStatementOptions,
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
  ProductionSqlAssessment,
  ProductionTargetAssessment,
  RedisDatabaseInfo,
  RedisCommandRequest,
  RedisCommandResult,
  RedisCreateKeyRequest,
  RedisHashFieldRequest,
  RedisHashSetRequest,
  RedisKeyRequest,
  RedisListIndexRequest,
  RedisListPushRequest,
  RedisListSetRequest,
  RedisLoadMoreRequest,
  RedisCollectionPage,
  RedisScanKeysRequest,
  RedisScanResult,
  RedisSetAddRequest,
  RedisSetMemberRequest,
  RedisSetTtlRequest,
  RedisSetValueRequest,
  RedisValue,
  RedisZaddRequest,
  SqlPreviewResponse,
  TableExportRequest,
  TableImportPreview,
  TableImportRequest,
  TableImportSummary,
  TableDataRequest,
  TableDataResponse,
  CreateDatabaseSqlOptions,
  DatabaseSearchSql,
  DatabaseSearchSqlOptions,
  DatabaseNameSqlOptions,
  DropObjectSqlOptions,
  DropTableChildObjectSqlOptions,
  DuckDbAttachDatabaseSqlOptions,
  DuplicateTableStructureSqlOptions,
  RenameObjectSqlOptions,
  SchemaNameSqlOptions,
  SearchResultWhereOptions,
  SingleColumnAlterSqlOptions,
  TableAdminSqlOptions,
  TableStructureSqlOptions,
  TableStructureSqlResult,
} from "../types/database";

export function isTerminalDbxTransferProgress(progress: DbxTransferProgress): boolean {
  return (
    progress.terminal === true ||
    progress.status === "done" ||
    progress.status === "error" ||
    progress.status === "cancelled"
  );
}

async function startDbxTransferWithProgress(
  request: DbxTransferRequest,
  onProgress: (progress: DbxTransferProgress) => void,
): Promise<DbxTransferProgress> {
  return new Promise((resolve, reject) => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        unlisten = await listen<DbxTransferProgress>("dbx-transfer-progress", (event) => {
          if (event.payload.transferId !== request.transferId) return;
          const terminal = isTerminalDbxTransferProgress(event.payload);
          try {
            onProgress(event.payload);
          } finally {
            if (terminal) {
              unlisten?.();
              resolve(event.payload);
            }
          }
        });
        await invoke<void>("dbx_start_transfer", { request });
      } catch (error) {
        unlisten?.();
        reject(error);
      }
    })();
  });
}

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
    connectionId?: string;
    projectRoot?: string;
  }) => invoke<DbExecuteResult>("db_execute_sql", params),
  updateCell: (params: {
    endpoint: DbEndpoint;
    table: string;
    rowKey: { rowId?: number | null; keyValues: Array<{ column: string; value: unknown }> };
    column: string;
    value: string | null;
    readOnly: boolean;
    connectionId?: string;
    projectRoot?: string;
  }) => invoke<void>("db_update_cell", params),
  insertRow: (params: {
    endpoint: DbEndpoint;
    table: string;
    values: Array<{ column: string; value: string | null }>;
    readOnly: boolean;
    connectionId?: string;
    projectRoot?: string;
  }) => invoke<void>("db_insert_row", params),
  deleteRow: (params: {
    endpoint: DbEndpoint;
    table: string;
    rowKey: { rowId?: number | null; keyValues: Array<{ column: string; value: unknown }> };
    readOnly: boolean;
    connectionId?: string;
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
  dbxBackupSqliteDatabase: (connectionId: string, destinationPath: string) =>
    invoke<void>("dbx_backup_sqlite_database", { connectionId, destinationPath }),
  dbxListDatabases: (connectionId: string) =>
    invoke<DbxDatabaseInfo[]>("dbx_list_databases", { connectionId }),
  dbxListSchemas: (connectionId: string, database?: string | null) =>
    invoke<string[]>("dbx_list_schemas", { connectionId, database }),
  dbxListObjects: (
    connectionId: string,
    database?: string | null,
    schema?: string | null,
    options: DbxListObjectsOptions = {},
  ) =>
    invoke<DbxObjectInfo[]>("dbx_list_objects", {
      connectionId,
      database,
      schema,
      filter: options.filter ?? null,
      limit: options.limit ?? null,
      offset: options.offset ?? null,
      objectTypes: options.objectTypes ?? null,
    }),
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
  dbxGetObjectSource: (
    connectionId: string,
    database: string | null | undefined,
    schema: string | null | undefined,
    name: string,
    objectType: DbxObjectSourceKind,
    signature?: string | null,
  ) =>
    invoke<DbxObjectSource>("dbx_get_object_source", {
      connectionId,
      database,
      schema,
      name,
      objectType,
      signature: signature ?? null,
    }),
  dbxAssessProductionSql: (request: AssessProductionSqlRequest) =>
    invoke<ProductionSqlAssessment>("dbx_assess_production_sql", { request }),
  dbxAssessProductionTarget: (request: AssessProductionTargetRequest) =>
    invoke<ProductionTargetAssessment>("dbx_assess_production_target", { request }),
  dbxExecuteQuery: (request: ExecuteQueryRequest) =>
    invoke<DbxQueryResult>("dbx_execute_query", { request }),
  dbxExecuteMulti: (request: ExecuteMultiRequest) =>
    invoke<DbxQueryResult[]>("dbx_execute_multi", { request }),
  dbxCancelQuery: (executionId: string) => invoke<void>("dbx_cancel_query", { executionId }),
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
  dbxBuildDataGridContextFilterCondition: (options: DataGridContextFilterConditionOptions) =>
    invoke<string | null>("dbx_build_data_grid_context_filter_condition", { options }),
  dbxBuildDataGridCopyInsertStatement: (options: DataGridCopyInsertStatementOptions) =>
    invoke<string | null>("dbx_build_data_grid_copy_insert_statement", { options }),
  dbxBuildDataGridCopyUpdateStatements: (options: DataGridCopyUpdateStatementOptions) =>
    invoke<string[]>("dbx_build_data_grid_copy_update_statements", { options }),
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
  dbxRedisLoadMore: (request: RedisLoadMoreRequest) =>
    invoke<RedisCollectionPage>("dbx_redis_load_more", {
      connectionId: request.connectionId,
      db: request.db,
      keyRaw: request.keyRaw,
      keyType: request.keyType,
      cursor: request.cursor,
      count: request.count ?? 200,
      filter: request.filter ?? null,
    }),
  dbxRedisSetValue: (request: RedisSetValueRequest) =>
    invoke<void>("dbx_redis_set_value", { ...request }),
  dbxRedisDeleteKey: (request: RedisKeyRequest) =>
    invoke<void>("dbx_redis_delete_key", { ...request }),
  dbxRedisSetTtl: (request: RedisSetTtlRequest) =>
    invoke<void>("dbx_redis_set_ttl", { ...request }),
  dbxRedisCreateKey: (request: RedisCreateKeyRequest) =>
    invoke<void>("dbx_redis_create_key", { request }),
  dbxRedisHashDel: (request: RedisHashFieldRequest) =>
    invoke<void>("dbx_redis_hash_del", { ...request }),
  dbxRedisHashSet: (request: RedisHashSetRequest) =>
    invoke<void>("dbx_redis_hash_set", { ...request, ttl: request.ttl ?? null }),
  dbxRedisListRemove: (request: RedisListIndexRequest) =>
    invoke<void>("dbx_redis_list_remove", { ...request }),
  dbxRedisListPush: (request: RedisListPushRequest) =>
    invoke<void>("dbx_redis_list_push", { ...request, ttl: request.ttl ?? null }),
  dbxRedisListSet: (request: RedisListSetRequest) =>
    invoke<void>("dbx_redis_list_set", { ...request }),
  dbxRedisSetRemove: (request: RedisSetMemberRequest) =>
    invoke<void>("dbx_redis_set_remove", { ...request }),
  dbxRedisSetAdd: (request: RedisSetAddRequest) =>
    invoke<void>("dbx_redis_set_add", { ...request, ttl: request.ttl ?? null }),
  dbxRedisZrem: (request: RedisSetMemberRequest) => invoke<void>("dbx_redis_zrem", { ...request }),
  dbxRedisZadd: (request: RedisZaddRequest) =>
    invoke<void>("dbx_redis_zadd", { ...request, ttl: request.ttl ?? null }),
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
  dbxStartTransfer: (
    request: DbxTransferRequest,
    onProgress?: (progress: DbxTransferProgress) => void,
  ): Promise<void | DbxTransferProgress> =>
    onProgress
      ? startDbxTransferWithProgress(request, onProgress)
      : invoke<void>("dbx_start_transfer", { request }),
  dbxCancelTransfer: (transferId: string) => invoke<void>("dbx_cancel_transfer", { transferId }),
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
  dbxBuildSingleColumnAlterSql: (options: SingleColumnAlterSqlOptions) =>
    invoke<TableStructureSqlResult>("dbx_build_single_column_alter_sql", { options }),
  dbxBuildCreateDatabaseSql: (options: CreateDatabaseSqlOptions) =>
    invoke<string>("dbx_build_create_database_sql", { options }),
  dbxBuildDuckDbAttachDatabaseSql: (path: string, name: string) =>
    invoke<string>("dbx_build_duckdb_attach_database_sql", {
      options: { path, name } satisfies DuckDbAttachDatabaseSqlOptions,
    }),
  dbxBuildRenameObjectSql: (options: RenameObjectSqlOptions) =>
    invoke<string>("dbx_build_rename_object_sql", { options }),
  dbxBuildDropDatabaseSql: (options: DatabaseNameSqlOptions) =>
    invoke<string>("dbx_build_drop_database_sql", { options }),
  dbxBuildCreateSchemaSql: (options: SchemaNameSqlOptions) =>
    invoke<string>("dbx_build_create_schema_sql", { options }),
  dbxBuildDropSchemaSql: (options: SchemaNameSqlOptions) =>
    invoke<string>("dbx_build_drop_schema_sql", { options }),
  dbxBuildDropTableSql: (options: TableAdminSqlOptions) =>
    invoke<string>("dbx_build_drop_table_sql", { options }),
  dbxBuildTruncateTableSql: (options: TableAdminSqlOptions) =>
    invoke<string>("dbx_build_truncate_table_sql", { options }),
  dbxBuildEmptyTableSql: (options: TableAdminSqlOptions) =>
    invoke<string>("dbx_build_empty_table_sql", { options }),
  dbxBuildDropObjectSql: (options: DropObjectSqlOptions) =>
    invoke<string>("dbx_build_drop_object_sql", { options }),
  dbxBuildDropTableChildObjectSql: (options: DropTableChildObjectSqlOptions) =>
    invoke<string>("dbx_build_drop_table_child_object_sql", { options }),
  dbxBuildDuplicateTableStructureSql: (options: DuplicateTableStructureSqlOptions) =>
    invoke<string>("dbx_build_duplicate_table_structure_sql", { options }),
  dbxBuildDatabaseSearchSql: (options: DatabaseSearchSqlOptions) =>
    invoke<DatabaseSearchSql | null>("dbx_build_database_search_sql", { options }),
  dbxBuildSearchResultWhere: (options: SearchResultWhereOptions) =>
    invoke<string>("dbx_build_search_result_where", { options }),
};
