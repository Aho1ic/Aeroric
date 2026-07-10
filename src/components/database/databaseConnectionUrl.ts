import type { DbxDatabaseType } from "../../types";

export type ParsedConnectionUrl = {
  host?: string;
  port?: string;
  user?: string;
  password?: string;
  database?: string;
  urlParams?: string;
};

function normalizeConnectionUrl(raw: string, dbType: DbxDatabaseType): string {
  const value = raw.trim();
  if (!value) return "";
  if (value.startsWith("jdbc:")) {
    if (dbType === "postgres" && value.startsWith("jdbc:postgresql:"))
      return value.slice("jdbc:".length);
    if (dbType === "mysql" && value.startsWith("jdbc:mysql:")) return value.slice("jdbc:".length);
    if (dbType === "clickhouse" && value.startsWith("jdbc:clickhouse:"))
      return value.slice("jdbc:".length);
    if (dbType === "sqlserver" && value.startsWith("jdbc:sqlserver:"))
      return value.slice("jdbc:".length);
    if (dbType === "oracle" && value.startsWith("jdbc:oracle:")) return value;
  }
  return value;
}

function parseSqlServerConnectionUrl(raw: string): ParsedConnectionUrl | null {
  const normalized = normalizeConnectionUrl(raw, "sqlserver");
  const match = normalized.match(/^sqlserver:\/\/([^;/?]+)(.*)$/i);
  if (!match) return null;
  const hostPort = match[1] ?? "";
  const rest = match[2] ?? "";
  const [host, port] = hostPort.split(":");
  const params = Object.fromEntries(
    rest
      .split(";")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const separatorIndex = part.indexOf("=");
        return separatorIndex >= 0
          ? [part.slice(0, separatorIndex), part.slice(separatorIndex + 1)]
          : [part, ""];
      }),
  );
  const database = params.databaseName || params.database || params.initialCatalog;
  const user = params.user || params.username;
  const password = params.password;
  const urlParams = Object.entries(params)
    .filter(
      ([key]) =>
        !["databaseName", "database", "initialCatalog", "user", "username", "password"].includes(
          key,
        ),
    )
    .map(([key, value]) => `${key}=${value}`)
    .join(";");
  return {
    host: host ? decodeURIComponent(host) : undefined,
    port,
    database,
    user,
    password,
    urlParams,
  };
}

function parseOracleConnectionUrl(raw: string): ParsedConnectionUrl | null {
  const value = raw.trim();
  const serviceMatch = value.match(
    /^jdbc:oracle:thin:@\/\/([^:/?#]+)(?::(\d+))?\/([^?#]+)(?:\?(.+))?$/i,
  );
  if (serviceMatch) {
    return {
      host: decodeURIComponent(serviceMatch[1] ?? ""),
      port: serviceMatch[2],
      database: serviceMatch[3] ? decodeURIComponent(serviceMatch[3]) : undefined,
      urlParams: serviceMatch[4],
    };
  }
  const sidMatch = value.match(/^jdbc:oracle:thin:@([^:/?#]+)(?::(\d+))?:([^?#]+)(?:\?(.+))?$/i);
  if (!sidMatch) return null;
  return {
    host: decodeURIComponent(sidMatch[1] ?? ""),
    port: sidMatch[2],
    database: sidMatch[3] ? decodeURIComponent(sidMatch[3]) : undefined,
    urlParams: sidMatch[4],
  };
}

function parseStandardConnectionUrl(
  raw: string,
  dbType: DbxDatabaseType,
): ParsedConnectionUrl | null {
  const normalized = normalizeConnectionUrl(raw, dbType);
  const parsed = new URL(normalized);
  const database = parsed.pathname.replace(/^\/+/, "");
  return {
    host: parsed.hostname ? decodeURIComponent(parsed.hostname) : undefined,
    port: parsed.port,
    user: parsed.username ? decodeURIComponent(parsed.username) : undefined,
    password: parsed.password ? decodeURIComponent(parsed.password) : undefined,
    database: database ? decodeURIComponent(database) : undefined,
    urlParams: parsed.searchParams.toString(),
  };
}

export function parseConnectionUrl(
  raw: string,
  dbType: DbxDatabaseType,
): ParsedConnectionUrl | null {
  const value = raw.trim();
  if (!value) return null;
  try {
    if (dbType === "sqlserver") return parseSqlServerConnectionUrl(value);
    if (dbType === "oracle") return parseOracleConnectionUrl(value);
    return parseStandardConnectionUrl(value, dbType);
  } catch {
    return null;
  }
}
