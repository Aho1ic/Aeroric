import { Database, Table2 } from "lucide-react";
import type { DbObject, DbQueryResult, DbSchema } from "../../types";
import { useI18n } from "../../i18n";

export type SqlitePreviewData = {
  schema: DbSchema;
  selectedObjectName: string | null;
  tableData: DbQueryResult | null;
  tableLoading: boolean;
  tableError: string | null;
};

function sqliteCellText(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "object") {
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }
  return String(value);
}

function SqliteObjectButton({
  object,
  selected,
  onSelect,
}: {
  object: DbObject;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = useI18n();
  const rowCount =
    typeof object.rowCount === "number"
      ? t("file.sqlitePreviewRowCount", { count: String(object.rowCount) })
      : object.objectType;
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onSelect}
      style={{
        width: "100%",
        minHeight: 40,
        display: "grid",
        gridTemplateColumns: "16px minmax(0, 1fr)",
        gap: 7,
        alignItems: "center",
        padding: "6px 8px",
        border: "none",
        borderRadius: 6,
        background: selected ? "var(--bg-selected)" : "transparent",
        color: selected ? "var(--text-primary)" : "var(--text-secondary)",
        textAlign: "left",
        cursor: "pointer",
      }}
      onMouseEnter={(event) => {
        if (!selected) event.currentTarget.style.background = "var(--bg-hover)";
      }}
      onMouseLeave={(event) => {
        if (!selected) event.currentTarget.style.background = "transparent";
      }}
    >
      <Table2 size={13} strokeWidth={2} />
      <span style={{ minWidth: 0 }}>
        <span
          style={{
            display: "block",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            fontSize: 12,
            fontWeight: 650,
          }}
        >
          {object.name}
        </span>
        <span
          style={{
            display: "block",
            marginTop: 2,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            color: "var(--text-muted)",
            fontSize: 10.5,
            fontFamily: "var(--font-mono)",
          }}
        >
          {rowCount}
        </span>
      </span>
    </button>
  );
}

