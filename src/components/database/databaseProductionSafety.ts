import { confirm } from "@tauri-apps/plugin-dialog";
import { databaseApi } from "../../lib/databaseApi";
import type { AeroricDbConnectionConfig } from "../../types";
import { dbxBoolean, dbxConfigRecord } from "./databaseConnectionDraft";

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

export function hasProductionProtection(connection: AeroricDbConnectionConfig): boolean {
  const config = dbxConfigRecord(connection);
  return (
    dbxBoolean(config, "is_production") ||
    (Array.isArray(config.production_databases) && config.production_databases.length > 0)
  );
}

export async function confirmDbxProductionOperation(options: {
  connection: AeroricDbConnectionConfig;
  database?: string | null;
  operation: string;
  okLabel: string;
  t: TranslateFn;
}): Promise<boolean> {
  const { connection, database, operation, okLabel, t } = options;
  if (!hasProductionProtection(connection)) return true;

  const assessment = await databaseApi.dbxAssessProductionTarget({
    connectionId: connection.id,
    database,
  });
  if (!assessment.requiresConfirmation) return true;

  const productionScope =
    assessment.productionDatabases.length > 0
      ? assessment.productionDatabases.join(", ")
      : t("database.productionEntireConnection");
  return confirm(
    t("database.productionOperationWarning", {
      connection: connection.name,
      databases: productionScope,
      operation,
    }),
    {
      title: t("database.productionOperationWarningTitle"),
      kind: "warning",
      okLabel,
      cancelLabel: t("common.cancel"),
    },
  );
}
