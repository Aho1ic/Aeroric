/* 跨文件改标签名的小窗:输入新名字 → 执行 → 看报告。
 *
 * 为什么不做成行内编辑:重命名会**改别人的文件**,而报告(改了哪些、跳过哪些、哪些
 * 失败)是这个操作最重要的输出。侧栏只有一列宽,行内编辑塞不下报告,而把报告扔进
 * 面板那条错误提示里等于把"成功改了 12 篇"也说成错误。
 *
 * 三个状态在同一个窗里依次出现,不换位置:输入 → 执行中 → 报告。换位置(比如报告
 * 弹到别处)会让"刚才那次改了什么"变成要去找的东西。
 */

import { useEffect, useRef, useState } from "react";
import { AlertTriangle, Check, SkipForward } from "lucide-react";
import { zLayers } from "../../styles/zLayers";
import type { TagRenameReport } from "./notebookApi";

export type TagRenameDialogState = {
  x: number;
  y: number;
  /** 要改的那个标签的归一化 key。匹配按它做(大小写不敏感)。 */
  key: string;
  /** 显示用的原样写法。输入框的初值 —— 多数重命名是小改一两个字。 */
  label: string;
  /** 面板上那一行报的处数。用来和报告里的总数对账。 */
  count: number;
};

export type TagRenameDialogProps = {
  state: TagRenameDialogState;
  /** `null` = 还没执行过。执行完把报告放进来,窗不关 —— 报告就是结果。 */
  report: TagRenameReport | null;
  running: boolean;
  /** 执行失败(整次失败,不是单篇失败)的文案。 */
  error: string | null;
  onSubmit: (next: string) => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

export function TagRenameDialog({
  state,
  report,
  running,
  error,
  onSubmit,
  onClose,
  t,
}: TagRenameDialogProps) {
  const [value, setValue] = useState(state.label);
  const inputRef = useRef<HTMLInputElement | null>(null);

  /* 开窗就聚焦并全选:初值是旧名字,用户下一步要么整个换掉、要么改一两个字,
     全选让前者是一次输入。 */
  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    input.select();
  }, []);

  const trimmed = value.trim().replace(/^#+/, "").trim();
  const unchanged = trimmed === state.label;
  const canSubmit = trimmed.length > 0 && !unchanged && !running;

  return (
    <div
      role="dialog"
      aria-label={t("notebook.tagRenameTitle", { tag: state.label })}
      data-notebook-context-menu
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          /* 不往上冒。写这一句时**目前**没有可碰撞的对象:面板自己没有 Esc 处理,
             宿主那个 window 监听要按住修饰键才进。留着是因为它是"浮层拦掉自己的
             Esc"这件事的正确写法,而加一层 Esc(关面板 / 退出全屏)是随时会发生的
             改动 —— 那时候少了这句就变成一次按键关两层。测不出来,所以在这里写清。 */
          event.stopPropagation();
          onClose();
          return;
        }
        if (event.key === "Enter" && canSubmit) {
          event.stopPropagation();
          onSubmit(trimmed);
        }
      }}
      style={{
        position: "fixed",
        left: state.x,
        top: state.y,
        zIndex: zLayers.contextMenu,
        width: 280,
        padding: 10,
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        background: "var(--bg-sidebar)",
        boxShadow: "var(--shadow-popover)",
        display: "flex",
        flexDirection: "column",
        gap: 8,
      }}
    >
      <div style={{ fontSize: 11, color: "var(--text-muted)", lineHeight: 1.4 }}>
        {/* 处数写在提示里:重命名会改别人的文件,动手前该知道波及多少处。 */}
        {t("notebook.tagRenameHint", { tag: state.label, count: String(state.count) })}
      </div>

      <input
        ref={inputRef}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        aria-label={t("notebook.tagRenameInput")}
        disabled={running}
        style={{
          width: "100%",
          boxSizing: "border-box",
          height: 26,
          border: "1px solid var(--border-medium)",
          borderRadius: 5,
          background: "var(--bg-card)",
          color: "var(--text-primary)",
          padding: "0 7px",
          fontSize: 12,
        }}
      />

      {/* 子标签不跟着改:面板里 `#work` 和 `#work/deep` 是两行、各有各的处数,改这一行
          就该正好改那么多处。不说清的话用户会以为漏改了。 */}
      <div style={{ fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
        {t("notebook.tagRenameChildNote")}
      </div>

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

      {report ? <TagRenameSummary report={report} t={t} /> : null}

      <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
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
          {/* 执行过之后这个按钮是"看完了",不是"取消" —— 改动已经落盘,叫取消会
              让人以为点它能撤回。 */}
          {report ? t("notebook.tagRenameClose") : t("common.cancel")}
        </button>
        <button
          type="button"
          onClick={() => onSubmit(trimmed)}
          disabled={!canSubmit}
          style={{
            height: 24,
            padding: "0 10px",
            border: "1px solid var(--border-medium)",
            borderRadius: 5,
            background: canSubmit ? "var(--control-active-bg)" : "var(--bg-card)",
            color: canSubmit ? "var(--control-active-fg)" : "var(--text-hint)",
            cursor: canSubmit ? "pointer" : "not-allowed",
            fontSize: 11,
          }}
        >
          {running ? t("notebook.tagRenameRunning") : t("notebook.tagRenameApply")}
        </button>
      </div>
    </div>
  );
}

