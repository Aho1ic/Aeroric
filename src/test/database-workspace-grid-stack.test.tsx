/**
 * `DatabaseWorkspaceGridStack` 里三处只存在于回调内部的判断 —— 它们不会因为渲染
 * 而被执行,得把回调拿出来直接调,所以子组件全部换成「只记录 props」的替身。
 *
 * 覆盖的三条:
 * - `onImportData` 缺连接或缺对象时提前 return(按钮本身不禁用,判断在回调里);
 * - `onPromptPageSize` 的 `1..10000` 闭区间与取消/非法输入都返回 null;
 * - 过滤条要三个条件同时成立才挂载,页脚只看 queryResult。
 */

import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { I18nProvider } from "../i18n";
import { prompt } from "../lib/appDialog";
import { DatabaseWorkspaceGridStack } from "../components/database/DatabaseWorkspaceGridStack";
import type { DatabaseWorkspaceGridStackProps } from "../components/database/DatabaseWorkspaceGridStack";
import type { DbxDataGridController } from "../components/database/useDbxDataGrid";
import type { AeroricDbConnectionConfig, DbQueryResult, DbxObjectInfo } from "../types";

vi.mock("../lib/appDialog", () => ({ prompt: vi.fn(), confirm: vi.fn(), message: vi.fn() }));

/** 子组件替身:只把收到的 props 记下来,不渲染真实结构。 */
const captured = {
  toolbar: null as Record<string, unknown> | null,
  footer: null as Record<string, unknown> | null,
  filterBar: null as Record<string, unknown> | null,
  gridView: null as Record<string, unknown> | null,
};

vi.mock("../components/database/DatabaseGridToolbar", () => ({
  DatabaseGridToolbar: (props: Record<string, unknown>) => {
    captured.toolbar = props;
    return <div data-testid="toolbar" />;
  },
}));

vi.mock("../components/database/DataGridChrome", () => ({
  DataGridFilterBar: (props: Record<string, unknown>) => {
    captured.filterBar = props;
    return <div data-testid="filter-bar" />;
  },
  DataGridFooter: (props: Record<string, unknown>) => {
    captured.footer = props;
    return <div data-testid="footer" />;
  },
}));

vi.mock("../components/database/DataGridView", () => ({
  DataGridView: (props: Record<string, unknown>) => {
    captured.gridView = props;
    return <div data-testid="grid-view" />;
  },
}));

const CONNECTION = {
  id: "c1",
  name: "conn",
  dbType: "mysql",
} as unknown as AeroricDbConnectionConfig;

const OBJECT = {
  name: "users",
  schema: "public",
  object_type: "table",
} as unknown as DbxObjectInfo;

const QUERY_RESULT = {
  columns: ["id"],
  rows: [["1"]],
} as unknown as DbQueryResult;

const GRID = {
  state: { dbxGridPageSize: 200 },
} as unknown as DbxDataGridController;

function propsFor(overrides: Partial<DatabaseWorkspaceGridStackProps> = {}) {
  const noop = () => {};
  const asyncNoop = async () => {};
  const base: DatabaseWorkspaceGridStackProps = {
    workspaceMode: "table",
    grid: GRID,
    loading: false,
    error: null,
    page: 1,
    totalPages: 1,
    sql: "",
    setSql: noop,
    activeSqlCapable: true,
    runSql: asyncNoop,
    handleSqlDragOver: noop,
    handleSqlDrop: noop,
    queryResult: null,
    sqlResult: null,
    tableColumns: ["id"],
    showRowIdColumn: false,
    canInsertActiveTable: true,
    hideDatabaseWorkspaceTopbar: false,
    activeConnection: null,
    activeObject: null,
    activeDbxConnection: null,
    activeDbxDatabase: null,
    activeDbxObject: null,
    dbxSqlPreview: {} as DatabaseWorkspaceGridStackProps["dbxSqlPreview"],
    tableImport: { open: vi.fn() } as unknown as DatabaseWorkspaceGridStackProps["tableImport"],
    tableFooterRowCountText: "1 row",
    tableFooterSqlText: "SELECT 1",
    loadActiveObjectPage: noop,
    insertRow: noop,
    savePendingGridChanges: asyncNoop,
    openActiveTableProperties: noop,
    exportActiveDbxGrid: asyncNoop,
    copySelectedDbxRows: asyncNoop,
    deleteSelectedDbxRows: asyncNoop,
    resetActiveDbxGrid: asyncNoop,
    reloadActiveDbxGrid: asyncNoop,
    changeDbxGridPageSize: asyncNoop,
    handleDbxGridKeyDown: noop,
    toggleDbxGridColumnSort: asyncNoop,
    applyDbxGridColumnFuzzyFilter: asyncNoop,
    setContextMenu: noop,
    updateCell: asyncNoop,
  };
  return { ...base, ...overrides };
}

