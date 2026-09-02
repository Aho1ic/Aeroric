import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { databaseApi } from "../lib/databaseApi";
import {
  useTableInfoPanel,
  type TableInfoPanelDeps,
} from "../components/database/useTableInfoPanel";
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

const reloadMetadata = vi.fn<TableInfoPanelDeps["reloadMetadata"]>(async () => {});

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
    reloadMetadata,
  };
}

describe("useTableInfoPanel DDL request sequencing", () => {
  beforeEach(() => {
    getTableDdl.mockReset();
    reloadMetadata.mockReset();
    reloadMetadata.mockResolvedValue(undefined);
  });

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

describe("useTableInfoPanel metadata refresh", () => {
  beforeEach(() => {
    getTableDdl.mockReset();
    reloadMetadata.mockReset();
    reloadMetadata.mockResolvedValue(undefined);
  });

  it("reloads the current object's metadata and clears the in-flight flag", async () => {
    const { result } = renderHook(
      (props: { object: DbxObjectInfo }) => useTableInfoPanel(deps(props.object)),
      { initialProps: { object: users } },
    );

    await act(async () => {
      await result.current.refreshMetadata();
    });

    expect(reloadMetadata).toHaveBeenCalledTimes(1);
    expect(reloadMetadata.mock.calls[0]?.slice(0, 3)).toEqual([users, connection, "main"]);
    expect(result.current.metadataRefreshing).toBe(false);
    expect(result.current.metadataError).toBe("");
    // 没进过 DDL tab 也没拉到过 DDL,就不该白拉一次。
    expect(getTableDdl).not.toHaveBeenCalled();
  });

  it("marks itself refreshing while in flight and ignores a second click", async () => {
    const pending = deferred<void>();
    reloadMetadata.mockReturnValue(pending.promise);
    const { result } = renderHook(
      (props: { object: DbxObjectInfo }) => useTableInfoPanel(deps(props.object)),
      { initialProps: { object: users } },
    );

    act(() => void result.current.refreshMetadata());
    expect(result.current.metadataRefreshing).toBe(true);

    await act(async () => {
      await result.current.refreshMetadata();
    });
    expect(reloadMetadata).toHaveBeenCalledTimes(1);

    await act(async () => {
      pending.resolve();
      await pending.promise;
    });
    expect(result.current.metadataRefreshing).toBe(false);
  });

  it("surfaces a refresh failure and recovers on the next attempt", async () => {
    reloadMetadata.mockRejectedValueOnce(new Error("metadata boom"));
    const { result } = renderHook(
      (props: { object: DbxObjectInfo }) => useTableInfoPanel(deps(props.object)),
      { initialProps: { object: users } },
    );

    await act(async () => {
      await result.current.refreshMetadata();
    });
    expect(result.current.metadataError).toContain("metadata boom");
    expect(result.current.metadataRefreshing).toBe(false);

    reloadMetadata.mockResolvedValueOnce(undefined);
    await act(async () => {
      await result.current.refreshMetadata();
    });
    expect(result.current.metadataError).toBe("");
  });

  it("drops a late refresh that resolves after the object changed", async () => {
    const pending = deferred<void>();
    reloadMetadata.mockReturnValue(pending.promise);
    const { result, rerender } = renderHook(
      (props: { object: DbxObjectInfo }) => useTableInfoPanel(deps(props.object)),
      { initialProps: { object: users } },
    );

    act(() => void result.current.refreshMetadata());
    expect(result.current.metadataRefreshing).toBe(true);
    rerender({ object: teams });
    // 换表就作废:新表的按钮不该继承上一张表的「正在刷新」。
    expect(result.current.metadataRefreshing).toBe(false);

    await act(async () => {
      pending.resolve();
      await pending.promise;
    });
    // 迟到的响应既不会把新表标成刷新中,也不会把它的错误 / 成功态写上去。
    expect(result.current.metadataRefreshing).toBe(false);
    expect(result.current.metadataError).toBe("");
    // 落地由 `shouldApply` 拦:传给外层加载器的那支断言此刻应为 false。
    const shouldApply = reloadMetadata.mock.calls[0]?.[3];
    expect(shouldApply?.()).toBe(false);
  });

  it("refreshes the DDL together with metadata once the DDL has been loaded", async () => {
    getTableDdl.mockResolvedValueOnce("CREATE TABLE users (v1)");
    const { result } = renderHook(
      (props: { object: DbxObjectInfo }) => useTableInfoPanel(deps(props.object)),
      { initialProps: { object: users } },
    );

    act(() => result.current.setActiveTab("ddl"));
    await waitFor(() => expect(result.current.ddl).toBe("CREATE TABLE users (v1)"));

    getTableDdl.mockResolvedValueOnce("CREATE TABLE users (v2)");
    await act(async () => {
      await result.current.refreshMetadata();
    });

    expect(reloadMetadata).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(result.current.ddl).toBe("CREATE TABLE users (v2)"));
  });
});
