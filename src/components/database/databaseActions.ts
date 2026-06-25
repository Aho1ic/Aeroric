import {
  Braces,
  Code2,
  Database,
  Download,
  FileCode,
  FileUp,
  GitCompare,
  GitMerge,
  GitPullRequest,
  Network,
  PencilRuler,
  Play,
  Plus,
  RefreshCcw,
  Save,
  Shield,
  Square,
  Table2,
  Trash2,
  Upload,
  Wrench,
} from "lucide-react";
import type { ComponentType } from "react";
import type { DbxDatabaseType } from "../../types/database";

export type DatabaseActionId =
  | "newConnection"
  | "newQuery"
  | "execute"
  | "cancel"
  | "explain"
  | "formatSql"
  | "saveSql"
  | "openSql"
  | "executeSqlFile"
  | "importResultArchive"
  | "driverManager"
  | "refresh"
  | "tableImport"
  | "tableExport"
  | "databaseExport"
  | "dataTransfer"
  | "schemaDiff"
  | "dataCompare"
  | "erDiagram"
  | "tableStructure"
  | "redisCreateKey"
  | "redisDeleteKey"
  | "mongoInsertDocument"
  | "mongoDeleteDocument";

export interface DatabaseActionContext {
  hasConnection: boolean;
  dbType?: DbxDatabaseType | null;
  hasSql: boolean;
  isExecuting: boolean;
  isExplaining: boolean;
  hasSelectedTable: boolean;
  hasResult: boolean;
  readOnly: boolean;
}

export interface DatabaseActionDefinition {
  id: DatabaseActionId;
  labelKey: string;
  icon: ComponentType<{ size?: number; className?: string }>;
  group: "connection" | "query" | "grid" | "tools" | "redis" | "mongo";
}

export const DATABASE_ACTIONS: DatabaseActionDefinition[] = [
  { id: "newConnection", labelKey: "database.newConnection", icon: Plus, group: "connection" },
  { id: "newQuery", labelKey: "database.newQuery", icon: Code2, group: "query" },
  { id: "execute", labelKey: "database.execute", icon: Play, group: "query" },
  { id: "cancel", labelKey: "database.cancel", icon: Square, group: "query" },
  { id: "explain", labelKey: "database.explain", icon: GitPullRequest, group: "query" },
  { id: "formatSql", labelKey: "database.formatSql", icon: Wrench, group: "query" },
  { id: "saveSql", labelKey: "database.saveSql", icon: Save, group: "query" },
  { id: "openSql", labelKey: "database.openSql", icon: FileCode, group: "query" },
  { id: "executeSqlFile", labelKey: "database.executeSqlFile", icon: FileUp, group: "query" },
  {
    id: "importResultArchive",
    labelKey: "database.importResultArchive",
    icon: Upload,
    group: "query",
  },
  { id: "driverManager", labelKey: "database.driverManager", icon: Shield, group: "tools" },
  { id: "refresh", labelKey: "database.refresh", icon: RefreshCcw, group: "connection" },
  { id: "tableImport", labelKey: "database.tableImport", icon: Upload, group: "grid" },
  { id: "tableExport", labelKey: "database.tableExport", icon: Download, group: "grid" },
  { id: "databaseExport", labelKey: "database.databaseExport", icon: Database, group: "grid" },
  { id: "dataTransfer", labelKey: "database.dataTransfer", icon: GitMerge, group: "tools" },
  { id: "schemaDiff", labelKey: "database.schemaDiff", icon: GitCompare, group: "tools" },
  { id: "dataCompare", labelKey: "database.dataCompare", icon: Network, group: "tools" },
  { id: "erDiagram", labelKey: "database.erDiagram", icon: Table2, group: "tools" },
  { id: "tableStructure", labelKey: "database.tableStructure", icon: PencilRuler, group: "grid" },
  { id: "redisCreateKey", labelKey: "database.redisCreateKey", icon: Braces, group: "redis" },
  { id: "redisDeleteKey", labelKey: "database.redisDeleteKey", icon: Trash2, group: "redis" },
  {
    id: "mongoInsertDocument",
    labelKey: "database.mongoInsertDocument",
    icon: Braces,
    group: "mongo",
  },
  {
    id: "mongoDeleteDocument",
    labelKey: "database.mongoDeleteDocument",
    icon: Trash2,
    group: "mongo",
  },
];

const sqlTypes = new Set<DbxDatabaseType>([
  "sqlite",
  "mysql",
  "postgres",
  "duckdb",
  "sqlserver",
  "oracle",
  "clickhouse",
]);

export function isSqlDatabase(dbType?: DbxDatabaseType | null): boolean {
  return !!dbType && sqlTypes.has(dbType);
}

export function isDatabaseActionEnabled(
  id: DatabaseActionId,
  context: DatabaseActionContext,
): boolean {
  if (id === "newConnection" || id === "driverManager") return true;
  if (!context.hasConnection) return false;
  if (id === "cancel") return context.isExecuting || context.isExplaining;
  if (id === "execute")
    return isSqlDatabase(context.dbType) && context.hasSql && !context.isExecuting;
  if (id === "explain") {
    return (
      isSqlDatabase(context.dbType) &&
      context.hasSql &&
      !context.isExecuting &&
      !context.isExplaining
    );
  }
  if (id === "formatSql" || id === "saveSql")
    return isSqlDatabase(context.dbType) && context.hasSql;
  if (
    id === "openSql" ||
    id === "executeSqlFile" ||
    id === "newQuery" ||
    id === "importResultArchive"
  ) {
    return isSqlDatabase(context.dbType);
  }
  if (id === "refresh") return true;
  if (id === "tableExport" || id === "databaseExport" || id === "erDiagram") {
    return isSqlDatabase(context.dbType) && context.hasSelectedTable;
  }
  if (id === "schemaDiff" || id === "dataCompare" || id === "dataTransfer")
    return isSqlDatabase(context.dbType);
  if (id === "tableImport" || id === "tableStructure") {
    return isSqlDatabase(context.dbType) && context.hasSelectedTable && !context.readOnly;
  }
  if (id === "redisCreateKey" || id === "redisDeleteKey")
    return context.dbType === "redis" && !context.readOnly;
  if (id === "mongoInsertDocument" || id === "mongoDeleteDocument")
    return context.dbType === "mongodb" && !context.readOnly;
  return false;
}
