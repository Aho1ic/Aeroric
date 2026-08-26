/**
 * 三个「值预览」对话框的展示层:列预览 / 行预览 / 单元格预览。
 *
 * 从 `DatabaseView.tsx` 抽出时保持 DOM 结构、`role` / `aria-*` 与文案 key 逐字不变,
 * 以免影响已有的 `database-view-*` 用例。
 *
 * 这一簇没有配套的 `use*` hook —— 三份预览内容、两个搜索词以及它们的派生字段本来就
 * 住在 `useDbxDataGrid` 里(在网格里点列头 / 行头 / 单元格才会写进去),这里只是读。
 * 渲染顺序按原来那三段条件表达式排:同时开着两个时的叠放次序不能变。
 */

import { Copy, Search } from "lucide-react";

import { useI18n } from "../../i18n";
import {
  dbxGridColumnType,
  dbxGridRowsToJson,
  dbxGridRowsToTsv,
  valueToText,
} from "../../lib/databaseUtils";
import s from "../../styles";
import type { DbQueryResult } from "../../types";
import { Button as DbxButton } from "../ui/Button";
import type { DbxDataGridController } from "./useDbxDataGrid";

/** 值列里的等宽预览块:列预览与行预览逐字相同。 */
const PREVIEW_VALUE_STYLE = {
  margin: 0,
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
  fontFamily: "var(--font-mono)",
} as const;

/** 「计数 + 搜索框」那一行。 */
const PREVIEW_META_ROW_STYLE = {
  display: "flex",
  alignItems: "center",
  gap: 10,
  color: "var(--text-muted)",
  fontSize: 12,
} as const;

/** 只读字段的取值文字。 */
const PREVIEW_FIELD_VALUE_STYLE = { color: "var(--text-primary)", fontSize: 12 } as const;

export interface DbxValuePreviewDialogsProps {
  /** 三份预览内容与两个搜索词都住在 useDbxDataGrid 里,这里只读不建。 */
  grid: DbxDataGridController;
  /** 列预览要按当前结果集取列类型。 */
  queryResult: DbQueryResult | null;
  /** 复制列名走 DatabaseView 的统一实现。 */
  onCopyColumnName: (name: string) => void;
  /** 复制失败时写到工作区顶部的错误条。 */
  onError: (message: string) => void;
}

/** 复制统一走这里:剪贴板不可用时静默,失败时把错误抛回错误条。 */
function copyText(text: string, onError: (message: string) => void) {
  navigator.clipboard?.writeText(text).catch((err) => onError(String(err)));
}

/** 列预览与行预览共用的搜索框:放大镜绝对定位在左侧,input 让出 28px 内边距。 */
function PreviewSearchInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const { t } = useI18n();

  return (
    <label style={{ position: "relative", marginLeft: "auto", width: 220, maxWidth: "52%" }}>
      <Search
        size={13}
        style={{
          position: "absolute",
          left: 9,
          top: "50%",
          transform: "translateY(-50%)",
          color: "var(--text-hint)",
          pointerEvents: "none",
        }}
      />
      <input
        style={{ ...s.databaseDialogInput, width: "100%", height: 28, paddingLeft: 28 }}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={t("database.detailSearchPlaceholder")}
        aria-label={t("database.detailSearchPlaceholder")}
      />
    </label>
  );
}

