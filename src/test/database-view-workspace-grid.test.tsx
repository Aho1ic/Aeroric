import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm, save } from "@tauri-apps/plugin-dialog";
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
  duckDbxConnection,
  mysqlDbxConnection,
  resetDatabaseViewMocks,
} from "./databaseViewTestUtils";

describe("DatabaseView workspace and data grid", () => {
  beforeEach(resetDatabaseViewMocks);
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
    const ddlPreview = await screen.findByTestId("database-ddl-highlight");
    expect(ddlPreview).toHaveTextContent("CREATE TABLE public.users (id integer);");
    expect(within(ddlPreview).getByText("CREATE")).toHaveAttribute("data-sql-token", "keyword");
    expect(within(ddlPreview).getByText("TABLE")).toHaveAttribute("data-sql-token", "keyword");
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
});
