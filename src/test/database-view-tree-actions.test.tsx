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
  createMockDataTransfer,
  dbxConnection,
  dbxConnectionWithTargetDatabase,
  duckDbxConnection,
  menuItemLabels,
  resetDatabaseViewMocks,
} from "./databaseViewTestUtils";

describe("DatabaseView tree actions", () => {
  beforeEach(resetDatabaseViewMocks);
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
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve([]);
      if (command === "dbx_list_objects") {
        const request = args as { objectTypes?: string[] | null; offset?: number | null };
        if (request.objectTypes) {
          if (request.offset === 200) {
            return Promise.resolve([{ name: "orders", object_type: "table", schema: "public" }]);
          }
          return Promise.resolve([
            { name: "users", object_type: "table", schema: "public" },
            ...Array.from({ length: 199 }, (_, index) => ({
              name: `procedure_${String(index + 1).padStart(3, "0")}`,
              object_type: "PROCEDURE",
              schema: "public",
            })),
            { name: "orders", object_type: "table", schema: "public" },
          ]);
        }
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
    expect(invoke).toHaveBeenCalledWith("dbx_list_objects", {
      connectionId: "dbx-source",
      database: "main",
      schema: null,
      filter: null,
      limit: 201,
      offset: 0,
      objectTypes: ["TABLE", "VIEW", "MATERIALIZED_VIEW"],
    });
    expect(invoke).toHaveBeenCalledWith("dbx_list_objects", {
      connectionId: "dbx-source",
      database: "main",
      schema: null,
      filter: null,
      limit: 201,
      offset: 200,
      objectTypes: ["TABLE", "VIEW", "MATERIALIZED_VIEW"],
    });
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

  it("does not execute a production SQL file when the confirmation is rejected", async () => {
    const user = userEvent.setup();
    const productionConnection = {
      ...dbxConnection,
      dbx: {
        db_type: "postgres",
        is_production: true,
        production_databases: [],
      },
    };
    vi.mocked(open).mockResolvedValue("/tmp/production.sql");
    vi.mocked(confirm).mockResolvedValue(false);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([productionConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve([]);
      if (command === "dbx_list_objects") return Promise.resolve([]);
      if (command === "db_read_sql_file") {
        return Promise.resolve("DELETE FROM users WHERE id = 1;");
      }
      if (command === "dbx_assess_production_sql") {
        return Promise.resolve({
          requiresConfirmation: true,
          isMutation: true,
          productionDatabases: [],
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
    await user.click(screen.getByRole("button", { name: "Execute SQL file" }));
    await user.click(screen.getByRole("button", { name: "Choose SQL file" }));
    await user.click(
      within(screen.getByRole("main")).getByRole("button", { name: "Execute SQL file" }),
    );

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("dbx_assess_production_sql", {
        request: {
          connectionId: "dbx-source",
          database: "main",
          sql: "DELETE FROM users WHERE id = 1;",
        },
      });
    });
    expect(invoke).not.toHaveBeenCalledWith("dbx_execute_sql_file", expect.anything());
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
      filter: null,
      limit: 201,
      offset: 0,
      objectTypes: null,
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
});