function renderStack(overrides: Partial<DatabaseWorkspaceGridStackProps> = {}) {
  const props = propsFor(overrides);
  render(
    <I18nProvider>
      <DatabaseWorkspaceGridStack {...props} />
    </I18nProvider>,
  );
  return props;
}

describe("DatabaseWorkspaceGridStack: 回调内部的判断", () => {
  beforeEach(() => {
    captured.toolbar = null;
    captured.footer = null;
    captured.filterBar = null;
    captured.gridView = null;
    vi.mocked(prompt).mockReset();
  });

  it("缺 dbx 连接时 onImportData 什么都不做", () => {
    const props = renderStack({ activeDbxConnection: null, activeDbxObject: OBJECT });
    (captured.toolbar?.onImportData as () => void)();
    expect(props.tableImport.open).not.toHaveBeenCalled();
  });

  it("缺 dbx 对象时 onImportData 什么都不做", () => {
    const props = renderStack({ activeDbxConnection: CONNECTION, activeDbxObject: null });
    (captured.toolbar?.onImportData as () => void)();
    expect(props.tableImport.open).not.toHaveBeenCalled();
  });

  it("连接与对象都在时 onImportData 才真的打开导入对话框", () => {
    const props = renderStack({
      activeDbxConnection: CONNECTION,
      activeDbxObject: OBJECT,
      activeDbxDatabase: "main",
    });
    (captured.toolbar?.onImportData as () => void)();
    expect(props.tableImport.open).toHaveBeenCalledWith(CONNECTION, "main", OBJECT);
  });
});

describe("DatabaseWorkspaceGridStack: onPromptPageSize 的取值区间", () => {
  beforeEach(() => {
    captured.footer = null;
    vi.mocked(prompt).mockReset();
  });

  async function promptPageSizeWith(input: string | null): Promise<number | null> {
    renderStack({ queryResult: QUERY_RESULT, activeDbxConnection: CONNECTION });
    vi.mocked(prompt).mockResolvedValue(input);
    const ask = captured.footer?.onPromptPageSize as () => Promise<number | null>;
    return await ask();
  }

  it("取消输入返回 null", async () => {
    expect(await promptPageSizeWith(null)).toBeNull();
  });

  it("空字符串按取消处理", async () => {
    expect(await promptPageSizeWith("")).toBeNull();
  });

  it("非数字返回 null", async () => {
    expect(await promptPageSizeWith("abc")).toBeNull();
  });

  it("下界 1 是合法的", async () => {
    expect(await promptPageSizeWith("1")).toBe(1);
  });

  it("上界 10000 是合法的", async () => {
    expect(await promptPageSizeWith("10000")).toBe(10000);
  });

  it("0 越界返回 null", async () => {
    expect(await promptPageSizeWith("0")).toBeNull();
  });

  it("10001 越界返回 null", async () => {
    expect(await promptPageSizeWith("10001")).toBeNull();
  });

  it("Infinity 返回 null", async () => {
    expect(await promptPageSizeWith("Infinity")).toBeNull();
  });

  it("默认值取自当前页大小", async () => {
    await promptPageSizeWith("500");
    expect(vi.mocked(prompt).mock.calls[0]?.[1]).toMatchObject({ defaultValue: "200" });
  });
});

describe("DatabaseWorkspaceGridStack: 挂载条件", () => {
  beforeEach(() => {
    captured.footer = null;
    captured.filterBar = null;
  });

  it("过滤条要 queryResult + dbx 连接 + dbx 对象三者齐全", () => {
    renderStack({
      queryResult: QUERY_RESULT,
      activeDbxConnection: CONNECTION,
      activeDbxObject: null,
    });
    expect(captured.filterBar).toBeNull();
  });

  it("三者齐全时过滤条才挂载", () => {
    renderStack({
      queryResult: QUERY_RESULT,
      activeDbxConnection: CONNECTION,
      activeDbxObject: OBJECT,
    });
    expect(captured.filterBar).not.toBeNull();
  });

  it("页脚只看 queryResult,没有 dbx 连接也挂载", () => {
    renderStack({ queryResult: QUERY_RESULT, activeDbxConnection: null });
    expect(captured.footer).not.toBeNull();
    expect(captured.footer?.showPageSize).toBe(false);
  });

  it("没有 queryResult 时页脚不挂载", () => {
    renderStack({ queryResult: null });
    expect(captured.footer).toBeNull();
  });
});
