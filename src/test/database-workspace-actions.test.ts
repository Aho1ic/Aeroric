import { describe, expect, it } from "vitest";
import {
  DATABASE_ACTIONS,
  isDatabaseActionEnabled,
  type DatabaseActionContext,
} from "../components/database/databaseActions";

const baseContext: DatabaseActionContext = {
  hasConnection: true,
  dbType: "postgres",
  hasSql: true,
  isExecuting: false,
  isExplaining: false,
  hasSelectedTable: true,
  hasResult: true,
  readOnly: false,
};

describe("database action registry", () => {
  it("contains the DBX toolbar and workspace actions Aeroric exposes", () => {
    expect(DATABASE_ACTIONS.map((action) => action.id)).toEqual([
      "newConnection",
      "newQuery",
      "execute",
      "cancel",
      "explain",
      "formatSql",
      "saveSql",
      "openSql",
      "executeSqlFile",
      "importResultArchive",
      "driverManager",
      "refresh",
      "tableImport",
      "tableExport",
      "databaseExport",
      "dataTransfer",
      "schemaDiff",
      "dataCompare",
      "erDiagram",
      "tableStructure",
      "redisCreateKey",
      "redisDeleteKey",
      "mongoInsertDocument",
      "mongoDeleteDocument",
    ]);
  });

  it("enables SQL actions only for SQL-capable connections", () => {
    expect(isDatabaseActionEnabled("execute", baseContext)).toBe(true);
    expect(isDatabaseActionEnabled("explain", { ...baseContext, dbType: "redis" })).toBe(false);
    expect(isDatabaseActionEnabled("formatSql", { ...baseContext, dbType: "mongodb" })).toBe(false);
  });

  it("disables mutating actions for read-only connections", () => {
    expect(isDatabaseActionEnabled("tableImport", { ...baseContext, readOnly: true })).toBe(false);
    expect(isDatabaseActionEnabled("tableExport", { ...baseContext, readOnly: true })).toBe(true);
    expect(isDatabaseActionEnabled("redisDeleteKey", { ...baseContext, dbType: "redis", readOnly: true })).toBe(false);
  });
});
