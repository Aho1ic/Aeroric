import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { DatabaseView } from "../components/database/DatabaseView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
}));

vi.mock("../lib/appDialog", () => ({
  confirm: vi.fn(),
  prompt: vi.fn(),
}));

import { connection, dbxConnection, resetDatabaseViewMocks } from "./databaseViewTestUtils";

const USERS = { name: "users", object_type: "table", schema: "public" };
const TEAMS = { name: "teams", object_type: "table", schema: "public" };

function tableRows(marker: string) {
  return {
    result: {
      columns: ["marker"],
      column_types: ["text"],
      column_sortables: [true],
      rows: [[marker]],
    },
    totalRows: 1,
    sql: 'SELECT * FROM "public"."users"',
  };
}

function columnsFor(name: string) {
  return [{ name, data_type: "text", nullable: false, is_primary_key: true }];
}

/** 侧边栏那颗刷新按钮是纯图标按钮(可及名来自 title),面板上那颗带可见文字。 */
function sidebarRefresh(container: HTMLElement) {
  const aside = container.querySelector("aside");
  if (!aside) throw new Error("sidebar not rendered");
  return within(aside).getByRole("button", { name: "Refresh" });
}

function panelRefresh() {
  const button = screen
    .getAllByRole("button", { name: "Refresh" })
    .find((candidate) => candidate.textContent === "Refresh");
  if (!button) throw new Error("table info refresh button not rendered");
  return button;
}

function callsFor(
  command: string,
  predicate: (args: Record<string, unknown>) => boolean = () => true,
) {
  return vi
    .mocked(invoke)
    .mock.calls.filter(
      ([name, args]) => name === command && predicate((args ?? {}) as Record<string, unknown>),
    ).length;
}

function renderDatabaseView() {
  return render(
    React.createElement(
      I18nProvider,
      null,
      React.createElement(DatabaseView, { sshConnections: [connection()] }),
    ),
  );
}

