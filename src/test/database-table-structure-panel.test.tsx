import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { TableStructurePanel } from "../components/database/TableStructurePanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("TableStructurePanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("previews DBX table structure SQL for added columns", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      statements: ['ALTER TABLE "public"."users" ADD COLUMN "age" int;'],
      warnings: [],
    });

    render(
      <I18nProvider>
        <TableStructurePanel
          connectionId="pg"
          database="main"
          schema="public"
          databaseType="postgres"
          tableName="users"
          readOnly={false}
          columns={[
            {
              name: "id",
              data_type: "int",
              is_nullable: false,
              is_primary_key: true,
              column_default: null,
            },
          ]}
        />
      </I18nProvider>,
    );

    await userEvent.click(screen.getByRole("button", { name: /添加字段|Add column/ }));
    await userEvent.type(screen.getByLabelText(/新字段名|New column name/), "age");
    await userEvent.type(screen.getByLabelText(/新字段类型|New column type/), "int");
    await userEvent.click(screen.getByRole("button", { name: /预览 SQL|Preview SQL/ }));

    expect(invoke).toHaveBeenCalledWith("dbx_build_table_structure_change_sql", {
      options: expect.objectContaining({
        databaseType: "postgres",
        schema: "public",
        tableName: "users",
        columns: expect.arrayContaining([
          expect.objectContaining({ name: "age", dataType: "int", original: null }),
        ]),
      }),
    });
    expect(await screen.findByText(/ADD COLUMN/)).toBeInTheDocument();
  });
});
