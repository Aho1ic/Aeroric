import { describe, expect, it } from "vitest";
import { parseConnectionUrl } from "../components/database/databaseConnectionUrl";

describe("database connection URL parsing", () => {
  it("parses standard URLs and decodes credentials and database names", () => {
    expect(
      parseConnectionUrl(
        "postgres://alice%40example.com:s%23cret@db.example.com:5432/app%20data?sslmode=require",
        "postgres",
      ),
    ).toEqual({
      host: "db.example.com",
      port: "5432",
      user: "alice@example.com",
      password: "s#cret",
      database: "app data",
      urlParams: "sslmode=require",
    });
  });

  it("normalizes JDBC URLs for standard database drivers", () => {
    expect(
      parseConnectionUrl("jdbc:mysql://root:secret@localhost:3306/inventory?useSSL=false", "mysql"),
    ).toEqual({
      host: "localhost",
      port: "3306",
      user: "root",
      password: "secret",
      database: "inventory",
      urlParams: "useSSL=false",
    });
  });

  it("parses SQL Server semicolon properties", () => {
    expect(
      parseConnectionUrl(
        "jdbc:sqlserver://sql.example.com:1433;databaseName=warehouse;user=sa;password=secret;encrypt=true",
        "sqlserver",
      ),
    ).toEqual({
      host: "sql.example.com",
      port: "1433",
      database: "warehouse",
      user: "sa",
      password: "secret",
      urlParams: "encrypt=true",
    });
  });

  it("parses Oracle service and SID JDBC formats", () => {
    expect(
      parseConnectionUrl("jdbc:oracle:thin:@//oracle.example.com:1521/ORCLPDB1", "oracle"),
    ).toEqual({
      host: "oracle.example.com",
      port: "1521",
      database: "ORCLPDB1",
      urlParams: undefined,
    });
    expect(parseConnectionUrl("jdbc:oracle:thin:@oracle.example.com:1521:ORCL", "oracle")).toEqual({
      host: "oracle.example.com",
      port: "1521",
      database: "ORCL",
      urlParams: undefined,
    });
  });

  it("returns null for blank or malformed values", () => {
    expect(parseConnectionUrl("   ", "postgres")).toBeNull();
    expect(parseConnectionUrl("not a connection URL", "postgres")).toBeNull();
    expect(parseConnectionUrl("jdbc:oracle:thin:invalid", "oracle")).toBeNull();
  });
});
