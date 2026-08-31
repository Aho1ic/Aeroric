/* 快速捕获的窗:打一句话 → 选落点 → ⌘↩ 存。
 *
 * 为什么不是「新建一篇笔记然后打字」:捕获要的是**不打断当前在做的事**。新建会切走
 * 当前笔记、把编辑器的滚动位置和未落盘的编辑都带进另一个上下文,而用户只是想记一句
 * 「记得回复那封邮件」。这个窗开在最上层,关掉之后当前笔记还在原处。
 *
 * 只画 UI。落点路径、追加拼法在 `noteCapture.ts`(不叫 `noteQuickCapture.ts` ——
 * 在大小写不敏感的文件系统上会和本文件重名),落盘编排在面板里(要等已有
 * 编辑落盘、要拿磁盘上那份当基线、要把结果写回内存)。
 *
 * 失败时**窗不关、文字不清**:捕获的那句话只存在这个 textarea 里,关掉就没了。
 */

import { useEffect, useRef, useState } from "react";
import { zLayers } from "../../styles/zLayers";
import type { CaptureTarget } from "./noteCapture";

export type NoteQuickCaptureProps = {
  /** 两个落点的显示路径(vault 相对)。写出来是为了让用户知道东西去了哪。 */
  paths: Record<CaptureTarget, string>;
  busy: boolean;
  error: string | null;
  /** 提交。成功与否由面板决定 —— 成功时面板会关掉这个窗。 */
  onSubmit: (target: CaptureTarget, text: string) => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

const TARGETS: readonly CaptureTarget[] = ["today", "inbox"];

export function NoteQuickCapture({
  paths,
  busy,
  error,
  onSubmit,
  onClose,
  t,
}: NoteQuickCaptureProps) {
  const [text, setText] = useState("");
  const [target, setTarget] = useState<CaptureTarget>("today");
  const areaRef = useRef<HTMLTextAreaElement | null>(null);

  // 开窗即聚焦:这个窗的全部用途就是马上开始打字。
  useEffect(() => {
    areaRef.current?.focus();
  }, []);

  const canSubmit = text.trim().length > 0 && !busy;

  return (
    <div
      role="dialog"
      aria-label={t("notebook.captureTitle")}
      data-notebook-context-menu
      onKeyDown={(event) => {
        /* IME 组字中的 Escape 是「取消候选词」,组字中的回车是「确认候选词」。不挡的话
           中文输入法下打第一个字就把窗关了或者提交了。 */
        if (event.nativeEvent.isComposing) return;
        if (event.key === "Escape") {
          // 不往上冒:面板和宿主也有 Esc 的处理,一次按键不该关两层。
          event.stopPropagation();
          onClose();
          return;
        }
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey) && canSubmit) {
          event.stopPropagation();
          event.preventDefault();
          onSubmit(target, text);
        }
      }}
      style={{
        position: "absolute",
        left: "50%",
        top: 40,
        transform: "translateX(-50%)",
        zIndex: zLayers.contextMenu,
        width: 360,
        maxWidth: "90%",
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
      <div
        role="radiogroup"
        aria-label={t("notebook.captureTargetLabel")}
        style={{ display: "flex", gap: 4 }}
      >
        {TARGETS.map((entry) => {
          const on = entry === target;
          return (
            <button
              key={entry}
              type="button"
              role="radio"
              aria-checked={on}
              onClick={() => setTarget(entry)}
              disabled={busy}
              style={{
                flex: 1,
                height: 24,
                border: "1px solid var(--border-medium)",
                borderRadius: 5,
                background: on ? "var(--control-active-bg)" : "var(--bg-card)",
                color: on ? "var(--control-active-fg)" : "var(--text-primary)",
                cursor: busy ? "not-allowed" : "pointer",
                fontSize: 11,
              }}
            >
              {t(entry === "today" ? "notebook.captureToday" : "notebook.captureInbox")}
            </button>
          );
        })}
      </div>

      {/* 落点写出来:两个目标都是约定路径,用户不该需要猜东西去了哪个文件。 */}
      <div style={{ fontSize: 10, color: "var(--text-hint)", lineHeight: 1.4 }}>
        {t("notebook.captureTargetHint", { path: paths[target] })}
      </div>

      <textarea
        ref={areaRef}
        value={text}
        onChange={(event) => setText(event.target.value)}
        aria-label={t("notebook.captureInput")}
        placeholder={t("notebook.capturePlaceholder")}
        disabled={busy}
        rows={5}
        style={{
          width: "100%",
          boxSizing: "border-box",
          resize: "vertical",
          border: "1px solid var(--border-medium)",
          borderRadius: 5,
          background: "var(--bg-card)",
          color: "var(--text-primary)",
          padding: "6px 7px",
          fontSize: 12,
          lineHeight: 1.5,
          fontFamily: "inherit",
        }}
      />

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

      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span style={{ flex: 1, fontSize: 10, color: "var(--text-hint)" }}>
          {t("notebook.captureHint")}
        </span>
        <button
          type="button"
          onClick={onClose}
          disabled={busy}
          style={{
            height: 24,
            padding: "0 10px",
            border: "1px solid var(--border-medium)",
            borderRadius: 5,
            background: "var(--bg-card)",
            color: "var(--text-primary)",
            cursor: busy ? "not-allowed" : "pointer",
            fontSize: 11,
          }}
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          onClick={() => onSubmit(target, text)}
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
          {busy ? t("notebook.captureSaving") : t("notebook.captureSave")}
        </button>
      </div>
    </div>
  );
}
