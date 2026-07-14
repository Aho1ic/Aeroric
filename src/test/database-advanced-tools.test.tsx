import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { I18nProvider } from "../i18n";
import { DatabaseAdvancedTools } from "../components/database/DatabaseAdvancedTools";
import { ErDiagramPanel } from "../components/database/ErDiagramPanel";
import { TableStructurePanel } from "../components/database/TableStructurePanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: vi.fn() }));

describe("database advanced tools", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
  });

  it("prepares schema diff through DBX API", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ diffs: [] });

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
          sourceColumnsByTable={{
            "public.users": [
              { name: "id", data_type: "int", is_nullable: false, is_primary_key: true },
            ],
          }}
          database="main"
          schema="public"
          table="users"
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /Compare/i }));

    expect(invoke).toHaveBeenCalledWith("dbx_prepare_schema_diff", {
      options: expect.objectContaining({
        sourceTables: [expect.objectContaining({ name: "users", table_type: "TABLE" })],
        targetTables: [expect.objectContaining({ name: "users", table_type: "TABLE" })],
        sourceDetails: [
          expect.objectContaining({
            name: "users",
            columns: [
              expect.objectContaining({ name: "id", data_type: "int", is_primary_key: true }),
            ],
          }),
        ],
        databaseType: "mysql",
      }),
    });
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
    vi.mocked(invoke).mockResolvedValueOnce({ result: { added: [], removed: [], modified: [] } });

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
      }),
    });
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
