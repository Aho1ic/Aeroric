import { useEffect, useMemo, useState } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { KeyRound, Lock, Play, RefreshCcw, Search, Trash2, Unlock, UserPlus } from "lucide-react";
import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import s from "../../styles";
import type { AeroricDbConnectionConfig, DbxDatabaseType, DbxQueryResult } from "../../types";
import { DbxButton } from "./DbxButton";

type UserAdminDialect = "mysql" | "postgres";
type PrivilegeScope = "mysql" | "database" | "schema" | "table" | "role";

interface DatabaseUserIdentity {
  user: string;
  host: string;
  plugin?: string;
  canLogin?: boolean;
}

interface Props {
  connection: AeroricDbConnectionConfig;
  database?: string | null;
  schema?: string | null;
}

const USER_ADMIN_TYPES = new Set<DbxDatabaseType>(["mysql", "postgres"]);
const MYSQL_COMMON_PRIVILEGES = ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "INDEX", "REFERENCES", "EXECUTE", "SHOW VIEW", "TRIGGER", "EVENT", "CREATE TEMPORARY TABLES"] as const;
const POSTGRES_DATABASE_PRIVILEGES = ["CONNECT", "CREATE", "TEMPORARY"] as const;
const POSTGRES_SCHEMA_PRIVILEGES = ["USAGE", "CREATE"] as const;
const POSTGRES_TABLE_PRIVILEGES = ["SELECT", "INSERT", "UPDATE", "DELETE", "TRUNCATE", "REFERENCES", "TRIGGER"] as const;

export function supportsDbxUserAdmin(dbType: DbxDatabaseType | undefined): boolean {
  return !!dbType && USER_ADMIN_TYPES.has(dbType);
}

function userAdminDialect(dbType: DbxDatabaseType): UserAdminDialect {
  return dbType === "postgres" ? "postgres" : "mysql";
}

function quoteSqlString(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

function quoteMySqlString(value: string): string {
  return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "''")}'`;
}

function quoteMySqlIdentifier(value: string): string {
  return `\`${value.replace(/`/g, "``")}\``;
}

function quotePostgresIdentifier(value: string): string {
  return `"${value.replace(/"/g, '""')}"`;
}

function mysqlUserAccount(user: DatabaseUserIdentity): string {
  return `${quoteMySqlString(user.user)}@${quoteMySqlString(user.host || "%")}`;
}

function postgresRole(user: DatabaseUserIdentity): string {
  return quotePostgresIdentifier(user.user);
}

function userLabel(user: DatabaseUserIdentity, dialect: UserAdminDialect): string {
  return dialect === "postgres" ? user.user : `${user.user}@${user.host}`;
}

function listUsersSql(dialect: UserAdminDialect): string {
  if (dialect === "postgres") {
    return [
      "SELECT",
      "  r.rolname AS user,",
      "  CASE WHEN r.rolcanlogin THEN 'LOGIN' ELSE 'ROLE' END AS host,",
      "  concat_ws(', ',",
      "    CASE WHEN r.rolsuper THEN 'SUPERUSER' END,",
      "    CASE WHEN r.rolcreatedb THEN 'CREATEDB' END,",
      "    CASE WHEN r.rolcreaterole THEN 'CREATEROLE' END,",
      "    CASE WHEN r.rolreplication THEN 'REPLICATION' END,",
      "    CASE WHEN r.rolbypassrls THEN 'BYPASSRLS' END",
      "  ) AS plugin",
      "FROM pg_roles r",
      "ORDER BY r.rolname;",
    ].join("\n");
  }
  return "SELECT User AS user, Host AS host, plugin AS plugin FROM mysql.user ORDER BY User, Host;";
}

function mysqlFallbackListUsersSql(): string {
  return "SELECT DISTINCT GRANTEE AS grantee FROM information_schema.USER_PRIVILEGES ORDER BY GRANTEE;";
}

