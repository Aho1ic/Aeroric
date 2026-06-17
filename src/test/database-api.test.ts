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

  it("wraps dbx_list_objects with nullable database and schema scope", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);

    await databaseApi.dbxListObjects("conn", "main", "public");

    expect(invoke).toHaveBeenCalledWith("dbx_list_objects", {
      connectionId: "conn",
      database: "main",
      schema: "public",
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
    vi.mocked(invoke).mockResolvedValueOnce({ result: { columns: [], rows: [] }, sql: "", countSql: "" });
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

  it("wraps dbx_mongo_find_documents with collection query parameters", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ documents: [], total: 0 });
    const request = {
      connectionId: "mongo",
      database: "app",
      collection: "users",
      skip: 20,
      limit: 10,
      filter: "{\"active\":true}",
      sort: "{\"createdAt\":-1}",
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
      filterJson: "{\"archived\":true}",
      many: true,
    };

    await databaseApi.dbxMongoDeleteDocuments(request);

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_delete_documents", request);
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

    vi.mocked(invoke).mockResolvedValueOnce({ importId: "import-1", rowsImported: 2, totalRows: 2 });
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

    vi.mocked(invoke).mockResolvedValueOnce({ result: { added: [], removed: [], modified: [] }, syncSql: "" });
    const compareOptions = { tableName: "users", columns: ["id"], keyColumns: ["id"] };
    await databaseApi.dbxPrepareDataCompare(compareOptions);
    expect(invoke).toHaveBeenCalledWith("dbx_prepare_data_compare", { options: compareOptions });
  });
});
