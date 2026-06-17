import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import type { SshConnection } from "../types";
import { DatabaseView } from "../components/database/DatabaseView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

const legacyConnection = {
  id: "db-1",
  name: "local.db",
  endpoint: { kind: "local", path: "/tmp/local.db" },
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
};

function connection(): SshConnection {
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

describe("DatabaseView connection flow", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(open).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([]);
      if (command === "db_save_connections") return Promise.resolve(undefined);
      if (command === "db_inspect") return Promise.resolve({ objects: [] });
      return Promise.resolve(undefined);
    });
  });

  it("opens the dbx-style new connection wizard without remote path fields", async () => {
    const user = userEvent.setup();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    expect(screen.queryByRole("button", { name: /Open DB/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Remote DB/i })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /New connection/i }));

    expect(screen.getByRole("dialog", { name: "New connection" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /SQLite/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Next/i }));

    expect(screen.getByRole("button", { name: /Connection info/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /TLS\/SSL/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /SSH tunnel \/ proxy/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Advanced/i })).toBeInTheDocument();
    expect(screen.queryByText("SSH connection")).not.toBeInTheDocument();
    expect(screen.queryByText("Database path")).not.toBeInTheDocument();
  });

  it("saves non-sqlite profiles through dbx connection commands", async () => {
    const user = userEvent.setup();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /New connection/i }));
    await user.click(screen.getByRole("button", { name: /Redis/i }));
    await user.click(screen.getByRole("button", { name: /Next/i }));
    await user.click(screen.getByRole("button", { name: /Add connection/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          name: "Redis",
          dbType: "redis",
          readOnly: false,
          dbx: expect.objectContaining({
            db_type: "redis",
            host: "127.0.0.1",
            port: 6379,
          }),
        }),
      });
    });
  });

  it("opens SQL file execution as a workspace instead of immediately selecting a file", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([legacyConnection]);
      if (command === "dbx_list_connections") return Promise.resolve([]);
      if (command === "db_inspect") return Promise.resolve({ objects: [] });
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    const executeSqlFile = await screen.findByRole("button", { name: /Execute SQL file/i });
    await waitFor(() => expect(executeSqlFile).not.toBeDisabled());
    await user.click(executeSqlFile);

    expect(screen.getByText("Choose a SQL file to preview its contents.")).toBeInTheDocument();
    expect(open).not.toHaveBeenCalled();
  });

  it("opens driver management as a database workspace page", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([]);
      if (command === "dbx_driver_manifest") {
        return Promise.resolve({
          schemaVersion: 1,
          drivers: [
            {
              dbType: "postgres",
              label: "PostgreSQL",
              runtimeMode: "native",
              defaultPort: 5432,
              supportLevel: "operate",
              capabilities: { queryExecution: true, sqlFileExecution: true },
            },
          ],
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /Driver manager/i }));

    expect(await screen.findByText("PostgreSQL")).toBeInTheDocument();
    expect(screen.getByText("native")).toBeInTheDocument();
    expect(screen.getByText(/queryExecution/)).toBeInTheDocument();
  });
});
