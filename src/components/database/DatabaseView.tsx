import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ChevronLeft,
  ChevronRight,
  Database,
  FilePlus,
  Plus,
  Play,
  Plug,
  RefreshCcw,
  Server,
  Trash2,
} from "lucide-react";
import type {
  DbConnectionConfig,
  DbEndpoint,
  DbExecuteResult,
  DbObject,
  DbQueryResult,
  DbSchema,
  SshConnection,
} from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";

interface Props {
  projectRoot?: string;
  remoteConnection?: SshConnection;
  remoteProjectPath?: string;
  sshConnections?: SshConnection[];
}

const PAGE_SIZE = 100;
type DatabaseRow = { rowId?: number | null; keyValues: Array<{ column: string; value: unknown }> };

function endpointLabel(endpoint: DbEndpoint): string {
  if (endpoint.kind === "local") return endpoint.path;
  return `${endpoint.connection.name}: ${endpoint.path}`;
}

function valueToText(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "string") return value;
  return String(value);
}

function textToCellValue(value: string): string | null {
  return value.trim().toUpperCase() === "NULL" ? null : value;
}

function rowKeyFor(row: DatabaseRow) {
  return {
    rowId: row.rowId ?? null,
    keyValues: row.keyValues
      .filter((item) => item.column !== "__aeroric_rowid__")
      .map((item) => ({
        column: item.column,
        value: item.value === null || item.value === undefined ? null : String(item.value),
      })),
  };
}

function createConnectionName(endpoint: DbEndpoint): string {
  const path = endpoint.kind === "local" ? endpoint.path : endpoint.path;
  const name = path.split("/").filter(Boolean).pop();
  return name || "SQLite";
}

function quoteSqlName(name: string): string {
  return `"${name.replace(/"/g, '""')}"`;
}