/** 报告:改了几处 / 跳过几篇 / 失败几篇,各自可以看到是哪些。 */
function TagRenameSummary({
  report,
  t,
}: {
  report: TagRenameReport;
  t: (key: string, vars?: Record<string, string>) => string;
}) {
  const changedRefs = report.changed.reduce((sum, item) => sum + item.count, 0);
  return (
    <div
      /* status 而不是 alert:大多数时候这是一次成功的报告,alert 会让屏读把它念成
         出错了。真正失败的那几篇在下面单独用 alert 语义标出来。 */
      role="status"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 4,
        padding: "6px 7px",
        borderRadius: 6,
        background: "var(--bg-card)",
        fontSize: 11,
        lineHeight: 1.45,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
        <Check size={11} aria-hidden="true" style={{ flexShrink: 0, color: "var(--success)" }} />
        <span>
          {t("notebook.tagRenameChanged", {
            count: String(changedRefs),
            notes: String(report.changed.length),
          })}
        </span>
      </div>

      {report.skipped.length ? (
        <details>
          <summary style={{ cursor: "pointer", display: "flex", alignItems: "center", gap: 4 }}>
            <SkipForward
              size={11}
              aria-hidden="true"
              style={{ flexShrink: 0, color: "var(--text-muted)" }}
            />
            <span>{t("notebook.tagRenameSkipped", { notes: String(report.skipped.length) })}</span>
          </summary>
          <ul style={{ margin: "4px 0 0", paddingLeft: 18, color: "var(--text-muted)" }}>
            {report.skipped.map((item) => (
              <li key={item.path}>
                {t(`notebook.tagSkip.${item.reason}`)} — {fileNameOf(item.path)}
              </li>
            ))}
          </ul>
        </details>
      ) : null}

      {report.failed.length ? (
        <details open>
          <summary
            /* 失败那一段用 alert:它是唯一需要用户再做点什么的部分。默认展开,
               折叠起来的失败等于没报。 */
            role="alert"
            style={{ cursor: "pointer", display: "flex", alignItems: "center", gap: 4 }}
          >
            <AlertTriangle
              size={11}
              aria-hidden="true"
              style={{ flexShrink: 0, color: "var(--danger)" }}
            />
            <span>{t("notebook.tagRenameFailed", { notes: String(report.failed.length) })}</span>
          </summary>
          <ul style={{ margin: "4px 0 0", paddingLeft: 18, color: "var(--text-muted)" }}>
            {report.failed.map((item) => (
              <li key={item.path}>
                {fileNameOf(item.path)} — {item.message}
              </li>
            ))}
          </ul>
        </details>
      ) : null}
    </div>
  );
}

/** 路径取文件名。报告里列的是"哪几篇",全路径在 280px 宽里只会挤成一团。 */
function fileNameOf(path: string): string {
  return path.replace(/^.*[/\\]/, "");
}