function showGrantsSql(user: DatabaseUserIdentity, dialect: UserAdminDialect): string {
  if (dialect === "mysql") return `SHOW GRANTS FOR ${mysqlUserAccount(user)};`;
  const role = quoteSqlString(user.user);
  return `
WITH target AS (
  SELECT oid, rolname, rolsuper, rolcreatedb, rolcreaterole, rolcanlogin, rolreplication, rolbypassrls
  FROM pg_catalog.pg_roles
  WHERE rolname = ${role}
)
SELECT line
FROM (
  SELECT 1 AS sort, 'Role: ' || quote_ident(rolname) AS line
  FROM target
  UNION ALL
  SELECT 2, 'Attributes: ' || COALESCE(NULLIF(concat_ws(', ',
    CASE WHEN rolsuper THEN 'SUPERUSER' END,
    CASE WHEN rolcreatedb THEN 'CREATEDB' END,
    CASE WHEN rolcreaterole THEN 'CREATEROLE' END,
    CASE WHEN rolcanlogin THEN 'LOGIN' ELSE 'NOLOGIN' END,
    CASE WHEN rolreplication THEN 'REPLICATION' END,
    CASE WHEN rolbypassrls THEN 'BYPASSRLS' END
  ), ''), 'none')
  FROM target
  UNION ALL
  SELECT 10, 'Member of: ' || quote_ident(parent.rolname) || CASE WHEN m.admin_option THEN ' WITH ADMIN OPTION' ELSE '' END
  FROM pg_catalog.pg_auth_members m
  JOIN target t ON t.oid = m.member
  JOIN pg_catalog.pg_roles parent ON parent.oid = m.roleid
  UNION ALL
  SELECT 20, 'Has member: ' || quote_ident(member.rolname) || CASE WHEN m.admin_option THEN ' WITH ADMIN OPTION' ELSE '' END
  FROM pg_catalog.pg_auth_members m
  JOIN target t ON t.oid = m.roleid
  JOIN pg_catalog.pg_roles member ON member.oid = m.member
  UNION ALL
  SELECT 30, 'Database: ' || quote_ident(d.datname) || ' = ' ||
    concat_ws(', ',
      CASE WHEN has_database_privilege(t.rolname, d.oid, 'CONNECT') THEN 'CONNECT' END,
      CASE WHEN has_database_privilege(t.rolname, d.oid, 'CREATE') THEN 'CREATE' END,
      CASE WHEN has_database_privilege(t.rolname, d.oid, 'TEMPORARY') THEN 'TEMPORARY' END
    )
  FROM target t
  CROSS JOIN pg_catalog.pg_database d
  WHERE has_database_privilege(t.rolname, d.oid, 'CONNECT')
     OR has_database_privilege(t.rolname, d.oid, 'CREATE')
     OR has_database_privilege(t.rolname, d.oid, 'TEMPORARY')
  UNION ALL
  SELECT 40, 'Schema: ' || quote_ident(n.nspname) || ' = ' ||
    concat_ws(', ',
      CASE WHEN has_schema_privilege(t.rolname, n.oid, 'USAGE') THEN 'USAGE' END,
      CASE WHEN has_schema_privilege(t.rolname, n.oid, 'CREATE') THEN 'CREATE' END
    )
  FROM target t
  CROSS JOIN pg_catalog.pg_namespace n
  WHERE n.nspname NOT LIKE 'pg\\_%' ESCAPE '\\'
    AND n.nspname <> 'information_schema'
    AND (has_schema_privilege(t.rolname, n.oid, 'USAGE') OR has_schema_privilege(t.rolname, n.oid, 'CREATE'))
  UNION ALL
  SELECT 50, 'Table: ' || quote_ident(table_schema) || '.' || quote_ident(table_name) || ' = ' ||
    string_agg(privilege_type || CASE WHEN is_grantable = 'YES' THEN ' WITH GRANT OPTION' ELSE '' END, ', ' ORDER BY privilege_type)
  FROM information_schema.role_table_grants
  WHERE grantee = ${role}
  GROUP BY table_schema, table_name
) grants
ORDER BY sort, line;`.trim();
}