function DbxColumnPreviewDialog({
  grid,
  queryResult,
  onCopyColumnName,
  onError,
}: DbxValuePreviewDialogsProps) {
  const { t } = useI18n();
  const {
    dbxColumnPreview,
    setDbxColumnPreview,
    dbxColumnPreviewSearch,
    setDbxColumnPreviewSearch,
  } = grid.state;
  const { dbxColumnPreviewFields, filteredDbxColumnPreviewFields } = grid.derived;
  if (!dbxColumnPreview) return null;

  // 关闭要连搜索词一起清:留着的话下次点开另一列会带着上一列的过滤条件。
  const close = () => {
    setDbxColumnPreview(null);
    setDbxColumnPreviewSearch("");
  };

  return (
    <div
      style={s.databaseDialogOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("database.columnDetailsFor", { column: dbxColumnPreview.column })}
        style={{ ...s.databaseConnectionDialog, width: 720, maxWidth: "min(94vw, 720px)" }}
      >
        <div style={s.databaseDialogHeader}>
          {t("database.columnDetailsFor", { column: dbxColumnPreview.column })}
        </div>
        <div style={s.databaseDialogBody}>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 10 }}>
            <div style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.columnName")}</span>
              <div style={PREVIEW_FIELD_VALUE_STYLE}>{dbxColumnPreview.column}</div>
            </div>
            <div style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.columnType")}</span>
              <div style={PREVIEW_FIELD_VALUE_STYLE}>
                {dbxGridColumnType(queryResult, dbxColumnPreview.columnIndex) ?? "-"}
              </div>
            </div>
            <div style={s.databaseDialogField}>
              <span style={s.databaseDialogLabel}>{t("database.rowCount")}</span>
              <div style={PREVIEW_FIELD_VALUE_STYLE}>{dbxColumnPreviewFields.length}</div>
            </div>
          </div>
          <div style={PREVIEW_META_ROW_STYLE}>
            <span style={{ whiteSpace: "nowrap" }}>
              {t("database.rowCount")}: {dbxColumnPreviewFields.length}
            </span>
            <PreviewSearchInput
              value={dbxColumnPreviewSearch}
              onChange={setDbxColumnPreviewSearch}
            />
          </div>
          <div style={{ ...s.databaseTableWrap, maxHeight: "52vh" }}>
            <table style={{ ...s.databaseTable, minWidth: 520 }}>
              <thead>
                <tr>
                  <th style={s.databaseTh}>{t("database.rowNumber")}</th>
                  <th style={s.databaseTh}>{t("database.cellValue")}</th>
                  <th style={s.databaseTh} />
                </tr>
              </thead>
              <tbody>
                {filteredDbxColumnPreviewFields.map((field) => (
                  <tr key={`${dbxColumnPreview.column}:${field.rowNumber}`}>
                    <td style={s.databaseTd}>{field.rowNumber}</td>
                    <td style={s.databaseTd}>
                      <pre style={PREVIEW_VALUE_STYLE}>{field.preview}</pre>
                    </td>
                    <td style={{ ...s.databaseTd, width: 44 }}>
                      <DbxButton
                        variant="ghost"
                        size="icon-sm"
                        icon={Copy}
                        aria-label={t("database.copyRowValue", { row: field.rowNumber })}
                        title={t("database.copyRowValue", { row: field.rowNumber })}
                        onClick={() => copyText(field.preview, onError)}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {dbxColumnPreviewSearch && filteredDbxColumnPreviewFields.length === 0 && (
              <div style={s.databaseEmptyCompact}>{t("database.detailSearchNoMatch")}</div>
            )}
          </div>
        </div>
        <div style={s.databaseDialogFooter}>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Copy}
            onClick={() =>
              copyText(
                JSON.stringify(
                  dbxColumnPreviewFields.map((field) => ({
                    row: field.rowNumber,
                    value: field.value,
                  })),
                  null,
                  2,
                ),
                onError,
              )
            }
          >
            {t("database.copyColumnValues")}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Copy}
            onClick={() =>
              copyText(
                dbxColumnPreviewFields.map((field) => valueToText(field.value)).join("\n"),
                onError,
              )
            }
          >
            {t("database.copyColumnTsv")}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Copy}
            onClick={() => onCopyColumnName(dbxColumnPreview.column)}
          >
            {t("database.copyColumnName")}
          </DbxButton>
          <DbxButton variant="outline" size="sm" onClick={close}>
            {t("common.close")}
          </DbxButton>
        </div>
      </section>
    </div>
  );
}

