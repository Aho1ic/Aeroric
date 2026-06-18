import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import type { SshConnection } from "../types";
import { DatabaseView } from "../components/database/DatabaseView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
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

const dbxConnection = {
  id: "dbx-source",
  name: "DBX Source",
  dbType: "postgres",
  readOnly: false,
  createdAt: 1,
  lastOpenedAt: 1,
};

const dbxConnectionWithTargetDatabase = {
  ...dbxConnection,
  dbx: {
    database: "target_db",
  },
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
    vi.mocked(confirm).mockReset();
    vi.mocked(open).mockReset();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([]);
      if (command === "db_save_connections") return Promise.resolve(undefined);
      if (command === "db_inspect") return Promise.resolve({ objects: [] });
      return Promise.resolve(undefined);
    });
  });

  it("does not delete a DBX connection until the confirmation resolves true", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(false);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    const row = await screen.findByRole("button", { name: /DBX Source/i });
    await user.click(row.querySelector("svg") as SVGElement);

    await waitFor(() => expect(confirm).toHaveBeenCalled());
    expect(invoke).not.toHaveBeenCalledWith("dbx_delete_connection", expect.anything());
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

  it("shows only implemented database connection actions from the connection context menu", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    fireEvent.contextMenu(await screen.findByRole("button", { name: /DBX Source/i }));

    for (const label of [
      "Close connection",
      "New query",
      "Execute SQL file",
      "Refresh",
      "Copy connection",
      "Delete connection",
    ]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
    }

    for (const label of [
      "Query history",
      "Users and permissions",
      "Create database",
      "Move to group",
      "Select visible databases",
      "Edit connection",
    ]) {
      expect(screen.queryByRole("menuitem", { name: label })).not.toBeInTheDocument();
    }
  });

  it("limits DBX database and table browsing to the configured target database", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnectionWithTargetDatabase]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") {
        return Promise.resolve([{ name: "target_db" }, { name: "other_db" }]);
      }
      if (command === "dbx_list_objects") {
        return Promise.resolve(
          (args as { database?: string | null }).database === "target_db"
            ? [{ name: "target_table", object_type: "table", schema: "public" }]
            : [{ name: "other_table", object_type: "table", schema: "public" }],
        );
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));

    expect(await screen.findByText("target_db")).toBeInTheDocument();
    expect(screen.queryByText("other_db")).not.toBeInTheDocument();
    expect(await screen.findByText("public.target_table")).toBeInTheDocument();
    expect(screen.queryByText("other_table")).not.toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_list_objects", {
      connectionId: "dbx-source",
      database: "target_db",
      schema: null,
    });
  });

  it("reports a missing configured target database instead of loading another database", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnectionWithTargetDatabase]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "other_db" }]);
      if (command === "dbx_list_objects") return Promise.resolve([{ name: "other_table", object_type: "table", schema: "public" }]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));

    expect(await screen.findByText(/Configured database "target_db" was not found/i)).toBeInTheDocument();
    expect(screen.queryByText("other_db")).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("dbx_list_objects", expect.anything());
  });

  it("opens DBX advanced workspaces from toolbar buttons", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") return Promise.resolve([{ name: "id", data_type: "int", is_nullable: false, is_primary_key: true }]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));

    await user.click(screen.getByRole("button", { name: /Data transfer/i }));
    expect(screen.getByLabelText("Source connection")).toHaveValue("dbx-source");
    expect(screen.getByLabelText("Target connection")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Schema diff/i }));
    expect(screen.getAllByRole("button", { name: /Compare/i }).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /Data compare/i }));
    expect(screen.getAllByRole("button", { name: /Compare/i }).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /ER diagram/i }));
    expect(screen.getByText("Inspect table nodes and loaded column metadata.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Table structure/i }));
    expect(await screen.findByDisplayValue("id")).toBeInTheDocument();
  });

  it("shows guidance instead of silently ignoring DBX-only toolbar buttons without a DBX selection", async () => {
    const user = userEvent.setup();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /Data transfer/i }));
    expect(screen.getAllByText("Select a SQL-capable DBX connection to use this tool.").length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /Table structure/i }));
    expect(screen.getAllByText("Select a DBX SQL table to inspect or edit its structure.").length).toBeGreaterThan(0);
  });
});