function createUserSql(input: DatabaseUserIdentity & { password: string; canLogin?: boolean }, dialect: UserAdminDialect): string {
  if (dialect === "postgres") {
    const login = input.canLogin === false ? "NOLOGIN" : "LOGIN";
    return `CREATE ROLE ${quotePostgresIdentifier(input.user)} ${login} PASSWORD ${quoteSqlString(input.password)};`;
  }
  return `CREATE USER ${mysqlUserAccount(input)} IDENTIFIED BY ${quoteMySqlString(input.password)};`;
}

function alterPasswordSql(user: DatabaseUserIdentity, password: string, dialect: UserAdminDialect): string {
  return dialect === "postgres"
    ? `ALTER ROLE ${postgresRole(user)} PASSWORD ${quoteSqlString(password)};`
    : `ALTER USER ${mysqlUserAccount(user)} IDENTIFIED BY ${quoteMySqlString(password)};`;
}

function alterLoginSql(user: DatabaseUserIdentity, enabled: boolean, dialect: UserAdminDialect): string {
  return dialect === "postgres"
    ? `ALTER ROLE ${postgresRole(user)} ${enabled ? "LOGIN" : "NOLOGIN"};`
    : `ALTER USER ${mysqlUserAccount(user)} ACCOUNT ${enabled ? "UNLOCK" : "LOCK"};`;
}

function dropUserSql(user: DatabaseUserIdentity, dialect: UserAdminDialect): string {
  return dialect === "postgres" ? `DROP ROLE ${postgresRole(user)};` : `DROP USER ${mysqlUserAccount(user)};`;
}

function postgresDefaultPrivilege(scope: PrivilegeScope): string {
  if (scope === "database") return "CONNECT";
  if (scope === "schema") return "USAGE";
  return "SELECT";
}

function defaultPrivilegesForScope(dialect: UserAdminDialect, scope: PrivilegeScope): string[] {
  if (dialect === "postgres" && scope === "role") return [];
  return [dialect === "postgres" ? postgresDefaultPrivilege(scope) : "SELECT"];
}

function privilegesForScope(dialect: UserAdminDialect, scope: PrivilegeScope): readonly string[] {
  if (dialect === "mysql") return MYSQL_COMMON_PRIVILEGES;
  if (scope === "database") return POSTGRES_DATABASE_PRIVILEGES;
  if (scope === "schema") return POSTGRES_SCHEMA_PRIVILEGES;
  if (scope === "table") return POSTGRES_TABLE_PRIVILEGES;
  return [];
}

function privilegeTokens(value: string, fallback = "SELECT"): string[] {
  const privileges = value
    .split(",")
    .map((item) => item.trim().toUpperCase())
    .filter(Boolean);
  return Array.from(new Set(privileges.length > 0 ? privileges : (fallback ? [fallback] : [])));
}

function normalizePrivileges(value: string, fallback = "SELECT"): string {
  return privilegeTokens(value, fallback).join(", ");
}

function grantTargetSql(dialect: UserAdminDialect, scope: PrivilegeScope, database: string, schema: string, table: string): string {
  if (dialect === "postgres") {
    if (scope === "database") return `DATABASE ${quotePostgresIdentifier(database.trim() || "postgres")}`;
    const schemaSql = quotePostgresIdentifier(schema.trim() || "public");
    if (scope === "schema") return `SCHEMA ${schemaSql}`;
    const tableName = table.trim() || "*";
    return tableName === "*" ? `ALL TABLES IN SCHEMA ${schemaSql}` : `TABLE ${schemaSql}.${quotePostgresIdentifier(tableName)}`;
  }
  const db = database.trim() || "*";
  const tableName = table.trim() || "*";
  const dbSql = db === "*" ? "*" : quoteMySqlIdentifier(db);
  const tableSql = tableName === "*" ? "*" : quoteMySqlIdentifier(tableName);
  return `${dbSql}.${tableSql}`;
}