function DbxRowPreviewDialog({
  grid,
  onError,
}: Pick<DbxValuePreviewDialogsProps, "grid" | "onError">) {
  const { t } = useI18n();
  const { dbxRowPreview, setDbxRowPreview, dbxRowPreviewSearch, setDbxRowPreviewSearch } =
    grid.state;
  const { visibleTableColumns, dbxRowPreviewFields, filteredDbxRowPreviewFields } = grid.derived;
  if (!dbxRowPreview) return null;

  const close = () => {
    setDbxRowPreview(null);
    setDbxRowPreviewSearch("");
  };

  return (
    <div
      style={s.databaseDialogOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("database.rowDetailsFor", { row: dbxRowPreview.rowIndex + 1 })}
        style={{ ...s.databaseConnectionDialog, width: 760, maxWidth: "min(94vw, 760px)" }}
      >
        <div style={s.databaseDialogHeader}>
          {t("database.rowDetailsFor", { row: dbxRowPreview.rowIndex + 1 })}
        </div>
        <div style={s.databaseDialogBody}>
          <div style={PREVIEW_META_ROW_STYLE}>
            <span style={{ whiteSpace: "nowrap" }}>
              {t("database.columnsCount", { count: dbxRowPreviewFields.length })}
            </span>
            <PreviewSearchInput value={dbxRowPreviewSearch} onChange={setDbxRowPreviewSearch} />
          </div>
          <div style={{ ...s.databaseTableWrap, maxHeight: "52vh" }}>
            <table style={{ ...s.databaseTable, minWidth: 620 }}>
              <thead>
                <tr>
                  <th style={s.databaseTh}>{t("database.fieldIndex")}</th>
                  <th style={s.databaseTh}>{t("database.columnName")}</th>
                  <th style={s.databaseTh}>{t("database.cellValue")}</th>
                  <th style={s.databaseTh} />
                </tr>
              </thead>
              <tbody>
                {filteredDbxRowPreviewFields.map((field, index) => (
                  <tr key={`${index}:${field.column}`}>
                    <td style={s.databaseTd}>{index + 1}</td>
                    <td style={s.databaseTd}>
                      <div style={{ color: "var(--text-primary)" }}>{field.column}</div>
                      <div style={{ color: "var(--text-hint)", fontSize: 11 }}>
                        {field.type ?? "-"}
                      </div>
                    </td>
                    <td style={s.databaseTd}>
                      <pre style={PREVIEW_VALUE_STYLE}>{field.preview}</pre>
                    </td>
                    <td style={{ ...s.databaseTd, width: 44 }}>
                      <DbxButton
                        variant="ghost"
                        size="icon-sm"
                        icon={Copy}
                        aria-label={t("database.copyFieldValue", { column: field.column })}
                        title={t("database.copyFieldValue", { column: field.column })}
                        onClick={() => copyText(field.preview, onError)}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {dbxRowPreviewSearch && filteredDbxRowPreviewFields.length === 0 && (
              <div style={s.databaseEmptyCompact}>{t("database.detailSearchNoMatch")}</div>
            )}
          </div>
        </div>
        <div style={s.databaseDialogFooter}>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Copy}
            onClick={() =>
              copyText(dbxGridRowsToJson(visibleTableColumns, [dbxRowPreview.row]), onError)
            }
          >
            {t("database.copyRow")}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Copy}
            onClick={() =>
              copyText(dbxGridRowsToTsv(visibleTableColumns, [dbxRowPreview.row]), onError)
            }
          >
            {t("database.copyRowTsv")}
          </DbxButton>
          <DbxButton variant="outline" size="sm" onClick={close}>
            {t("common.close")}
          </DbxButton>
        </div>
      </section>
    </div>
  );
}

function DbxCellPreviewDialog({
  grid,
  onCopyColumnName,
  onError,
}: Pick<DbxValuePreviewDialogsProps, "grid" | "onCopyColumnName" | "onError">) {
  const { t } = useI18n();
  const { dbxCellPreview, setDbxCellPreview } = grid.state;
  const { formattedDbxCellPreview } = grid.derived;
  if (!dbxCellPreview || !formattedDbxCellPreview) return null;

  // 这一支没有搜索框,关闭只清内容。
  const close = () => setDbxCellPreview(null);
  // Mongo 文档会被格式化成 JSON,标签和 aria-label 都要跟着换。
  const valueLabel = formattedDbxCellPreview.json
    ? t("database.documentJson")
    : t("database.cellValue");

  return (
    <div
      style={s.databaseDialogOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("database.cellValuePreview")}
        style={{ ...s.databaseConnectionDialog, width: 640, maxWidth: "min(92vw, 640px)" }}
      >
        <div style={s.databaseDialogHeader}>{t("database.cellValuePreview")}</div>
        <div style={s.databaseDialogBody}>
          <div style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{t("database.columnName")}</span>
            <div style={PREVIEW_FIELD_VALUE_STYLE}>{dbxCellPreview.column}</div>
          </div>
          <label style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{valueLabel}</span>
            <textarea
              style={{ ...s.databaseSqlInput, minHeight: 280, resize: "vertical" }}
              readOnly
              value={formattedDbxCellPreview.text}
              aria-label={valueLabel}
            />
          </label>
        </div>
        <div style={s.databaseDialogFooter}>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Copy}
            onClick={() => copyText(formattedDbxCellPreview.text, onError)}
          >
            {t("database.copyValue")}
          </DbxButton>
          <DbxButton
            variant="outline"
            size="sm"
            icon={Copy}
            onClick={() => onCopyColumnName(dbxCellPreview.column)}
          >
            {t("database.copyColumnName")}
          </DbxButton>
          <DbxButton variant="outline" size="sm" onClick={close}>
            {t("common.close")}
          </DbxButton>
        </div>
      </section>
    </div>
  );
}

export function DbxValuePreviewDialogs({
  grid,
  queryResult,
  onCopyColumnName,
  onError,
}: DbxValuePreviewDialogsProps) {
  return (
    <>
      <DbxColumnPreviewDialog
        grid={grid}
        queryResult={queryResult}
        onCopyColumnName={onCopyColumnName}
        onError={onError}
      />
      <DbxRowPreviewDialog grid={grid} onError={onError} />
      <DbxCellPreviewDialog grid={grid} onCopyColumnName={onCopyColumnName} onError={onError} />
    </>
  );
}
