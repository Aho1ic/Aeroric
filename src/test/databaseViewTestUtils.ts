import { screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { vi } from "vitest";
import type { SshConnection } from "../types";

export const legacyConnection = {
  id: "db-1",
  name: "local.db",
  endpoint: { kind: "local", path: "/tmp/local.db" },
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
};

export const dbxConnection = {
  id: "dbx-source",
  name: "DBX Source",
  dbType: "postgres",
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
};

export const dbxConnectionWithTargetDatabase = {
  ...dbxConnection,
  dbx: {
    database: "target_db",
  },
};

export const mysqlDbxConnection = {
  id: "dbx-mysql",
  name: "MySQL Source",
  dbType: "mysql",
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
  dbx: {
    db_type: "mysql",
    driver_profile: "mysql",
  },
};

export const duckDbxConnection = {
  id: "dbx-duck",
  name: "DuckDB Source",
  dbType: "duckdb",
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
  dbx: {
    db_type: "duckdb",
    attached_databases: [],
  },
};

export function connection(): SshConnection {
  return {
    id: "conn-1",
    name: "Prod SSH",
    host: "192.168.10.95",
    port: 22,
    username: "root",
    remotePath: "/srv/app",
    createdAt: 1,
  };
}

export function createMockDataTransfer(initialData: Record<string, string> = {}) {
  const data = new Map(Object.entries(initialData));
  return {
    effectAllowed: "all",
    dropEffect: "none",
    setData: vi.fn((type: string, value: string) => {
      data.set(type, value);
    }),
    getData: vi.fn((type: string) => data.get(type) ?? ""),
    clearData: vi.fn((type?: string) => {
      if (type) data.delete(type);
      else data.clear();
    }),
  } as unknown as DataTransfer;
}

export function menuItemLabels() {
  return screen.getAllByRole("menuitem").map((item) => item.textContent?.trim() ?? "");
}

export function resetDatabaseViewMocks() {
  vi.mocked(invoke).mockReset();
  vi.mocked(confirm).mockReset();
  vi.mocked(open).mockReset();
  vi.mocked(save).mockReset();
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
    configurable: true,
  });
  window.localStorage.removeItem("aeroric:database:pinned-nosql-tree-nodes");
  window.localStorage.removeItem("aeroric:database:extra-dbx-connection-groups");
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "db_load_connections") return Promise.resolve([]);
    if (command === "dbx_list_connections") return Promise.resolve([]);
    if (command === "db_save_connections") return Promise.resolve(undefined);
    if (command === "db_inspect") return Promise.resolve({ objects: [] });
    return Promise.resolve(undefined);
  });
}
