/* 导入面板:从第三方笔记应用把笔记搬进当前笔记库。
 *
 * 壳照 `NoteExportSheet` 那一套(同样的浮层定位、同样的 Esc 处理、同样的按钮样式)——
 * 两个面板是同一类操作的两个方向,外观不该有差别。
 *
 * 只画 UI。provider 清单、对话框、命令调用、报告读成文案都在 `noteImport.ts`。
 *
 * 报告区有一处刻意的取舍:**计数和明细分开显示,而且不把计数加在一起**。
 * `resourceLost` / `degraded` 是跨状态的「受影响条目数」,一条成功导入的笔记也可能
 * 算在里面 —— 汇总成一个总数会得出比真实条目还大的数字,而这个面板唯一的价值就是
 * 让用户能对账。
 */

import { useEffect, useRef } from "react";
import { zLayers } from "../../styles/zLayers";
import type { ImportReport } from "./notebookApi";
import {
  importIssueText,
  importStatusText,
  importSummary,
  type ImportProvider,
  type ImportProviderId,
} from "./noteImport";

export type NoteImportSheetProps = {
  /** 当前平台可用的 provider。过滤在 `availableImportProviders` 里做,不在这里。 */
  providers: readonly ImportProvider[];
  /** 正在跑的 provider。非 null 时全部按钮禁用。 */
  busy: ImportProviderId | null;
  /** 上一次导入的报告。null = 还没跑过。 */
  report: ImportReport | null;
  error: string | null;
  onRun: (provider: ImportProvider) => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

/** 明细最多画这么多行。报告本身已经在后端截过(2000 条),这里再收一道是为了
 *  DOM 大小 —— 2000 行会让面板滚起来发卡。全量始终在落盘的那篇报告笔记里。 */
const VISIBLE_ITEMS = 50;

export function NoteImportSheet({
  providers,
  busy,
  report,
  error,
  onRun,
  onClose,
  t,
}: NoteImportSheetProps) {
  const firstRef = useRef<HTMLButtonElement | null>(null);

  // 开窗聚焦到第一项:键盘用户不该先按一串 Tab 才能到内容上。
  useEffect(() => {
    firstRef.current?.focus();
  }, []);

  const running = busy !== null;
  const hidden = report ? report.items.length - VISIBLE_ITEMS : 0;

  return (
    <div
      role="dialog"
      aria-label={t("notebook.importTitle")}
      data-notebook-context-menu
      onKeyDown={(event) => {
        // IME 组字中的 Escape 是「取消候选词」,不是「关窗」。
        if (event.nativeEvent.isComposing) return;
        if (event.key === "Escape") {
          // 不往上冒:面板和宿主也有 Esc,一次按键不该关两层。
          event.stopPropagation();
          onClose();
        }
      }}
      style={{
        position: "absolute",
        left: "50%",
        top: 40,
        transform: "translateX(-50%)",
        zIndex: zLayers.contextMenu,
        width: 420,
        maxWidth: "90%",
        maxHeight: "80%",
        padding: 12,
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        background: "var(--bg-sidebar)",
        boxShadow: "var(--shadow-popover)",
        display: "flex",
        flexDirection: "column",
        gap: 10,
        overflowY: "auto",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-primary)" }}>
          {t("notebook.importTitle")}
        </span>
        <span style={{ flex: 1, fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
          {t("notebook.importSheetDescription")}
        </span>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        {providers.map((provider, index) => {
          const isBusy = busy === provider.id;
          return (
            <button
              key={provider.id}
              ref={index === 0 ? firstRef : undefined}
              type="button"
              disabled={running}
              onClick={() => onRun(provider)}
              style={{
                display: "flex",
                flexDirection: "column",
                alignItems: "flex-start",
                gap: 1,
                padding: "5px 8px",
                border: "1px solid var(--border-dim)",
                borderRadius: 5,
                background: isBusy ? "var(--control-active-bg)" : "var(--bg-card)",
                color: running && !isBusy ? "var(--text-muted)" : "var(--text-primary)",
                cursor: running ? "not-allowed" : "pointer",
                opacity: running && !isBusy ? 0.55 : 1,
                textAlign: "left",
              }}
            >
              <span style={{ fontSize: 11.5 }}>
                {t(provider.labelKey)}
                {isBusy ? ` · ${t("notebook.importRunning")}` : ""}
              </span>
              <span style={{ fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
                {t(provider.hintKey)}
              </span>
            </button>
          );
        })}
      </div>

      {report ? (
        <>
          <div style={{ height: 1, background: "var(--border-dim)" }} />
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {/* 汇总用 role="status":它会变,但不该像 alert 那样打断屏幕阅读器。 */}
            <span
              role="status"
              style={{ fontSize: 11, color: "var(--text-secondary)", lineHeight: 1.4 }}
            >
              {importSummary(report, t)}
            </span>

            {report.reportPath ? (
              <span style={{ fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
                {t("notebook.importReportSaved", { path: report.reportPath })}
              </span>
            ) : (
              /* 报告写不进去**不算导入失败** —— 笔记已经在库里了。所以这是提示,
                 不是 alert。 */
              <span style={{ fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
                {t("notebook.importReportUnsaved")}
              </span>
            )}

            {report.items.length > 0 ? (
              <ul
                style={{
                  margin: 0,
                  padding: 0,
                  listStyle: "none",
                  display: "flex",
                  flexDirection: "column",
                  gap: 4,
                }}
              >
                {report.items.slice(0, VISIBLE_ITEMS).map((item, index) => (
                  <li
                    key={`${item.source}-${index}`}
                    style={{
                      display: "flex",
                      flexDirection: "column",
                      gap: 1,
                      padding: "3px 6px",
                      borderRadius: 4,
                      background: "var(--bg-card)",
                    }}
                  >
                    <span
                      style={{
                        fontSize: 10.5,
                        color: "var(--text-primary)",
                        wordBreak: "break-all",
                      }}
                    >
                      {item.source}
                    </span>
                    <span style={{ fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
                      {importStatusText(item.status, t)}
                    </span>
                    {/* issue 和 status 正交:一条 imported 也可以带 issue,所以这里
                        不放在 status 的分支里,而是无条件跟在后面。 */}
                    {(item.issues ?? []).map((issue, issueIndex) => (
                      <span
                        key={issueIndex}
                        style={{
                          fontSize: 10,
                          color: "var(--warning, var(--text-secondary))",
                          lineHeight: 1.4,
                          wordBreak: "break-word",
                        }}
                      >
                        {importIssueText(issue, t)}
                      </span>
                    ))}
                  </li>
                ))}
              </ul>
            ) : null}

            {hidden > 0 ? (
              <span style={{ fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
                {t("notebook.importMoreInReport", { count: String(hidden) })}
              </span>
            ) : null}
          </div>
        </>
      ) : null}

      {error ? (
        <div
          role="alert"
          style={{
            fontSize: 11,
            lineHeight: 1.4,
            color: "var(--danger, var(--text-primary))",
            wordBreak: "break-word",
          }}
        >
          {error}
        </div>
      ) : null}

      <div style={{ display: "flex", justifyContent: "flex-end" }}>
        <button
          type="button"
          onClick={onClose}
          style={{
            height: 24,
            padding: "0 10px",
            border: "1px solid var(--border-medium)",
            borderRadius: 5,
            background: "var(--bg-card)",
            color: "var(--text-primary)",
            cursor: "pointer",
            fontSize: 11,
          }}
        >
          {t("common.close")}
        </button>
      </div>
    </div>
  );
}
