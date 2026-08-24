import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "../lib/appDialog";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DatabaseUserAdminPanel } from "../components/database/DatabaseUserAdminPanel";
import { I18nProvider } from "../i18n";
import type { AeroricDbConnectionConfig } from "../types";
import { mysqlDbxConnection } from "./databaseViewTestUtils";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("../lib/appDialog", () => ({
  confirm: vi.fn(),
}));

const emptyResult = {
  columns: [],
  column_types: [],
  column_sortables: [],
  rows: [],
  affected_rows: 0,
  execution_time_ms: 1,
  truncated: false,
  has_more: false,
};

function renderPanel(users: unknown[][] = []) {
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "dbx_execute_query") {
      const sql = String((args as { request?: { sql?: string } })?.request?.sql ?? "");
      if (sql.includes("mysql.user")) {
        return Promise.resolve({
          ...emptyResult,
          columns: ["user", "host", "plugin"],
          rows: users,
        });
      }
      if (sql.includes("SHOW GRANTS")) return Promise.resolve(emptyResult);
    }
    if (command === "dbx_execute_multi") return Promise.resolve([]);
    return Promise.resolve(undefined);
  });

  render(
    <I18nProvider>
      <DatabaseUserAdminPanel
        connection={mysqlDbxConnection as AeroricDbConnectionConfig}
        database="app"
      />
    </I18nProvider>,
  );
}

describe("DatabaseUserAdminPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(confirm).mockReset();
    vi.mocked(confirm).mockResolvedValue(true);
  });

  it("redacts a new user's password from confirmation while executing the real SQL", async () => {
    const user = userEvent.setup();
    renderPanel();
    await screen.findByText(/No users found/);

    await user.type(screen.getByLabelText("User name"), "reporter");
    await user.type(screen.getByLabelText("Password"), "secret-create");
    await user.click(screen.getByRole("button", { name: "Create user" }));

    await waitFor(() => expect(confirm).toHaveBeenCalled());
    const confirmation = String(vi.mocked(confirm).mock.calls[0]?.[0]);
    expect(confirmation).toContain("********");
    expect(confirmation).not.toContain("secret-create");
    expect(invoke).toHaveBeenCalledWith("dbx_execute_multi", {
      request: expect.objectContaining({
        sql: "CREATE USER 'reporter'@'%' IDENTIFIED BY 'secret-create';",
      }),
    });
  });

  it("redacts an altered password from confirmation while executing the real SQL", async () => {
    const user = userEvent.setup();
    renderPanel([["app", "%", "mysql_native_password"]]);
    await screen.findByText("app@%");

    await user.type(screen.getByLabelText("Password"), "secret-alter");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    await waitFor(() => expect(confirm).toHaveBeenCalled());
    const confirmation = String(vi.mocked(confirm).mock.calls[0]?.[0]);
    expect(confirmation).toContain("********");
    expect(confirmation).not.toContain("secret-alter");
    expect(invoke).toHaveBeenCalledWith("dbx_execute_multi", {
      request: expect.objectContaining({
        sql: "ALTER USER 'app'@'%' IDENTIFIED BY 'secret-alter';",
      }),
    });
  });
});
