/**
 * 「新建数据库」与「新建 schema」两个对话框的展示层。状态与动作来自
 * `useCreateContainerDialogs.ts`,这里不持有任何 state —— 从 `DatabaseView.tsx`
 * 抽出时保持 DOM 结构、aria-label 与文案 key 逐字不变,以免影响已有的
 * `database-view-*` 用例。
 */

import { Plus } from "lucide-react";

import { useI18n } from "../../i18n";
import s from "../../styles";
import { DialogFooterButton as DbxDialogFooterButton } from "../ui/Button";
import type {
  CreateDatabaseDialogState,
  CreateSchemaDialogState,
} from "./useCreateContainerDialogs";

export function CreateDatabaseDialog({
  state,
  busy,
}: {
  state: CreateDatabaseDialogState;
  /** DatabaseView 的全局忙碌态:提交期间要一起禁用按钮。 */
  busy: boolean;
}) {
  const { t } = useI18n();
  if (!state.connection) return null;

  return (
    <div
      style={s.databaseDialogOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) state.close();
      }}
    >
      <form
        role="dialog"
        aria-modal="true"
        aria-label={t("database.createDatabase")}
        style={s.databaseDialog}
        onSubmit={(event) => {
          event.preventDefault();
          void state.submit();
        }}
      >
        <div style={s.databaseDialogHeader}>{t("database.createDatabase")}</div>
        <div style={s.databaseDialogBody}>
          <div style={s.databaseDialogHint}>{t("database.createDatabaseHint")}</div>
          <label style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{t("database.createDatabaseName")}</span>
            <input
              aria-label={t("database.createDatabaseName")}
              style={s.databaseDialogInput}
              value={state.name}
              onChange={(event) => state.setName(event.target.value)}
              autoFocus
            />
          </label>
          {state.supportsCharset && (
            <div style={s.databaseDialogFormGrid}>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.charset")}</span>
                <input
                  aria-label={t("database.charset")}
                  style={s.databaseDialogInput}
                  value={state.charset}
                  onChange={(event) => state.setCharset(event.target.value)}
                />
              </label>
              <label style={s.databaseDialogField}>
                <span style={s.databaseDialogLabel}>{t("database.collation")}</span>
                <input
                  aria-label={t("database.collation")}
                  style={s.databaseDialogInput}
                  value={state.collation}
                  onChange={(event) => state.setCollation(event.target.value)}
                />
              </label>
            </div>
          )}
        </div>
        <div style={s.databaseDialogFooter}>
          <DbxDialogFooterButton type="button" onClick={state.close} disabled={busy}>
            {t("common.cancel")}
          </DbxDialogFooterButton>
          <DbxDialogFooterButton
            type="submit"
            variant="default"
            icon={Plus}
            disabled={busy || !state.name.trim()}
          >
            {t("database.createDatabase")}
          </DbxDialogFooterButton>
        </div>
      </form>
    </div>
  );
}

export function CreateSchemaDialog({
  state,
  busy,
}: {
  state: CreateSchemaDialogState;
  busy: boolean;
}) {
  const { t } = useI18n();
  const { target, connection } = state;
  if (!target || !connection) return null;

  return (
    <div
      style={s.databaseDialogOverlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) state.close();
      }}
    >
      <form
        role="dialog"
        aria-modal="true"
        aria-label={t("database.createSchema")}
        style={s.databaseDialog}
        onSubmit={(event) => {
          event.preventDefault();
          void state.submit();
        }}
      >
        <div style={s.databaseDialogHeader}>{t("database.createSchema")}</div>
        <div style={s.databaseDialogBody}>
          <div style={s.databaseDialogHint}>
            {t("database.createSchemaHint", { database: target.database })}
          </div>
          <label style={s.databaseDialogField}>
            <span style={s.databaseDialogLabel}>{t("database.schemaName")}</span>
            <input
              aria-label={t("database.schemaName")}
              style={s.databaseDialogInput}
              value={state.name}
              onChange={(event) => state.setName(event.target.value)}
              autoFocus
            />
          </label>
        </div>
        <div style={s.databaseDialogFooter}>
          <DbxDialogFooterButton type="button" onClick={state.close} disabled={busy}>
            {t("common.cancel")}
          </DbxDialogFooterButton>
          <DbxDialogFooterButton
            type="submit"
            variant="default"
            icon={Plus}
            disabled={busy || !state.name.trim()}
          >
            {t("database.createSchema")}
          </DbxDialogFooterButton>
        </div>
      </form>
    </div>
  );
}
