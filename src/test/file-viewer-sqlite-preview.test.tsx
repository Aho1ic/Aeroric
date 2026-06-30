import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "../components/FileViewer";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@uiw/react-codemirror", async () => {
  function MockCodeMirror({ value }: { value: string }) {
    return <textarea aria-label="editor" readOnly value={value} />;
  }

  return {
    default: MockCodeMirror,
    Decoration: {
      set: () => [],
      widget: () => ({ range: () => ({}) }),
    },
    EditorView: {
      decorations: { compute: () => [] },
      domEventHandlers: () => [],
      theme: () => [],
      scrollIntoView: () => ({}),
    },
    GutterMarker: class {},
    StateEffect: {
      define: () => ({
        of: (value: unknown) => ({ value, is: () => true }),
      }),
    },
    StateField: {
      define: (config: unknown) => config,
    },
    ViewPlugin: {
      fromClass: () => [],
    },
    WidgetType: class {},
    gutter: () => [],
    hoverTooltip: () => [],
    showTooltip: {
      from: () => [],
    },
  };
});

function renderSqliteFileViewer() {
  render(
    <I18nProvider>
      <FileViewer
        tabs={[{ path: "/tmp/aeroric/data/app.db", name: "app.db" }]}
        activeFilePath="/tmp/aeroric/data/app.db"
        projectPath="/tmp/aeroric"
        onSelectTab={vi.fn()}
        onCloseTab={vi.fn()}
        onCloseOtherTabs={vi.fn()}
        onCloseTabsToRight={vi.fn()}
        onCloseAllTabs={vi.fn()}
        themeVariant="light"
      />
    </I18nProvider>,
  );
}

describe("FileViewer SQLite preview", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("loads .db files through the SQLite preview path instead of the text reader", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "read_project_config") {
        return Promise.resolve({ editor: { format_on_save: false } });
      }
      if (command === "db_inspect") {
        return Promise.resolve({
          objects: [
            {
              name: "users",
              objectType: "table",
              columns: [
                {
                  name: "id",
                  dataType: "INTEGER",
                  nullable: false,
                  notNull: true,
                  primaryKey: true,
                  primaryKeyOrdinal: 1,
                  defaultValue: null,
                },
                {
                  name: "name",
                  dataType: "TEXT",
                  nullable: true,
                  notNull: false,
                  primaryKey: false,
                  primaryKeyOrdinal: 0,
                  defaultValue: null,
                },
              ],
              indexes: [],
              foreignKeys: [],
              triggers: [],
              ddl: null,
              rowCount: 1,
              editable: true,
              primaryKeys: ["id"],
              hasRowId: true,
            },
          ],
        });
      }
      if (command === "db_query_table") {
        return Promise.resolve({
          columns: ["id", "name"],
          rows: [{ rowId: 1, keyValues: [], values: [1, "Ada"] }],
          page: 1,
          pageSize: 100,
          totalRows: 1,
          editable: true,
          primaryKeys: ["id"],
          hasRowId: true,
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    renderSqliteFileViewer();

    expect(await screen.findByText("SQLite preview")).toBeInTheDocument();
    expect(await screen.findAllByText("users")).toHaveLength(2);
    expect(await screen.findByText("Ada")).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("db_inspect", {
      endpoint: { kind: "local", path: "/tmp/aeroric/data/app.db" },
      projectRoot: "/tmp/aeroric",
    });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("db_query_table", {
      endpoint: { kind: "local", path: "/tmp/aeroric/data/app.db" },
      table: "users",
      page: 1,
      pageSize: 100,
      projectRoot: "/tmp/aeroric",
    });
    await waitFor(() =>
      expect(
        vi.mocked(invoke).mock.calls.some(([command]) => command === "read_file_content"),
      ).toBe(false),
    );
  });
});
