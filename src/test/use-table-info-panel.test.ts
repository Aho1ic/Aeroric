import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { databaseApi } from "../lib/databaseApi";
import { useTableInfoPanel } from "../components/database/useTableInfoPanel";
import type { AeroricDbConnectionConfig, DbxObjectInfo } from "../types";

vi.mock("../lib/databaseApi", () => ({
  databaseApi: {
    dbxGetTableDdl: vi.fn(),
  },
}));

const getTableDdl = vi.mocked(databaseApi.dbxGetTableDdl);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}

const connection: AeroricDbConnectionConfig = {
  id: "connection",
  name: "Connection",
  dbType: "postgres",
  readOnly: false,
  createdAt: 1,
};

const users: DbxObjectInfo = { name: "users", object_type: "table", schema: "public" };
const teams: DbxObjectInfo = { name: "teams", object_type: "table", schema: "public" };

function deps(object: DbxObjectInfo) {
  return {
    connection,
    database: "main",
    object,
    objectKey: `${object.schema}.${object.name}`,
    columns: [],
    indexes: [],
    foreignKeys: [],
    triggers: [],
  };
}

describe("useTableInfoPanel DDL request sequencing", () => {
  beforeEach(() => getTableDdl.mockReset());

  it("loads DDL when the DDL tab is selected on the initial object", async () => {
    getTableDdl.mockResolvedValue("CREATE TABLE users (...)");
    const { result } = renderHook(
      (props: { object: DbxObjectInfo }) => useTableInfoPanel(deps(props.object)),
      {
        initialProps: { object: users },
      },
    );

    act(() => result.current.setActiveTab("ddl"));

    await waitFor(() => expect(result.current.ddl).toBe("CREATE TABLE users (...)"));
    expect(getTableDdl).toHaveBeenCalledWith("connection", "users", "main", "public");
  });

  it("ignores an old success while the new object's request is still loading", async () => {
    const oldRequest = deferred<string>();
    const newRequest = deferred<string>();
    getTableDdl.mockImplementation((_connectionId, table) =>
      table === "users" ? oldRequest.promise : newRequest.promise,
    );
    const { result, rerender } = renderHook(
      (props: { object: DbxObjectInfo }) => useTableInfoPanel(deps(props.object)),
      { initialProps: { object: users } },
    );

    act(() => void result.current.loadDdlForObject(connection, "main", users));
    rerender({ object: teams });
    act(() => void result.current.loadDdlForObject(connection, "main", teams));

    expect(result.current.ddlLoading).toBe(true);
    act(() => oldRequest.resolve("STALE USERS DDL"));
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.ddl).toBe("");
    expect(result.current.ddlLoading).toBe(true);

    act(() => newRequest.resolve("CURRENT TEAMS DDL"));
    await waitFor(() => expect(result.current.ddl).toBe("CURRENT TEAMS DDL"));
    expect(result.current.ddlLoading).toBe(false);
  });

  it("ignores an old error after switching objects", async () => {
    const oldRequest = deferred<string>();
    const newRequest = deferred<string>();
    getTableDdl.mockImplementation((_connectionId, table) =>
      table === "users" ? oldRequest.promise : newRequest.promise,
    );
    const { result, rerender } = renderHook(
      (props: { object: DbxObjectInfo }) => useTableInfoPanel(deps(props.object)),
      { initialProps: { object: users } },
    );

    act(() => void result.current.loadDdlForObject(connection, "main", users));
    rerender({ object: teams });
    act(() => void result.current.loadDdlForObject(connection, "main", teams));

    act(() => oldRequest.reject(new Error("stale failure")));
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.ddlError).toBe("");
    expect(result.current.ddlLoading).toBe(true);

    act(() => newRequest.resolve("CURRENT TEAMS DDL"));
    await waitFor(() => expect(result.current.ddl).toBe("CURRENT TEAMS DDL"));
    expect(result.current.ddlError).toBe("");
  });
});