describe("DatabaseView refresh", () => {
  beforeEach(resetDatabaseViewMocks);

  it("keeps the open table open and re-queries it when the sidebar refresh is clicked", async () => {
    const user = userEvent.setup();
    const objects = [USERS, TEAMS];
    let marker = "row-v1";
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") return Promise.resolve(objects);
      if (command === "dbx_query_table_data") return Promise.resolve(tableRows(marker));
      if (command === "dbx_get_columns") return Promise.resolve(columnsFor("marker"));
      return Promise.resolve(undefined);
    });

    const { container } = renderDatabaseView();
    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    expect(await screen.findByText("row-v1")).toBeInTheDocument();

    marker = "row-v2";
    await user.click(sidebarRefresh(container));

    // 刷新后仍停在原来那张表上,并且真的重新查了一次它的数据。
    expect(await screen.findByText("row-v2")).toBeInTheDocument();
    expect(
      callsFor(
        "dbx_query_table_data",
        (args) => (args.request as { table?: string } | undefined)?.table === "users",
      ),
    ).toBeGreaterThanOrEqual(2);
    // 顺手确认它走的是「重连」那条路,不是只重查一次数据。
    expect(callsFor("dbx_connect")).toBeGreaterThanOrEqual(2);
  });

  it("falls back to the empty state without an error when the open table is gone", async () => {
    const user = userEvent.setup();
    let objects: (typeof USERS)[] = [USERS, TEAMS];
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") return Promise.resolve(objects);
      if (command === "dbx_query_table_data") return Promise.resolve(tableRows("row-v1"));
      if (command === "dbx_get_columns") return Promise.resolve(columnsFor("marker"));
      return Promise.resolve(undefined);
    });

    const { container } = renderDatabaseView();
    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    expect(await screen.findByText("row-v1")).toBeInTheDocument();

    // 表在服务端被删掉了。
    objects = [TEAMS];
    const queriesBefore = callsFor(
      "dbx_query_table_data",
      (args) => (args.request as { table?: string } | undefined)?.table === "users",
    );
    await user.click(sidebarRefresh(container));

    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /^users\s+table$/i })).not.toBeInTheDocument(),
    );
    expect(
      callsFor(
        "dbx_query_table_data",
        (args) => (args.request as { table?: string } | undefined)?.table === "users",
      ),
    ).toBe(queriesBefore);
    expect(screen.queryByText("row-v1")).not.toBeInTheDocument();
  });

  it("stays on the table properties panel across a sidebar refresh", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") return Promise.resolve([USERS, TEAMS]);
      if (command === "dbx_query_table_data") return Promise.resolve(tableRows("row-v1"));
      if (command === "dbx_get_columns") return Promise.resolve(columnsFor("marker"));
      if (command === "dbx_get_table_ddl") return Promise.resolve("CREATE TABLE users (...)");
      return Promise.resolve(undefined);
    });

    const { container } = renderDatabaseView();
    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    expect(await screen.findByText("row-v1")).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: /Table properties/i }));
    expect(await screen.findByLabelText("Search table info")).toBeInTheDocument();

    await user.click(sidebarRefresh(container));

    // 刷新不该把用户从属性页踢回数据页。
    expect(await screen.findByLabelText("Search table info")).toBeInTheDocument();
  });

  it("re-requests columns and sibling objects from the table properties refresh", async () => {
    const user = userEvent.setup();
    let columnName = "marker";
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections") return Promise.resolve([dbxConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") return Promise.resolve([USERS, TEAMS]);
      if (command === "dbx_query_table_data") return Promise.resolve(tableRows("row-v1"));
      if (command === "dbx_get_columns") return Promise.resolve(columnsFor(columnName));
      if (command === "dbx_get_table_ddl") return Promise.resolve("CREATE TABLE users (...)");
      return Promise.resolve(undefined);
    });

    renderDatabaseView();
    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    expect(await screen.findByText("row-v1")).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: /Table properties/i }));
    expect(await screen.findByLabelText("Search table info")).toBeInTheDocument();

    const columnCallsBefore = callsFor("dbx_get_columns");
    const objectCallsBefore = callsFor("dbx_list_objects");
    // 服务端加了一列。
    columnName = "marker_renamed";
    await user.click(panelRefresh());

    // 列元数据换成了服务端的最新一份 —— 说明刷新按钮不是只重画,而是真的重拉了。
    expect((await screen.findAllByText("marker_renamed")).length).toBeGreaterThan(0);
    expect(callsFor("dbx_get_columns")).toBeGreaterThan(columnCallsBefore);
    // 索引 / 外键 / 触发器都在对象列表里,所以它也要重新列一次。
    expect(callsFor("dbx_list_objects")).toBeGreaterThan(objectCallsBefore);
  });

  it("drops cached column metadata when switching to another connection", async () => {
    const user = userEvent.setup();
    // 两条连接里都有 public.users —— `dbxObjectKey` 的键不含连接,正好会撞。
    const otherConnection = { ...dbxConnection, id: "dbx-other", name: "DBX Other" };
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "db_load_connections") return Promise.resolve([]);
      if (command === "dbx_list_connections")
        return Promise.resolve([dbxConnection, otherConnection]);
      if (command === "dbx_connect") return Promise.resolve(undefined);
      if (command === "dbx_list_databases") return Promise.resolve([{ name: "main" }]);
      if (command === "dbx_list_schemas") return Promise.resolve(["public"]);
      if (command === "dbx_list_objects") return Promise.resolve([USERS]);
      if (command === "dbx_query_table_data") return Promise.resolve(tableRows("row-v1"));
      if (command === "dbx_get_columns") {
        const id = (args as { connectionId?: string } | undefined)?.connectionId;
        return Promise.resolve(
          columnsFor(id === "dbx-other" ? "other_only_col" : "source_only_col"),
        );
      }
      return Promise.resolve(undefined);
    });

    renderDatabaseView();
    await user.click(await screen.findByRole("button", { name: /DBX Source/i }));
    await user.click(await screen.findByRole("button", { name: /^users\s+table$/i }));
    // 打开表会把第一条连接的列写进缓存。
    await waitFor(() =>
      expect(
        callsFor("dbx_get_columns", (args) => args.connectionId === "dbx-source"),
      ).toBeGreaterThan(0),
    );

    // 换到第二条连接,不打开任何表 —— 缓存里那份列元数据已经不属于当前连接了。
    await user.click(screen.getByRole("button", { name: /DBX Other/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "dbx_connect",
        expect.objectContaining({ connectionId: "dbx-other" }),
      ),
    );

    // 侧边栏按列名搜索读的就是那份缓存:搜第一条连接独有的列名,不该在这里搜出表来。
    await user.type(screen.getByLabelText("Sidebar search"), "source_only_col");
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /^users\s+table$/i })).not.toBeInTheDocument(),
    );
  });
});