export function SqlitePreviewPane({
  fileName,
  preview,
  onSelectObject,
}: {
  fileName: string;
  preview: SqlitePreviewData;
  onSelectObject: (name: string) => void;
}) {
  const { t } = useI18n();
  const objects = preview.schema.objects;
  const selectedObject =
    objects.find((object) => object.name === preview.selectedObjectName) ?? objects[0] ?? null;
  const rows = preview.tableData?.rows ?? [];
  const columns =
    preview.tableData?.columns ?? selectedObject?.columns.map((column) => column.name) ?? [];

  return (
    <div
      style={{
        height: "100%",
        display: "grid",
        gridTemplateColumns: "230px minmax(0, 1fr)",
        minWidth: 0,
        minHeight: 0,
        background: "var(--bg-panel)",
      }}
    >
      <aside
        style={{
          minWidth: 0,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          borderRight: "1px solid var(--border-dim)",
          background: "var(--bg-sidebar)",
        }}
      >
        <div
          style={{
            minHeight: 44,
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "8px 10px",
            borderBottom: "1px solid var(--border-dim)",
            color: "var(--text-primary)",
          }}
        >
          <Database size={15} />
          <span style={{ minWidth: 0 }}>
            <span
              style={{
                display: "block",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                fontSize: 12,
                fontWeight: 700,
              }}
            >
              {fileName}
            </span>
            <span style={{ display: "block", color: "var(--text-muted)", fontSize: 10.5 }}>
              {t("file.sqlitePreview")}
            </span>
          </span>
        </div>
        <div
          style={{
            padding: "7px 8px 5px",
            color: "var(--text-muted)",
            fontSize: 10.5,
            fontWeight: 700,
            textTransform: "uppercase",
          }}
        >
          {t("file.sqlitePreviewObjects", { count: String(objects.length) })}
        </div>
        <div style={{ minHeight: 0, overflowY: "auto", padding: "0 6px 8px" }}>
          {objects.length === 0 ? (
            <div style={{ padding: "8px 4px", color: "var(--text-hint)", fontSize: 12 }}>
              {t("file.sqlitePreviewNoObjects")}
            </div>
          ) : (
            objects.map((object) => (
              <SqliteObjectButton
                key={`${object.objectType}:${object.name}`}
                object={object}
                selected={object.name === selectedObject?.name}
                onSelect={() => onSelectObject(object.name)}
              />
            ))
          )}
        </div>
      </aside>
      <main
        style={{
          minWidth: 0,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            minHeight: 44,
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "8px 12px",
            borderBottom: "1px solid var(--border-dim)",
          }}
        >
          <Table2 size={15} color="var(--accent)" />
          <span style={{ minWidth: 0, flex: 1 }}>
            <span
              style={{
                display: "block",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                color: "var(--text-primary)",
                fontSize: 13,
                fontWeight: 700,
              }}
            >
              {selectedObject?.name ?? t("file.sqlitePreviewSelectObject")}
            </span>
            {selectedObject ? (
              <span
                style={{
                  display: "block",
                  marginTop: 2,
                  color: "var(--text-muted)",
                  fontFamily: "var(--font-mono)",
                  fontSize: 10.5,
                }}
              >
                {t("file.sqlitePreviewObjectMeta", {
                  type: selectedObject.objectType,
                  columns: String(selectedObject.columns.length),
                })}
              </span>
            ) : null}
          </span>
        </div>
        {selectedObject ? (
          <div
            style={{
              flex: 1,
              minHeight: 0,
              display: "grid",
              gridTemplateRows: "auto minmax(0, 1fr)",
            }}
          >
            <div
              style={{
                display: "flex",
                gap: 6,
                overflowX: "auto",
                padding: "8px 10px",
                borderBottom: "1px solid var(--border-dim)",
              }}
            >
              {selectedObject.columns.length === 0 ? (
                <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
                  {t("file.sqlitePreviewNoColumns")}
                </span>
              ) : (
                selectedObject.columns.map((column) => (
                  <span
                    key={column.name}
                    title={`${column.name}${column.dataType ? ` · ${column.dataType}` : ""}`}
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 5,
                      maxWidth: 220,
                      height: 22,
                      padding: "0 7px",
                      border: "1px solid var(--border-dim)",
                      borderRadius: 5,
                      background: "var(--bg-subtle)",
                      color: column.primaryKey ? "var(--accent)" : "var(--text-secondary)",
                      fontFamily: "var(--font-mono)",
                      fontSize: 10.5,
                      whiteSpace: "nowrap",
                    }}
                  >
                    <span
                      style={{
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {column.name}
                    </span>
                    {column.dataType ? (
                      <span style={{ color: "var(--text-hint)" }}>{column.dataType}</span>
                    ) : null}
                  </span>
                ))
              )}
            </div>
            <div style={{ minHeight: 0, overflow: "auto", position: "relative" }}>
              {preview.tableLoading ? (
                <div
                  style={{
                    height: "100%",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    color: "var(--text-hint)",
                    fontSize: 12,
                  }}
                >
                  {t("file.sqlitePreviewLoadingRows")}
                </div>
              ) : preview.tableError ? (
                <div
                  style={{
                    height: "100%",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    color: "var(--warning)",
                    fontSize: 12,
                    padding: 18,
                    textAlign: "center",
                  }}
                >
                  {t("file.sqlitePreviewFailed", { error: preview.tableError })}
                </div>
              ) : rows.length === 0 ? (
                <div
                  style={{
                    height: "100%",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    color: "var(--text-hint)",
                    fontSize: 12,
                  }}
                >
                  {t("file.sqlitePreviewEmpty")}
                </div>
              ) : (
                <table
                  style={{
                    width: "100%",
                    minWidth: "max-content",
                    borderCollapse: "collapse",
                    fontFamily: "var(--font-mono)",
                    fontSize: 11.5,
                  }}
                >
                  <thead>
                    <tr>
                      {columns.map((column) => (
                        <th
                          key={column}
                          style={{
                            position: "sticky",
                            top: 0,
                            zIndex: 1,
                            maxWidth: 260,
                            padding: "7px 9px",
                            borderBottom: "1px solid var(--border)",
                            borderRight: "1px solid var(--border-dim)",
                            background: "var(--bg-sidebar)",
                            color: "var(--text-secondary)",
                            textAlign: "left",
                            fontWeight: 700,
                            whiteSpace: "nowrap",
                          }}
                        >
                          {column}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((row, rowIndex) => (
                      <tr key={row.rowId ?? rowIndex}>
                        {columns.map((column, columnIndex) => (
                          <td
                            key={`${rowIndex}:${column}`}
                            title={sqliteCellText(row.values[columnIndex])}
                            style={{
                              maxWidth: 260,
                              padding: "6px 9px",
                              borderBottom: "1px solid var(--border-dim)",
                              borderRight: "1px solid var(--border-dim)",
                              color:
                                row.values[columnIndex] === null
                                  ? "var(--text-hint)"
                                  : "var(--text-primary)",
                              whiteSpace: "nowrap",
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                            }}
                          >
                            {sqliteCellText(row.values[columnIndex])}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        ) : (
          <div
            style={{
              flex: 1,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--text-hint)",
              fontSize: 12,
            }}
          >
            {t("file.sqlitePreviewNoObjects")}
          </div>
        )}
      </main>
    </div>
  );
}
