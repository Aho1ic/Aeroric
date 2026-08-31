/* 导出面板:把当前笔记导出成文件 / 复制到剪贴板,或把整库导出成静态站点。
 *
 * 为什么导出和「复制为…」在同一个面板里(Markio 分成 ExportSheet 和 MultiCopySheet
 * 两个):它们是同一个问题的两半 —— 用户想的是「把这篇笔记弄到别处去」,区别只在落点
 * 是文件还是剪贴板。随手记是个面板而不是独立应用,开两个窗要用户先猜「复制」属于哪
 * 一个。分成两组标题放在一个面板里,一次就能看全。
 *
 * 只画 UI 和管进度显示。渲染、内联图、落盘在 `noteExport.ts` / `noteSiteExportRun.ts`。
 */

import { useEffect, useRef } from "react";
import { zLayers } from "../../styles/zLayers";
import type { ExportAction } from "./noteExportRun";
import type { SiteExportProgress } from "./noteSiteExportRun";

export type NoteExportSheetProps = {
  /** 有没有一篇可导出的笔记。没有时单篇那几项禁用。 */
  hasNote: boolean;
  /** 正在跑的动作。非 null 时全部按钮禁用。 */
  busy: ExportAction | null;
  /** 整库导出的进度。null 表示没在跑。 */
  progress: SiteExportProgress | null;
  /** 成功后的一句话(已导出到…/已复制)。 */
  notice: string | null;
  error: string | null;
  onRun: (action: ExportAction) => void;
  /** 取消整库导出。只在 `progress` 非 null 时画出来。 */
  onCancelSite: () => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

/** 单篇导出成文件的那几项。 */
const FILE_ACTIONS: readonly { action: ExportAction; label: string; hint: string }[] = [
  { action: "pdf", label: "notebook.exportAsPdf", hint: "notebook.exportAsPdfHint" },
  { action: "html", label: "notebook.exportAsHtml", hint: "notebook.exportAsHtmlHint" },
  {
    action: "markdown",
    label: "notebook.exportAsMarkdown",
    hint: "notebook.exportAsMarkdownHint",
  },
];

/** 复制到剪贴板的那几项。 */
const COPY_ACTIONS: readonly { action: ExportAction; label: string; hint: string }[] = [
  {
    action: "copyHtml",
    label: "notebook.exportCopyHtml",
    hint: "notebook.exportCopyHtmlHint",
  },
  {
    action: "copyMarkdown",
    label: "notebook.exportCopyMarkdown",
    hint: "notebook.exportCopyMarkdownHint",
  },
];

export function NoteExportSheet({
  hasNote,
  busy,
  progress,
  notice,
  error,
  onRun,
  onCancelSite,
  onClose,
  t,
}: NoteExportSheetProps) {
  const firstRef = useRef<HTMLButtonElement | null>(null);

  // 开窗聚焦到第一项:键盘用户不该先按一串 Tab 才能到内容上。
  useEffect(() => {
    firstRef.current?.focus();
  }, []);

  const running = busy !== null;

  return (
    <div
      role="dialog"
      aria-label={t("notebook.exportTitle")}
      data-notebook-context-menu
      onKeyDown={(event) => {
        // IME 组字中的 Escape 是「取消候选词」,不是「关窗」。
        if (event.nativeEvent.isComposing) return;
        if (event.key === "Escape") {
          // 不往上冒:面板和宿主也有 Esc,一次按键不该关两层。
          event.stopPropagation();
          /* 导出跑着的时候 Esc 也允许关窗 —— 单篇导出很快,整库导出的取消由那个
             专门的按钮负责,而把窗锁住会让用户觉得应用卡了。 */
          onClose();
        }
      }}
      style={{
        position: "absolute",
        left: "50%",
        top: 40,
        transform: "translateX(-50%)",
        zIndex: zLayers.contextMenu,
        width: 380,
        maxWidth: "90%",
        padding: 12,
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        background: "var(--bg-sidebar)",
        boxShadow: "var(--shadow-popover)",
        display: "flex",
        flexDirection: "column",
        gap: 10,
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-primary)" }}>
          {t("notebook.exportTitle")}
        </span>
        <span style={{ flex: 1, fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
          {t("notebook.exportSheetDescription")}
        </span>
      </div>

      <ActionGroup
        actions={FILE_ACTIONS}
        disabled={!hasNote || running}
        busy={busy}
        onRun={onRun}
        t={t}
        firstRef={firstRef}
      />

      <Divider />

      <ActionGroup
        actions={COPY_ACTIONS}
        disabled={!hasNote || running}
        busy={busy}
        onRun={onRun}
        t={t}
      />

      <Divider />

      {/* 整库导出和单篇无关,所以 hasNote 不参与它的禁用条件。 */}
      <ActionGroup
        actions={[
          { action: "site", label: "notebook.exportSite", hint: "notebook.exportSiteHint" },
        ]}
        disabled={running}
        busy={busy}
        onRun={onRun}
        t={t}
      />

      {progress ? (
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {/* 进度用 role="status":它会变,但不该像 alert 那样打断屏幕阅读器。 */}
          <span
            role="status"
            style={{ flex: 1, fontSize: 10, color: "var(--text-secondary)", lineHeight: 1.4 }}
          >
            {t("notebook.exportSiteRunning", {
              current: progress.current,
              done: String(progress.done),
              total: String(progress.total),
            })}
          </span>
          <button
            type="button"
            onClick={onCancelSite}
            style={{
              height: 22,
              padding: "0 8px",
              border: "1px solid var(--border-medium)",
              borderRadius: 5,
              background: "var(--bg-card)",
              color: "var(--text-primary)",
              cursor: "pointer",
              fontSize: 10,
            }}
          >
            {t("notebook.exportSiteCancel")}
          </button>
        </div>
      ) : null}

      {notice ? (
        <div
          role="status"
          style={{
            fontSize: 11,
            lineHeight: 1.4,
            color: "var(--text-secondary)",
            wordBreak: "break-word",
            // 成功文案可能带第二行(「N 张图片未内联」),HTML 默认会把换行折成空格。
            whiteSpace: "pre-wrap",
          }}
        >
          {notice}
        </div>
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

function Divider() {
  return <div style={{ height: 1, background: "var(--border-dim)" }} />;
}

type ActionGroupProps = {
  actions: readonly { action: ExportAction; label: string; hint: string }[];
  disabled: boolean;
  busy: ExportAction | null;
  onRun: (action: ExportAction) => void;
  t: (key: string, vars?: Record<string, string>) => string;
  firstRef?: React.RefObject<HTMLButtonElement | null>;
};

function ActionGroup({ actions, disabled, busy, onRun, t, firstRef }: ActionGroupProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      {actions.map((entry, index) => {
        const isBusy = busy === entry.action;
        return (
          <button
            key={entry.action}
            ref={index === 0 ? firstRef : undefined}
            type="button"
            onClick={() => onRun(entry.action)}
            disabled={disabled}
            /* 忙的那一项标出来:导出要读盘、渲染、内联图,几百 KB 的笔记会有可感的
               延迟,没有反馈用户会以为没点上然后再点一次。 */
            aria-busy={isBusy}
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-start",
              gap: 1,
              padding: "5px 8px",
              border: "1px solid var(--border-medium)",
              borderRadius: 5,
              background: isBusy ? "var(--control-active-bg)" : "var(--bg-card)",
              color: disabled ? "var(--text-hint)" : "var(--text-primary)",
              cursor: disabled ? "not-allowed" : "pointer",
              textAlign: "left",
              width: "100%",
            }}
          >
            <span style={{ fontSize: 11 }}>{t(entry.label)}</span>
            <span style={{ fontSize: 9.5, color: "var(--text-hint)", lineHeight: 1.35 }}>
              {t(entry.hint)}
            </span>
          </button>
        );
      })}
    </div>
  );
}