function grantSql(
  user: DatabaseUserIdentity,
  dialect: UserAdminDialect,
  scope: PrivilegeScope,
  privilegesText: string,
  database: string,
  schema: string,
  table: string,
  role: string,
  grantOption: boolean,
): string {
  if (dialect === "postgres" && scope === "role") {
    return `GRANT ${quotePostgresIdentifier(role.trim())} TO ${postgresRole(user)}${grantOption ? " WITH ADMIN OPTION" : ""};`;
  }
  const privileges = normalizePrivileges(privilegesText, dialect === "postgres" ? postgresDefaultPrivilege(scope) : "SELECT");
  const target = grantTargetSql(dialect, scope, database, schema, table);
  const suffix = grantOption ? " WITH GRANT OPTION" : "";
  return dialect === "postgres" ? `GRANT ${privileges} ON ${target} TO ${postgresRole(user)}${suffix};` : `GRANT ${privileges} ON ${target} TO ${mysqlUserAccount(user)}${suffix};`;
}

function revokeSql(
  user: DatabaseUserIdentity,
  dialect: UserAdminDialect,
  scope: PrivilegeScope,
  privilegesText: string,
  database: string,
  schema: string,
  table: string,
  role: string,
): string {
  if (dialect === "postgres" && scope === "role") {
    return `REVOKE ${quotePostgresIdentifier(role.trim())} FROM ${postgresRole(user)};`;
  }
  const privileges = normalizePrivileges(privilegesText, dialect === "postgres" ? postgresDefaultPrivilege(scope) : "SELECT");
  const target = grantTargetSql(dialect, scope, database, schema, table);
  return dialect === "postgres" ? `REVOKE ${privileges} ON ${target} FROM ${postgresRole(user)};` : `REVOKE ${privileges} ON ${target} FROM ${mysqlUserAccount(user)};`;
}

function columnIndex(result: DbxQueryResult, ...names: string[]): number {
  const wanted = new Set(names.map((name) => name.toLowerCase()));
  return result.columns.findIndex((column) => wanted.has(column.toLowerCase()));
}

function parseUsers(result: DbxQueryResult): DatabaseUserIdentity[] {
  const userIndex = columnIndex(result, "user", "User", "rolname");
  const hostIndex = columnIndex(result, "host", "Host");
  const pluginIndex = columnIndex(result, "plugin", "Plugin");
  if (userIndex < 0) return [];
  return result.rows
    .map((row) => ({
      user: String(row[userIndex] ?? ""),
      host: hostIndex >= 0 ? String(row[hostIndex] ?? "") : "",
      plugin: pluginIndex >= 0 && row[pluginIndex] != null ? String(row[pluginIndex]) : undefined,
    }))
    .filter((user) => user.user || user.host);
}

function parseMySqlFallbackUsers(result: DbxQueryResult): DatabaseUserIdentity[] {
  const granteeIndex = columnIndex(result, "grantee", "GRANTEE");
  if (granteeIndex < 0) return [];
  return result.rows.flatMap((row) => {
    const raw = String(row[granteeIndex] ?? "").trim();
    const match = /^'((?:''|[^'])*)'@'((?:''|[^'])*)'$/.exec(raw);
    if (!match) return [];
    return [{
      user: match[1].replace(/''/g, "'"),
      host: match[2].replace(/''/g, "'"),
    }];
  });
}

function grantsText(result: DbxQueryResult): string {
  if (result.rows.length === 0) return "";
  return result.rows
    .map((row) => {
      if (row.length === 1) return String(row[0] ?? "");
      return result.columns.map((column, index) => `${column}: ${String(row[index] ?? "")}`).join(" | ");
    })
    .join("\n");
}

