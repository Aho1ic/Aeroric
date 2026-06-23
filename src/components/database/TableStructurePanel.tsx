import { useMemo, useState } from "react";
import { Code, Plus, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { databaseApi } from "../../lib/databaseApi";
import s from "../../styles";
import type { DbxColumnInfo, EditableStructureColumn } from "../../types/database";
import { DbxButton, DbxIconButton } from "./DbxButton";

interface Props {
  connectionId?: string;
  database?: string | null;
  schema?: string | null;
  databaseType?: string | null;
  tableName: string;
  columns: DbxColumnInfo[];
  readOnly: boolean;
}

export function TableStructurePanel({ databaseType, schema, tableName, columns, readOnly }: Props) {
  const { t } = useI18n();
  const [drafts, setDrafts] = useState<Array<{ id: string; name: string; dataType: string }>>([]);
  const [preview, setPreview] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState("");

  const editableColumns = useMemo<EditableStructureColumn[]>(() => {
    const existing = columns.map((column, index) => ({
      id: `existing:${column.name}`,
      name: column.name,
      dataType: column.data_type,
      isNullable: column.is_nullable,
      defaultValue: column.column_default ?? "",
      comment: column.comment ?? "",
      isPrimaryKey: column.is_primary_key,
      originalPosition: index,
      original: {
        name: column.name,
        data_type: column.data_type,
        is_nullable: column.is_nullable,
        column_default: column.column_default ?? null,
        is_primary_key: column.is_primary_key,
        extra: column.extra ?? null,
        comment: column.comment ?? null,
      },
      markedForDrop: false,
    }));
    const added = drafts
      .filter((draft) => draft.name.trim() && draft.dataType.trim())
      .map((draft) => ({
        id: draft.id,
        name: draft.name.trim(),
        dataType: draft.dataType.trim(),
        isNullable: true,
        defaultValue: "",
        comment: "",
        isPrimaryKey: false,
        original: null,
        markedForDrop: false,
      }));
    return [...existing, ...added];
  }, [columns, drafts]);

  const addDraft = () => setDrafts((current) => [...current, { id: `new:${Date.now()}`, name: "", dataType: "" }]);
  const updateDraft = (id: string, patch: Partial<{ name: string; dataType: string }>) =>
    setDrafts((current) => current.map((draft) => (draft.id === id ? { ...draft, ...patch } : draft)));
  const removeDraft = (id: string) => setDrafts((current) => current.filter((draft) => draft.id !== id));

  const previewSql = async () => {
    setError("");
    try {
      const result = await databaseApi.dbxBuildTableStructureChangeSql({
        databaseType: databaseType ?? null,
        schema: schema ?? null,
        tableName,
        columns: editableColumns,
        indexes: [],
        foreignKeys: [],
        triggers: [],
      });
      setPreview(result.statements.join("\n"));
      setWarnings(result.warnings);
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div style={s.databaseWorkspacePanel}>
      <div style={s.databaseWorkspaceHeader}>
        <div>
          <div style={s.databaseWorkspaceTitle}>{t("database.tableStructure")}: {tableName}</div>
          <div style={s.databaseDialogHint}>{t("database.tableStructureHint")}</div>
        </div>
        <div style={s.databaseButtonRow}>
          <DbxButton variant="outline" size="sm" icon={Plus} disabled={readOnly} onClick={addDraft}>
            {t("database.addColumn")}
          </DbxButton>
          <DbxButton variant="outline" size="sm" icon={Code} disabled={readOnly || editableColumns.length === columns.length} onClick={() => void previewSql()}>
            {t("database.previewSql")}
          </DbxButton>
        </div>
      </div>
      <div style={s.databaseTableWrap}>
        <table style={s.databaseTable}>
          <thead>
            <tr>
              <th style={s.databaseTh}>{t("database.columnName")}</th>
              <th style={s.databaseTh}>{t("database.columnType")}</th>
              <th style={s.databaseTh}>{t("database.defaultValue")}</th>
              <th style={s.databaseTh}>{t("database.columnComment")}</th>
              <th style={{ ...s.databaseTh, width: 72 }}>{t("database.actions")}</th>
            </tr>
          </thead>
          <tbody>
            {columns.map((column) => (
              <tr key={column.name}>
                <td style={s.databaseTd}>
                  {column.is_primary_key && <span title={t("database.primaryKey")} style={{ marginRight: 4 }}>🔑</span>}
                  <span style={{ fontWeight: 700 }}>{column.name}</span>
                </td>
                <td style={s.databaseTd}>{column.data_type}{column.is_nullable ? " NULL" : " NOT NULL"}</td>
                <td style={s.databaseTd}>{column.column_default ?? ""}</td>
                <td style={s.databaseTd}>{column.comment ?? ""}</td>
                <td style={s.databaseTd}>
                  <DbxIconButton icon={Trash2} size="icon-xs" disabled={readOnly} aria-label={t("common.delete")} />
                </td>
              </tr>
            ))}
            {drafts.map((draft) => (
              <tr key={draft.id}>
                <td style={s.databaseTd}>
                  <input
                    aria-label={t("database.newColumnName")}
                    style={s.databaseCellInput}
                    value={draft.name}
                    onChange={(event) => updateDraft(draft.id, { name: event.target.value })}
                  />
                </td>
                <td style={s.databaseTd}>
                  <input
                    aria-label={t("database.newColumnType")}
                    style={s.databaseCellInput}
                    value={draft.dataType}
                    onChange={(event) => updateDraft(draft.id, { dataType: event.target.value })}
                  />
                </td>
                <td style={s.databaseTd} />
                <td style={s.databaseTd} />
                <td style={s.databaseTd}>
                  <DbxIconButton icon={Trash2} size="icon-xs" onClick={() => removeDraft(draft.id)} aria-label={t("common.delete")} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {preview && <pre style={s.databaseSqlPreview}>{preview}</pre>}
      {warnings.length > 0 && <div style={s.databaseDialogHint}>{warnings.join("\n")}</div>}
      {error && <div style={s.databaseError}>{error}</div>}
      <div style={s.databaseDialogHint}>{t("database.tableStructureApplyRequiresBackend")}</div>
    </div>
  );
}
