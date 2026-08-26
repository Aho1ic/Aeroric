/**
 * 「表属性」面板的展示层。状态与派生值全部来自 `useTableInfoPanel`,
 * 这里不持有任何 state —— 从 `DatabaseView.tsx` 抽出时保持 DOM 结构、
 * data-testid、aria-label 与文案 key 逐字不变,以免影响已有的 `database-view-*` 用例。
 */

import { Columns3, FileCode, Hash, KeyRound, RefreshCcw, Search, Zap } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { AnimatedSelectionGroup } from "../ui/AnimatedSelection";
import { Button as DbxButton } from "../ui/Button";
import { renderSqlTokens } from "./DatabaseViewPrimitives";
import type { TableInfoPanelState } from "./useTableInfoPanel";

export function TableInfoPanel({
  state,
  onViewDdl,
}: {
  state: TableInfoPanelState;
  /** 头部「查看 DDL」按钮:打开的是 DatabaseView 那个全局 DDL 对话框,不是本面板的 tab。 */
  onViewDdl: () => void;
}) {
  const { t } = useI18n();
  // 非 columns / ddl 的三个 tab 共用同一张对象表,行数据只差这一份列表。
  const objectRows =
    state.activeTab === "indexes"
      ? state.filteredIndexes
      : state.activeTab === "foreignKeys"
        ? state.filteredForeignKeys
        : state.filteredTriggers;

  return (
    <div style={s.databaseTableInfoRoot}>
      <div style={s.databaseTableInfoHeader}>
        <span style={{ position: "relative", display: "flex", alignItems: "center", flex: 1 }}>
          <Search
            aria-hidden="true"
            size={14}
            style={{ position: "absolute", left: 9, color: "var(--text-hint)" }}
          />
          <input
            style={{ ...s.databaseDialogInput, paddingLeft: 30, minWidth: 220 }}
            value={state.search}
            onChange={(event) => state.setSearch(event.target.value)}
            placeholder={t("database.searchPlaceholder")}
            aria-label="Search table info"
          />
        </span>
        <DbxButton variant="outline" size="sm" icon={FileCode} onClick={onViewDdl}>
          {t("database.viewDdl")}
        </DbxButton>
      </div>
      <AnimatedSelectionGroup
        value={state.activeTab}
        onChange={state.setActiveTab}
        ariaLabel={t("database.tableInfoSections")}
        role="tablist"
        variant="underline"
        style={s.databaseTableInfoTabs}
        options={[
          {
            key: "columns" as const,
            label: t("database.columns"),
            count: state.columns.length,
            icon: <Columns3 size={14} aria-hidden="true" />,
          },
          {
            key: "indexes" as const,
            label: t("database.indexes"),
            count: state.indexes.length,
            icon: <Hash size={14} aria-hidden="true" />,
          },
          {
            key: "foreignKeys" as const,
            label: t("database.foreignKeys"),
            count: state.foreignKeys.length,
            icon: <KeyRound size={14} aria-hidden="true" />,
          },
          {
            key: "triggers" as const,
            label: t("database.triggers"),
            count: state.triggers.length,
            icon: <Zap size={14} aria-hidden="true" />,
          },
          {
            key: "ddl" as const,
            label: t("database.ddl"),
            count: state.ddl ? 1 : 0,
            icon: <FileCode size={14} aria-hidden="true" />,
          },
        ].map((tab) => ({
          value: tab.key,
          ariaLabel: `${tab.label} ${tab.count}`,
          label: (
            <>
              {tab.icon}
              <span>{tab.label}</span>
              <span>{tab.count}</span>
            </>
          ),
        }))}
        itemStyle={{ ...s.databaseTableInfoTab, border: "none" }}
      />
      <div style={s.databaseTableInfoContent} role="tabpanel">
        {state.activeTab === "ddl" ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: 12 }}>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                gap: 8,
                alignItems: "center",
              }}
            >
              <span style={s.databaseDialogHint}>
                {state.ddlLoading ? t("database.loading") : t("database.ddl")}
              </span>
              <DbxButton
                variant="outline"
                size="xs"
                icon={RefreshCcw}
                disabled={state.ddlLoading}
                onClick={() => void state.loadDdl()}
              >
                {t("common.refresh")}
              </DbxButton>
            </div>
            <pre
              data-testid={!state.ddlLoading && state.ddl ? "database-ddl-highlight" : undefined}
              style={{ ...s.databaseSqlPreview, margin: 0, minHeight: 180 }}
            >
              {state.ddlLoading
                ? t("database.loading")
                : state.ddl
                  ? renderSqlTokens(state.ddl)
                  : t("database.empty")}
            </pre>
            {state.ddlError && <div style={s.databaseError}>{state.ddlError}</div>}
          </div>
        ) : state.activeTab === "columns" ? (
          <table style={s.databaseTable}>
            <thead>
              <tr>
                <th style={s.databaseTh}>{t("database.columnName")}</th>
                <th style={s.databaseTh}>{t("database.columnType")}</th>
                <th style={s.databaseTh}>{t("database.defaultValue")}</th>
                <th style={s.databaseTh}>{t("database.columnComment")}</th>
              </tr>
            </thead>
            <tbody>
              {state.filteredColumns.map((column) => (
                <tr key={column.name}>
                  <td style={s.databaseTd}>
                    {column.is_primary_key && (
                      <span title={t("database.primaryKey")} style={{ marginRight: 4 }}>
                        🔑
                      </span>
                    )}
                    <span style={{ fontWeight: 700 }}>{column.name}</span>
                  </td>
                  <td style={s.databaseTd}>
                    {column.data_type}
                    {column.is_nullable ? " NULL" : " NOT NULL"}
                  </td>
                  <td style={s.databaseTd}>{column.column_default ?? "-"}</td>
                  <td style={s.databaseTd}>{column.comment ?? "-"}</td>
                </tr>
              ))}
              {state.filteredColumns.length === 0 && (
                <tr>
                  <td style={s.databaseTd} colSpan={4}>
                    {t("database.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        ) : (
          <table style={s.databaseTable}>
            <thead>
              <tr>
                <th style={s.databaseTh}>{t("database.objectName")}</th>
                <th style={s.databaseTh}>{t("database.objectType")}</th>
                <th style={s.databaseTh}>{t("database.schemaName")}</th>
              </tr>
            </thead>
            <tbody>
              {objectRows.map((object) => (
                <tr key={`${object.object_type}:${object.name}`}>
                  <td style={s.databaseTd}>{object.name}</td>
                  <td style={s.databaseTd}>{object.object_type}</td>
                  <td style={s.databaseTd}>{object.schema || "-"}</td>
                </tr>
              ))}
              {objectRows.length === 0 && (
                <tr>
                  <td style={s.databaseTd} colSpan={3}>
                    {t("database.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
