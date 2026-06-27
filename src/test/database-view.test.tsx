import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import type { SshConnection } from "../types";
import { DatabaseView } from "../components/database/DatabaseView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
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

const mysqlDbxConnection = {
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

const duckDbxConnection = {
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

function createMockDataTransfer(initialData: Record<string, string> = {}) {
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

function menuItemLabels() {
  return screen.getAllByRole("menuitem").map((item) => item.textContent?.trim() ?? "");
}

describe("DatabaseView connection flow", () => {
  beforeEach(() => {
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

  it("records executed SQL and restores it from query history", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve([]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: ["answer"],
          column_types: ["integer"],
          column_sortables: [true],
          rows: [[42]],
          affected_rows: 1,
          execution_time_ms: 7,
          truncated: false,
          has_more: false,
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

    const connectionButton = await screen.findByRole("button", { name: /DBX Source/i });
    await user.click(connectionButton);
    await user.click(screen.getByRole("button", { name: /New query/i }));
    await user.type(
      screen.getByPlaceholderText("Run SQL against the active database connection"),
      "select 42 as answer",
    );
    await user.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          sql: "select 42 as answer",
        }),
      });
    });

    fireEvent.contextMenu(connectionButton);
    await user.click(screen.getByRole("menuitem", { name: "Query history" }));

    expect(await screen.findByText("select 42 as answer")).toBeInTheDocument();
    expect(screen.getByText("1 rows affected")).toBeInTheDocument();
    expect(screen.getByText("7 ms")).toBeInTheDocument();

    await user.click(
      screen.getByText("select 42 as answer").closest("button") as HTMLButtonElement,
    );
    expect(
      screen.getByPlaceholderText("Run SQL against the active database connection"),
    ).toHaveValue("select 42 as answer");
  });

  it("saves DBX visible database selection and refreshes the tree", async () => {
    const user = userEvent.setup();
    let savedConnection = dbxConnection;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([savedConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") {
        return Promise.resolve([{ name: "template0" }, { name: "main" }, { name: "analytics" }]);
      }
      if (command === "dbx_list_schemas") return Promise.resolve([]);
      if (command === "dbx_list_objects") {
        const database = (args as { database?: string | null } | undefined)?.database;
        return Promise.resolve(
          database === "analytics"
            ? [{ name: "events", object_type: "table", schema: "public" }]
            : [],
        );
      }
      if (command === "dbx_save_connection") {
        savedConnection = (args as { connection: typeof dbxConnection }).connection;
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    expect(await screen.findByRole("button", { name: /^main$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^analytics$/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^template0$/i })).not.toBeInTheDocument();

    fireEvent.contextMenu(screen.getByRole("button", { name: /DBX Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Select visible databases" }));

    const dialog = await screen.findByRole("dialog", { name: "Select visible databases" });
    expect(within(dialog).queryByRole("button", { name: "template0" })).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "main" }));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({
          id: "dbx-source",
          dbx: expect.objectContaining({ visible_databases: ["analytics"] }),
        }),
      });
    });
    expect(await screen.findByRole("button", { name: /^analytics$/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^main$/i })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^events\s+table$/i })).toBeInTheDocument();
  });

  it("creates a database from the DBX connection context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mysqlDbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_build_create_database_sql") {
        return Promise.resolve(
          "CREATE DATABASE `vision` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;",
        );
      }
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    fireEvent.contextMenu(await screen.findByRole("button", { name: /MySQL Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Create database" }));

    const dialog = await screen.findByRole("dialog", { name: "Create database" });
    await user.type(screen.getByLabelText("Database name"), "vision");
    await user.click(within(dialog).getByRole("button", { name: "Create database" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_create_database_sql", {
        options: {
          databaseType: "mysql",
          driverProfile: "mysql",
          name: "vision",
          charset: "utf8mb4",
          collation: "utf8mb4_unicode_ci",
        },
      });
    });
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-mysql",
        database: "",
        sql: "CREATE DATABASE `vision` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;",
      }),
    });
  });

  it("creates and attaches a DuckDB database file from the connection context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(save).mockResolvedValue("/tmp/analytics");
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([duckDbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "dbx_build_duckdb_attach_database_sql") {
        return Promise.resolve("ATTACH '/tmp/analytics.duckdb' AS \"analytics\";");
      }
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
        });
      }
      if (command === "dbx_save_connection") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    fireEvent.contextMenu(await screen.findByRole("button", { name: /DuckDB Source/i }));
    await user.click(screen.getByRole("menuitem", { name: "Create DuckDB file" }));

    await waitFor(() => {
      expect(save).toHaveBeenCalledWith({
        defaultPath: "database.duckdb",
        filters: [{ name: "DuckDB", extensions: ["duckdb", "db"] }],
      });
    });
    expect(invoke).toHaveBeenCalledWith("dbx_build_duckdb_attach_database_sql", {
      options: { path: "/tmp/analytics.duckdb", name: "analytics" },
    });
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-duck",
        database: "",
        sql: "ATTACH '/tmp/analytics.duckdb' AS \"analytics\";",
      }),
    });
    expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
      connection: expect.objectContaining({
        id: "dbx-duck",
        dbx: expect.objectContaining({
          attached_databases: [{ name: "analytics", path: "/tmp/analytics.duckdb" }],
        }),
      }),
    });
  });

  it("runs DBX table administration actions from the object context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: { columns: ["id"], column_types: ["int"], column_sortables: [true], rows: [[1]] },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users"',
        });
      }
      if (command === "dbx_build_truncate_table_sql")
        return Promise.resolve('TRUNCATE TABLE "public"."users";');
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+table$/i }));

    expect(screen.getByRole("menuitem", { name: "View Data" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Edit Structure" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Empty Table" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Truncate Table" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Drop Table" })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Truncate Table" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_truncate_table_sql", {
        options: {
          databaseType: "postgres",
          schema: "public",
          tableName: "users",
        },
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('TRUNCATE TABLE "public"."users";'),
      {
        title: "Truncate Table",
        kind: "warning",
        okLabel: "Truncate Table",
        cancelLabel: "Cancel",
      },
    );
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        sql: 'TRUNCATE TABLE "public"."users";',
      }),
    });
  });

  it("hides DBX truncate table for database types that do not support truncate", async () => {
    const user = userEvent.setup();
    const sqliteConnection = {
      id: "dbx-sqlite",
      name: "SQLite Source",
      dbType: "sqlite",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([sqliteConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve([]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: null }]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /SQLite Source/i }));
    const tableLabel = await screen.findByText("users");
    fireEvent.contextMenu(tableLabel.closest("button") as HTMLButtonElement);

    expect(screen.queryByRole("menuitem", { name: "Truncate Table" })).not.toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Empty Table" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Drop Table" })).toBeInTheDocument();
  });

  it("opens DBX table info with columns and child objects from the object context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "users", object_type: "table", schema: "public" },
          {
            name: "users_email_idx",
            object_type: "INDEX",
            schema: "public",
            parent_schema: "public",
            parent_name: "users",
          },
          {
            name: "users_org_fk",
            object_type: "FOREIGN_KEY",
            schema: "public",
            parent_schema: "public",
            parent_name: "users",
          },
          {
            name: "users_audit_trg",
            object_type: "TRIGGER",
            schema: "public",
            parent_schema: "public",
            parent_name: "users",
          },
        ]);
      }
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          {
            name: "id",
            data_type: "integer",
            is_nullable: false,
            is_primary_key: true,
            column_default: "nextval('users_id_seq'::regclass)",
          },
          {
            name: "email",
            data_type: "text",
            is_nullable: true,
            is_primary_key: false,
            column_default: null,
          },
        ]);
      }
      if (command === "dbx_get_table_ddl")
        return Promise.resolve("CREATE TABLE public.users (id integer);");
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
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await user.click(screen.getByRole("menuitem", { name: "Table info" }));

    await screen.findByRole("tablist", { name: "Table info sections" });
    expect(screen.queryByText("main / public.users")).not.toBeInTheDocument();
    expect(screen.queryByText("Table info")).not.toBeInTheDocument();
    const tableInfoTabs = screen.getByRole("tablist", { name: "Table info sections" });
    const columnsTab = within(tableInfoTabs).getByRole("tab", { name: /Columns 2/i });
    const indexesTab = within(tableInfoTabs).getByRole("tab", { name: /Indexes 1/i });
    const foreignKeysTab = within(tableInfoTabs).getByRole("tab", { name: /Foreign keys 1/i });
    const triggersTab = within(tableInfoTabs).getByRole("tab", { name: /Triggers 1/i });
    const ddlTab = await within(tableInfoTabs).findByRole("tab", { name: /DDL 1/i });
    expect(columnsTab).toHaveAttribute("aria-selected", "true");
    expect(indexesTab).toBeInTheDocument();
    expect(foreignKeysTab).toBeInTheDocument();
    expect(triggersTab).toBeInTheDocument();
    expect(ddlTab).toBeInTheDocument();
    const tableInfoSearch = screen.getByLabelText("Search table info");
    expect(tableInfoSearch).toBeInTheDocument();
    expect(screen.getAllByText("id").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText(/integer/).length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("nextval('users_id_seq'::regclass)")).toBeInTheDocument();
    expect(screen.getAllByText("email").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText(/text/).length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("🔑").length).toBeGreaterThanOrEqual(1);
    await user.type(tableInfoSearch, "email");
    expect(screen.queryByText("nextval('users_id_seq'::regclass)")).not.toBeInTheDocument();
    expect(screen.getAllByText("email").length).toBeGreaterThanOrEqual(1);
    await user.clear(tableInfoSearch);
    await user.click(indexesTab);
    expect(screen.getAllByText("users_email_idx").length).toBeGreaterThanOrEqual(1);
    await user.click(foreignKeysTab);
    expect(screen.getAllByText("users_org_fk").length).toBeGreaterThanOrEqual(1);
    await user.click(triggersTab);
    expect(screen.getAllByText("users_audit_trg").length).toBeGreaterThanOrEqual(1);
    expect(invoke).toHaveBeenCalledWith("dbx_get_table_ddl", {
      connectionId: "dbx-source",
      database: "main",
      schema: "public",
      table: "users",
    });
    await user.click(ddlTab);
    expect(await screen.findByText("CREATE TABLE public.users (id integer);")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "View DDL" })).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_get_columns", {
      connectionId: "dbx-source",
      database: "main",
      schema: "public",
      table: "users",
    });
  });

  it("applies DBX grid filtering, sorting, search, and column visibility controls", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          {
            name: "id",
            data_type: "integer",
            is_nullable: false,
            is_primary_key: true,
            comment: "User id",
          },
          {
            name: "email",
            data_type: "varchar(50)",
            is_nullable: true,
            is_primary_key: false,
            comment: "Email address",
          },
          { name: "status", data_type: "text", is_nullable: true, is_primary_key: false },
          { name: "notes", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email", "status", "notes"],
            column_types: ["integer", "varchar(50)", "text", "text"],
            column_sortables: [true, true, false, true],
            rows: [
              [1, "alice@example.com", "active", null],
              [2, "bob@example.com", "pending", null],
            ],
          },
          totalRows: 2,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
        });
      }
      if (command === "dbx_get_table_ddl") {
        return Promise.resolve('CREATE TABLE "public"."users" (id integer);');
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
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    expect(await screen.findByText("alice@example.com")).toBeInTheDocument();
    expect(screen.getByText("bob@example.com")).toBeInTheDocument();
    expect(
      screen.queryByPlaceholderText("Run SQL against the active database connection"),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Run" })).not.toBeInTheDocument();
    expect(screen.queryByRole("columnheader", { name: "rowid" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New query" })).toBeInTheDocument();
    expect(screen.getByLabelText("Rows")).toHaveValue("100");
    const filterGroup = screen.getByRole("group", { name: "Table filters" });
    expect(filterGroup).toHaveStyle({ flexWrap: "wrap" });
    expect(within(filterGroup).getByLabelText("WHERE")).toBeInTheDocument();
    expect(within(filterGroup).getByLabelText("ORDER BY")).toBeInTheDocument();
    expect(within(filterGroup).getByLabelText("Search page")).toBeInTheDocument();
    expect(within(filterGroup).queryByLabelText("Export format")).not.toBeInTheDocument();
    expect(
      within(filterGroup).queryByRole("button", { name: "Apply filter" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Data tools" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Field filter" })).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Table row count" })).toHaveTextContent(
      "2 rows total",
    );
    expect(screen.getByRole("status", { name: "Current SQL" })).toHaveTextContent(
      'SELECT * FROM "public"."users" LIMIT 100;',
    );
    const paginationGroup = screen.getByRole("group", { name: "Table pagination" });
    expect(paginationGroup).toHaveTextContent("Page 1 / 1");
    expect(paginationGroup).toHaveStyle({ whiteSpace: "nowrap" });
    expect(screen.getByText("Rows").closest("label")).toHaveStyle({ whiteSpace: "nowrap" });

    await user.selectOptions(screen.getByLabelText("Rows"), "200");
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          table: "users",
          page: 1,
          pageSize: 200,
        }),
      });
    });

    await user.type(screen.getByLabelText("WHERE"), "status = 'active'");
    await user.type(screen.getByLabelText("ORDER BY"), "email DESC");
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          table: "users",
          page: 1,
          whereInput: "status = 'active'",
          orderBy: "email DESC",
        }),
      });
    });

    const idHeaderButton = screen
      .getAllByRole("button", { name: "id" })
      .find(
        (button) => !button.hasAttribute("aria-pressed") && button.querySelector("svg"),
      ) as HTMLButtonElement;
    await user.click(idHeaderButton);
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({
          whereInput: "status = 'active'",
          orderBy: '"id" ASC',
        }),
      });
    });

    const table = screen.getByRole("table");
    const idType = within(table).getByTitle("Column type: integer");
    expect(idType).toHaveTextContent("integer");
    expect(idType).toHaveStyle({ color: "#3b82f6", fontWeight: "800" });
    const emailType = within(table).getByTitle("Column type: varchar(50)");
    expect(emailType).toHaveTextContent("varchar(50)");
    expect(emailType).toHaveStyle({ color: "#f59e0b", fontWeight: "800" });
    expect(emailType.style.fontFamily).toContain("Monaco");
    expect(emailType.parentElement).toHaveStyle({ alignItems: "flex-start", textAlign: "left" });
    expect(within(table).getAllByTitle("Column type: text")).toHaveLength(2);
    const emailHeaderButton = within(
      within(table).getByRole("columnheader", { name: /email/ }),
    ).getByRole("button", { name: "email" });
    expect(emailHeaderButton).toHaveStyle({
      display: "grid",
      gridTemplateColumns: "minmax(0, 1fr) 30px",
    });
    const idHeader = within(table).getByRole("columnheader", { name: /id/ });
    expect(idHeader).toHaveTextContent("id");
    expect(idHeader).toHaveTextContent("integer");
    expect(idHeader).toHaveTextContent("User id");
    expect(idHeader).toHaveAttribute("title", expect.stringContaining("Column: id"));
    expect(idHeader).toHaveAttribute("title", expect.stringContaining("Comment: User id"));
    const statusHeader = within(table).getByRole("columnheader", { name: "status" });
    expect(within(statusHeader).getAllByRole("button")).toHaveLength(1);
    expect(within(statusHeader).getByRole("button", { name: "Resize status" })).toBeInTheDocument();
    expect(screen.queryByText("Table")).not.toBeInTheDocument();
    expect(screen.queryByText("postgres: DBX Source")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Select visible rows")).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Select row 1" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select row 1" })).toHaveTextContent("1");
    const emailCellText = screen.getByText("alice@example.com");
    expect(emailCellText).toHaveStyle({ fontWeight: "700" });
    expect(screen.queryByRole("button", { name: "Preview email" })).not.toBeInTheDocument();
    fireEvent.mouseEnter(emailCellText.closest("td") as HTMLTableCellElement);
    expect(screen.getAllByRole("button", { name: "Preview email" }).length).toBeGreaterThanOrEqual(
      1,
    );
    fireEvent.mouseLeave(emailCellText.closest("td") as HTMLTableCellElement);
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Preview email" })).not.toBeInTheDocument(),
    );

    await user.type(screen.getByLabelText("Search page"), "active");
    await waitFor(() => {
      expect(screen.getAllByText(/@example\.com$/)).toHaveLength(1);
    });
    expect(screen.getByText("alice@example.com")).toBeInTheDocument();
    await user.clear(screen.getByLabelText("Search page"));

    expect(screen.queryByRole("group", { name: "Columns" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Field filter" }));
    const fieldFilter = screen.getByRole("menu", { name: "Field filter" });
    await user.type(within(fieldFilter).getByLabelText("Search columns..."), "note");
    expect(within(fieldFilter).getByRole("checkbox", { name: "notes" })).toBeInTheDocument();
    expect(within(fieldFilter).queryByRole("checkbox", { name: "email" })).not.toBeInTheDocument();
    await user.clear(within(fieldFilter).getByLabelText("Search columns..."));

    expect(screen.getAllByText("NULL").length).toBeGreaterThanOrEqual(2);
    await user.click(within(fieldFilter).getByRole("button", { name: "Invert" }));
    expect(screen.queryAllByText("NULL")).toHaveLength(0);
    await user.click(within(fieldFilter).getByRole("button", { name: "Show all" }));
    expect(screen.getAllByText("NULL").length).toBeGreaterThanOrEqual(2);

    const emailResizeHandle = screen.getByRole("button", { name: "Resize email" });
    expect(emailResizeHandle.closest("th")).toHaveStyle({ width: "184px" });
    fireEvent.pointerDown(emailResizeHandle, { clientX: 100 });
    await waitFor(() => expect(document.body.style.cursor).toBe("col-resize"));
    fireEvent.pointerMove(window, { clientX: 260 });
    await waitFor(() => expect(emailResizeHandle.closest("th")).toHaveStyle({ width: "344px" }));
    fireEvent.pointerUp(window);
    fireEvent.doubleClick(emailResizeHandle);
    await waitFor(() => expect(emailResizeHandle.closest("th")).toHaveStyle({ width: "184px" }));

    const emailVisibilityButton = within(fieldFilter).getByRole("checkbox", { name: "email" });
    await user.click(emailVisibilityButton);

    expect(screen.queryAllByDisplayValue(/@example\.com$/)).toHaveLength(0);
    expect(screen.getByText("active")).toBeInTheDocument();
    expect(screen.getByText("pending")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Table properties" }));
    await screen.findByRole("tablist", { name: "Table info sections" });
    expect(screen.queryByText("main / public.users")).not.toBeInTheDocument();
    expect(screen.queryByText("Table info")).not.toBeInTheDocument();
    const tableInfoTabs = screen.getByRole("tablist", { name: "Table info sections" });
    const ddlTab = await within(tableInfoTabs).findByRole("tab", { name: /DDL 1/i });
    expect(ddlTab).toBeInTheDocument();
  });

  it("keeps long DBX column type text clear of sort controls", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          {
            name: "description",
            data_type: "varchar(1000)",
            is_nullable: true,
            is_primary_key: false,
            comment: "Long text",
          },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["description"],
            column_types: ["text"],
            column_sortables: [true],
            rows: [["long value"]],
          },
          totalRows: 100,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));

    const table = await screen.findByRole("table");
    const header = within(table).getByRole("columnheader", { name: /description/ });
    expect(within(header).getByTitle("Column type: varchar(1000)")).toHaveTextContent(
      "varchar(1000)",
    );
    expect(header).toHaveStyle({ width: "152px", minWidth: "152px", maxWidth: "152px" });
    expect(within(header).getByRole("button", { name: "description" })).toHaveStyle({
      gridTemplateColumns: "minmax(0, 1fr) 30px",
      gap: "8px",
    });
  });

  it("resizes the database sidebar from its right edge", async () => {
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

    const resizeHandle = await screen.findByRole("separator", { name: "Resize database sidebar" });
    const root = resizeHandle.closest("aside")?.parentElement;
    expect(root).toHaveStyle({ gridTemplateColumns: "284px minmax(0, 1fr)" });

    fireEvent.pointerDown(resizeHandle, { clientX: 284 });
    await waitFor(() => expect(document.body.style.cursor).toBe("col-resize"));
    fireEvent.pointerMove(window, { clientX: 404 });
    await waitFor(() => expect(root).toHaveStyle({ gridTemplateColumns: "404px minmax(0, 1fr)" }));
    fireEvent.pointerUp(window);
  });

  it("opens workspace tab context menu with title, pin, and close actions", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id"],
            column_types: ["integer"],
            column_sortables: [true],
            rows: [[1]],
          },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    const tab = await screen.findByRole("tab", { name: /users/ });
    fireEvent.contextMenu(tab);

    const menu = await screen.findByRole("menu", { name: "Tab actions" });
    expect(
      within(menu).getByRole("menuitemcheckbox", { name: "Shorten tab title" }),
    ).toHaveAttribute("aria-checked", "false");
    const pinItem = within(menu).getByRole("menuitem", { name: "Pin" });
    expect(pinItem).toBeInTheDocument();
    expect(pinItem).toHaveAttribute("style", expect.stringContaining("color: var(--text-primary)"));
    expect(within(menu).getByRole("menuitem", { name: "Close tab" })).toBeInTheDocument();
    expect(within(menu).getByRole("menuitem", { name: "Close other tabs" })).toBeInTheDocument();
    expect(within(menu).getByRole("menuitem", { name: "Close all tabs" })).toBeInTheDocument();

    await user.click(within(menu).getByRole("menuitemcheckbox", { name: "Shorten tab title" }));
    await waitFor(() =>
      expect(screen.queryByRole("menu", { name: "Tab actions" })).not.toBeInTheDocument(),
    );
    await waitFor(() => {
      expect(within(screen.getByRole("tab", { name: /users/ })).getByText("users")).toHaveStyle({
        maxWidth: "72px",
      });
    });
  });

  it("exports the active DBX grid with filters, ordering, and visible columns", async () => {
    const user = userEvent.setup();
    vi.mocked(save).mockResolvedValue("/tmp/users.json");
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
          { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["integer", "text"],
            column_sortables: [true, true],
            rows: [[1, "alice@example.com"]],
          },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
        });
      }
      if (command === "dbx_export_table_json") return Promise.resolve(undefined);
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
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await screen.findByText("alice@example.com");
    await user.type(screen.getByLabelText("WHERE"), "status = 'active'");
    await user.type(screen.getByLabelText("ORDER BY"), "email DESC");
    await user.click(screen.getByRole("button", { name: "Field filter" }));
    const fieldFilter = screen.getByRole("menu", { name: "Field filter" });
    const emailVisibilityButton = within(fieldFilter).getByRole("checkbox", { name: "email" });
    await user.click(emailVisibilityButton);
    expect(screen.queryByLabelText("Export format")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Data tools" }));
    const dataTools = screen.getByRole("menu", { name: "Data tools" });
    await user.click(within(dataTools).getByRole("menuitem", { name: "Export data" }));
    await user.click(within(dataTools).getByRole("menuitem", { name: "JSON" }));

    await waitFor(() => {
      expect(save).toHaveBeenCalledWith({
        defaultPath: "users.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
    });
    expect(invoke).toHaveBeenCalledWith("dbx_export_table_json", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        schema: "public",
        tableName: "users",
        filePath: "/tmp/users.json",
        format: "json",
        columns: ["id"],
        columnTypes: ["integer"],
        primaryKeys: ["id"],
        whereInput: "status = 'active'",
        orderBy: "email DESC",
        batchSize: 1000,
      }),
    });
  });

  it("previews DBX grid JSON cell values in a dialog", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
          { name: "profile", data_type: "jsonb", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "profile"],
            column_types: ["integer", "jsonb"],
            column_sortables: [true, true],
            rows: [
              [1, '{"name":"Alice","roles":["admin","editor"]}'],
              [2, '{"name":"Bob","roles":["viewer"]}'],
            ],
          },
          totalRows: 2,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
        });
      }
      if (command === "dbx_get_table_ddl")
        return Promise.resolve("CREATE TABLE public.users (id integer);");
      if (command === "dbx_build_data_grid_context_filter_condition") {
        return Promise.resolve('"profile" = \'{"name":"Alice","roles":["admin","editor"]}\'');
      }
      if (command === "dbx_build_data_grid_copy_insert_statement") {
        const excludePrimaryKeys = (args as { options: { excludePrimaryKeys?: boolean } }).options
          .excludePrimaryKeys;
        return Promise.resolve(
          excludePrimaryKeys
            ? 'INSERT INTO "public"."users" ("profile") VALUES (\'{"name":"Alice"}\');'
            : 'INSERT INTO "public"."users" ("id", "profile") VALUES (1, \'{"name":"Alice"}\');',
        );
      }
      if (command === "dbx_build_data_grid_copy_update_statements") {
        return Promise.resolve([
          'UPDATE "public"."users" SET "profile" = \'{"name":"Alice"}\' WHERE "id" = 1;',
        ]);
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
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    const formattedProfile = `{
  "name": "Alice",
  "roles": [
    "admin",
    "editor"
  ]
}`;
    const firstProfileText = await screen.findByText('{"name":"Alice","roles":["admin","editor"]}');
    fireEvent.mouseEnter(firstProfileText.closest("td") as HTMLTableCellElement);
    await screen.findAllByRole("button", { name: "Preview profile" });
    const openProfileCellMenu = async () => {
      await waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());
      const profileCell = screen
        .getAllByRole("button", { name: "Preview profile" })[0]
        .closest("td") as HTMLTableCellElement;
      fireEvent.contextMenu(profileCell);
      return screen.findByRole("menu");
    };

    let menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy value" }));
    expect(writeText).toHaveBeenLastCalledWith(formattedProfile);

    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy column name" }));
    expect(writeText).toHaveBeenLastCalledWith("profile");

    const rowJson = JSON.stringify(
      { id: 1, profile: '{"name":"Alice","roles":["admin","editor"]}' },
      null,
      2,
    );
    const rowTsv = 'id\tprofile\n1\t{"name":"Alice","roles":["admin","editor"]}';
    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Open Row Details" }));
    const rowDialog = await screen.findByRole("dialog", { name: "Row 1 Details" });
    expect(within(rowDialog).getByText("2 columns")).toBeInTheDocument();
    expect(within(rowDialog).getByText("id")).toBeInTheDocument();
    expect(within(rowDialog).getByText("profile")).toBeInTheDocument();
    expect(rowDialog).toHaveTextContent('"name": "Alice"');
    expect(rowDialog).toHaveTextContent('"editor"');
    await user.type(within(rowDialog).getByPlaceholderText("Search field or value..."), "profile");
    expect(within(rowDialog).queryByText("integer")).not.toBeInTheDocument();
    await user.click(within(rowDialog).getByRole("button", { name: "Copy profile value" }));
    expect(writeText).toHaveBeenLastCalledWith(formattedProfile);
    await user.click(within(rowDialog).getByRole("button", { name: "Copy Row (JSON)" }));
    expect(writeText).toHaveBeenLastCalledWith(rowJson);
    await user.click(within(rowDialog).getByRole("button", { name: "Copy Row (TSV)" }));
    expect(writeText).toHaveBeenLastCalledWith(rowTsv);
    await user.click(within(rowDialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog", { name: "Row 1 Details" })).not.toBeInTheDocument();

    const formattedBobProfile = `{
  "name": "Bob",
  "roles": [
    "viewer"
  ]
}`;
    const columnJson = JSON.stringify(
      [
        { row: 1, value: '{"name":"Alice","roles":["admin","editor"]}' },
        { row: 2, value: '{"name":"Bob","roles":["viewer"]}' },
      ],
      null,
      2,
    );
    const columnTsv =
      '{"name":"Alice","roles":["admin","editor"]}\n{"name":"Bob","roles":["viewer"]}';
    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Open Column Details" }));
    const columnDialog = await screen.findByRole("dialog", { name: "profile Column Details" });
    expect(within(columnDialog).getByText("profile")).toBeInTheDocument();
    expect(within(columnDialog).getByText("jsonb")).toBeInTheDocument();
    await user.type(within(columnDialog).getByPlaceholderText("Search field or value..."), "Bob");
    expect(
      within(columnDialog).queryByRole("button", { name: "Copy row 1 value" }),
    ).not.toBeInTheDocument();
    await user.click(within(columnDialog).getByRole("button", { name: "Copy row 2 value" }));
    expect(writeText).toHaveBeenLastCalledWith(formattedBobProfile);
    await user.click(within(columnDialog).getByRole("button", { name: "Copy Column (JSON)" }));
    expect(writeText).toHaveBeenLastCalledWith(columnJson);
    await user.click(within(columnDialog).getByRole("button", { name: "Copy Column (TSV)" }));
    expect(writeText).toHaveBeenLastCalledWith(columnTsv);
    await user.click(within(columnDialog).getByRole("button", { name: "Copy column name" }));
    expect(writeText).toHaveBeenLastCalledWith("profile");
    await user.click(within(columnDialog).getByRole("button", { name: "Close" }));
    expect(
      screen.queryByRole("dialog", { name: "profile Column Details" }),
    ).not.toBeInTheDocument();

    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Filter by This Value" }));
    expect(invoke).toHaveBeenCalledWith("dbx_build_data_grid_context_filter_condition", {
      options: {
        databaseType: "postgres",
        columnName: "profile",
        mode: "equals",
        value: '{"name":"Alice","roles":["admin","editor"]}',
        columnInfo: {
          name: "profile",
          data_type: "jsonb",
          is_nullable: true,
          is_primary_key: false,
        },
      },
    });
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({
          whereInput: '"profile" = \'{"name":"Alice","roles":["admin","editor"]}\'',
        }),
      });
    });
    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Clear filter" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({ whereInput: null }),
      });
    });
    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Sort ascending" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({ orderBy: '"profile" ASC' }),
      });
    });
    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Sort descending" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({ orderBy: '"profile" DESC' }),
      });
    });
    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Clear sort" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({ orderBy: null }),
      });
    });

    const openProfileHeaderMenu = async () => {
      await waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());
      const profileHeaderButton = screen
        .getAllByRole("button", { name: "profile" })
        .find((button) => button.closest("th"));
      expect(profileHeaderButton).toBeTruthy();
      fireEvent.contextMenu(profileHeaderButton!.closest("th") as HTMLTableCellElement);
      return screen.findByRole("menu");
    };
    menu = await openProfileHeaderMenu();
    expect(within(menu).queryByRole("menuitem", { name: "Copy value" })).not.toBeInTheDocument();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy column name" }));
    expect(writeText).toHaveBeenLastCalledWith("profile");
    menu = await openProfileHeaderMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Sort ascending" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({ orderBy: '"profile" ASC' }),
      });
    });
    menu = await openProfileHeaderMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Sort descending" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({ orderBy: '"profile" DESC' }),
      });
    });
    menu = await openProfileHeaderMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Clear sort" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({ orderBy: null }),
      });
    });
    menu = await openProfileHeaderMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Open Column Details" }));
    const headerColumnDialog = await screen.findByRole("dialog", {
      name: "profile Column Details",
    });
    expect(within(headerColumnDialog).getByText("jsonb")).toBeInTheDocument();
    await user.click(within(headerColumnDialog).getByRole("button", { name: "Close" }));
    expect(
      screen.queryByRole("dialog", { name: "profile Column Details" }),
    ).not.toBeInTheDocument();

    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy All (TSV)" }));
    expect(writeText).toHaveBeenLastCalledWith(
      [
        "id\tprofile",
        '1\t{"name":"Alice","roles":["admin","editor"]}',
        '2\t{"name":"Bob","roles":["viewer"]}',
      ].join("\n"),
    );

    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy Row (JSON)" }));
    await waitFor(() => expect(writeText).toHaveBeenLastCalledWith(rowJson));

    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy as INSERT" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("dbx_build_data_grid_copy_insert_statement", {
        options: expect.objectContaining({
          databaseType: "postgres",
          columns: ["id", "profile"],
          sourceColumns: ["id", "profile"],
          rows: [[1, '{"name":"Alice","roles":["admin","editor"]}']],
          excludePrimaryKeys: false,
          tableMeta: expect.objectContaining({
            schema: "public",
            tableName: "users",
            primaryKeys: ["id"],
          }),
        }),
      }),
    );
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        'INSERT INTO "public"."users" ("id", "profile") VALUES (1, \'{"name":"Alice"}\');',
      ),
    );

    menu = await openProfileCellMenu();
    await user.click(
      within(menu).getByRole("menuitem", { name: "Copy as INSERT without Primary Keys" }),
    );
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        'INSERT INTO "public"."users" ("profile") VALUES (\'{"name":"Alice"}\');',
      ),
    );
    expect(invoke).toHaveBeenCalledWith("dbx_build_data_grid_copy_insert_statement", {
      options: expect.objectContaining({
        excludePrimaryKeys: true,
      }),
    });

    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy as UPDATE" }));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        'UPDATE "public"."users" SET "profile" = \'{"name":"Alice"}\' WHERE "id" = 1;',
      ),
    );
    expect(invoke).toHaveBeenCalledWith("dbx_build_data_grid_copy_update_statements", {
      options: expect.objectContaining({
        databaseType: "postgres",
        columns: ["id", "profile"],
        sourceColumns: ["id", "profile"],
        rows: [[1, '{"name":"Alice","roles":["admin","editor"]}']],
        tableMeta: expect.objectContaining({
          schema: "public",
          tableName: "users",
          primaryKeys: ["id"],
        }),
      }),
    });

    menu = await openProfileCellMenu();
    await user.click(within(menu).getByRole("menuitem", { name: "Preview value" }));

    const dialog = await screen.findByRole("dialog", { name: "Cell value" });
    expect(within(dialog).getByText("profile")).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Document JSON")).toHaveValue(formattedProfile);
    await user.click(within(dialog).getByRole("button", { name: "Copy value" }));
    expect(writeText).toHaveBeenLastCalledWith(formattedProfile);
    await user.click(within(dialog).getByRole("button", { name: "Copy column name" }));
    expect(writeText).toHaveBeenLastCalledWith("profile");

    await user.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog", { name: "Cell value" })).not.toBeInTheDocument();
  });

  it("selects DBX grid cell text on double click and saves edits only from the Save button", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
          { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["integer", "text"],
            column_sortables: [true, true],
            rows: [[1, "alice@example.com"]],
          },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
        });
      }
      if (command === "dbx_update_cell") {
        const execute = (args as { request: { execute?: boolean } }).request.execute;
        return Promise.resolve({
          statements: ['UPDATE "public"."users" SET "email" = \'alice@new.test\' WHERE "id" = 1;'],
          rollbackStatements: [
            'UPDATE "public"."users" SET "email" = \'alice@example.com\' WHERE "id" = 1;',
          ],
          validationError: null,
          executionSchema: "public",
          executed: Boolean(execute),
          rowsAffected: execute ? 1 : 0,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    const emailSpan = await screen.findByText("alice@example.com");
    const emailTd = emailSpan.closest("td") as HTMLTableCellElement;
    await user.dblClick(emailTd);
    const emailInput = emailTd.querySelector("input") as HTMLInputElement;
    await waitFor(() => {
      expect(emailInput.selectionStart).toBe(0);
      expect(emailInput.selectionEnd).toBe("alice@example.com".length);
    });
    await user.clear(emailInput);
    await user.type(emailInput, "alice@new.test");
    fireEvent.blur(emailInput);

    expect(invoke).not.toHaveBeenCalledWith("dbx_update_cell", expect.anything());
    expect(confirm).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_update_cell", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          execute: false,
          options: expect.objectContaining({
            databaseType: "postgres",
            tableMeta: expect.objectContaining({
              schema: "public",
              tableName: "users",
              primaryKeys: ["id"],
            }),
            columns: ["id", "email"],
            rows: [[1, "alice@example.com"]],
            dirtyRows: [[0, [[1, "alice@new.test"]]]],
          }),
        }),
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('UPDATE "public"."users" SET "email"'),
      {
        title: "Update cell",
        kind: "warning",
        okLabel: "Update cell",
        cancelLabel: "Cancel",
      },
    );
    expect(invoke).toHaveBeenCalledWith("dbx_update_cell", {
      request: expect.objectContaining({
        execute: true,
        options: expect.objectContaining({
          dirtyRows: [[0, [[1, "alice@new.test"]]]],
        }),
      }),
    });
  });

  it("previews and confirms DBX grid row inserts before executing them", async () => {
    const user = userEvent.setup();
    const promptSpy = vi.spyOn(window, "prompt").mockReturnValueOnce('{"email":"new@example.com"}');
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          {
            name: "id",
            data_type: "integer",
            is_nullable: false,
            is_primary_key: true,
            column_default: "nextval('users_id_seq'::regclass)",
          },
          { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["integer", "text"],
            column_sortables: [true, true],
            rows: [[1, "alice@example.com"]],
          },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
        });
      }
      if (command === "dbx_insert_row") {
        const execute = (args as { request: { execute?: boolean } }).request.execute;
        return Promise.resolve({
          statements: ['INSERT INTO "public"."users" ("email") VALUES (\'new@example.com\');'],
          rollbackStatements: ['DELETE FROM "public"."users" WHERE "email" = \'new@example.com\';'],
          validationError: null,
          executionSchema: "public",
          executed: Boolean(execute),
          rowsAffected: execute ? 1 : 0,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await screen.findByText("alice@example.com");
    await user.click(screen.getByRole("button", { name: "Insert" }));

    expect(promptSpy).toHaveBeenCalledWith("Insert row as JSON object", '{"email":null}');
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_insert_row", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          execute: false,
          options: expect.objectContaining({
            databaseType: "postgres",
            tableMeta: expect.objectContaining({
              schema: "public",
              tableName: "users",
              primaryKeys: ["id"],
            }),
            columns: ["id", "email"],
            rows: [[1, "alice@example.com"]],
            newRows: [[null, "new@example.com"]],
          }),
        }),
      });
    });
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining('INSERT INTO "public"."users"'), {
      title: "Insert row",
      kind: "warning",
      okLabel: "Insert",
      cancelLabel: "Cancel",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_insert_row", {
      request: expect.objectContaining({
        execute: true,
        options: expect.objectContaining({
          newRows: [[null, "new@example.com"]],
        }),
      }),
    });
    promptSpy.mockRestore();
  });

  it("previews and confirms DBX grid row deletes before executing them", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
          { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["integer", "text"],
            column_sortables: [true, true],
            rows: [[1, "alice@example.com"]],
          },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
        });
      }
      if (command === "dbx_delete_rows") {
        const execute = (args as { request: { execute?: boolean } }).request.execute;
        return Promise.resolve({
          statements: ['DELETE FROM "public"."users" WHERE "id" = 1;'],
          rollbackStatements: [
            'INSERT INTO "public"."users" ("id", "email") VALUES (1, \'alice@example.com\');',
          ],
          validationError: null,
          executionSchema: "public",
          executed: Boolean(execute),
          rowsAffected: execute ? 1 : 0,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await screen.findByText("alice@example.com");
    await user.click(screen.getByRole("button", { name: "Select row 1" }));
    await user.click(screen.getByRole("button", { name: /Delete selected/i }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_delete_rows", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          execute: false,
          options: expect.objectContaining({
            databaseType: "postgres",
            tableMeta: expect.objectContaining({
              schema: "public",
              tableName: "users",
              primaryKeys: ["id"],
            }),
            columns: ["id", "email"],
            rows: [[1, "alice@example.com"]],
            deletedRows: [0],
          }),
        }),
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('DELETE FROM "public"."users" WHERE "id"'),
      {
        title: "Delete selected",
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Cancel",
      },
    );
    expect(invoke).toHaveBeenCalledWith("dbx_delete_rows", {
      request: expect.objectContaining({
        execute: true,
        options: expect.objectContaining({
          deletedRows: [0],
        }),
      }),
    });
  });

  it("previews and confirms DBX selected row deletes before executing them", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
          { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["integer", "text"],
            column_sortables: [true, true],
            rows: [
              [1, "alice@example.com"],
              [2, "bob@example.com"],
            ],
          },
          totalRows: 2,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
        });
      }
      if (command === "dbx_delete_rows") {
        const execute = (args as { request: { execute?: boolean } }).request.execute;
        return Promise.resolve({
          statements: [
            'DELETE FROM "public"."users" WHERE "id" = 1;',
            'DELETE FROM "public"."users" WHERE "id" = 2;',
          ],
          rollbackStatements: [
            'INSERT INTO "public"."users" ("id", "email") VALUES (1, \'alice@example.com\');',
            'INSERT INTO "public"."users" ("id", "email") VALUES (2, \'bob@example.com\');',
          ],
          validationError: null,
          executionSchema: "public",
          executed: Boolean(execute),
          rowsAffected: execute ? 2 : 0,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await screen.findByText("alice@example.com");
    await user.click(screen.getByRole("button", { name: "Select row 1" }));
    await user.click(screen.getByRole("button", { name: "Select row 2" }));
    const emailCell = screen.getByText("alice@example.com").closest("td") as HTMLTableCellElement;
    fireEvent.contextMenu(emailCell);
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByRole("menuitem", { name: "Copy 2 Rows (JSON)" })).toBeInTheDocument();
    expect(
      within(menu).getByRole("menuitem", { name: "Copy 2 Rows as INSERT" }),
    ).toBeInTheDocument();
    expect(
      within(menu).getByRole("menuitem", { name: "Copy 2 Rows as INSERT without Primary Keys" }),
    ).toBeInTheDocument();
    expect(
      within(menu).getByRole("menuitem", { name: "Copy 2 Rows as UPDATE" }),
    ).toBeInTheDocument();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy 2 Rows (JSON)" }));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        JSON.stringify(
          [
            { id: 1, email: "alice@example.com" },
            { id: 2, email: "bob@example.com" },
          ],
          null,
          2,
        ),
      ),
    );
    writeText.mockClear();
    await user.click(emailCell);
    fireEvent.keyDown(screen.getByRole("grid", { name: "Data grid" }), { key: "c", metaKey: true });
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("id\temail\n1\talice@example.com\n2\tbob@example.com"),
    );
    writeText.mockClear();
    await user.click(screen.getByRole("button", { name: "Copy selected (2)" }));
    expect(writeText).toHaveBeenCalledWith("id\temail\n1\talice@example.com\n2\tbob@example.com");
    await user.click(screen.getByRole("button", { name: "Delete selected (2)" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_delete_rows", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          execute: false,
          options: expect.objectContaining({
            databaseType: "postgres",
            tableMeta: expect.objectContaining({
              schema: "public",
              tableName: "users",
              primaryKeys: ["id"],
            }),
            columns: ["id", "email"],
            rows: [
              [1, "alice@example.com"],
              [2, "bob@example.com"],
            ],
            deletedRows: [0, 1],
          }),
        }),
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('DELETE FROM "public"."users" WHERE "id" = 1;'),
      {
        title: "Delete selected",
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Cancel",
      },
    );
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining("Delete 2 selected rows?"),
      expect.anything(),
    );
    expect(invoke).toHaveBeenCalledWith("dbx_delete_rows", {
      request: expect.objectContaining({
        execute: true,
        options: expect.objectContaining({
          deletedRows: [0, 1],
        }),
      }),
    });
  });

  it("selects a contiguous DBX row range with shift-click and keeps row numbers borderless", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
          { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["integer", "text"],
            column_sortables: [true, true],
            rows: [
              [1, "alice@example.com"],
              [2, "bob@example.com"],
              [3, "carol@example.com"],
            ],
          },
          totalRows: 3,
          sql: 'SELECT * FROM "public"."users"',
          countSql: 'SELECT count(*) FROM "public"."users"',
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await screen.findByText("carol@example.com");

    const rowOne = screen.getByRole("button", { name: "Select row 1" });
    const rowThree = screen.getByRole("button", { name: "Select row 3" });
    await user.click(rowOne);
    await user.keyboard("{Shift>}");
    await user.click(rowThree);
    await user.keyboard("{/Shift}");

    expect(screen.getByRole("button", { name: "Copy selected (3)" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Delete selected (3)" })).toBeEnabled();
    expect(rowOne).toHaveStyle({ borderStyle: "none" });
    expect(rowThree).toHaveStyle({ borderStyle: "none" });
  });

  it("opens DBX object DDL and creates SQL drafts from the object context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_table_ddl")
        return Promise.resolve('CREATE TABLE "public"."users" ("id" int);');
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
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+table$/i }));
    expect(screen.getByRole("menuitem", { name: "View DDL" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "New SQL: SELECT" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "New SQL: INSERT" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "New SQL: UPDATE" })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "View DDL" }));
    expect(
      await screen.findByDisplayValue('CREATE TABLE "public"."users" ("id" int);'),
    ).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_get_table_ddl", {
      connectionId: "dbx-source",
      database: "main",
      schema: "public",
      table: "users",
    });

    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await user.click(screen.getByRole("menuitem", { name: "New SQL: UPDATE" }));
    expect(screen.getByDisplayValue(/UPDATE "public"\."users"/i)).toBeInTheDocument();
  });

  it("duplicates DBX table structure from the table context menu with SQL preview", async () => {
    const user = userEvent.setup();
    const promptSpy = vi.spyOn(window, "prompt").mockReturnValueOnce("users_archive");
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "users", object_type: "TABLE", schema: "public" },
          { name: "users_copy", object_type: "TABLE", schema: "public" },
        ]);
      }
      if (command === "dbx_build_duplicate_table_structure_sql") {
        return Promise.resolve(
          'CREATE TABLE "public"."users_archive" (LIKE "public"."users" INCLUDING ALL);',
        );
      }
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 0,
          execution_time_ms: 4,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    const tableNode = await screen.findByRole("button", { name: /^users\s+table$/i });
    fireEvent.contextMenu(tableNode);
    expect(screen.getByRole("menuitem", { name: "Duplicate structure" })).toBeInTheDocument();
    const tableMenuLabels = screen.getAllByRole("menuitem").map((item) => item.textContent ?? "");
    expect(tableMenuLabels.indexOf("Duplicate structure")).toBeLessThan(
      tableMenuLabels.indexOf("Truncate Table"),
    );
    expect(tableMenuLabels.indexOf("Truncate Table")).toBeLessThan(
      tableMenuLabels.indexOf("Empty Table"),
    );
    expect(tableMenuLabels.indexOf("Empty Table")).toBeLessThan(
      tableMenuLabels.indexOf("Drop Table"),
    );

    await user.click(screen.getByRole("menuitem", { name: "Duplicate structure" }));

    expect(promptSpy).toHaveBeenCalledWith("New table name", "users_copy_2");
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_duplicate_table_structure_sql", {
        options: {
          databaseType: "postgres",
          schema: "public",
          sourceName: "users",
          targetName: "users_archive",
        },
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      'Duplicate structure from "public.users" to "users_archive"?\n\nCREATE TABLE "public"."users_archive" (LIKE "public"."users" INCLUDING ALL);',
      {
        title: "Duplicate structure",
        kind: "warning",
        okLabel: "Duplicate structure",
        cancelLabel: "Cancel",
      },
    );
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        schema: "public",
        sql: 'CREATE TABLE "public"."users_archive" (LIKE "public"."users" INCLUDING ALL);',
      }),
    });
    promptSpy.mockRestore();
  });

  it("opens DBX view source from the view-specific Edit view menu item", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "active_users", object_type: "VIEW", schema: "public" }]);
      if (command === "dbx_get_object_source") {
        return Promise.resolve({
          name: "active_users",
          object_type: "VIEW",
          schema: "public",
          source: 'CREATE VIEW "public"."active_users" AS SELECT * FROM "public"."users";',
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    const viewNode = await screen.findByRole("button", { name: /^active_users\s+view$/i });
    fireEvent.contextMenu(viewNode);

    expect(screen.getByRole("menuitem", { name: "Edit view" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "View source" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "View DDL" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Drop view" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "New query" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Edit Structure" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "New SQL: INSERT" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "New SQL: UPDATE" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Data compare" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Edit view" }));

    expect(
      await screen.findByDisplayValue(
        'CREATE VIEW "public"."active_users" AS SELECT * FROM "public"."users";',
      ),
    ).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_get_object_source", {
      connectionId: "dbx-source",
      database: "main",
      schema: "public",
      name: "active_users",
      objectType: "VIEW",
    });

    fireEvent.contextMenu(viewNode);
    await user.click(screen.getByRole("menuitem", { name: "New query" }));
    expect(
      screen.getByDisplayValue(/SELECT \* FROM "public"\."active_users"/i),
    ).toBeInTheDocument();
  });

  it("renames DBX table objects through dbx-core SQL preview", async () => {
    const user = userEvent.setup();
    const promptSpy = vi.spyOn(window, "prompt").mockReturnValueOnce("app_users");
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_build_rename_object_sql") {
        return Promise.resolve('ALTER TABLE "public"."users" RENAME TO "app_users";');
      }
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await user.click(screen.getByRole("menuitem", { name: "Rename" }));

    expect(promptSpy).toHaveBeenCalledWith("New object name", "users");
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_rename_object_sql", {
        options: {
          databaseType: "postgres",
          objectType: "TABLE",
          schema: "public",
          oldName: "users",
          newName: "app_users",
        },
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('ALTER TABLE "public"."users" RENAME TO "app_users";'),
      {
        title: "Rename",
        kind: "warning",
        okLabel: "Rename",
        cancelLabel: "Cancel",
      },
    );
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        schema: "public",
        sql: 'ALTER TABLE "public"."users" RENAME TO "app_users";',
      }),
    });
    promptSpy.mockRestore();
  });

  it("opens DBX routine source and uses routine-specific context menu actions", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "refresh_stats", object_type: "PROCEDURE", schema: "public" },
        ]);
      }
      if (command === "dbx_get_object_source") {
        return Promise.resolve({
          name: "refresh_stats",
          object_type: "PROCEDURE",
          schema: "public",
          source: 'CREATE PROCEDURE "public"."refresh_stats"() LANGUAGE SQL AS $$ SELECT 1 $$;',
        });
      }
      if (command === "dbx_build_drop_object_sql")
        return Promise.resolve('DROP PROCEDURE "public"."refresh_stats";');
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    const procedureNode = await screen.findByRole("button", {
      name: /^refresh_stats\s+PROCEDURE$/i,
    });
    await user.click(procedureNode);

    expect(
      await screen.findByDisplayValue(
        'CREATE PROCEDURE "public"."refresh_stats"() LANGUAGE SQL AS $$ SELECT 1 $$;',
      ),
    ).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_get_object_source", {
      connectionId: "dbx-source",
      database: "main",
      schema: "public",
      name: "refresh_stats",
      objectType: "PROCEDURE",
    });

    fireEvent.contextMenu(procedureNode);
    expect(screen.getByRole("menuitem", { name: "Execute procedure" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "View source" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Drop procedure" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Copy name" })).not.toBeInTheDocument();
    const procedureMenuLabels = screen
      .getAllByRole("menuitem")
      .map((item) => item.textContent ?? "");
    expect(procedureMenuLabels.indexOf("Execute procedure")).toBeLessThan(
      procedureMenuLabels.indexOf("View source"),
    );
    expect(procedureMenuLabels.indexOf("View source")).toBeLessThan(
      procedureMenuLabels.indexOf("Drop procedure"),
    );
    expect(screen.queryByRole("menuitem", { name: "View Data" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "New SQL: INSERT" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Execute procedure" }));
    expect(screen.getByDisplayValue('CALL "public"."refresh_stats"();')).toBeInTheDocument();

    fireEvent.contextMenu(procedureNode);
    await user.click(screen.getByRole("menuitem", { name: "Drop procedure" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_drop_object_sql", {
        options: {
          databaseType: "postgres",
          objectType: "PROCEDURE",
          schema: "public",
          name: "refresh_stats",
        },
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('DROP PROCEDURE "public"."refresh_stats";'),
      {
        title: "Drop procedure",
        kind: "warning",
        okLabel: "Drop procedure",
        cancelLabel: "Cancel",
      },
    );
  });

  it("orders DBX sequence object context menus like dbx", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([{ name: "order_seq", object_type: "SEQUENCE", schema: "public" }]);
      }
      if (command === "dbx_get_object_source") {
        return Promise.resolve({
          name: "order_seq",
          object_type: "SEQUENCE",
          schema: "public",
          source: 'CREATE SEQUENCE "public"."order_seq";',
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    const sequenceNode = await screen.findByRole("button", { name: /^order_seq\s+SEQUENCE$/i });

    fireEvent.contextMenu(sequenceNode);
    expect(screen.getByRole("menuitem", { name: "View source" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Copy name" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Drop object" })).not.toBeInTheDocument();
    const sequenceMenuLabels = screen
      .getAllByRole("menuitem")
      .map((item) => item.textContent ?? "");
    expect(sequenceMenuLabels.indexOf("View source")).toBeLessThan(
      sequenceMenuLabels.indexOf("Copy name"),
    );

    await user.click(screen.getByRole("menuitem", { name: "Copy name" }));
    expect(writeText).toHaveBeenCalledWith("public.order_seq");

    fireEvent.contextMenu(sequenceNode);
    await user.click(screen.getByRole("menuitem", { name: "View source" }));
    expect(
      await screen.findByDisplayValue('CREATE SEQUENCE "public"."order_seq";'),
    ).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_get_object_source", {
      connectionId: "dbx-source",
      database: "main",
      schema: "public",
      name: "order_seq",
      objectType: "SEQUENCE",
    });
  });

  it("exports DBX tables and copies structure from the object context menu", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(save).mockResolvedValue("/tmp/users.csv");
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_export_table_csv") return Promise.resolve(undefined);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "int", is_nullable: false, is_primary_key: true },
          { name: "name", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
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
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+table$/i }));
    expect(screen.getByRole("menuitem", { name: "Export CSV" })).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "Copy structure as Markdown" }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Export CSV" }));
    await waitFor(() => {
      expect(save).toHaveBeenCalledWith({
        defaultPath: "users.csv",
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
    });
    expect(invoke).toHaveBeenCalledWith("dbx_export_table_csv", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        schema: "public",
        tableName: "users",
        filePath: "/tmp/users.csv",
        format: "csv",
      }),
    });

    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await user.click(screen.getByRole("menuitem", { name: "Copy structure as Markdown" }));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining("| Column | Type | Nullable | Primary key |"),
      );
    });
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining("| id | int | no | yes |"));
  });

  it("imports data into a DBX table from the object context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(open).mockResolvedValue("/tmp/users.csv");
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "int", is_nullable: false, is_primary_key: true },
          { name: "name", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_preview_table_import_file") {
        return Promise.resolve({
          fileName: "users.csv",
          filePath: "/tmp/users.csv",
          fileType: "csv",
          sizeBytes: 32,
          columns: ["id", "name"],
          rows: [[1, "Ada"]],
          totalRows: 1,
        });
      }
      if (command === "dbx_import_table_file") {
        return Promise.resolve({ importId: "import-1", rowsImported: 1, totalRows: 1 });
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          sql: 'SELECT * FROM "public"."users"',
          result: {
            columns: ["id", "name"],
            column_types: ["int", "text"],
            column_sortables: [true, true],
            rows: [[1, "Ada"]],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          },
          totalRows: 1,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^users\s+table$/i }));
    await user.click(screen.getByRole("menuitem", { name: "Import table" }));

    const dialog = await screen.findByRole("dialog", { name: "Import table" });
    await user.click(within(dialog).getByRole("button", { name: "Choose file" }));
    expect(open).toHaveBeenCalledWith({
      multiple: false,
      filters: [
        { name: "Data files", extensions: ["csv", "tsv", "json", "xlsx", "xlsm", "xls"] },
        { name: "CSV", extensions: ["csv", "tsv"] },
        { name: "JSON", extensions: ["json"] },
        { name: "Excel", extensions: ["xlsx", "xlsm", "xls"] },
      ],
    });

    expect(await within(dialog).findByText("users.csv")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Import table" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_import_table_file", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          table: "users",
          filePath: "/tmp/users.csv",
          mappings: [
            { sourceColumn: "id", targetColumn: "id" },
            { sourceColumn: "name", targetColumn: "name" },
          ],
          mode: "append",
          batchSize: 500,
        }),
      });
    });
  });

  it("drops DBX views through the generic object SQL builder", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "active_users", object_type: "view", schema: "public" }]);
      if (command === "dbx_build_drop_object_sql")
        return Promise.resolve('DROP VIEW "public"."active_users";');
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^active_users\s+view$/i }));
    await user.click(screen.getByRole("menuitem", { name: "Drop view" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_drop_object_sql", {
        options: {
          databaseType: "postgres",
          objectType: "VIEW",
          schema: "public",
          name: "active_users",
        },
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('DROP VIEW "public"."active_users";'),
      {
        title: "Drop view",
        kind: "warning",
        okLabel: "Drop view",
        cancelLabel: "Cancel",
      },
    );
  });

  it("renders DBX table columns and drops a column from the column context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["int", "text"],
            column_sortables: [true, true],
            rows: [[1, "a@example.com"]],
          },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users"',
        });
      }
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "int", is_nullable: false, is_primary_key: true },
          { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_build_drop_table_child_object_sql") {
        return Promise.resolve('ALTER TABLE "public"."users" DROP COLUMN "email";');
      }
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    const emailColumn = (await screen.findAllByRole("button", { name: /email/i })).find(
      (button) => button.textContent?.includes("#") && button.textContent?.includes("text"),
    ) as HTMLButtonElement;
    fireEvent.contextMenu(emailColumn);

    expect(screen.getByRole("menuitem", { name: "Copy name" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Drop column" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Drop column" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_drop_table_child_object_sql", {
        options: {
          databaseType: "postgres",
          objectType: "COLUMN",
          schema: "public",
          tableName: "users",
          name: "email",
        },
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('ALTER TABLE "public"."users" DROP COLUMN "email";'),
      {
        title: "Drop column",
        kind: "warning",
        okLabel: "Drop column",
        cancelLabel: "Cancel",
      },
    );
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        sql: 'ALTER TABLE "public"."users" DROP COLUMN "email";',
      }),
    });
  });

  it("renders DBX table indexes, foreign keys, and triggers as child nodes with drop SQL preview", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "users", object_type: "table", schema: "public" },
          {
            name: "users_email_idx",
            object_type: "INDEX",
            schema: "public",
            parent_schema: "public",
            parent_name: "users",
          },
          {
            name: "users_org_fk",
            object_type: "FOREIGN_KEY",
            schema: "public",
            parent_schema: "public",
            parent_name: "users",
          },
          {
            name: "users_touch",
            object_type: "TRIGGER",
            schema: "public",
            parent_schema: "public",
            parent_name: "users",
          },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["int", "text"],
            column_sortables: [true, true],
            rows: [[1, "a@example.com"]],
          },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users"',
        });
      }
      if (command === "dbx_get_columns") return Promise.resolve([]);
      if (command === "dbx_build_drop_table_child_object_sql") {
        return Promise.resolve('DROP INDEX "public"."users_email_idx";');
      }
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));

    const indexNode = await screen.findByRole("button", { name: /users_email_idx/i });
    expect(screen.getByRole("button", { name: /users_org_fk/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /users_touch/i })).toBeInTheDocument();

    fireEvent.contextMenu(indexNode);
    expect(screen.getByRole("menuitem", { name: "Copy name" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Drop index" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Drop index" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_drop_table_child_object_sql", {
        options: {
          databaseType: "postgres",
          objectType: "INDEX",
          schema: "public",
          tableName: "users",
          name: "users_email_idx",
        },
      });
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('DROP INDEX "public"."users_email_idx";'),
      {
        title: "Drop index",
        kind: "warning",
        okLabel: "Drop index",
        cancelLabel: "Cancel",
      },
    );
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        sql: 'DROP INDEX "public"."users_email_idx";',
      }),
    });
  });

  it("filters the DBX sidebar tree while keeping matched object parents visible", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "users", object_type: "table", schema: "public" },
          { name: "orders", object_type: "table", schema: "public" },
        ]);
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
    expect(await screen.findByRole("button", { name: /^users\s+table$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^orders\s+table$/i })).toBeInTheDocument();

    await user.type(screen.getByLabelText("Sidebar search"), "orders");

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /^users\s+table$/i })).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: /DBX Source/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /main/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^orders\s+table$/i })).toBeInTheDocument();
  });

  it("drops a DBX table reference from the tree into the SQL editor", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
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
    const tableNode = await screen.findByRole("button", { name: /^users\s+table$/i });
    const dataTransfer = createMockDataTransfer();
    fireEvent.dragStart(tableNode, { dataTransfer });

    await user.click(screen.getByRole("button", { name: "New query" }));
    const editor = screen.getByPlaceholderText(
      "Run SQL against the active database connection",
    ) as HTMLTextAreaElement;
    await user.type(editor, "select * from ");
    editor.setSelectionRange(editor.value.length, editor.value.length);
    fireEvent.drop(editor, { dataTransfer });

    expect(editor).toHaveValue("select * from public.users");
  });

  it("runs database node context menu actions through DBX SQL builders", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_build_create_schema_sql")
        return Promise.resolve('CREATE SCHEMA "analytics";');
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^main$/i }));

    expect(screen.getByRole("menuitem", { name: "Copy name" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Create schema" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Drop database" })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Create schema" }));
    const dialog = await screen.findByRole("dialog", { name: "Create schema" });
    await user.type(screen.getByLabelText("Schema name"), "analytics");
    await user.click(within(dialog).getByRole("button", { name: "Create schema" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_create_schema_sql", {
        options: { databaseType: "postgres", name: "analytics" },
      });
    });
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        sql: 'CREATE SCHEMA "analytics";',
      }),
    });
  });

  it("orders and gates DBX database and schema node context menus like dbx", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
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

    fireEvent.contextMenu(await screen.findByRole("button", { name: /^main$/i }));
    expect(menuItemLabels()).toEqual([
      "Pin",
      "Copy name",
      "New query",
      "Query history",
      "Set as default database",
      "Create table",
      "Create schema",
      "Execute SQL file",
      "ER diagram",
      "Database search",
      "Refresh",
      "Data transfer",
      "Schema diff",
      "Data compare",
      "Export database",
      "Close database connection",
      "Drop database",
    ]);

    fireEvent.contextMenu(await screen.findByRole("button", { name: /^public$/i }));
    expect(menuItemLabels()).toEqual([
      "Pin",
      "Copy name",
      "Open object browser",
      "New query",
      "Query history",
      "Create table",
      "Execute SQL file",
      "ER diagram",
      "Database search",
      "Refresh",
      "Data transfer",
      "Schema diff",
      "Data compare",
      "Export database",
      "Drop schema",
    ]);
  });

  it("gates DBX DuckDB database node actions like dbx", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([duckDbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
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

    await user.click(await screen.findByRole("button", { name: /DuckDB Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^main$/i }));

    expect(screen.queryByRole("menuitem", { name: "Open object browser" })).not.toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Create schema" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "ER diagram" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Drop database" })).not.toBeInTheDocument();
  });

  it("opens DBX object group context menu actions for create table, create view, and refresh", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "users", object_type: "TABLE", schema: "public" },
          { name: "active_users", object_type: "VIEW", schema: "public" },
        ]);
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
    const sidebar = screen.getByRole("complementary");
    const publicSchemaBranch = (await within(sidebar).findByRole("button", { name: /^public$/i }))
      .parentElement!;
    const tablesGroup = within(publicSchemaBranch).getByRole("button", { name: "Tables" });
    const viewsGroup = within(publicSchemaBranch).getByRole("button", { name: "Views" });

    const listObjectsCallsBefore = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === "dbx_list_objects").length;
    fireEvent.contextMenu(tablesGroup);
    expect(screen.getByRole("menuitem", { name: "Create table" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Refresh" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Refresh" }));
    await waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.filter(([command]) => command === "dbx_list_objects").length,
      ).toBeGreaterThan(listObjectsCallsBefore);
    });

    fireEvent.contextMenu(tablesGroup);
    await user.click(screen.getByRole("menuitem", { name: "Create table" }));
    expect(
      screen.getByPlaceholderText("Run SQL against the active database connection"),
    ).toHaveValue(
      "CREATE TABLE public.table_name (\n  id INTEGER PRIMARY KEY,\n  name TEXT NOT NULL\n);",
    );

    fireEvent.contextMenu(viewsGroup);
    expect(screen.getByRole("menuitem", { name: "Create view" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Create view" }));
    expect(
      screen.getByPlaceholderText("Run SQL against the active database connection"),
    ).toHaveValue("CREATE VIEW public.new_view AS\nSELECT\n  *\nFROM table_name;\n");
  });

  it("exports a DBX database from the database node context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(save).mockResolvedValue("/tmp/main.sql");
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve([]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "users", object_type: "table", schema: "public" },
          { name: "orders", object_type: "table", schema: "public" },
        ]);
      }
      if (command === "dbx_export_database") return Promise.resolve(undefined);
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
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^main$/i }));
    await user.click(screen.getByRole("menuitem", { name: "Export database" }));

    const dialog = await screen.findByRole("dialog", { name: "Export database" });
    expect(within(dialog).getByRole("button", { name: "users" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "orders" }));
    await user.click(within(dialog).getByRole("button", { name: "Export database" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_export_database", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "main",
          filePath: "/tmp/main.sql",
          selectedTables: ["users"],
          includeStructure: true,
          includeData: true,
          includeObjects: true,
          dropTableIfExists: false,
          batchSize: 1000,
        }),
      });
    });
  });

  it("opens DBX database search from a database node and opens a matched result", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "integer", is_nullable: false, is_primary_key: true },
          { name: "email", data_type: "text", is_nullable: true, is_primary_key: false },
        ]);
      }
      if (command === "dbx_build_database_search_sql") {
        return Promise.resolve({
          sql: 'SELECT * FROM "public"."users" WHERE LOWER(CAST("email" AS TEXT)) LIKE \'%alice%\' LIMIT 20;',
          searchableColumns: ["email"],
        });
      }
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: ["id", "email"],
          column_types: ["integer", "text"],
          column_sortables: [true, true],
          rows: [[1, "alice@example.com"]],
          affected_rows: 0,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
        });
      }
      if (command === "dbx_build_search_result_where") return Promise.resolve('"id" = 1');
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id", "email"],
            column_types: ["integer", "text"],
            column_sortables: [true, true],
            rows: [[1, "alice@example.com"]],
            affected_rows: 0,
            execution_time_ms: 2,
            truncated: false,
            has_more: false,
          },
          totalRows: 1,
          sql: 'SELECT * FROM "public"."users" WHERE "id" = 1',
          countSql: 'SELECT count(*) FROM "public"."users" WHERE "id" = 1',
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /^main$/i }));
    expect(screen.getByRole("menuitem", { name: "ER diagram" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Database search" }));

    expect(await screen.findByLabelText("Keyword")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Keyword"), "alice");
    await user.click(screen.getByRole("button", { name: "Search" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_database_search_sql", {
        options: expect.objectContaining({
          databaseType: "postgres",
          schema: "public",
          tableName: "users",
          term: "alice",
          limit: 20,
        }),
      });
    });
    expect(invoke).toHaveBeenCalledWith("dbx_build_search_result_where", {
      options: expect.objectContaining({
        databaseType: "postgres",
        resultColumns: ["id", "email"],
        row: [1, "alice@example.com"],
        matchedColumns: ["email"],
      }),
    });

    const resultPreview = await screen.findByText(/email: alice@example\.com/i);
    await user.click(resultPreview.closest("button") as HTMLButtonElement);

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_query_table_data", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          table: "users",
          whereInput: '"id" = 1',
        }),
      });
    });
  });

  it("renders DBX schema nodes and drops schema with SQL preview", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public", "analytics"]);
      if (command === "dbx_list_objects") {
        const schema = (args as { schema?: string | null } | undefined)?.schema;
        if (schema === "analytics")
          return Promise.resolve([{ name: "events", object_type: "table", schema: "analytics" }]);
        return Promise.resolve([
          { name: "users", object_type: "table", schema: "public" },
          { name: "events", object_type: "table", schema: "analytics" },
        ]);
      }
      if (command === "dbx_build_drop_schema_sql")
        return Promise.resolve('DROP SCHEMA "analytics";');
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    expect(await screen.findByRole("button", { name: /^public$/i })).toBeInTheDocument();
    const analyticsSchema = await screen.findByRole("button", { name: /^analytics$/i });
    fireEvent.contextMenu(analyticsSchema);

    expect(screen.getByRole("menuitem", { name: "Copy name" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Drop schema" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Drop schema" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_drop_schema_sql", {
        options: { databaseType: "postgres", name: "analytics" },
      });
    });
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining('DROP SCHEMA "analytics";'), {
      title: "Drop schema",
      kind: "warning",
      okLabel: "Drop schema",
      cancelLabel: "Cancel",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_execute_query", {
      request: expect.objectContaining({
        connectionId: "dbx-source",
        database: "main",
        sql: 'DROP SCHEMA "analytics";',
      }),
    });
  });

  it("opens DBX schema utility workspaces from the schema context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(open).mockResolvedValue("/tmp/schema.sql");
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "db_read_sql_file") return Promise.resolve("SELECT 1;");
      if (command === "dbx_execute_sql_file") {
        return Promise.resolve([
          {
            statement: "SELECT 1;",
            result: {
              columns: [],
              column_types: [],
              column_sortables: [],
              rows: [],
              affected_rows: 1,
              execution_time_ms: 3,
              truncated: false,
              has_more: false,
            },
          },
        ]);
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
    const publicSchema = await screen.findByRole("button", { name: /^public$/i });
    fireEvent.contextMenu(publicSchema);

    expect(screen.getByRole("menuitem", { name: "Execute SQL file" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Data transfer" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Schema diff" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Data compare" })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Execute SQL file" }));
    expect(screen.getByText("Choose a SQL file to preview its contents.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Choose SQL file" }));
    expect(await screen.findByText("SELECT 1;")).toBeInTheDocument();
    await user.click(
      within(screen.getByRole("main")).getByRole("button", { name: "Execute SQL file" }),
    );

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_execute_sql_file", {
        request: expect.objectContaining({
          connectionId: "dbx-source",
          database: "main",
          schema: "public",
          path: "/tmp/schema.sql",
        }),
      });
    });
  });

  it("groups DBX schema objects by type and keeps matching groups visible during search", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "users", object_type: "TABLE", schema: "public" },
          { name: "accounts", object_type: "TABLE", schema: "public" },
          { name: "users", object_type: "TABLE", schema: "public" },
          { name: "active_users", object_type: "VIEW", schema: "public" },
          { name: "refresh_stats", object_type: "PROCEDURE", schema: "public" },
          { name: "total_users", object_type: "FUNCTION", schema: "public" },
          { name: "users_id_seq", object_type: "SEQUENCE", schema: "public" },
          { name: "payroll", object_type: "PACKAGE", schema: "public" },
        ]);
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

    const publicSchema = await screen.findByRole("button", { name: /^public$/i });
    expect(publicSchema).toBeInTheDocument();
    const publicSchemaBranch = publicSchema.parentElement!;
    expect(within(publicSchemaBranch).getByRole("button", { name: "Tables" })).toBeInTheDocument();
    expect(within(publicSchemaBranch).getByRole("button", { name: "Views" })).toBeInTheDocument();
    expect(
      within(publicSchemaBranch).getByRole("button", { name: "Procedures" }),
    ).toBeInTheDocument();
    expect(
      within(publicSchemaBranch).getByRole("button", { name: "Functions" }),
    ).toBeInTheDocument();
    expect(
      within(publicSchemaBranch).getByRole("button", { name: "Sequences" }),
    ).toBeInTheDocument();
    expect(
      within(publicSchemaBranch).getByRole("button", { name: "Packages" }),
    ).toBeInTheDocument();
    const accountsRow = within(publicSchemaBranch).getByText("accounts");
    const usersRow = within(publicSchemaBranch).getByText("users");
    expect(accountsRow.closest("button")).toBeInTheDocument();
    expect(usersRow.closest("button")).toBeInTheDocument();
    expect(within(publicSchemaBranch).getAllByText("users")).toHaveLength(1);
    expect(
      accountsRow.compareDocumentPosition(usersRow) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      within(publicSchemaBranch).getByRole("button", { name: /^refresh_stats\s+PROCEDURE$/i }),
    ).toBeInTheDocument();

    await user.type(screen.getByLabelText("Sidebar search"), "refresh");

    await waitFor(() => {
      expect(screen.queryByText("users")).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: /^public$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Procedures" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^refresh_stats\s+PROCEDURE$/i }),
    ).toBeInTheDocument();
  });

  it("adds hover motion class to database tree nodes and auto-collapses the previous table rows", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "accounts", object_type: "TABLE", schema: "public" },
          { name: "users", object_type: "TABLE", schema: "public" },
        ]);
      }
      if (command === "dbx_get_columns") {
        const table = (args as { table?: string } | undefined)?.table;
        return Promise.resolve([
          { name: `${table ?? "table"}_id`, data_type: "integer", is_primary_key: true },
        ]);
      }
      if (command === "dbx_query_table_data") {
        return Promise.resolve({
          result: {
            columns: ["id"],
            column_types: ["integer"],
            column_sortables: [true],
            rows: [],
            affected_rows: 0,
            execution_time_ms: 1,
            truncated: false,
            has_more: false,
          },
          totalRows: 0,
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

    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    const accountsTable = await screen.findByRole("button", { name: /^accounts\s+TABLE$/i });
    expect(screen.getByRole("button", { name: /^users\s+TABLE$/i })).toBeInTheDocument();

    expect(accountsTable).toHaveClass("database-tree-node");
    await user.click(accountsTable);
    expect(await screen.findByText("accounts_id")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^users\s+TABLE$/i }));
    expect(await screen.findByText("users_id")).toBeInTheDocument();
    expect(screen.queryByText("accounts_id")).not.toBeInTheDocument();
  });

  it("supports DBX tree keyboard shortcuts for copy, refresh, rename, and delete", async () => {
    const user = userEvent.setup();
    const promptSpy = vi.spyOn(window, "prompt").mockReturnValue("Renamed Source");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(confirm).mockResolvedValue(true);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_build_drop_database_sql")
        return Promise.resolve('DROP DATABASE "main";');
      if (command === "dbx_save_connection") return Promise.resolve(undefined);
      if (command === "dbx_execute_query") {
        return Promise.resolve({
          columns: [],
          column_types: [],
          column_sortables: [],
          rows: [],
          affected_rows: 1,
          execution_time_ms: 3,
          truncated: false,
          has_more: false,
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

    const connectionButton = await screen.findByRole("button", { name: /DBX Source/i });
    await user.click(connectionButton);
    const databaseButton = await screen.findByRole("button", { name: /^main$/i });
    const listObjectsCallsBefore = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === "dbx_list_objects").length;

    fireEvent.keyDown(databaseButton, { key: "c", metaKey: true });
    expect(writeText).toHaveBeenCalledWith("main");

    fireEvent.keyDown(databaseButton, { key: "F5" });
    await waitFor(() => {
      expect(
        vi.mocked(invoke).mock.calls.filter(([command]) => command === "dbx_list_objects").length,
      ).toBeGreaterThan(listObjectsCallsBefore);
    });

    fireEvent.keyDown(connectionButton, { key: "F2" });
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
        connection: expect.objectContaining({ id: "dbx-source", name: "Renamed Source" }),
      });
    });
    expect(promptSpy).toHaveBeenCalledWith("Rename connection", "DBX Source");

    fireEvent.keyDown(databaseButton, { key: "Delete" });
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_build_drop_database_sql", {
        options: { databaseType: "postgres", name: "main" },
      });
    });
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining('DROP DATABASE "main";'), {
      title: "Drop database",
      kind: "warning",
      okLabel: "Drop database",
      cancelLabel: "Cancel",
    });
    promptSpy.mockRestore();
  });

  it("sets a DBX database node as the default database", async () => {
    const user = userEvent.setup();
    let savedConnection: (typeof dbxConnection & { dbx?: unknown }) | null = null;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections")
        return Promise.resolve([savedConnection ?? dbxConnection]);
      if (command === "dbx_save_connection") {
        savedConnection = (args as { connection: typeof dbxConnection & { dbx?: unknown } })
          .connection;
        return Promise.resolve(undefined);
      }
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases")
        return Promise.resolve([{ name: "main" }, { name: "analytics" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
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
    const mainDatabase = await screen.findByRole("button", { name: /^main$/i });
    expect(await screen.findByRole("button", { name: /^analytics$/i })).toBeInTheDocument();

    fireEvent.contextMenu(mainDatabase);
    await user.click(screen.getByRole("menuitem", { name: "Set as default database" }));

    await waitFor(() => {
      expect(savedConnection).toEqual(
        expect.objectContaining({
          id: "dbx-source",
          dbx: expect.objectContaining({ database: "main" }),
        }),
      );
    });
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /^analytics$/i })).not.toBeInTheDocument();
    });
  });

  it("clears the DBX default database from a database node", async () => {
    const user = userEvent.setup();
    const defaultConnection = {
      ...dbxConnection,
      dbx: {
        database: "main",
        visible_databases: ["main", "analytics"],
      },
    };
    let savedConnection: (typeof defaultConnection & { dbx?: unknown }) | null = null;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections")
        return Promise.resolve([savedConnection ?? defaultConnection]);
      if (command === "dbx_save_connection") {
        savedConnection = (args as { connection: typeof defaultConnection & { dbx?: unknown } })
          .connection;
        return Promise.resolve(undefined);
      }
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases")
        return Promise.resolve([{ name: "main" }, { name: "analytics" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
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
    const mainDatabase = await screen.findByRole("button", { name: /^main$/i });
    expect(screen.queryByRole("button", { name: /^analytics$/i })).not.toBeInTheDocument();

    fireEvent.contextMenu(mainDatabase);
    await user.click(screen.getByRole("menuitem", { name: "Clear default database" }));

    await waitFor(() => {
      expect(savedConnection?.dbx).toEqual(
        expect.objectContaining({ visible_databases: ["main", "analytics"] }),
      );
      expect(savedConnection?.dbx).not.toHaveProperty("database");
    });
    expect(await screen.findByRole("button", { name: /^analytics$/i })).toBeInTheDocument();
  });

  it("limits DBX database and table browsing to the configured target database", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections")
        return Promise.resolve([dbxConnectionWithTargetDatabase]);
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
    expect(await screen.findByText("target_table")).toBeInTheDocument();
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
      if (command === "dbx_list_connections")
        return Promise.resolve([dbxConnectionWithTargetDatabase]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "other_db" }]);
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "other_table", object_type: "table", schema: "public" }]);
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

    expect(
      await screen.findByText(/Configured database "target_db" was not found/i),
    ).toBeInTheDocument();
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
      if (command === "dbx_list_objects")
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      if (command === "dbx_get_columns")
        return Promise.resolve([
          { name: "id", data_type: "int", is_nullable: false, is_primary_key: true },
        ]);
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
    expect(screen.queryByText("No table selected")).not.toBeInTheDocument();
    expect(screen.queryByText("Select a database connection")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Schema diff/i }));
    expect(screen.getAllByRole("button", { name: /Compare/i }).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /Data compare/i }));
    expect(screen.getAllByRole("button", { name: /Compare/i }).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /ER diagram/i }));
    expect(screen.getByText("Inspect table nodes and loaded column metadata.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Table structure/i }));
    expect((await screen.findAllByText("id")).length).toBeGreaterThanOrEqual(1);
  });

  it("orders DBX NoSQL database and collection node context menus like dbx", async () => {
    const user = userEvent.setup();
    const redisConnection = {
      id: "redis-source",
      name: "Redis Source",
      dbType: "redis",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    const mongoConnection = {
      id: "mongo-source",
      name: "Mongo Source",
      dbType: "mongodb",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections")
        return Promise.resolve([redisConnection, mongoConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 12 }]);
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    const sidebar = screen.getByRole("complementary");
    await user.click(await within(sidebar).findByRole("button", { name: /Redis Source/i }));
    const redisDatabase = await within(sidebar).findByRole("button", { name: /db012/i });
    fireEvent.contextMenu(redisDatabase);
    expect(menuItemLabels()).toEqual([
      "Pin",
      "New query",
      "Set as default database",
      "Clear current DB",
    ]);

    await user.click(screen.getByRole("menuitem", { name: "New query" }));
    expect(
      screen.getByPlaceholderText("Run SQL against the active database connection"),
    ).toBeInTheDocument();

    await user.click(await within(sidebar).findByRole("button", { name: /Mongo Source/i }));
    expect(within(sidebar).queryByRole("button", { name: /^users$/i })).not.toBeInTheDocument();
    const mongoDatabaseButtons = await within(sidebar).findAllByRole("button", { name: /^app$/i });
    const mongoDatabase =
      mongoDatabaseButtons.find((button) => button.getAttribute("aria-selected") === "true") ??
      mongoDatabaseButtons[0];
    fireEvent.contextMenu(mongoDatabase);
    expect(menuItemLabels()).toEqual(["Pin", "New query", "Set as default database"]);

    await user.click(screen.getByRole("menuitem", { name: "New query" }));
    expect(
      screen.getByPlaceholderText("Run SQL against the active database connection"),
    ).toBeInTheDocument();

    const refreshedMongoDatabaseButtons = await within(sidebar).findAllByRole("button", {
      name: /^app$/i,
    });
    const refreshedMongoDatabase =
      refreshedMongoDatabaseButtons.find(
        (button) => button.getAttribute("aria-selected") === "true",
      ) ?? refreshedMongoDatabaseButtons[0];
    await user.click(refreshedMongoDatabase);
    const mongoDatabaseExpandGlyph = refreshedMongoDatabase.querySelector(
      "span",
    ) as HTMLSpanElement;
    fireEvent.click(mongoDatabaseExpandGlyph);
    const collection = await within(sidebar).findByRole("button", { name: /^users$/i });
    fireEvent.contextMenu(collection);

    expect(menuItemLabels()).toEqual(["Pin"]);
  });

  it("pins and unpins NoSQL database and collection nodes from the sidebar context menu", async () => {
    const user = userEvent.setup();
    const redisConnection = {
      id: "redis-source",
      name: "Redis Source",
      dbType: "redis",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    const mongoConnection = {
      id: "mongo-source",
      name: "Mongo Source",
      dbType: "mongodb",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections")
        return Promise.resolve([redisConnection, mongoConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 12 }]);
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    const sidebar = screen.getByRole("complementary");
    await user.click(await within(sidebar).findByRole("button", { name: /Redis Source/i }));
    const redisDatabase = await within(sidebar).findByRole("button", { name: /db012/i });
    fireEvent.contextMenu(redisDatabase);
    await user.click(screen.getByRole("menuitem", { name: "Pin" }));

    expect(await within(redisDatabase).findByLabelText("Pinned")).toBeInTheDocument();

    fireEvent.contextMenu(redisDatabase);
    await user.click(screen.getByRole("menuitem", { name: "Unpin" }));
    await waitFor(() => {
      expect(within(redisDatabase).queryByLabelText("Pinned")).not.toBeInTheDocument();
    });

    await user.click(await within(sidebar).findByRole("button", { name: /Mongo Source/i }));
    const mongoDatabaseButtons = await within(sidebar).findAllByRole("button", { name: /^app$/i });
    const mongoDatabase =
      mongoDatabaseButtons.find((button) => button.getAttribute("aria-selected") === "true") ??
      mongoDatabaseButtons[0];
    await user.click(mongoDatabase);
    const mongoDatabaseExpandGlyph = mongoDatabase.querySelector("span") as HTMLSpanElement;
    fireEvent.click(mongoDatabaseExpandGlyph);

    const collection = await within(sidebar).findByRole("button", { name: /^users$/i });
    fireEvent.contextMenu(collection);
    await user.click(screen.getByRole("menuitem", { name: "Pin" }));

    expect(await within(collection).findByLabelText("Pinned")).toBeInTheDocument();
  });

  it("pins and unpins SQL database, schema, and table nodes from the sidebar context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") {
        return Promise.resolve([
          { name: "accounts", object_type: "TABLE", schema: "public" },
          { name: "users", object_type: "TABLE", schema: "public" },
        ]);
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

    const databaseButton = await screen.findByRole("button", { name: /^main$/i });
    fireEvent.contextMenu(databaseButton);
    await user.click(screen.getByRole("menuitem", { name: "Pin" }));
    expect(await within(databaseButton).findByLabelText("Pinned")).toBeInTheDocument();

    fireEvent.contextMenu(databaseButton);
    await user.click(screen.getByRole("menuitem", { name: "Unpin" }));
    await waitFor(() => {
      expect(within(databaseButton).queryByLabelText("Pinned")).not.toBeInTheDocument();
    });

    const schemaButton = await screen.findByRole("button", { name: /^public$/i });
    fireEvent.contextMenu(schemaButton);
    await user.click(screen.getByRole("menuitem", { name: "Pin" }));
    expect(await within(schemaButton).findByLabelText("Pinned")).toBeInTheDocument();

    const usersButton = await screen.findByRole("button", { name: /^users\s+table$/i });
    fireEvent.contextMenu(usersButton);
    await user.click(screen.getByRole("menuitem", { name: "Pin" }));
    expect(await within(usersButton).findByLabelText("Pinned")).toBeInTheDocument();
  });

  it("confirms and flushes a Redis database from the sidebar context menu", async () => {
    const user = userEvent.setup();
    vi.mocked(confirm).mockResolvedValue(true);
    const redisConnection = {
      id: "redis-source",
      name: "Redis Source",
      dbType: "redis",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([redisConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 2 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      if (command === "dbx_redis_execute_command")
        return Promise.resolve({ command: "FLUSHDB", safety: "confirm", value: "OK" });
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /Redis Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /db02/i }));
    await user.click(screen.getByRole("menuitem", { name: "Clear current DB" }));

    expect(confirm).toHaveBeenCalledWith(
      "This will delete every key in Redis db0 and cannot be undone. Continue?",
      {
        title: "Clear current DB",
        kind: "warning",
        okLabel: "Clear DB",
        cancelLabel: "Cancel",
      },
    );
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_execute_command", {
        connectionId: "redis-source",
        db: 0,
        command: "FLUSHDB",
        skipSafetyCheck: true,
      });
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis-source",
      db: 0,
      cursor: 0,
      pattern: "*",
      count: 100,
    });
  });

  it("sets and clears default databases from NoSQL database node context menus", async () => {
    const user = userEvent.setup();
    const redisConnection = {
      id: "redis-source",
      name: "Redis Source",
      dbType: "redis",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    const mongoConnection = {
      id: "mongo-source",
      name: "Mongo Source",
      dbType: "mongodb",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
      dbx: { database: "app" },
    };
    let dbxConnections = [redisConnection, mongoConnection];
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve(dbxConnections);
      if (command === "dbx_save_connection") {
        const saved = (args as { connection: typeof redisConnection }).connection;
        dbxConnections = dbxConnections.map((connection) =>
          connection.id === saved.id ? saved : connection,
        );
        return Promise.resolve(undefined);
      }
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 1 }]);
      if (command === "dbx_redis_scan_keys")
        return Promise.resolve({ cursor: 0, total_keys: 0, keys: [] });
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /Redis Source/i }));
    fireEvent.contextMenu(await screen.findByRole("button", { name: /db01/i }));
    await user.click(screen.getByRole("menuitem", { name: "Set as default database" }));
    expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
      connection: expect.objectContaining({
        id: "redis-source",
        dbx: { database: "0" },
      }),
    });

    await user.click(await screen.findByRole("button", { name: /Mongo Source/i }));
    const mongoDatabaseButtons = await screen.findAllByRole("button", { name: /app/i });
    const mongoDatabase =
      mongoDatabaseButtons.find((button) => button.getAttribute("aria-selected") === "true") ??
      mongoDatabaseButtons[0];
    fireEvent.contextMenu(mongoDatabase);
    await user.click(screen.getByRole("menuitem", { name: "Clear default database" }));
    expect(invoke).toHaveBeenCalledWith("dbx_save_connection", {
      connection: expect.objectContaining({
        id: "mongo-source",
        dbx: {},
      }),
    });
  });

  it("lazy-loads Redis keys from the sidebar and opens the selected key in the workspace", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(confirm).mockResolvedValue(true);
    const redisConnection = {
      id: "redis-source",
      name: "Redis Source",
      dbType: "redis",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([redisConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_redis_list_databases") return Promise.resolve([{ db: 0, keys: 2 }]);
      if (command === "dbx_redis_scan_keys") {
        const request = args as { cursor?: number };
        if (request.cursor === 7) {
          return Promise.resolve({
            cursor: 0,
            total_keys: 2,
            keys: [
              {
                key_display: "user:2",
                key_raw: "user:2",
                key_type: "string",
                ttl: -1,
                size: 5,
                value_preview: "Grace",
              },
            ],
          });
        }
        return Promise.resolve({
          cursor: 7,
          total_keys: 2,
          keys: [
            {
              key_display: "user:1",
              key_raw: "user:1",
              key_type: "string",
              ttl: -1,
              size: 3,
              value_preview: "Ada",
            },
          ],
        });
      }
      if (command === "dbx_redis_get_value") {
        return Promise.resolve({
          key_display: "user:1",
          key_raw: "user:1",
          key_type: "string",
          ttl: -1,
          value_is_binary: false,
          value: "Ada",
        });
      }
      if (command === "dbx_redis_delete_key") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /Redis Source/i }));
    const redisDatabase = await screen.findByRole("button", { name: /db02/i });
    const expandGlyph = redisDatabase.querySelector("span") as HTMLSpanElement;
    fireEvent.click(expandGlyph);
    const sidebar = screen.getByRole("complementary");
    await within(sidebar).findByRole("button", { name: /user:1 string/i });
    await user.click(await within(sidebar).findByRole("button", { name: /Load more \(1\/2\)/i }));
    expect(
      await within(sidebar).findByRole("button", { name: /user:2 string/i }),
    ).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis-source",
      db: 0,
      cursor: 7,
      pattern: "*",
      count: 100,
    });
    const redisKey = within(sidebar).getByRole("button", {
      name: /user:1 string/i,
    }) as HTMLButtonElement;
    await user.click(redisKey);

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_redis_get_value", {
        connectionId: "redis-source",
        db: 0,
        keyRaw: "user:1",
      });
    });
    expect(await screen.findByText("Ada")).toBeInTheDocument();

    fireEvent.contextMenu(redisKey);
    expect(screen.getByRole("menuitem", { name: "Open workspace" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Refresh" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Delete key" })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Copy name" }));
    expect(writeText).toHaveBeenCalledWith("user:1");

    fireEvent.contextMenu(redisKey);
    await user.click(screen.getByRole("menuitem", { name: "Refresh" }));
    expect(invoke).toHaveBeenCalledWith("dbx_redis_scan_keys", {
      connectionId: "redis-source",
      db: 0,
      cursor: 0,
      pattern: "*",
      count: 100,
    });

    fireEvent.contextMenu(redisKey);
    await user.click(screen.getByRole("menuitem", { name: "Delete key" }));
    expect(confirm).toHaveBeenCalledWith('Delete Redis key "user:1"?', {
      title: "Delete key",
      kind: "warning",
      okLabel: "Delete key",
      cancelLabel: "Cancel",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_redis_delete_key", {
      connectionId: "redis-source",
      db: 0,
      keyRaw: "user:1",
    });
  });

  it("lazy-loads MongoDB document previews from the sidebar and opens the selected document", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    vi.mocked(confirm).mockResolvedValue(true);
    const mongoConnection = {
      id: "mongo-source",
      name: "Mongo Source",
      dbType: "mongodb",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mongoConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { skip?: number; limit?: number };
        if (request.limit === 20 && request.skip === 1) {
          return Promise.resolve({ documents: [{ _id: "2", name: "Grace" }], total: 2 });
        }
        if (request.limit === 100) {
          return Promise.resolve({
            documents: [
              { _id: "1", name: "Ada" },
              { _id: "2", name: "Grace" },
            ],
            total: 2,
          });
        }
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 2 });
      }
      if (command === "dbx_mongo_delete_documents") return Promise.resolve(1);
      return Promise.resolve(undefined);
    });

    render(
      React.createElement(
        I18nProvider,
        null,
        React.createElement(DatabaseView, { sshConnections: [connection()] }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: /Mongo Source/i }));
    const sidebar = screen.getByRole("complementary");
    const databaseButtons = await within(sidebar).findAllByRole("button", { name: /^app$/i });
    const databaseButton =
      databaseButtons.find((button) => button.getAttribute("aria-selected") === "true") ??
      databaseButtons[0];
    await user.click(databaseButton);
    const databaseExpandGlyph = databaseButton.querySelector("span") as HTMLSpanElement;
    fireEvent.click(databaseExpandGlyph);

    const collectionButton = await within(sidebar).findByRole("button", { name: /^users$/i });
    const expandGlyph = collectionButton.querySelector("span") as HTMLSpanElement;
    fireEvent.click(expandGlyph);
    await user.click(await screen.findByRole("button", { name: /Load more \(1\/2\)/i }));
    expect(await screen.findByText(/2 name: Grace/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 1,
      limit: 20,
    });

    const documentButton = (await screen.findByText(/1 name: Ada/)).closest(
      "button",
    ) as HTMLButtonElement;
    await user.click(documentButton);

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 0,
      limit: 20,
    });
    await waitFor(() => {
      expect(screen.getByLabelText(/Document JSON|文档 JSON/)).toHaveValue(
        JSON.stringify({ _id: "1", name: "Ada" }, null, 2),
      );
    });
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 0,
      limit: 100,
    });

    fireEvent.contextMenu(documentButton);
    expect(screen.getByRole("menuitem", { name: "Open workspace" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Refresh" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Delete document" })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: "Copy name" }));
    expect(writeText).toHaveBeenCalledWith("1");

    fireEvent.contextMenu(documentButton);
    await user.click(screen.getByRole("menuitem", { name: "Refresh" }));
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: "{}",
      sort: "{}",
      skip: 0,
      limit: 20,
    });

    fireEvent.contextMenu(documentButton);
    await user.click(screen.getByRole("menuitem", { name: "Delete document" }));
    expect(confirm).toHaveBeenCalledWith('Delete document "1" from "users"?', {
      title: "Delete document",
      kind: "warning",
      okLabel: "Delete document",
      cancelLabel: "Cancel",
    });
    expect(invoke).toHaveBeenCalledWith("dbx_mongo_delete_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filterJson: JSON.stringify({ _id: "1" }),
      many: false,
    });
  });

  it("refreshes MongoDB sidebar document previews with the workspace filter", async () => {
    const user = userEvent.setup();
    const mongoConnection = {
      id: "mongo-source",
      name: "Mongo Source",
      dbType: "mongodb",
      readOnly: false,
      createdAt: 1,
      lastOpenedAt: 1,
    };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([mongoConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_mongo_list_databases") return Promise.resolve(["app"]);
      if (command === "dbx_mongo_list_collections") return Promise.resolve(["users"]);
      if (command === "dbx_mongo_find_documents") {
        const request = args as { filter?: string; skip?: number; limit?: number };
        if (request.filter === '{"active":true}' && request.limit === 20 && request.skip === 1) {
          return Promise.resolve({
            documents: [{ _id: "2", name: "Grace", active: true }],
            total: 2,
          });
        }
        if (request.filter === '{"active":true}' && request.limit === 20) {
          return Promise.resolve({
            documents: [{ _id: "1", name: "Ada", active: true }],
            total: 2,
          });
        }
        if (request.limit === 100) {
          return Promise.resolve({
            documents: [{ _id: "1", name: "Ada", active: true }],
            total: 1,
          });
        }
        return Promise.resolve({ documents: [{ _id: "1", name: "Ada" }], total: 1 });
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

    await user.click(await screen.findByRole("button", { name: /Mongo Source/i }));
    const sidebar = screen.getByRole("complementary");
    const databaseButtons = await within(sidebar).findAllByRole("button", { name: /^app$/i });
    const databaseButton =
      databaseButtons.find((button) => button.getAttribute("aria-selected") === "true") ??
      databaseButtons[0];
    await user.click(databaseButton);
    const databaseExpandGlyph = databaseButton.querySelector("span") as HTMLSpanElement;
    fireEvent.click(databaseExpandGlyph);
    await user.click(await within(sidebar).findByRole("button", { name: /^users$/i }));

    const filterInput = await screen.findByLabelText(/Filter JSON|过滤 JSON/);
    fireEvent.change(filterInput, { target: { value: '{"active":true}' } });
    fireEvent.keyDown(filterInput, { key: "Enter", code: "Enter" });

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
        connectionId: "mongo-source",
        database: "app",
        collection: "users",
        filter: '{"active":true}',
        sort: "{}",
        skip: 0,
        limit: 20,
      });
    });

    await user.click(await screen.findByRole("button", { name: /Load more \(1\/2\)/i }));

    expect(invoke).toHaveBeenCalledWith("dbx_mongo_find_documents", {
      connectionId: "mongo-source",
      database: "app",
      collection: "users",
      filter: '{"active":true}',
      sort: "{}",
      skip: 1,
      limit: 20,
    });
    expect(await screen.findByText(/2 name: Grace/)).toBeInTheDocument();
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
    expect(
      screen.getAllByText("Select a SQL-capable DBX connection to use this tool.").length,
    ).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /Table structure/i }));
    expect(
      screen.getAllByText("Select a DBX SQL table to inspect or edit its structure.").length,
    ).toBeGreaterThan(0);
  });
});