export function DatabaseView({
  projectRoot,
  remoteConnection,
  remoteProjectPath,
  sshConnections = [],
}: Props) {
  const { t } = useI18n();
  const [connections, setConnections] = useState<DbConnectionConfig[]>([]);
  const [activeConnectionId, setActiveConnectionId] = useState<string | null>(null);
  const [schema, setSchema] = useState<DbSchema | null>(null);
  const [activeObject, setActiveObject] = useState<DbObject | null>(null);
  const [queryResult, setQueryResult] = useState<DbQueryResult | null>(null);
  const [sqlResult, setSqlResult] = useState<DbExecuteResult | null>(null);
  const [page, setPage] = useState(1);
  const [sql, setSql] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const allSshConnections = useMemo(() => {
    if (!remoteConnection) return sshConnections;
    return sshConnections.some((connection) => connection.id === remoteConnection.id)
      ? sshConnections
      : [remoteConnection, ...sshConnections];
  }, [remoteConnection, sshConnections]);

  const activeConnection = useMemo(
    () => connections.find((connection) => connection.id === activeConnectionId) ?? null,
    [activeConnectionId, connections],
  );

  const activeEndpoint = activeConnection?.endpoint ?? null;
  const tableRows = queryResult?.rows ?? sqlResult?.rows ?? [];
  const tableColumns = queryResult?.columns ?? sqlResult?.columns ?? [];
  const totalPages =
    queryResult?.totalRows && queryResult.totalRows > 0
      ? Math.max(1, Math.ceil(queryResult.totalRows / queryResult.pageSize))
      : null;

  const saveConnections = useCallback((next: DbConnectionConfig[]) => {
    setConnections(next);
    invoke("db_save_connections", { connections: next }).catch((err) => {
      setError(String(err));
    });
  }, []);

  const inspect = useCallback(
    async (connection: DbConnectionConfig) => {
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const nextSchema = await invoke<DbSchema>("db_inspect", {
          endpoint: connection.endpoint,
          projectRoot,
        });
        setSchema(nextSchema);
        const firstTable =
          nextSchema.objects.find((object) => object.objectType === "table") ??
          nextSchema.objects[0] ??
          null;
        setActiveObject(firstTable);
        setPage(1);
        if (firstTable) {
          const result = await invoke<DbQueryResult>("db_query_table", {
            endpoint: connection.endpoint,
            table: firstTable.name,
            page: 1,
            pageSize: PAGE_SIZE,
            projectRoot,
          });
          setQueryResult(result);
          setSql(`SELECT * FROM ${quoteSqlName(firstTable.name)}`);
        } else {
          setQueryResult(null);
        }
        if (connections.some((item) => item.id === connection.id)) {
          const now = Date.now();
          saveConnections(
            connections.map((item) =>
              item.id === connection.id ? { ...item, lastOpenedAt: now } : item,
            ),
          );
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [connections, projectRoot, saveConnections],
  );

  useEffect(() => {
    invoke<DbConnectionConfig[]>("db_load_connections")
      .then((items) => {
        setConnections(items);
        if (items[0]) {
          setActiveConnectionId(items[0].id);
          inspect(items[0]);
        }
      })
      .catch((err) => setError(String(err)));
    // Load once; inspect is intentionally not a dependency here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const addConnection = useCallback(
    (endpoint: DbEndpoint) => {
      const now = Date.now();
      const connection: DbConnectionConfig = {
        id: `db:${now}:${Math.random().toString(36).slice(2)}`,
        name: createConnectionName(endpoint),
        endpoint,
        readOnly: false,
        createdAt: now,
        lastOpenedAt: now,
      };
      const next = [connection, ...connections];
      saveConnections(next);
      setActiveConnectionId(connection.id);
      inspect(connection);
    },
    [connections, inspect, saveConnections],
  );

  const handleOpenLocal = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "SQLite", extensions: ["db", "sqlite", "sqlite3"] }],
      defaultPath: projectRoot,
    });
    if (typeof selected !== "string") return;
    addConnection({ kind: "local", path: selected });
  }, [addConnection, projectRoot]);

  const handleAddRemote = useCallback(() => {
    const connection = remoteConnection ?? allSshConnections[0];
    if (!connection) {
      setError(t("database.noSshConnection"));
      return;
    }
    const defaultPath = remoteProjectPath
      ? `${remoteProjectPath.replace(/\/$/, "")}/database.db`
      : (connection.remotePath ? `${connection.remotePath.replace(/\/$/, "")}/database.db` : "/tmp/database.db");
    const path = window.prompt(t("database.remotePathPrompt"), defaultPath)?.trim();
    if (!path) return;
    addConnection({
      kind: "ssh",
      connection,
      path,
      projectPath: remoteProjectPath,
    });
  }, [addConnection, allSshConnections, remoteConnection, remoteProjectPath, t]);

  const handleSelectConnection = useCallback(
    (connection: DbConnectionConfig) => {
      setActiveConnectionId(connection.id);
      inspect(connection);
    },
    [inspect],
  );

  const handleDeleteConnection = useCallback(
    (connectionId: string) => {
      if (!window.confirm(t("database.confirmDeleteConnection"))) return;
      const next = connections.filter((connection) => connection.id !== connectionId);
      saveConnections(next);
      if (activeConnectionId === connectionId) {
        setActiveConnectionId(next[0]?.id ?? null);
        setSchema(null);
        setActiveObject(null);
        setQueryResult(null);
        setSqlResult(null);
      }
    },
    [activeConnectionId, connections, saveConnections, t],
  );

  const toggleReadOnly = useCallback(() => {
    if (!activeConnection) return;
    const next = connections.map((connection) =>
      connection.id === activeConnection.id
        ? { ...connection, readOnly: !connection.readOnly }
        : connection,
    );
    saveConnections(next);
  }, [activeConnection, connections, saveConnections]);

  const loadTable = useCallback(
    async (object: DbObject, nextPage: number) => {
      if (!activeEndpoint) return;
      setLoading(true);
      setError(null);
      setSqlResult(null);
      try {
        const result = await invoke<DbQueryResult>("db_query_table", {
          endpoint: activeEndpoint,
          table: object.name,
          page: nextPage,
          pageSize: PAGE_SIZE,
          projectRoot,
        });
        setActiveObject(object);
        setPage(nextPage);
        setQueryResult(result);
        setSql(`SELECT * FROM ${quoteSqlName(object.name)}`);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoading(false);
      }
    },
    [activeEndpoint, projectRoot],
  );

  const refresh = useCallback(() => {
    if (activeConnection) inspect(activeConnection);
  }, [activeConnection, inspect]);

  const runSql = useCallback(async () => {
    if (!activeEndpoint) return;
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<DbExecuteResult>("db_execute_sql", {
        endpoint: activeEndpoint,
        sql,
        page: 1,
        pageSize: PAGE_SIZE,
        readOnly: activeConnection?.readOnly ?? false,
        projectRoot,
      });
      setSqlResult(result);
      setQueryResult(null);
      if (activeConnection) {
        const nextSchema = await invoke<DbSchema>("db_inspect", {
          endpoint: activeConnection.endpoint,
          projectRoot,
        });
        setSchema(nextSchema);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [activeConnection, activeEndpoint, projectRoot, sql]);

  const updateCell = useCallback(
    async (
      row: DatabaseRow,
      column: string,
      value: string,
      original: string,
    ) => {
      if (!activeEndpoint || !activeObject || value === original || activeConnection?.readOnly) return;
      setError(null);
      try {
        await invoke("db_update_cell", {
          endpoint: activeEndpoint,
          table: activeObject.name,
          rowKey: rowKeyFor(row),
          column,
          value: textToCellValue(value),
          readOnly: activeConnection?.readOnly ?? false,
          projectRoot,
        });
        await loadTable(activeObject, page);
      } catch (err) {
        setError(String(err));
      }
    },
    [activeConnection?.readOnly, activeEndpoint, activeObject, loadTable, page, projectRoot],
  );

  const insertRow = useCallback(async () => {
    if (!activeEndpoint || !activeObject || activeConnection?.readOnly) return;
    setError(null);
    const sample = Object.fromEntries(
      activeObject.columns
        .filter((column) => !column.primaryKey && !column.defaultValue)
        .map((column) => [column.name, null]),
    );
    const raw = window.prompt(t("database.insertJsonPrompt"), JSON.stringify(sample));
    if (!raw) return;
    try {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      const values = Object.entries(parsed).map(([column, value]) => ({
        column,
        value: value === null || value === undefined ? null : String(value),
      }));
      await invoke("db_insert_row", {
        endpoint: activeEndpoint,
        table: activeObject.name,
        values,
        readOnly: activeConnection?.readOnly ?? false,
        projectRoot,
      });
      await loadTable(activeObject, page);
    } catch (err) {
      setError(String(err));
    }
  }, [activeConnection?.readOnly, activeEndpoint, activeObject, loadTable, page, projectRoot, t]);

  const deleteRow = useCallback(
    async (row: DatabaseRow) => {
      if (!activeEndpoint || !activeObject || activeConnection?.readOnly) return;
      if (!window.confirm(t("database.confirmDeleteRow"))) return;
      setError(null);
      try {
        await invoke("db_delete_row", {
          endpoint: activeEndpoint,
          table: activeObject.name,
          rowKey: rowKeyFor(row),
          readOnly: activeConnection?.readOnly ?? false,
          projectRoot,
        });
        await loadTable(activeObject, page);
      } catch (err) {
        setError(String(err));
      }
    },
    [activeConnection?.readOnly, activeEndpoint, activeObject, loadTable, page, projectRoot, t],
  );

  return (
    <div style={s.databaseRoot}>
      <aside style={s.databaseSidebar}>
        <div style={s.databaseSidebarHeader}>
          <div style={s.databaseTitleRow}>
            <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
              <Database size={16} />
              <span style={s.databaseTitle}>{t("database.title")}</span>
            </div>
            <button type="button" style={s.databaseSmallButton} onClick={refresh} disabled={!activeConnection || loading}>
              <RefreshCcw size={13} />
            </button>
          </div>
          <div style={s.databaseActions}>
            <button type="button" style={s.databaseSmallButton} onClick={handleOpenLocal}>
              <FilePlus size={13} />
              <span>{t("database.openLocal")}</span>
            </button>
            <button type="button" style={s.databaseSmallButton} onClick={handleAddRemote}>
              <Server size={13} />
              <span>{t("database.addRemote")}</span>
            </button>
          </div>
          {activeConnection && (
            <button
              type="button"
              style={{
                ...s.databaseSmallButton,
                justifyContent: "flex-start",
                padding: "0 9px",
                background: activeConnection.readOnly ? "var(--warning-surface)" : "var(--bg-card)",
                color: activeConnection.readOnly ? "var(--warning)" : "var(--text-secondary)",
              }}
              onClick={toggleReadOnly}
            >
              <span>{activeConnection.readOnly ? t("database.readOnlyOn") : t("database.readOnlyOff")}</span>
            </button>
          )}
        </div>

        <div style={s.databaseScroll}>
          <div style={s.databaseSection}>
            <div style={s.databaseSectionTitle}>{t("database.connections")}</div>
            {connections.map((connection) => (
              <button
                key={connection.id}
                type="button"
                style={{
                  ...s.databaseListButton,
                  ...(connection.id === activeConnectionId ? s.databaseListButtonActive : {}),
                }}
                onClick={() => handleSelectConnection(connection)}
              >
                {connection.endpoint.kind === "local" ? <Database size={14} /> : <Plug size={14} />}
                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {connection.name}
                </span>
                <Trash2
                  size={13}
                  onClick={(event) => {
                    event.stopPropagation();
                    handleDeleteConnection(connection.id);
                  }}
                />
              </button>
            ))}
          </div>

          <div style={s.databaseSection}>
            <div style={s.databaseSectionTitle}>{t("database.objects")}</div>
            {schema?.objects.map((object) => (
              <button
                key={`${object.objectType}:${object.name}`}
                type="button"
                style={{
                  ...s.databaseListButton,
                  ...(object.name === activeObject?.name ? s.databaseListButtonActive : {}),
                }}
                onClick={() => loadTable(object, 1)}
              >
                <Database size={14} />
                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {object.name}
                </span>
                {object.rowCount != null && (
                  <span style={{ color: "var(--text-hint)", fontSize: 11 }}>{object.rowCount}</span>
                )}
              </button>
            ))}
          </div>

          {activeObject && (
            <div style={s.databaseSection}>
              <div style={s.databaseSectionTitle}>{t("database.structure")}</div>
              <div style={{ padding: "0 6px", display: "flex", flexDirection: "column", gap: 8 }}>
                <div style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.5 }}>
                  {activeObject.primaryKeys.length > 0
                    ? t("database.primaryKeys", { keys: activeObject.primaryKeys.join(", ") })
                    : activeObject.hasRowId
                      ? t("database.rowidEditable")
                      : t("database.notEditable")}
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  {activeObject.columns.map((column) => (
                    <div key={column.name} style={{ fontSize: 11.5, color: "var(--text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={`${column.name} ${column.dataType}`}>
                      {column.primaryKey ? "* " : ""}
                      {column.name}
                      <span style={{ color: "var(--text-hint)" }}> {column.dataType || "TEXT"}</span>
                    </div>
                  ))}
                </div>
                {(activeObject.indexes.length > 0 || activeObject.foreignKeys.length > 0 || activeObject.triggers.length > 0) && (
                  <div style={{ fontSize: 11, color: "var(--text-hint)", lineHeight: 1.5 }}>
                    {t("database.objectStats", {
                      indexes: activeObject.indexes.length,
                      foreignKeys: activeObject.foreignKeys.length,
                      triggers: activeObject.triggers.length,
                    })}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </aside>

      <main style={s.databaseMain}>
        <div style={s.databaseTopbar}>
          <div style={{ minWidth: 0 }}>
            <div style={s.databaseTitle}>{activeObject?.name ?? t("database.noSelection")}</div>
            <div style={s.databasePath}>{activeEndpoint ? endpointLabel(activeEndpoint) : t("database.chooseConnection")}</div>
          </div>
          {error && <div style={s.databaseError} title={error}>{error}</div>}
        </div>

        <div style={s.databaseSqlPanel}>
          <textarea
            style={s.databaseSqlInput}
            value={sql}
            onChange={(event) => setSql(event.target.value)}
            spellCheck={false}
            placeholder={t("database.sqlPlaceholder")}
          />
          <button type="button" style={{ ...s.databaseSmallButton, width: 86, height: "auto" }} onClick={runSql} disabled={!activeEndpoint || loading}>
            <Play size={14} />
            <span>{t("database.run")}</span>
          </button>
        </div>

        <div style={s.databaseToolbar}>
          <button
            type="button"
            style={s.databaseSmallButton}
            disabled={!activeObject || !queryResult?.editable || activeConnection?.readOnly || loading}
            onClick={insertRow}
          >
            <Plus size={13} />
            <span>{t("database.insert")}</span>
          </button>
          <button
            type="button"
            style={s.databaseSmallButton}
            disabled={!activeObject || page <= 1 || loading}
            onClick={() => activeObject && loadTable(activeObject, Math.max(1, page - 1))}
          >
            <ChevronLeft size={13} />
          </button>
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
            {t("database.page", {
              page,
              total: totalPages ?? "?",
            })}
          </span>
          <button
            type="button"
            style={s.databaseSmallButton}
            disabled={!activeObject || loading || (totalPages != null && page >= totalPages)}
            onClick={() => activeObject && loadTable(activeObject, page + 1)}
          >
            <ChevronRight size={13} />
          </button>
          <span style={{ color: "var(--text-hint)", fontSize: 12 }}>
            {loading
              ? t("database.loading")
              : activeConnection?.readOnly
                ? t("database.readOnlyBadge")
                : sqlResult?.message}
          </span>
        </div>

        {tableColumns.length === 0 ? (
          <div style={s.databaseEmpty}>{t("database.empty")}</div>
        ) : (
          <div style={s.databaseTableWrap}>
            <table style={s.databaseTable}>
              <thead>
                <tr>
                  <th style={{ ...s.databaseTh, width: 86 }}>rowid</th>
                  {queryResult && <th style={{ ...s.databaseTh, width: 74 }}>{t("database.actions")}</th>}
                  {tableColumns.map((column) => (
                    <th key={column} style={s.databaseTh} title={column}>
                      {column}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {tableRows.map((row, rowIndex) => (
                  <tr key={`${row.rowId ?? "sql"}:${rowIndex}`}>
                    <td style={{ ...s.databaseTd, color: "var(--text-hint)" }}>{row.rowId ?? "-"}</td>
                    {queryResult && (
                      <td style={s.databaseTd}>
                        <button
                          type="button"
                          style={{ ...s.databaseSmallButton, height: 24, padding: "0 8px" }}
                          disabled={!queryResult.editable || activeConnection?.readOnly}
                          onClick={() => deleteRow(row)}
                        >
                          <Trash2 size={12} />
                        </button>
                      </td>
                    )}
                    {tableColumns.map((column, columnIndex) => {
                      const original = valueToText(row.values[columnIndex]);
                      const editable = Boolean(
                        queryResult &&
                          activeObject?.objectType === "table" &&
                          queryResult.editable &&
                          !activeConnection?.readOnly,
                      );
                      return (
                        <td key={`${column}:${columnIndex}`} style={s.databaseTd} title={original}>
                          {editable ? (
                            <input
                              style={s.databaseCellInput}
                              defaultValue={original}
                              onFocus={(event) => {
                                event.currentTarget.style.borderColor = "var(--border-focus)";
                                event.currentTarget.style.background = "var(--bg-input)";
                              }}
                              onBlur={(event) => {
                                event.currentTarget.style.borderColor = "transparent";
                                event.currentTarget.style.background = "transparent";
                                updateCell(row, column, event.currentTarget.value, original);
                              }}
                            />
                          ) : (
                            original
                          )}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </main>
    </div>
  );
}
