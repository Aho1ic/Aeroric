import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import { databaseApi } from "../lib/databaseApi";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("databaseApi", () => {
  it("wraps db_inspect with endpoint and projectRoot", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ objects: [] });

    await databaseApi.inspect({ kind: "local", path: "/tmp/a.db" }, "/tmp");

    expect(invoke).toHaveBeenCalledWith("db_inspect", {
      endpoint: { kind: "local", path: "/tmp/a.db" },
      projectRoot: "/tmp",
    });
  });

  it("wraps dbx_save_connection with the connection payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    const connection = {
      id: "pg",
      name: "Postgres",
      dbType: "postgres" as const,
      readOnly: false,
      dbx: { id: "pg", name: "Postgres", db_type: "postgres" },
      createdAt: 1,
      lastOpenedAt: null,
    };

    await databaseApi.dbxSaveConnection(connection);

    expect(invoke).toHaveBeenCalledWith("dbx_save_connection", { connection });
  });

  it("wraps dbx_backup_sqlite_database with connection and destination paths", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await databaseApi.dbxBackupSqliteDatabase("sqlite-source", "/tmp/app.backup.db");

    expect(invoke).toHaveBeenCalledWith("dbx_backup_sqlite_database", {
      connectionId: "sqlite-source",
      destinationPath: "/tmp/app.backup.db",
    });
  });

  it("wraps dbx_list_objects with nullable database and schema scope", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);

    await databaseApi.dbxListObjects("conn", "main", "public");

    expect(invoke).toHaveBeenCalledWith("dbx_list_objects", {
      connectionId: "conn",
      database: "main",
      schema: "public",
    });
  });

  it("wraps dbx_get_object_source with object source scope", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      name: "refresh_stats",
      object_type: "PROCEDURE",
      source: "CREATE PROCEDURE ...",
    });

    await databaseApi.dbxGetObjectSource("conn", "main", "public", "refresh_stats", "PROCEDURE");

    expect(invoke).toHaveBeenCalledWith("dbx_get_object_source", {
      connectionId: "conn",
      database: "main",
      schema: "public",
      name: "refresh_stats",
      objectType: "PROCEDURE",
    });
  });

  it("wraps dbx_execute_query with a request object", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ columns: [], rows: [] });
    const request = {
      connectionId: "conn",
      database: "main",
      sql: "select 1",
      executionId: "exec-1",
    };

    await databaseApi.dbxExecuteQuery(request);

    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", { request });
  });

  it("wraps dbx_query_table_data with grid request options", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      result: { columns: [], rows: [] },
      sql: "",
      countSql: "",
    });
    const request = {
      connectionId: "conn",
      database: "main",
      schema: "public",
      table: "users",
      page: 2,
      pageSize: 50,
      orderBy: "id desc",
    };

    await databaseApi.dbxQueryTableData(request);

    expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", { request });
  });

  it("wraps DBX grid save commands with request payloads", async () => {
    vi.mocked(invoke).mockResolvedValue({
      statements: [],
      rollbackStatements: [],
      validationError: null,
    });
    const request = {
      connectionId: "conn",
      database: "main",
      schema: "public",
      execute: false,
      options: {
        databaseType: "postgres",
        tableMeta: { schema: "public", tableName: "users", primaryKeys: ["id"] },
        columns: ["id", "email"],
        rows: [[1, "alice@example.com"]],
        dirtyRows: [[0, [[1, "alice@new.test"]]]] as Array<[number, Array<[number, unknown]>]>,
      },
    };

    await databaseApi.dbxUpdateCell(request);
    await databaseApi.dbxInsertRow({
      ...request,
      options: { ...request.options, dirtyRows: [], newRows: [[2, "bob@example.com"]] },
    });
    await databaseApi.dbxDeleteRows({
      ...request,
      options: { ...request.options, dirtyRows: [], deletedRows: [0] },
    });

    expect(invoke).toHaveBeenCalledWith("dbx_update_cell", { request });
    expect(invoke).toHaveBeenCalledWith("dbx_insert_row", {
      request: {
        ...request,
        options: { ...request.options, dirtyRows: [], newRows: [[2, "bob@example.com"]] },
      },
    });
    expect(invoke).toHaveBeenCalledWith("dbx_delete_rows", {
      request: { ...request, options: { ...request.options, dirtyRows: [], deletedRows: [0] } },
    });
  });

  it("wraps DBX grid row copy SQL builders with options payloads", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    const insertOptions = {
      databaseType: "postgres",
      tableMeta: { schema: "public", tableName: "users", primaryKeys: ["id"] },
      columns: ["id", "email"],
      sourceColumns: ["id", "email"],
      rows: [[1, "alice@example.com"]],
      excludePrimaryKeys: false,
    };
    const updateOptions = {
      databaseType: "postgres",
      tableMeta: { schema: "public", tableName: "users", primaryKeys: ["id"] },
      columns: ["id", "email"],
      sourceColumns: ["id", "email"],
      rows: [[1, "alice@example.com"]],
    };
    const filterOptions = {
      databaseType: "postgres",
      columnName: "email",
      mode: "like" as const,
      value: "alice@example.com",
      columnInfo: { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
    };

    await databaseApi.dbxBuildDataGridCopyInsertStatement(insertOptions);
    await databaseApi.dbxBuildDataGridCopyUpdateStatements(updateOptions);
    await databaseApi.dbxBuildDataGridContextFilterCondition(filterOptions);

    expect(invoke).toHaveBeenCalledWith("dbx_build_data_grid_copy_insert_statement", {
      options: insertOptions,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_build_data_grid_copy_update_statements", {
      options: updateOptions,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_build_data_grid_context_filter_condition", {
      options: filterOptions,
    });
  });

  it("wraps dbx_export_table_csv with an export request", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    const request = {
      exportId: "export-1",
      connectionId: "conn",
      database: "main",
      tableName: "users",
      filePath: "/tmp/users.csv",
      format: "csv",
      skipCount: true,
    };

    await databaseApi.dbxExportTableCsv(request);

    expect(invoke).toHaveBeenCalledWith("dbx_export_table_csv", { request });
  });

  it("wraps dbx_redis_scan_keys with normalized scan defaults", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ cursor: 0, keys: [], total_keys: 0 });

    await databaseApi.dbxRedisScanKeys({ connectionId: "redis", db: 0 });

    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis",
      db: 0,
      cursor: 0,
      pattern: null,
      count: null,
    });
  });

  it("wraps dbx_redis_set_value with key and ttl payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await databaseApi.dbxRedisSetValue({
      connectionId: "redis",
      db: 1,
      keyRaw: "session:1",
      value: "active",
      ttl: 60,
    });

    expect(invoke).toHaveBeenCalledWith("dbx_redis_set_value", {
      connectionId: "redis",
      db: 1,
      keyRaw: "session:1",
      value: "active",
      ttl: 60,
    });
  });

  it("wraps dbx_redis_load_more with collection pagination payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      key_display: "profile:1",
      key_raw: "profile:1",
      key_type: "hash",
      ttl: -1,
      value_is_binary: false,
      value: [],
      total: null,
      scan_cursor: null,
    });

    await databaseApi.dbxRedisLoadMore({
      connectionId: "redis",
      db: 0,
      keyRaw: "profile:1",
      keyType: "hash",
      cursor: 12,
    });

    expect(invoke).toHaveBeenCalledWith("dbx_redis_load_more", {
      connectionId: "redis",
      db: 0,
      keyRaw: "profile:1",
      keyType: "hash",
      cursor: 12,
      count: 200,
    });
  });

  it("wraps Redis collection member delete commands", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    await databaseApi.dbxRedisHashDel({
      connectionId: "redis",
      db: 0,
      keyRaw: "profile:1",
      field: "name",
    });
    await databaseApi.dbxRedisListRemove({
      connectionId: "redis",
      db: 1,
      keyRaw: "queue",
      index: 2,
    });
    await databaseApi.dbxRedisSetRemove({
      connectionId: "redis",
      db: 2,
      keyRaw: "tags",
      member: "admin",
    });
    await databaseApi.dbxRedisZrem({
      connectionId: "redis",
      db: 3,
      keyRaw: "rank",
      member: "ada",
    });

    expect(invoke).toHaveBeenCalledWith("dbx_redis_hash_del", {
      connectionId: "redis",
      db: 0,
      keyRaw: "profile:1",
      field: "name",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_list_remove", {
      connectionId: "redis",
      db: 1,
      keyRaw: "queue",
      index: 2,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_set_remove", {
      connectionId: "redis",
      db: 2,
      keyRaw: "tags",
      member: "admin",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_zrem", {
      connectionId: "redis",
      db: 3,
      keyRaw: "rank",
      member: "ada",
    });
  });

  it("wraps Redis collection member edit commands", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    await databaseApi.dbxRedisHashSet({
      connectionId: "redis",
      db: 0,
      keyRaw: "profile:1",
      field: "name",
      value: "Grace",
      ttl: 60,
    });
    await databaseApi.dbxRedisListSet({
      connectionId: "redis",
      db: 1,
      keyRaw: "queue",
      index: 2,
      value: "retry",
    });
    await databaseApi.dbxRedisSetAdd({
      connectionId: "redis",
      db: 2,
      keyRaw: "tags",
      member: "staff",
      ttl: 120,
    });
    await databaseApi.dbxRedisZadd({
      connectionId: "redis",
      db: 3,
      keyRaw: "rank",
      member: "ada",
      score: 42.5,
      ttl: 180,
    });

    expect(invoke).toHaveBeenCalledWith("dbx_redis_hash_set", {
      connectionId: "redis",
      db: 0,
      keyRaw: "profile:1",
      field: "name",
      value: "Grace",
      ttl: 60,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_list_set", {
      connectionId: "redis",
      db: 1,
      keyRaw: "queue",
      index: 2,
      value: "retry",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_set_add", {
      connectionId: "redis",
      db: 2,
      keyRaw: "tags",
      member: "staff",
      ttl: 120,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_zadd", {
      connectionId: "redis",
      db: 3,
      keyRaw: "rank",
      member: "ada",
      score: 42.5,
      ttl: 180,
    });
  });

  it("wraps Redis collection member add commands", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    await databaseApi.dbxRedisListPush({
      connectionId: "redis",
      db: 0,
      keyRaw: "queue",
      value: "next",
    });
    await databaseApi.dbxRedisHashSet({
      connectionId: "redis",
      db: 1,
      keyRaw: "profile:1",
      field: "city",
      value: "Paris",
    });
    await databaseApi.dbxRedisSetAdd({
      connectionId: "redis",
      db: 2,
      keyRaw: "tags",
      member: "admin",
    });
    await databaseApi.dbxRedisZadd({
      connectionId: "redis",
      db: 3,
      keyRaw: "rank",
      member: "ada",
      score: 99,
    });

    expect(invoke).toHaveBeenCalledWith("dbx_redis_list_push", {
      connectionId: "redis",
      db: 0,
      keyRaw: "queue",
      value: "next",
      ttl: null,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_hash_set", {
      connectionId: "redis",
      db: 1,
      keyRaw: "profile:1",
      field: "city",
      value: "Paris",
      ttl: null,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_set_add", {
      connectionId: "redis",
      db: 2,
      keyRaw: "tags",
      member: "admin",
      ttl: null,
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_zadd", {
      connectionId: "redis",
      db: 3,
      keyRaw: "rank",
      member: "ada",
      score: 99,
      ttl: null,
    });
  });

  it("wraps dbx_redis_create_key with the typed create-key request", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    const request = {
      connectionId: "redis",
      db: 2,
      keyRaw: "users:1",
      keyType: "hash" as const,
      value: "Ada",
      field: "name",
      score: 0,
      entryId: "*",
      ttl: 120,
    };

    await databaseApi.dbxRedisCreateKey(request);

    expect(invoke).toHaveBeenCalledWith("dbx_redis_create_key", { request });
  });

  it("wraps dbx_redis_execute_command with command text and safety override", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ command: "GET", safety: "allowed", value: "Ada" });

    await databaseApi.dbxRedisExecuteCommand({
      connectionId: "redis",
      db: 0,
      command: "GET users:1",
      skipSafetyCheck: false,
    });

    expect(invoke).toHaveBeenCalledWith("dbx_redis_execute_command", {
      connectionId: "redis",
      db: 0,
      command: "GET users:1",
      skipSafetyCheck: false,
    });
  });

  it("wraps dbx_mongo_find_documents with collection query parameters", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ documents: [], total: 0 });
    const request = {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      skip: 20,
      limit: 10,
      filter: '{"active":true}',
      sort: '{"createdAt":-1}',
      executionId: "mongo-find-1",
    };

    await databaseApi.dbxMongoFindDocuments(request);

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", request);
  });

  it("wraps dbx_mongo_delete_documents with the bulk delete payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(2);
    const request = {
      connectionId: "mongo",
      database: "app",
      collection: "events",
      filterJson: '{"archived":true}',
      many: true,
    };

    await databaseApi.dbxMongoDeleteDocuments(request);

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_delete_documents", request);
  });

  it("wraps dbx_build_table_structure_change_sql with DBX Core options", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      statements: ["ALTER TABLE users ADD COLUMN age int;"],
      warnings: [],
    });
    const options = {
      databaseType: "postgres",
      schema: "public",
      tableName: "users",
      columns: [],
      indexes: [],
      foreignKeys: [],
      triggers: [],
    };

    await databaseApi.dbxBuildTableStructureChangeSql(options);

    expect(invoke).toHaveBeenCalledWith("dbx_build_table_structure_change_sql", { options });
  });

  it("wraps dbx admin SQL builder commands with options", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(
      "CREATE DATABASE `app` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;",
    );
    await databaseApi.dbxBuildCreateDatabaseSql({
      databaseType: "mysql",
      driverProfile: "mysql",
      name: "app",
      charset: "utf8mb4",
      collation: "utf8mb4_unicode_ci",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_build_create_database_sql", {
      options: {
        databaseType: "mysql",
        driverProfile: "mysql",
        name: "app",
        charset: "utf8mb4",
        collation: "utf8mb4_unicode_ci",
      },
    });

    vi.mocked(invoke).mockResolvedValueOnce("ATTACH '/tmp/app.duckdb' AS app;");
    await databaseApi.dbxBuildDuckDbAttachDatabaseSql("/tmp/app.duckdb", "app");
    expect(invoke).toHaveBeenCalledWith("dbx_build_duckdb_attach_database_sql", {
      options: { path: "/tmp/app.duckdb", name: "app" },
    });

    vi.mocked(invoke).mockResolvedValueOnce('DROP DATABASE "app";');
    await databaseApi.dbxBuildDropDatabaseSql({ databaseType: "postgres", name: "app" });
    expect(invoke).toHaveBeenCalledWith("dbx_build_drop_database_sql", {
      options: { databaseType: "postgres", name: "app" },
    });

    vi.mocked(invoke).mockResolvedValueOnce('CREATE SCHEMA "analytics";');
    await databaseApi.dbxBuildCreateSchemaSql({ databaseType: "postgres", name: "analytics" });
    expect(invoke).toHaveBeenCalledWith("dbx_build_create_schema_sql", {
      options: { databaseType: "postgres", name: "analytics" },
    });

    vi.mocked(invoke).mockResolvedValueOnce('DROP SCHEMA "analytics" CASCADE;');
    await databaseApi.dbxBuildDropSchemaSql({ databaseType: "postgres", name: "analytics" });
    expect(invoke).toHaveBeenCalledWith("dbx_build_drop_schema_sql", {
      options: { databaseType: "postgres", name: "analytics" },
    });
  });

  it("wraps dbx table administration SQL builder commands with options", async () => {
    const tableOptions = {
      databaseType: "postgres",
      schema: "public",
      tableName: "users",
    };

    vi.mocked(invoke).mockResolvedValueOnce('DROP TABLE "public"."users";');
    await databaseApi.dbxBuildDropTableSql(tableOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_build_drop_table_sql", { options: tableOptions });

    vi.mocked(invoke).mockResolvedValueOnce('TRUNCATE TABLE "public"."users";');
    await databaseApi.dbxBuildTruncateTableSql(tableOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_build_truncate_table_sql", { options: tableOptions });

    vi.mocked(invoke).mockResolvedValueOnce('DELETE FROM "public"."users";');
    await databaseApi.dbxBuildEmptyTableSql(tableOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_build_empty_table_sql", { options: tableOptions });

    const renameObjectOptions = {
      databaseType: "postgres",
      objectType: "TABLE" as const,
      schema: "public",
      oldName: "users",
      newName: "app_users",
    };
    vi.mocked(invoke).mockResolvedValueOnce('ALTER TABLE "public"."users" RENAME TO "app_users";');
    await databaseApi.dbxBuildRenameObjectSql(renameObjectOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_build_rename_object_sql", {
      options: renameObjectOptions,
    });

    const dropObjectOptions = {
      databaseType: "postgres",
      objectType: "VIEW" as const,
      schema: "public",
      name: "active_users",
    };
    vi.mocked(invoke).mockResolvedValueOnce('DROP VIEW "public"."active_users";');
    await databaseApi.dbxBuildDropObjectSql(dropObjectOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_build_drop_object_sql", {
      options: dropObjectOptions,
    });

    const dropChildOptions = {
      databaseType: "postgres",
      objectType: "COLUMN" as const,
      schema: "public",
      tableName: "users",
      name: "legacy_id",
    };
    vi.mocked(invoke).mockResolvedValueOnce(
      'ALTER TABLE "public"."users" DROP COLUMN "legacy_id";',
    );
    await databaseApi.dbxBuildDropTableChildObjectSql(dropChildOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_build_drop_table_child_object_sql", {
      options: dropChildOptions,
    });

    const duplicateOptions = {
      databaseType: "postgres",
      schema: "public",
      sourceName: "users",
      targetName: "users_copy",
    };
    vi.mocked(invoke).mockResolvedValueOnce(
      'CREATE TABLE "public"."users_copy" (LIKE "public"."users" INCLUDING ALL);',
    );
    await databaseApi.dbxBuildDuplicateTableStructureSql(duplicateOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_build_duplicate_table_structure_sql", {
      options: duplicateOptions,
    });
  });

  it("wraps dbx database search SQL builder commands with options", async () => {
    const searchOptions = {
      databaseType: "postgres" as const,
      schema: "public",
      tableName: "users",
      columns: [
        { name: "id", data_type: "integer", is_primary_key: true },
        { name: "email", data_type: "text", is_primary_key: false },
      ],
      term: "alice",
      limit: 20,
    };
    vi.mocked(invoke).mockResolvedValueOnce({
      sql: 'SELECT * FROM "public"."users" WHERE (LOWER(CAST("email" AS TEXT)) LIKE \'%alice%\') LIMIT 20;',
      searchableColumns: ["email"],
    });

    await databaseApi.dbxBuildDatabaseSearchSql(searchOptions);

    expect(invoke).toHaveBeenCalledWith("dbx_build_database_search_sql", {
      options: searchOptions,
    });

    const whereOptions = {
      databaseType: "postgres" as const,
      columns: searchOptions.columns,
      resultColumns: ["id", "email"],
      row: [1, "alice@example.com"],
      matchedColumns: ["email"],
    };
    vi.mocked(invoke).mockResolvedValueOnce('"id" = 1');

    await databaseApi.dbxBuildSearchResultWhere(whereOptions);

    expect(invoke).toHaveBeenCalledWith("dbx_build_search_result_where", { options: whereOptions });
  });

  it("wraps dbx_driver_manifest", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ schemaVersion: 1, drivers: [] });

    await databaseApi.dbxDriverManifest();

    expect(invoke).toHaveBeenCalledWith("dbx_driver_manifest");
  });

  it("wraps dbx table import preview and execution", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ columns: [], rows: [], totalRows: 0 });
    await databaseApi.dbxPreviewTableImportFile("/tmp/users.csv");
    expect(invoke).toHaveBeenCalledWith("dbx_preview_table_import_file", {
      filePath: "/tmp/users.csv",
    });

    vi.mocked(invoke).mockResolvedValueOnce({
      importId: "import-1",
      rowsImported: 2,
      totalRows: 2,
    });
    const request = {
      importId: "import-1",
      connectionId: "pg",
      database: "postgres",
      schema: "public",
      table: "users",
      filePath: "/tmp/users.csv",
      mappings: [{ sourceColumn: "id", targetColumn: "id" }],
      mode: "append" as const,
      batchSize: 500,
    };

    await databaseApi.dbxImportTableFile(request);

    expect(invoke).toHaveBeenCalledWith("dbx_import_table_file", { request });
  });

  it("wraps dbx database export", async () => {
    const request = {
      exportId: "export-1",
      connectionId: "pg",
      database: "postgres",
      schema: "public",
      filePath: "/tmp/postgres.sql",
      selectedTables: ["users"],
      includeStructure: true,
      includeData: true,
      includeObjects: true,
      dropTableIfExists: false,
      batchSize: 1000,
    };

    await databaseApi.dbxExportDatabase(request);

    expect(invoke).toHaveBeenCalledWith("dbx_export_database", { request });
  });

  it("wraps dbx transfer and comparison commands", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    const transfer = {
      transferId: "transfer-1",
      sourceConnectionId: "source",
      sourceDatabase: "main",
      sourceSchema: "public",
      targetConnectionId: "target",
      targetDatabase: "main",
      targetSchema: "public",
      tables: ["users"],
      createTable: true,
      mode: "append",
      batchSize: 500,
    };
    await databaseApi.dbxStartTransfer(transfer);
    expect(invoke).toHaveBeenCalledWith("dbx_start_transfer", { request: transfer });

    vi.mocked(invoke).mockResolvedValueOnce({ diffs: [], syncSql: "" });
    const schemaOptions = { sourceTables: [], targetTables: [], databaseType: "postgres" };
    await databaseApi.dbxPrepareSchemaDiff(schemaOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_prepare_schema_diff", { options: schemaOptions });

    vi.mocked(invoke).mockResolvedValueOnce({
      result: { added: [], removed: [], modified: [] },
      syncSql: "",
    });
    const compareOptions = { tableName: "users", columns: ["id"], keyColumns: ["id"] };
    await databaseApi.dbxPrepareDataCompare(compareOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_prepare_data_compare", { options: compareOptions });
  });
});
