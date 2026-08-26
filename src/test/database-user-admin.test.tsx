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

  /**
   * 面板里五个写操作(建用户 / 改密码 / 改登录 / 删用户 / 改权限)共用
   * `executeConfirmed` 这一个出口,守卫就是里面那句 `if (!accepted) return`。
   * 用最破坏性的 DROP USER 当载体:这里变红就说明五条路一起漏了。
   */
  it("drops nothing when the user declines the confirmation", async () => {
    vi.mocked(confirm).mockResolvedValue(false);
    const user = userEvent.setup();
    renderPanel([["app", "%", "mysql_native_password"]]);
    await screen.findByText("app@%");

    await user.click(screen.getByRole("button", { name: "Drop user" }));

    await waitFor(() => expect(confirm).toHaveBeenCalled());
    // 只读路径(列用户 / SHOW GRANTS)仍会走 dbx_execute_query,所以只断言写出口。
    expect(invoke).not.toHaveBeenCalledWith("dbx_execute_multi", expect.anything());
  });

  it("shows the real DROP statement before dropping a user", async () => {
    const user = userEvent.setup();
    renderPanel([["app", "%", "mysql_native_password"]]);
    await screen.findByText("app@%");

    await user.click(screen.getByRole("button", { name: "Drop user" }));

    await waitFor(() => expect(confirm).toHaveBeenCalled());
    // 弹窗必须写清删的是谁,否则多账号下极易误删。
    expect(String(vi.mocked(confirm).mock.calls[0]?.[0])).toContain("DROP USER 'app'@'%'");
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
