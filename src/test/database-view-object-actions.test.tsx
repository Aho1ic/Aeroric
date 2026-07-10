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

import { connection, dbxConnection, resetDatabaseViewMocks } from "./databaseViewTestUtils";

describe("DatabaseView object actions", () => {
  beforeEach(resetDatabaseViewMocks);
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
});
