import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "../lib/appDialog";
import { I18nProvider } from "../i18n";
import { DatabaseAdvancedTools } from "../components/database/DatabaseAdvancedTools";
import { ErDiagramPanel } from "../components/database/ErDiagramPanel";
import { TableStructurePanel } from "../components/database/TableStructurePanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../lib/appDialog", () => ({ confirm: vi.fn() }));

describe("database advanced tools", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
    vi.mocked(listen).mockReset();
    vi.mocked(listen).mockResolvedValue(() => {});
  });

  it("prepares schema diff through DBX API", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_list_objects") {
        return Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      }
      if (command === "dbx_get_columns") {
        const connectionId = (args as { connectionId: string }).connectionId;
        return Promise.resolve(
          connectionId === "source"
            ? [{ name: "id", data_type: "int", is_nullable: false, is_primary_key: true }]
            : [{ name: "email", data_type: "text", is_nullable: false, is_primary_key: false }],
        );
      }
      if (command === "dbx_prepare_schema_diff") return Promise.resolve({ diffs: [] });
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="schema-diff"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            { id: "target", name: "Target", dbType: "postgres", readOnly: false, createdAt: 2 },
          ]}
          sourceObjects={[{ name: "users", object_type: "table", schema: "public" }]}
          database="main"
          schema="public"
          table="users"
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /Compare/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("dbx_prepare_schema_diff", expect.anything()),
    );
    const prepareCall = vi
      .mocked(invoke)
      .mock.calls.find(([command]) => command === "dbx_prepare_schema_diff");
    expect(prepareCall?.[1]).toEqual({
      options: expect.objectContaining({
        sourceTables: [expect.objectContaining({ name: "users", table_type: "TABLE" })],
        targetTables: [expect.objectContaining({ name: "users", table_type: "TABLE" })],
        sourceDetails: [
          expect.objectContaining({
            name: "users",
            columns: [expect.objectContaining({ name: "id", data_type: "int" })],
          }),
        ],
        targetDetails: [
          expect.objectContaining({
            name: "users",
            columns: [expect.objectContaining({ name: "email", data_type: "text" })],
          }),
        ],
        databaseType: "postgres",
        targetSchema: "public",
      }),
    });
    expect(invoke).toHaveBeenCalledWith(
      "dbx_list_objects",
      expect.objectContaining({ connectionId: "source", database: "main", schema: "public" }),
    );
    expect(invoke).toHaveBeenCalledWith(
      "dbx_list_objects",
      expect.objectContaining({ connectionId: "target", database: "main", schema: "public" }),
    );
  });

  it("stops schema diff when target metadata fails", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_list_objects") {
        const connectionId = (args as { connectionId: string }).connectionId;
        return connectionId === "target"
          ? Promise.reject(new Error("target metadata failed"))
          : Promise.resolve([{ name: "users", object_type: "table", schema: "public" }]);
      }
      if (command === "dbx_get_columns") {
        return Promise.resolve([
          { name: "id", data_type: "int", is_nullable: false, is_primary_key: true },
        ]);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="schema-diff"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            { id: "target", name: "Target", dbType: "postgres", readOnly: false, createdAt: 2 },
          ]}
          database="main"
          schema="public"
          table="users"
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /Compare/i }));
    expect(await screen.findByText(/target metadata failed/)).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("dbx_prepare_schema_diff", expect.anything());
  });

  it("starts transfer through DBX API", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    let emitProgress: ((event: { payload: unknown }) => void) | undefined;
    vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
      emitProgress = handler as typeof emitProgress;
      return () => undefined;
    });

    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="transfer"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            { id: "target", name: "Target", dbType: "mysql", readOnly: false, createdAt: 2 },
          ]}
          table="users"
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /Start transfer/i }));

    expect(invoke).toHaveBeenCalledWith("dbx_start_transfer", {
      request: expect.objectContaining({
        sourceConnectionId: "source",
        targetConnectionId: "target",
        tables: ["users"],
      }),
    });
    const transferRequest = vi
      .mocked(invoke)
      .mock.calls.find(([command]) => command === "dbx_start_transfer")?.[1] as
      | { request?: { transferId?: string } }
      | undefined;

    emitProgress?.({
      payload: {
        transferId: transferRequest?.request?.transferId,
        table: "",
        tableIndex: 1,
        totalTables: 1,
        rowsTransferred: 0,
        totalRows: null,
        status: "done",
        error: null,
        terminal: true,
      },
    });
    expect(await screen.findByText("Transfer completed.")).toBeInTheDocument();
  });

  it("does not start a transfer when production confirmation is cancelled", async () => {
    vi.mocked(confirm).mockResolvedValue(false);
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_assess_production_target") {
        return Promise.resolve({
          requiresConfirmation: true,
          productionDatabases: ["prod_app"],
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="transfer"
          database="prod_app"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            {
              id: "target",
              name: "Production Target",
              dbType: "mysql",
              readOnly: false,
              createdAt: 2,
              dbx: { is_production: true },
            },
          ]}
          table="users"
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /Start transfer/i }));

    expect(invoke).toHaveBeenCalledWith("dbx_assess_production_target", {
      request: { connectionId: "target", database: "prod_app" },
    });
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining('Transfer 1 table(s) into database "prod_app".'),
      expect.objectContaining({
        title: "Confirm production operation",
        okLabel: "Start transfer",
      }),
    );
    expect(invoke).not.toHaveBeenCalledWith("dbx_start_transfer", expect.anything());
  });

  it("prepares data compare from selected tables", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_get_columns") {
        const connectionId = (args as { connectionId: string }).connectionId;
        return Promise.resolve(
          connectionId === "source"
            ? [
                { name: "tenant_id", data_type: "int", is_nullable: false, is_primary_key: true },
                { name: "id", data_type: "int", is_nullable: false, is_primary_key: true },
                { name: "name", data_type: "text", is_nullable: true, is_primary_key: false },
              ]
            : [
                { name: "id", data_type: "int", is_nullable: false, is_primary_key: true },
                { name: "tenant_id", data_type: "int", is_nullable: false, is_primary_key: true },
                { name: "name", data_type: "text", is_nullable: true, is_primary_key: false },
              ],
        );
      }
      if (command === "dbx_prepare_data_compare_from_tables") {
        return Promise.resolve({ result: { added: [], removed: [], modified: [] } });
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="data-compare"
          database="main"
          schema="public"
          table="users"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            { id: "target", name: "Target", dbType: "postgres", readOnly: false, createdAt: 2 },
          ]}
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /Compare/i }));

    expect(invoke).toHaveBeenCalledWith("dbx_prepare_data_compare_from_tables", {
      options: expect.objectContaining({
        sourceConnectionId: "source",
        targetConnectionId: "target",
        sourceTable: "users",
        targetTable: "users",
        columns: ["tenant_id", "id", "name"],
        keyColumns: ["tenant_id", "id"],
      }),
    });
  });

  it("does not start data compare without an identical primary-key set", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "dbx_get_columns") {
        const connectionId = (args as { connectionId: string }).connectionId;
        return Promise.resolve([
          {
            name: "id",
            data_type: "int",
            is_nullable: false,
            is_primary_key: connectionId === "source",
          },
        ]);
      }
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="data-compare"
          database="main"
          schema="public"
          table="users"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            { id: "target", name: "Target", dbType: "postgres", readOnly: false, createdAt: 2 },
          ]}
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /Compare/i }));
    expect(await screen.findByText(/no matching primary-key column/i)).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith(
      "dbx_prepare_data_compare_from_tables",
      expect.anything(),
    );
  });

  it("blocks a transfer whose source and target are the same object", async () => {
    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="transfer"
          database="main"
          schema="public"
          table="users"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            { id: "target", name: "Target", dbType: "postgres", readOnly: false, createdAt: 2 },
          ]}
        />
      </I18nProvider>,
    );

    await userEvent.selectOptions(screen.getByLabelText("Target connection"), "source");
    expect(
      await screen.findByText(/source and target resolve to the same table/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Start transfer/i })).toBeDisabled();
    expect(invoke).not.toHaveBeenCalledWith("dbx_start_transfer", expect.anything());
  });

  it("cancels a running transfer and waits for the terminal event", async () => {
    let emitProgress: ((event: { payload: unknown }) => void) | undefined;
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      emitProgress = handler as typeof emitProgress;
      return () => {};
    });
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "dbx_start_transfer") return Promise.resolve(undefined);
      if (command === "dbx_cancel_transfer") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });

    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="transfer"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            { id: "target", name: "Target", dbType: "mysql", readOnly: false, createdAt: 2 },
          ]}
          table="users"
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /Start transfer/i }));
    const cancelButton = await screen.findByRole("button", { name: /Cancel transfer/i });
    await userEvent.click(cancelButton);
    expect(invoke).toHaveBeenCalledWith("dbx_cancel_transfer", expect.anything());
    expect(screen.getByRole("button", { name: /Cancelling/i })).toBeDisabled();

    const transferRequest = vi
      .mocked(invoke)
      .mock.calls.find(([command]) => command === "dbx_start_transfer")?.[1] as
      | { request?: { transferId?: string } }
      | undefined;
    emitProgress?.({
      payload: {
        transferId: transferRequest?.request?.transferId,
        table: "",
        tableIndex: 1,
        totalTables: 1,
        rowsTransferred: 0,
        totalRows: null,
        status: "cancelled",
        error: null,
        terminal: true,
      },
    });
    expect(await screen.findByText("Transfer cancelled.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Start transfer/i })).toBeEnabled();
  });

  it("defaults target connection to another sql connection", () => {
    render(
      <I18nProvider>
        <DatabaseAdvancedTools
          connectionId="source"
          mode="transfer"
          availableConnections={[
            { id: "source", name: "Source", dbType: "postgres", readOnly: false, createdAt: 1 },
            { id: "target", name: "Target", dbType: "mysql", readOnly: false, createdAt: 2 },
          ]}
          table="users"
        />
      </I18nProvider>,
    );

    expect(screen.getByLabelText("Target connection")).toHaveValue("target");
  });

  it("renders ER diagram and table structure from metadata", () => {
    render(
      <I18nProvider>
        <ErDiagramPanel
          tables={[
            {
              name: "users",
              object_type: "table",
              schema: "public",
            },
          ]}
          columnsByTable={{
            users: [{ name: "id", data_type: "int", is_nullable: false, is_primary_key: true }],
          }}
        />
        <TableStructurePanel
          tableName="users"
          columns={[{ name: "id", data_type: "int", is_nullable: false, is_primary_key: true }]}
          readOnly={false}
        />
      </I18nProvider>,
    );

    expect(screen.getAllByText("users").length).toBeGreaterThan(0);
    expect(screen.getAllByText("id").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText(/int/).length).toBeGreaterThanOrEqual(1);
  });
});
