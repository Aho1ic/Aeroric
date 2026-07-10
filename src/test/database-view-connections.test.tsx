import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import { DatabaseView } from "../components/database/DatabaseView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
}));

import {
  connection,
  dbxConnection,
  legacyConnection,
  menuItemLabels,
  mysqlDbxConnection,
  resetDatabaseViewMocks,
} from "./databaseViewTestUtils";

describe("DatabaseView connection management", () => {
  beforeEach(resetDatabaseViewMocks);
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
    fireEvent.contextMenu(row);
    await user.click(screen.getByRole("menuitem", { name: "Delete connection" }));

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

    const dialog = screen.getByRole("dialog", { name: "New connection" });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /SQLite/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Icon view" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "List view" })).toBeInTheDocument();

    await user.type(screen.getByLabelText("Search database types"), "mongo");
    expect(screen.getByRole("button", { name: /MongoDB/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /SQLite/i })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "List view" }));
    await user.dblClick(screen.getByRole("button", { name: /MongoDB/i }));

    expect(screen.getByRole("button", { name: /Connection info/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /TLS\/SSL/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /SSH tunnel \/ proxy/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Advanced/i })).toBeInTheDocument();
    expect(screen.queryByText("SSH connection")).not.toBeInTheDocument();
    expect(screen.queryByText("Database path")).not.toBeInTheDocument();
  });

  it("does not show stale workspace title content above refresh and insert before a table is selected", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve([]);
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));

    expect(screen.getAllByRole("button", { name: "Refresh" }).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Insert" })).toBeInTheDocument();
    expect(screen.queryByText("No selection")).not.toBeInTheDocument();
    expect(screen.queryByText("postgres: DBX Source")).not.toBeInTheDocument();
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
    await user.click(screen.getByRole("button", { name: "Green" }));
    await user.click(screen.getByRole("button", { name: /Advanced/i }));
    await user.clear(screen.getByLabelText("Query timeout seconds"));
    await user.type(screen.getByLabelText("Query timeout seconds"), "45");
    await user.clear(screen.getByLabelText("Idle timeout seconds"));
    await user.type(screen.getByLabelText("Idle timeout seconds"), "120");
    await user.click(screen.getByLabelText("Use keepalive"));
    await user.clear(screen.getByLabelText("Keepalive interval seconds"));
    await user.type(screen.getByLabelText("Keepalive interval seconds"), "45");
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
            color: "#22c55e",
            connect_timeout_secs: 5,
            query_timeout_secs: 45,
            idle_timeout_secs: 120,
            keepalive_interval_secs: 45,
          }),
        }),
      });
    });
  });

  it("saves Redis sentinel mode fields through dbx connection commands", async () => {
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
    await user.click(screen.getByRole("button", { name: "Sentinel" }));
    await user.type(screen.getByLabelText("Sentinel nodes"), "sentinel-a:26379\nsentinel-b:26380");
    await user.type(screen.getByLabelText("Sentinel master"), "mymaster");
    await user.type(screen.getByLabelText("Sentinel user"), "sentinel_user");
    await user.type(screen.getByLabelText("Sentinel password"), "sentinel_secret");
    await user.click(screen.getByLabelText("Use TLS for Sentinel"));
    await user.clear(screen.getByLabelText("Key separator"));
    await user.type(screen.getByLabelText("Key separator"), "/");
    await user.click(screen.getByRole("button", { name: /Add connection/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          name: "Redis",
          dbType: "redis",
          dbx: expect.objectContaining({
            db_type: "redis",
            host: "sentinel-a",
            port: 26379,
            redis_connection_mode: "sentinel",
            redis_sentinel_nodes: "sentinel-a:26379\nsentinel-b:26380",
            redis_sentinel_master: "mymaster",
            redis_sentinel_username: "sentinel_user",
            redis_sentinel_password: "sentinel_secret",
            redis_sentinel_tls: true,
            redis_cluster_nodes: undefined,
            redis_key_separator: "/",
          }),
        }),
      });
    });
  });

  it("parses connection URLs into DBX connection fields before saving", async () => {
    const user = userEvent.setup();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /New connection/i }));
    await user.click(screen.getByRole("button", { name: /PostgreSQL/i }));
    await user.click(screen.getByRole("button", { name: /Next/i }));
    await user.type(
      screen.getByLabelText("Connection string"),
      "postgresql://alice:secret@db.example.com:6543/app_db?sslmode=require&connectTimeout=15",
    );
    await user.click(screen.getByRole("button", { name: "Parse URL" }));

    expect(screen.getByLabelText("Host")).toHaveValue("db.example.com");
    expect(screen.getByLabelText("Port")).toHaveValue("6543");
    expect(screen.getByLabelText("User")).toHaveValue("alice");
    expect(screen.getByLabelText("Database")).toHaveValue("app_db");
    expect(screen.getByLabelText("URL parameters")).toHaveValue(
      "sslmode=require&connectTimeout=15",
    );

    await user.click(screen.getByRole("button", { name: /Add connection/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          name: "PostgreSQL",
          dbType: "postgres",
          dbx: expect.objectContaining({
            host: "db.example.com",
            port: 6543,
            username: "alice",
            password: "secret",
            database: "app_db",
            connection_string:
              "postgresql://alice:secret@db.example.com:6543/app_db?sslmode=require&connectTimeout=15",
            url_params: "sslmode=require&connectTimeout=15",
          }),
        }),
      });
    });
  });

  it("saves MongoDB URL mode with connection string and parsed fields", async () => {
    const user = userEvent.setup();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /New connection/i }));
    await user.click(screen.getByRole("button", { name: /MongoDB/i }));
    await user.click(screen.getByRole("button", { name: /Next/i }));
    expect(screen.getByRole("button", { name: "Form" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "URL" }));
    await user.type(
      screen.getByLabelText("URL", { selector: "input" }),
      "mongodb+srv://REDACTED_USER:REDACTED_PASSWORD@cluster0.example.mongodb.net/app?authSource=admin&authMechanism=SCRAM-SHA-256",
    );
    await user.click(screen.getByRole("button", { name: "Parse URL" }));
    await user.click(screen.getByRole("button", { name: /Add connection/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          name: "MongoDB",
          dbType: "mongodb",
          dbx: expect.objectContaining({
            db_type: "mongodb",
            host: "cluster0.example.mongodb.net",
            username: "REDACTED_USER",
            password: "REDACTED_PASSWORD",
            database: "app",
            connection_string:
              "mongodb+srv://REDACTED_USER:REDACTED_PASSWORD@cluster0.example.mongodb.net/app?authSource=admin&authMechanism=SCRAM-SHA-256",
            url_params: "authSource=admin&authMechanism=SCRAM-SHA-256",
          }),
        }),
      });
    });
  });

  it("chooses TLS certificate files and saves their paths in DBX connection config", async () => {
    const user = userEvent.setup();
    vi.mocked(open)
      .mockResolvedValueOnce("/certs/ca.pem")
      .mockResolvedValueOnce("/certs/client.crt")
      .mockResolvedValueOnce("/certs/client.key");

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /New connection/i }));
    await user.click(screen.getByRole("button", { name: /PostgreSQL/i }));
    await user.click(screen.getByRole("button", { name: /Next/i }));
    await user.click(screen.getByRole("button", { name: /TLS\/SSL/i }));
    await user.click(screen.getByLabelText("Use TLS/SSL"));
    await user.click(screen.getByRole("button", { name: "Choose CA certificate" }));
    await user.click(screen.getByRole("button", { name: "Choose client certificate" }));
    await user.click(screen.getByRole("button", { name: "Choose client key" }));

    expect(screen.getByLabelText("CA certificate path")).toHaveValue("/certs/ca.pem");
    expect(screen.getByLabelText("Client certificate path")).toHaveValue("/certs/client.crt");
    expect(screen.getByLabelText("Client key path")).toHaveValue("/certs/client.key");

    await user.click(screen.getByRole("button", { name: /Add connection/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          dbType: "postgres",
          dbx: expect.objectContaining({
            ssl: true,
            ca_cert_path: "/certs/ca.pem",
            client_cert_path: "/certs/client.crt",
            client_key_path: "/certs/client.key",
          }),
        }),
      });
    });
  });

  it("saves SSH and proxy transport layers in DBX connection config", async () => {
    const user = userEvent.setup();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /New connection/i }));
    await user.click(screen.getByRole("button", { name: /PostgreSQL/i }));
    await user.click(screen.getByRole("button", { name: /Next/i }));
    await user.click(screen.getByRole("button", { name: /SSH tunnel \/ proxy/i }));
    await user.click(screen.getByRole("button", { name: "Add SSH hop" }));
    await user.type(screen.getByLabelText("SSH host"), "ssh.example.com");
    await user.type(screen.getByLabelText("SSH user"), "deployer");
    await user.type(screen.getByLabelText("SSH password"), "ssh_secret");
    await user.type(screen.getByLabelText("SSH key path"), "/keys/deploy.pem");
    await user.type(screen.getByLabelText("SSH key passphrase"), "key_secret");
    await user.clear(screen.getByLabelText("Connect timeout seconds"));
    await user.type(screen.getByLabelText("Connect timeout seconds"), "10");
    await user.click(screen.getByLabelText("Use SSH agent"));
    await user.click(screen.getByLabelText("Expose LAN"));

    await user.click(screen.getByRole("button", { name: "Add proxy layer" }));
    await user.type(screen.getByLabelText("Proxy host"), "proxy.example.com");
    await user.clear(screen.getByLabelText("Port"));
    await user.type(screen.getByLabelText("Port"), "8080");
    await user.click(screen.getByRole("button", { name: "HTTP" }));
    await user.type(screen.getByLabelText("Proxy username"), "proxy_user");
    await user.type(screen.getByLabelText("Proxy password"), "proxy_secret");
    await user.click(screen.getByRole("button", { name: /Add connection/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          name: "PostgreSQL",
          dbType: "postgres",
          dbx: expect.objectContaining({
            transport_layers: [
              expect.objectContaining({
                type: "ssh",
                enabled: true,
                name: "SSH 1",
                host: "ssh.example.com",
                port: 22,
                user: "deployer",
                password: "ssh_secret",
                key_path: "/keys/deploy.pem",
                key_passphrase: "key_secret",
                connect_timeout_secs: 10,
                expose_lan: true,
                use_ssh_agent: true,
              }),
              expect.objectContaining({
                type: "proxy",
                enabled: true,
                name: "Proxy 2",
                proxy_type: "http",
                host: "proxy.example.com",
                port: 8080,
                username: "proxy_user",
                password: "proxy_secret",
              }),
            ],
          }),
        }),
      });
    });
  });

  it("copies the connection test footer result", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([]);
      if (command === "dbx_test_connection") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /New connection/i }));
    await user.click(screen.getByRole("button", { name: /PostgreSQL/i }));
    await user.click(screen.getByRole("button", { name: /Next/i }));
    await user.click(screen.getByRole("button", { name: /Test connection/i }));

    expect(await screen.findByText("Connection test succeeded.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Copy test result" }));

    expect(writeText).toHaveBeenCalledWith("Connection test succeeded.");
  });

  it("copies and reorders transport layers before saving", async () => {
    const user = userEvent.setup();
    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(screen.getByRole("button", { name: /New connection/i }));
    await user.click(screen.getByRole("button", { name: /PostgreSQL/i }));
    await user.click(screen.getByRole("button", { name: /Next/i }));
    await user.click(screen.getByRole("button", { name: /SSH tunnel \/ proxy/i }));
    await user.click(screen.getByRole("button", { name: "Add SSH hop" }));
    await user.type(screen.getByLabelText("SSH host"), "ssh.example.com");
    await user.type(screen.getByLabelText("SSH user"), "deployer");
    await user.click(screen.getByRole("button", { name: "Copy transport layer" }));
    await user.click(screen.getByRole("button", { name: "Move transport layer up" }));
    await user.clear(screen.getByLabelText("Layer name"));
    await user.type(screen.getByLabelText("Layer name"), "Copied first");
    await user.click(screen.getByRole("button", { name: /Add connection/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          dbType: "postgres",
          dbx: expect.objectContaining({
            transport_layers: [
              expect.objectContaining({
                type: "ssh",
                name: "Copied first",
                host: "ssh.example.com",
                user: "deployer",
              }),
              expect.objectContaining({
                type: "ssh",
                name: "SSH 1",
                host: "ssh.example.com",
                user: "deployer",
              }),
            ],
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
    expect(screen.queryByText("No table selected")).not.toBeInTheDocument();
    expect(screen.queryByText("Select a database connection")).not.toBeInTheDocument();
  });

  it("does not show the table empty state on first database page entry", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await screen.findByRole("button", { name: /New connection/i });

    expect(screen.queryByText("No table selected")).not.toBeInTheDocument();
    expect(screen.queryByText("Select a database connection")).not.toBeInTheDocument();
  });

  it("opens an initial sqlite file path as a database connection", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([]);
      if (command === "db_inspect") {
        return Promise.resolve({
          objects: [
            {
              name: "users",
              objectType: "table",
              columns: [],
              indexes: [],
              foreignKeys: [],
              triggers: [],
              editable: true,
              primaryKeys: [],
              hasRowId: true,
            },
          ],
        });
      }
      if (command === "db_query_table") {
        return Promise.resolve({
          columns: ["id", "name"],
          rows: [],
          page: 1,
          pageSize: 100,
          totalRows: 0,
          editable: true,
          hasRowId: true,
          primaryKeys: [],
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, {
          initialSqliteFilePath: "/repo/app.db",
          projectRoot: "/repo",
          sshConnections: [connection()],
        }),
      ),
    );

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("db_inspect", {
        endpoint: { kind: "local", path: "/repo/app.db" },
        projectRoot: "/repo",
      }),
    );
    expect(invoke).toHaveBeenCalledWith("db_query_table", {
      endpoint: { kind: "local", path: "/repo/app.db" },
      table: "users",
      page: 1,
      pageSize: 100,
      projectRoot: "/repo",
    });
  });

  it("shows only implemented database connection actions from the connection context menu", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    const proxiedConnection = {
      ...dbxConnection,
      dbx: {
        transport_layers: [{ type: "ssh", enabled: true }],
        final_proxy_port: 15432,
      },
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([proxiedConnection]);
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

    const expectedInactiveMenuLabels = [
      "Pin connection",
      "Open connection",
      "New query",
      "Query history",
      "Users and permissions",
      "Copy final proxy port",
      "Execute SQL file",
      "Create database",
      "Move to New Group",
      "Refresh",
      "Select visible databases",
      "Edit connection",
      "Duplicate Connection",
      "Delete connection",
    ];
    expect(menuItemLabels()).toEqual(expectedInactiveMenuLabels);
    for (const label of expectedInactiveMenuLabels) {
      expect(screen.getByRole("menuitem", { name: label })).toBeInTheDocument();
    }
    expect(screen.queryByRole("menuitem", { name: "Close connection" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Open connection" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_connect", { connectionId: proxiedConnection.id });
    });

    fireEvent.contextMenu(await screen.findByRole("button", { name: /DBX Source/i }));
    expect(screen.getByRole("menuitem", { name: "Close connection" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Open connection" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Copy final proxy port" }));
    expect(writeText).toHaveBeenCalledWith("15432");

    fireEvent.contextMenu(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Duplicate Connection" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          name: "DBX Source (Copy)",
          dbx: proxiedConnection.dbx,
        }),
      });
    });
  });

  it("opens DBX user management from the connection context menu and confirms generated SQL", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mysqlDbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("mysql.user")) {
          return Promise.resolve({
            columns: ["user", "host", "plugin"],
            column_types: [],
            column_sortables: [],
            rows: [["app", "%", "mysql_native_password"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("SHOW GRANTS")) {
          return Promise.resolve({
            columns: ["Grants for app@%"],
            column_types: [],
            column_sortables: [],
            rows: [["GRANT SELECT ON `app`.* TO 'app'@'%'"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
      }
      if (command === "dbx_execute_multi") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    fireEvent.contextMenu(await screen.findByRole("button", { name: /MySQL Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Users and permissions" }));

    expect(await screen.findByText("app@%")).toBeInTheDocument();
    expect(await screen.findByText("GRANT SELECT ON `app`.* TO 'app'@'%'")).toBeInTheDocument();

    await user.type(screen.getByLabelText("User name"), "reporter");
    await user.clear(screen.getByLabelText("Password"));
    await user.type(screen.getByLabelText("Password"), "secret");
    await user.click(screen.getByRole("button", { name: "Create user" }));

    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith(
        expect.stringContaining("CREATE USER 'reporter'@'%' IDENTIFIED BY 'secret';"),
        {
          title: "Preview SQL",
          kind: "warning",
        },
      );
    });
    expect(invoke).toHaveBeenCalledWith("dbx_execute_multi", {
      request: expect.objectContaining({
        connectionId: "dbx-mysql",
        sql: "CREATE USER 'reporter'@'%' IDENTIFIED BY 'secret';",
      }),
    });
  });

  it("opens DBX user management from the sidebar utility node context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mysqlDbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_schemas") return Promise.resolve([]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("mysql.user")) {
          return Promise.resolve({
            columns: ["user", "host", "plugin"],
            column_types: [],
            column_sortables: [],
            rows: [["app", "%", "mysql_native_password"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("SHOW GRANTS")) {
          return Promise.resolve({
            columns: ["Grants for app@%"],
            column_types: [],
            column_sortables: [],
            rows: [["GRANT SELECT ON `app`.* TO 'app'@'%'"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
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

    await user.click(await screen.findByRole("button", { name: /MySQL Source/i }));
    const sidebar = screen.getByRole("complementary");
    const userAdminNode = await within(sidebar).findByRole("button", {
      name: "Users and permissions",
    });
    fireEvent.contextMenu(userAdminNode);
    await user.click(screen.getByRole("menuitem", { name: "Open Users & Privileges" }));

    expect(await screen.findByText("app@%")).toBeInTheDocument();
    expect(await screen.findByText("GRANT SELECT ON `app`.* TO 'app'@'%'")).toBeInTheDocument();
  });

  it("filters DBX user management users by search text", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mysqlDbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("mysql.user")) {
          return Promise.resolve({
            columns: ["user", "host", "plugin"],
            column_types: [],
            column_sortables: [],
            rows: [
              ["app", "%", "mysql_native_password"],
              ["readonly", "10.%", "caching_sha2_password"],
            ],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("SHOW GRANTS")) {
          return Promise.resolve({
            columns: ["Grants"],
            column_types: [],
            column_sortables: [],
            rows: [["GRANT SELECT ON `app`.* TO CURRENT_USER"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
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

    fireEvent.contextMenu(await screen.findByRole("button", { name: /MySQL Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Users and permissions" }));

    expect(await screen.findByRole("button", { name: /app@%/ })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /readonly@10.%/ })).toBeInTheDocument();

    await user.type(screen.getByLabelText("Search user"), "readonly");

    expect(screen.queryByRole("button", { name: /app@%/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /readonly@10.%/ })).toBeInTheDocument();

    await user.clear(screen.getByLabelText("Search user"));
    await user.type(screen.getByLabelText("Search user"), "missing");

    expect(screen.getByText("No matching users.")).toBeInTheDocument();
  });

  it("falls back to MySQL privilege metadata when direct user listing is unavailable", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mysqlDbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("mysql.user"))
          return Promise.reject(new Error("SELECT command denied for table 'user'"));
        if (sql.includes("information_schema.USER_PRIVILEGES")) {
          return Promise.resolve({
            columns: ["GRANTEE"],
            column_types: [],
            column_sortables: [],
            rows: [["'readonly'@'10.%'"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("SHOW GRANTS")) {
          return Promise.resolve({
            columns: ["Grants for readonly@10.%"],
            column_types: [],
            column_sortables: [],
            rows: [["GRANT SELECT ON `app`.* TO 'readonly'@'10.%'"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
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

    fireEvent.contextMenu(await screen.findByRole("button", { name: /MySQL Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Users and permissions" }));

    expect(await screen.findByText("readonly@10.%")).toBeInTheDocument();
    expect(
      await screen.findByText("GRANT SELECT ON `app`.* TO 'readonly'@'10.%'"),
    ).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-mysql",
        sql: "SELECT DISTINCT GRANTEE AS grantee FROM information_schema.USER_PRIVILEGES ORDER BY GRANTEE;",
      }),
    });
  });

  it("previews MySQL login locking and grant option SQL from user management", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mysqlDbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("mysql.user")) {
          return Promise.resolve({
            columns: ["user", "host", "plugin"],
            column_types: [],
            column_sortables: [],
            rows: [["app", "%", "mysql_native_password"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("SHOW GRANTS")) {
          return Promise.resolve({
            columns: ["Grants for app@%"],
            column_types: [],
            column_sortables: [],
            rows: [["GRANT SELECT ON `app`.* TO 'app'@'%'"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
      }
      if (command === "dbx_execute_multi") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    fireEvent.contextMenu(await screen.findByRole("button", { name: /MySQL Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Users and permissions" }));
    expect(await screen.findByText("app@%")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Lock account" }));
    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith(
        expect.stringContaining("ALTER USER 'app'@'%' ACCOUNT LOCK;"),
        expect.anything(),
      );
    });

    await user.click(screen.getByLabelText("Allow further grants"));
    await user.click(screen.getByRole("button", { name: "Grant privileges" }));
    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith(
        expect.stringContaining("GRANT SELECT ON `app`.* TO 'app'@'%' WITH GRANT OPTION;"),
        expect.anything(),
      );
    });
  });

  it("loads PostgreSQL user grants with the DBX role summary query", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("r.rolname AS user")) {
          return Promise.resolve({
            columns: ["user", "host", "plugin"],
            column_types: [],
            column_sortables: [],
            rows: [["app_role", "LOGIN", "CREATEDB"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("pg_auth_members")) {
          return Promise.resolve({
            columns: ["line"],
            column_types: [],
            column_sortables: [],
            rows: [["Role: app_role"], ["Attributes: LOGIN, CREATEDB"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
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

    fireEvent.contextMenu(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Users and permissions" }));

    expect(await screen.findByText("app_role")).toBeInTheDocument();
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          sql: expect.stringContaining("pg_auth_members"),
        }),
      });
    });
    expect(await screen.findByText(/Attributes: LOGIN, CREATEDB/)).toBeInTheDocument();
  });

  it("previews PostgreSQL role grants with admin option from user management", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("r.rolname AS user")) {
          return Promise.resolve({
            columns: ["user", "host", "plugin"],
            column_types: [],
            column_sortables: [],
            rows: [["app_role", "LOGIN", "CREATEDB"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("pg_auth_members")) {
          return Promise.resolve({
            columns: ["line"],
            column_types: [],
            column_sortables: [],
            rows: [["Role: app_role"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
      }
      if (command === "dbx_execute_multi") return Promise.resolve([]);
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
    await user.click(screen.getByRole("menuitem", { name: "Users and permissions" }));
    expect(await screen.findByText("app_role")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Scope"), "role");
    await user.type(screen.getByLabelText("Member role"), "reporting");
    await user.click(screen.getByLabelText("Allow admin option"));
    await user.click(screen.getByRole("button", { name: "Grant privileges" }));

    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith(
        expect.stringContaining('GRANT "reporting" TO "app_role" WITH ADMIN OPTION;'),
        expect.anything(),
      );
    });
  });

  it("previews PostgreSQL table grants from the privilege picker", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("r.rolname AS user")) {
          return Promise.resolve({
            columns: ["user", "host", "plugin"],
            column_types: [],
            column_sortables: [],
            rows: [["app_role", "LOGIN", "CREATEDB"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("pg_auth_members")) {
          return Promise.resolve({
            columns: ["line"],
            column_types: [],
            column_sortables: [],
            rows: [["Role: app_role"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
      }
      if (command === "dbx_execute_multi") return Promise.resolve([]);
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
    await user.click(screen.getByRole("menuitem", { name: "Users and permissions" }));
    expect(await screen.findByText("app_role")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Scope"), "table");
    await user.click(screen.getByRole("button", { name: "UPDATE" }));
    await user.click(screen.getByRole("button", { name: "Grant privileges" }));

    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith(
        expect.stringContaining(
          'GRANT SELECT, UPDATE ON ALL TABLES IN SCHEMA "public" TO "app_role";',
        ),
        expect.anything(),
      );
    });
  });

  it("previews PostgreSQL role creation with the can-login option", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "app" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
        if (sql.includes("r.rolname AS user")) {
          return Promise.resolve({
            columns: ["user", "host", "plugin"],
            column_types: [],
            column_sortables: [],
            rows: [["app_role", "LOGIN", "CREATEDB"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
        if (sql.includes("pg_auth_members")) {
          return Promise.resolve({
            columns: ["line"],
            column_types: [],
            column_sortables: [],
            rows: [["Role: app_role"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          });
        }
      }
      if (command === "dbx_execute_multi") return Promise.resolve([]);
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
    await user.click(screen.getByRole("menuitem", { name: "Users and permissions" }));
    expect(await screen.findByText("app_role")).toBeInTheDocument();

    await user.type(screen.getByLabelText("User name"), "batch_role");
    await user.type(screen.getByLabelText("Password"), "secret");
    await user.click(screen.getByLabelText("Can login"));
    await user.click(screen.getByRole("button", { name: "Create user" }));

    await waitFor(() => {
      expect(confirm).toHaveBeenCalledWith(
        expect.stringContaining("CREATE ROLE \"batch_role\" NOLOGIN PASSWORD 'secret';"),
        expect.anything(),
      );
    });
  });

  it("reveals local DBX database files from the connection context menu", async () => {
    const user = userEvent.setup();
    const sqliteConnection = {
      ...dbxConnection,
      id: "sqlite-source",
      name: "SQLite Source",
      dbType: "sqlite",
      dbx: {
        db_type: "sqlite",
        host: "/tmp/project/app.db",
      },
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([sqliteConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "open_in_system_file_manager") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, {
          projectRoot: "/tmp/project",
          sshConnections: [connection()],
        }),
      ),
    );

    fireEvent.contextMenu(await screen.findByRole("button", { name: /SQLite Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Reveal in File Manager" }));

    expect(invoke).toHaveBeenCalledWith("open_in_system_file_manager", {
      path: "/tmp/project/app.db",
      projectPath: "/tmp/project",
    });
  });

  it("backs up local SQLite DBX connections from the connection context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(save).mockResolvedValue("/tmp/project/app.backup.db");
    const sqliteConnection = {
      ...dbxConnection,
      id: "sqlite-source",
      name: "SQLite Source",
      dbType: "sqlite",
      dbx: {
        db_type: "sqlite",
        host: "/tmp/project/app.db",
      },
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([sqliteConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_backup_sqlite_database") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, {
          projectRoot: "/tmp/project",
          sshConnections: [connection()],
        }),
      ),
    );

    fireEvent.contextMenu(await screen.findByRole("button", { name: /SQLite Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Backup SQLite Database" }));

    expect(save).toHaveBeenCalledWith({
      defaultPath: "app.backup.db",
      filters: [{ name: "SQLite", extensions: ["db", "sqlite", "sqlite3"] }],
    });
    expect(invoke).toHaveBeenCalledWith("dbx_backup_sqlite_database", {
      connectionId: "sqlite-source",
      destinationPath: "/tmp/project/app.backup.db",
    });
  });

  it("edits an existing DBX connection from the connection context menu", async () => {
    const user = userEvent.setup();
    const editableConnection = {
      ...dbxConnection,
      name: "Prod PG",
      createdAt: 11,
      lastOpenedAt: 12,
      projectScope: {
        kind: "local",
        projectRoot: "/workspace/app",
        remoteProjectPath: null,
        sshConnectionId: null,
      },
      dbx: {
        id: "dbx-source",
        name: "Prod PG",
        db_type: "postgres",
        driver_profile: "postgres",
        driver_label: "PostgreSQL",
        color: "#3b82f6",
        host: "db.old.example.com",
        port: 5432,
        username: "old_user",
        password: "old_secret",
        database: "old_db",
        url_params: "sslmode=require",
        connect_timeout_secs: 7,
        query_timeout_secs: 20,
        idle_timeout_secs: 30,
        keepalive_interval_secs: 15,
        ssl: true,
        ca_cert_path: "/old/ca.pem",
        visible_databases: ["old_db"],
      },
    };
    let savedConnection = editableConnection;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([savedConnection]);
      if (command === "dbx_save_connection") {
        savedConnection = (args as { connection: typeof editableConnection }).connection;
        return Promise.resolve(undefined);
      }
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "old_db" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
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

    fireEvent.contextMenu(await screen.findByRole("button", { name: /Prod PG/i }));
    await user.click(screen.getByRole("menuitem", { name: "Edit connection" }));

    const dialog = await screen.findByRole("dialog", { name: "Edit connection" });
    expect(within(dialog).getByLabelText("Connection name")).toHaveValue("Prod PG");
    expect(within(dialog).getByLabelText("Host")).toHaveValue("db.old.example.com");
    expect(within(dialog).getByLabelText("User")).toHaveValue("old_user");
    expect(within(dialog).getByLabelText("Database")).toHaveValue("old_db");

    await user.clear(within(dialog).getByLabelText("Connection name"));
    await user.type(within(dialog).getByLabelText("Connection name"), "Prod PG Updated");
    await user.clear(within(dialog).getByLabelText("Host"));
    await user.type(within(dialog).getByLabelText("Host"), "db.new.example.com");
    await user.clear(within(dialog).getByLabelText("User"));
    await user.type(within(dialog).getByLabelText("User"), "new_user");
    await user.clear(within(dialog).getByLabelText("Database"));
    await user.type(within(dialog).getByLabelText("Database"), "new_db");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          id: "dbx-source",
          name: "Prod PG Updated",
          dbType: "postgres",
          createdAt: 11,
          projectScope: editableConnection.projectScope,
          dbx: expect.objectContaining({
            id: "dbx-source",
            name: "Prod PG Updated",
            host: "db.new.example.com",
            username: "new_user",
            database: "new_db",
            visible_databases: ["old_db"],
          }),
        }),
      });
    });
  });

  it("pins and groups DBX connections from the sidebar context menu", async () => {
    const user = userEvent.setup();
    const promptSpy = vi.spyOn(window, "prompt");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(confirm).mockResolvedValue(true);
    const groupedConnection = {
      ...dbxConnection,
      name: "Reports DB",
      connectionGroup: "Analytics",
      pinned: false,
      dbx: {
        id: "dbx-source",
        name: "Reports DB",
        db_type: "postgres",
        driver_profile: "postgres",
        host: "reports.example.com",
        port: 5432,
      },
    };
    let savedConnections: Array<typeof groupedConnection> = [groupedConnection];
    const savedReportsConnection = () => savedConnections.find((item) => item.id === "dbx-source");
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve(savedConnections);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_save_connection") {
        const connection = (args as { connection: typeof groupedConnection }).connection;
        savedConnections = savedConnections.some((item) => item.id === connection.id)
          ? savedConnections.map((item) => (item.id === connection.id ? connection : item))
          : [connection, ...savedConnections];
        return Promise.resolve(undefined);
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

    expect(await screen.findByRole("button", { name: /^Analytics$/i })).toBeInTheDocument();
    const reportsConnection = await screen.findByRole("button", { name: /Reports DB/i });

    fireEvent.contextMenu(screen.getByRole("button", { name: /^Analytics$/i }));
    expect(screen.getByRole("menuitem", { name: "Copy name" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "New connection" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "New group" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Copy name" }));
    expect(writeText).toHaveBeenCalledWith("Analytics");

    promptSpy.mockReturnValueOnce("Archive");
    fireEvent.contextMenu(screen.getByRole("button", { name: /^Analytics$/i }));
    await user.click(screen.getByRole("menuitem", { name: "New group" }));
    expect(await screen.findByRole("button", { name: /Analytics\/Archive/i })).toBeInTheDocument();

    fireEvent.contextMenu(screen.getByRole("button", { name: /^Analytics$/i }));
    await user.click(screen.getByRole("menuitem", { name: "New connection" }));
    await user.click(screen.getByRole("button", { name: /PostgreSQL/i }));
    await user.click(screen.getByRole("button", { name: /Next/i }));
    await user.click(screen.getByRole("button", { name: /Add connection/i }));
    await waitFor(() => {
      expect(savedConnections).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            dbType: "postgres",
            connectionGroup: "Analytics",
          }),
        ]),
      );
    });

    fireEvent.contextMenu(reportsConnection);
    await user.click(screen.getByRole("menuitem", { name: "Pin connection" }));
    await waitFor(() => {
      expect(savedReportsConnection()).toEqual(expect.objectContaining({ pinned: true }));
    });

    promptSpy.mockReturnValueOnce("Warehouse");
    fireEvent.contextMenu(await screen.findByRole("button", { name: /Reports DB/i }));
    await user.click(screen.getByRole("menuitem", { name: "Move to group" }));
    await waitFor(() => {
      expect(savedReportsConnection()).toEqual(
        expect.objectContaining({ connectionGroup: "Warehouse" }),
      );
    });
    expect(await screen.findByRole("button", { name: /Warehouse/i })).toBeInTheDocument();

    promptSpy.mockReturnValueOnce("Ops");
    fireEvent.contextMenu(screen.getByRole("button", { name: /Warehouse/i }));
    await user.click(screen.getByRole("menuitem", { name: "Rename group" }));
    await waitFor(() => {
      expect(savedReportsConnection()).toEqual(expect.objectContaining({ connectionGroup: "Ops" }));
    });
    expect(await screen.findByRole("button", { name: /Ops/i })).toBeInTheDocument();

    fireEvent.contextMenu(screen.getByRole("button", { name: /Ops/i }));
    await user.click(screen.getByRole("menuitem", { name: "Delete group" }));
    await waitFor(() => {
      expect(savedReportsConnection()).toEqual(expect.objectContaining({ connectionGroup: null }));
    });
    expect(confirm).toHaveBeenCalledWith(
      'Delete group "Ops"? Connections stay saved and move to ungrouped.',
      {
        title: "Delete group",
        kind: "warning",
        okLabel: "Delete group",
        cancelLabel: "Cancel",
      },
    );
    promptSpy.mockRestore();
  });
});