export function DatabaseUserAdminPanel({ connection, database, schema }: Props) {
  const { t } = useI18n();
  const dialect = userAdminDialect(connection.dbType);
  const [users, setUsers] = useState<DatabaseUserIdentity[]>([]);
  const [selectedLabel, setSelectedLabel] = useState("");
  const [grants, setGrants] = useState("");
  const [status, setStatus] = useState("");
  const [loadingUsers, setLoadingUsers] = useState(false);
  const [loadingGrants, setLoadingGrants] = useState(false);
  const [userSearch, setUserSearch] = useState("");
  const [draftUser, setDraftUser] = useState("");
  const [draftHost, setDraftHost] = useState("%");
  const [draftPassword, setDraftPassword] = useState("");
  const [draftCanLogin, setDraftCanLogin] = useState(true);
  const [privileges, setPrivileges] = useState("SELECT");
  const [grantDatabase, setGrantDatabase] = useState(database ?? "");
  const [grantSchema, setGrantSchema] = useState(schema ?? "public");
  const [grantTable, setGrantTable] = useState("");
  const [privilegeScope, setPrivilegeScope] = useState<PrivilegeScope>(dialect === "postgres" ? "database" : "mysql");
  const [privilegeRole, setPrivilegeRole] = useState("");
  const [grantOption, setGrantOption] = useState(false);
  const availablePrivileges = useMemo(() => privilegesForScope(dialect, privilegeScope), [dialect, privilegeScope]);
  const selectedPrivilegeSet = useMemo(() => new Set(privilegeTokens(privileges, "")), [privileges]);

  const selectedUser = useMemo(
    () => users.find((user) => userLabel(user, dialect) === selectedLabel) ?? users[0] ?? null,
    [dialect, selectedLabel, users],
  );
  const filteredUsers = useMemo(() => {
    const query = userSearch.trim().toLowerCase();
    if (!query) return users;
    return users.filter((user) =>
      [userLabel(user, dialect), user.user, user.host, user.plugin ?? ""]
        .join(" ")
        .toLowerCase()
        .includes(query),
    );
  }, [dialect, userSearch, users]);

  useEffect(() => {
    setGrantDatabase(database ?? "");
  }, [database]);

  useEffect(() => {
    setGrantSchema(schema ?? "public");
  }, [schema]);

  useEffect(() => {
    const nextScope = dialect === "postgres" ? "database" : "mysql";
    setPrivilegeScope(nextScope);
    setPrivileges(defaultPrivilegesForScope(dialect, nextScope).join(", "));
    setGrantTable(dialect === "postgres" ? "" : "*");
    setDraftCanLogin(true);
  }, [dialect]);

  useEffect(() => {
    if (dialect !== "postgres") return;
    setPrivileges(defaultPrivilegesForScope(dialect, privilegeScope).join(", "));
    if (privilegeScope === "database") {
      setGrantDatabase(database ?? "postgres");
      setGrantTable("");
    } else if (privilegeScope === "schema") {
      setGrantSchema(schema ?? "public");
      setGrantTable("");
    } else if (privilegeScope === "table") {
      setGrantSchema(schema ?? "public");
      setGrantTable("*");
    }
  }, [database, dialect, privilegeScope, schema]);

  function togglePrivilege(privilege: string) {
    const next = new Set(selectedPrivilegeSet);
    if (next.has(privilege)) next.delete(privilege);
    else next.add(privilege);
    setPrivileges(Array.from(next).join(", "));
  }

  async function runQuery(sql: string) {
    return databaseApi.dbxExecuteQuery({
      connectionId: connection.id,
      database,
      schema,
      sql,
      maxRows: 1000,
      pageSize: 1000,
    });
  }

  async function loadUsers() {
    setLoadingUsers(true);
    setStatus("");
    try {
      let parsed: DatabaseUserIdentity[];
      try {
        const result = await runQuery(listUsersSql(dialect));
        parsed = parseUsers(result);
      } catch (err) {
        if (dialect !== "mysql") throw err;
        const fallback = await runQuery(mysqlFallbackListUsersSql());
        parsed = parseMySqlFallbackUsers(fallback);
        setStatus(t("database.userAdminFallbackUsers"));
      }
      setUsers(parsed);
      setSelectedLabel((current) => (parsed.some((user) => userLabel(user, dialect) === current) ? current : (parsed[0] ? userLabel(parsed[0], dialect) : "")));
      if (parsed.length === 0) setGrants("");
    } catch (err) {
      setStatus(String(err));
    } finally {
      setLoadingUsers(false);
    }
  }

  async function loadGrants(user = selectedUser) {
    if (!user) return;
    setLoadingGrants(true);
    setStatus("");
    try {
      const result = await runQuery(showGrantsSql(user, dialect));
      setGrants(grantsText(result) || t("database.userAdminNoGrants"));
    } catch (err) {
      setGrants("");
      setStatus(String(err));
    } finally {
      setLoadingGrants(false);
    }
  }

  useEffect(() => {
    void loadUsers();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connection.id, connection.dbType]);

  useEffect(() => {
    if (selectedUser) void loadGrants(selectedUser);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedLabel]);

  async function executeConfirmed(sql: string, messageKey: string) {
    const accepted = await confirm(`${t(messageKey)}\n\n${sql}`, { title: t("database.previewSql"), kind: "warning" });
    if (!accepted) return;
    setStatus("");
    await databaseApi.dbxExecuteMulti({
      connectionId: connection.id,
      database,
      schema,
      sql,
      maxRows: 1000,
      pageSize: 1000,
    });
    setStatus(t("database.userAdminSqlExecuted"));
    await loadUsers();
    if (selectedUser) await loadGrants(selectedUser);
  }

  async function createUser() {
    if (!draftUser.trim() || !draftPassword) return;
    await executeConfirmed(
      createUserSql({ user: draftUser.trim(), host: draftHost.trim() || "%", password: draftPassword, canLogin: draftCanLogin }, dialect),
      "database.confirmCreateUser",
    );
    setDraftPassword("");
  }

  async function alterPassword() {
    if (!selectedUser || !draftPassword) return;
    await executeConfirmed(alterPasswordSql(selectedUser, draftPassword, dialect), "database.confirmAlterUserPassword");
    setDraftPassword("");
  }

  async function alterLogin(enabled: boolean) {
    if (!selectedUser) return;
    await executeConfirmed(alterLoginSql(selectedUser, enabled, dialect), enabled ? "database.confirmEnableUserLogin" : "database.confirmDisableUserLogin");
  }

  async function dropUser() {
    if (!selectedUser) return;
    await executeConfirmed(dropUserSql(selectedUser, dialect), "database.confirmDropUser");
  }

  async function changePrivileges(action: "grant" | "revoke") {
    if (!selectedUser) return;
    if (dialect === "postgres" && privilegeScope === "role" && !privilegeRole.trim()) return;
    const sql =
      action === "grant"
        ? grantSql(selectedUser, dialect, privilegeScope, privileges, grantDatabase, grantSchema, grantTable, privilegeRole, grantOption)
        : revokeSql(selectedUser, dialect, privilegeScope, privileges, grantDatabase, grantSchema, grantTable, privilegeRole);
    await executeConfirmed(sql, action === "grant" ? "database.confirmGrantPrivileges" : "database.confirmRevokePrivileges");
  }

  return (
    <div style={s.databaseWorkspacePanel}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>{t("database.userAdmin")}</div>
          <div style={s.databaseDialogHint}>{t("database.userAdminHint")}</div>
        </div>
        <DbxButton variant="outline" size="sm" icon={RefreshCcw} onClick={() => void loadUsers()} disabled={loadingUsers}>
          {t("database.refresh")}
        </DbxButton>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "minmax(220px, 320px) minmax(0, 1fr)", gap: 12, minHeight: 0 }}>
        <div style={{ ...s.databaseDialogPanel, minHeight: 260 }}>
          <div style={s.databaseDialogLabel}>{t("database.users")}</div>
          <label style={{ ...s.databaseDialogField, marginTop: 8 }}>
            <span style={s.databaseDialogLabel}>{t("database.userAdminSearchUser")}</span>
            <span style={{ position: "relative", display: "flex", alignItems: "center" }}>
              <Search size={13} style={{ position: "absolute", left: 8, color: "var(--text-muted)" }} />
              <input
                style={{ ...s.databaseDialogInput, paddingLeft: 28 }}
                value={userSearch}
                onChange={(event) => setUserSearch(event.target.value)}
                placeholder={t("database.userAdminSearchPlaceholder")}
              />
            </span>
          </label>
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {filteredUsers.map((user) => {
              const label = userLabel(user, dialect);
              return (
                <DbxButton
                  key={label}
                  variant="outline"
                  size="sm"
                  onClick={() => setSelectedLabel(label)}
                  style={{
                    justifyContent: "flex-start",
                    borderColor: selectedUser && userLabel(selectedUser, dialect) === label ? "var(--accent)" : "var(--border-dim)",
                  }}
                >
                  <span>{label}</span>
                  {user.plugin && <span style={{ color: "var(--text-muted)" }}>{user.plugin}</span>}
                </DbxButton>
              );
            })}
            {!loadingUsers && users.length === 0 && <div style={s.databaseDialogHint}>{t("database.userAdminNoUsers")}</div>}
            {!loadingUsers && users.length > 0 && filteredUsers.length === 0 && (
              <div style={s.databaseDialogHint}>{t("database.userAdminNoSearchResults")}</div>
            )}
          </div>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
          <div style={s.databaseDialogFormGrid}>
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.userName")}</span>
              <input style={s.databaseDialogInput} value={draftUser} onChange={(event) => setDraftUser(event.target.value)} />
            </label>
            {dialect === "mysql" && (
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.userHost")}</span>
                <input style={s.databaseDialogInput} value={draftHost} onChange={(event) => setDraftHost(event.target.value)} />
              </label>
            )}
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.password")}</span>
              <input style={s.databaseDialogInput} type="password" value={draftPassword} onChange={(event) => setDraftPassword(event.target.value)} />
            </label>
            {dialect === "postgres" && (
              <label style={{ ...s.databaseDialogField, flexDirection: "row", alignItems: "center", gap: 8 }}>
                <input type="checkbox" checked={draftCanLogin} onChange={(event) => setDraftCanLogin(event.target.checked)} />
                <span style={s.databaseDialogLabel}>{t("database.createUserCanLogin")}</span>
              </label>
            )}
          </div>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <DbxButton variant="default" size="sm" icon={UserPlus} onClick={() => void createUser()} disabled={!draftUser.trim() || !draftPassword || connection.readOnly}>
              {t("database.createUser")}
            </DbxButton>
            <DbxButton variant="outline" size="sm" icon={KeyRound} onClick={() => void alterPassword()} disabled={!selectedUser || !draftPassword || connection.readOnly}>
              {t("database.alterPassword")}
            </DbxButton>
            <DbxButton variant="outline" size="sm" icon={Lock} onClick={() => void alterLogin(false)} disabled={!selectedUser || connection.readOnly}>
              {dialect === "postgres" ? t("database.disableLogin") : t("database.lockUser")}
            </DbxButton>
            <DbxButton variant="outline" size="sm" icon={Unlock} onClick={() => void alterLogin(true)} disabled={!selectedUser || connection.readOnly}>
              {dialect === "postgres" ? t("database.enableLogin") : t("database.unlockUser")}
            </DbxButton>
            <DbxButton variant="destructive" size="sm" icon={Trash2} onClick={() => void dropUser()} disabled={!selectedUser || connection.readOnly}>
              {t("database.dropUser")}
            </DbxButton>
          </div>
          <div style={s.databaseDialogFormGrid}>
            {dialect === "postgres" && (
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.privilegeScope")}</span>
                <select style={s.databaseDialogInput} value={privilegeScope} onChange={(event) => setPrivilegeScope(event.target.value as PrivilegeScope)}>
                  <option value="database">{t("database.scopeDatabase")}</option>
                  <option value="schema">{t("database.scopeSchema")}</option>
                  <option value="table">{t("database.scopeTable")}</option>
                  <option value="role">{t("database.scopeRole")}</option>
                </select>
              </label>
            )}
            {dialect === "postgres" && privilegeScope === "role" ? (
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.memberRole")}</span>
                <input style={s.databaseDialogInput} value={privilegeRole} onChange={(event) => setPrivilegeRole(event.target.value)} />
              </label>
            ) : null}
            <label style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.privileges")}</span>
              <input style={s.databaseDialogInput} value={privileges} onChange={(event) => setPrivileges(event.target.value)} disabled={dialect === "postgres" && privilegeScope === "role"} />
            </label>
            {availablePrivileges.length > 0 && (
              <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(104px, 1fr))", gap: 6 }}>
                {availablePrivileges.map((privilege) => {
                  const selected = selectedPrivilegeSet.has(privilege);
                  return (
                    <DbxButton
                      key={privilege}
                      variant="outline"
                      size="sm"
                      aria-pressed={selected}
                      onClick={() => togglePrivilege(privilege)}
                      disabled={connection.readOnly}
                      style={{
                        justifyContent: "flex-start",
                        minHeight: 30,
                        borderColor: selected ? "var(--accent)" : "var(--border-dim)",
                        background: selected ? "var(--accent-soft)" : "var(--panel)",
                      }}
                    >
                      <span>{privilege}</span>
                    </DbxButton>
                  );
                })}
              </div>
            )}
            {!(dialect === "postgres" && privilegeScope === "role") && (
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{dialect === "postgres" && privilegeScope !== "database" ? t("database.schemaName") : t("database.databaseName")}</span>
                <input
                  style={s.databaseDialogInput}
                  value={dialect === "postgres" && privilegeScope !== "database" ? grantSchema : grantDatabase}
                  onChange={(event) => {
                    if (dialect === "postgres" && privilegeScope !== "database") setGrantSchema(event.target.value);
                    else setGrantDatabase(event.target.value);
                  }}
                />
              </label>
            )}
            {(dialect === "mysql" || privilegeScope === "table") && (
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.tableName")}</span>
                <input style={s.databaseDialogInput} value={grantTable} onChange={(event) => setGrantTable(event.target.value)} />
              </label>
            )}
            <label style={{ ...s.databaseDialogField, flexDirection: "row", alignItems: "center", gap: 8 }}>
              <input type="checkbox" checked={grantOption} onChange={(event) => setGrantOption(event.target.checked)} />
              <span style={s.databaseDialogLabel}>{dialect === "postgres" && privilegeScope === "role" ? t("database.adminOption") : t("database.grantOption")}</span>
            </label>
          </div>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <DbxButton variant="default" size="sm" icon={Play} onClick={() => void changePrivileges("grant")} disabled={!selectedUser || connection.readOnly}>
              {t("database.grantPrivileges")}
            </DbxButton>
            <DbxButton variant="outline" size="sm" icon={Play} onClick={() => void changePrivileges("revoke")} disabled={!selectedUser || connection.readOnly}>
              {t("database.revokePrivileges")}
            </DbxButton>
            <DbxButton variant="outline" size="sm" icon={RefreshCcw} onClick={() => void loadGrants()} disabled={!selectedUser || loadingGrants}>
              {t("database.refreshGrants")}
            </DbxButton>
          </div>
          {status && <div style={s.databaseDialogHint}>{status}</div>}
          <pre style={s.databaseSqlPreview}>{grants || t("database.userAdminSelectUser")}</pre>
        </div>
      </div>
    </div>
  );
}
