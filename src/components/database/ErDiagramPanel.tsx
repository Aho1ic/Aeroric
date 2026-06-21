import { Download } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import type { DbxColumnInfo, DbxObjectInfo } from "../../types/database";
import { DbxButton } from "./DbxButton";

interface Props {
  tables: DbxObjectInfo[];
  columnsByTable: Record<string, DbxColumnInfo[]>;
}

export function ErDiagramPanel({ tables, columnsByTable }: Props) {
  const { t } = useI18n();

  return (
    <div style={s.databaseWorkspacePanel}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>{t("database.erDiagram")}</div>
          <div style={s.databaseDialogHint}>{t("database.erDiagramHint")}</div>
        </div>
        <DbxButton variant="outline" size="sm" icon={Download} disabled>
          {t("database.exportSvg")}
        </DbxButton>
      </div>
      <div style={s.databaseDiagramCanvas}>
        {tables.map((table, index) => {
          const key = table.schema ? `${table.schema}.${table.name}` : table.name;
          const columns = columnsByTable[key] ?? columnsByTable[table.name] ?? [];
          return (
            <div key={key} style={{ ...s.databaseDiagramNode, marginLeft: (index % 3) * 20 }}>
              <div style={s.databaseDiagramNodeTitle}>{table.name}</div>
              {columns.length === 0 ? (
                <div style={s.databaseDiagramColumn}>{t("database.columnsNotLoaded")}</div>
              ) : (
                columns.map((column) => (
                  <div key={column.name} style={s.databaseDiagramColumn}>
                    {column.is_primary_key ? "* " : ""}
                    {column.name}
                    <span style={{ color: "var(--text-hint)" }}> {column.data_type}</span>
                  </div>
                ))
              )}
            </div>
          );
        })}
        {tables.length === 0 && <div style={s.databaseEmpty}>{t("database.empty")}</div>}
      </div>
    </div>
  );
}
