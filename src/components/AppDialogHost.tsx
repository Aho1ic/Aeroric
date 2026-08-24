/* 应用内提示弹窗的宿主 —— 渲染 `src/lib/appDialog.ts` 发来的请求。
 *
 * 为什么存在:见 `appDialog.ts` 顶部注释。这里只负责「怎么画」和「怎么答」。
 *
 * 关键取舍:
 * - 队列串行。并发请求不能互相顶掉,否则先来的那个 promise 永远不 settle,
 *   调用方会永久 await。
 * - confirm 的焦点默认落在取消键。原生 MessageBox 默认焦点在确认键,但这里
 *   绝大多数调用点是删除/丢弃这类破坏性操作,不该让误触 Enter 直接执行。
 *   prompt 则落在输入框——那是要打字的。
 * - 点遮罩不关闭。与原生 confirm/prompt 一致:必须显式选一个。Esc 等价取消。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, CircleAlert, Info } from "lucide-react";
import {
  registerAppDialogHandler,
  type AppConfirmKind,
  type AppDialogRequest,
  type AppDialogResult,
} from "../lib/appDialog";
import { useI18n } from "../i18n";
import s from "../styles";
import { Button } from "./ui/Button";

type PendingDialog = AppDialogRequest & {
  id: number;
  resolve: (result: AppDialogResult) => void;
};

/** kind → 图标与强调色。`--info` 在 themes.css 里没有,info 走 accent。 */
const KIND_PRESENTATION: Record<
  AppConfirmKind,
  { Icon: typeof AlertTriangle; color: string; surface: string }
> = {
  info: { Icon: Info, color: "var(--accent)", surface: "var(--accent-subtle)" },
  warning: { Icon: AlertTriangle, color: "var(--warning)", surface: "var(--warning-subtle)" },
  error: { Icon: CircleAlert, color: "var(--danger)", surface: "var(--danger-subtle)" },
};

let nextRequestId = 0;

export function AppDialogHost() {
  const { t } = useI18n();
  const [queue, setQueue] = useState<PendingDialog[]>([]);
  // 输入框状态连同它属于哪个请求一起存。这样新请求的首帧就能直接显示自己的
  // defaultValue,不必等一个 effect 回填 —— 那会多一次渲染,期间输入框是空的。
  const [draft, setDraft] = useState<{ requestId: number; value: string } | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  // 弹窗关闭后要把焦点还给触发它的元素,否则键盘用户会被丢回 body。
  const previouslyFocused = useRef<HTMLElement | null>(null);

  const current = queue[0] ?? null;
  const isPrompt = current?.kind === "prompt";
  // 草稿只在归属当前请求时才算数,否则回落到该请求的初值。
  const inputValue =
    current && draft?.requestId === current.id
      ? draft.value
      : current?.kind === "prompt"
        ? (current.options.defaultValue ?? "")
        : "";

  useEffect(() => {
    return registerAppDialogHandler((request) => {
      return new Promise<AppDialogResult>((resolve) => {
        const id = nextRequestId++;
        setQueue((prev) => [...prev, { ...request, id, resolve }]);
      });
    });
  }, []);

  const settle = useCallback((accepted: boolean, value: string) => {
    setDraft(null);
    setQueue((prev) => {
      const [head, ...rest] = prev;
      if (head) {
        if (head.kind === "prompt") {
          // 空输入默认按取消处理:多数调用点都是 `if (!name) return;` 这个形状,
          // 返回空串会让它们把空名字当成有效输入。allowEmpty 的调用点例外——
          // 它们用空串表达"清空",必须能和取消区分开。
          const trimmed = value.trim();
          const empty = trimmed.length === 0;
          if (!accepted) head.resolve(null);
          else if (!empty) head.resolve(trimmed);
          else head.resolve(head.options.allowEmpty ? "" : null);
        } else {
          head.resolve(accepted);
        }
      }
      return rest;
    });
  }, []);

  // 记录触发元素,并把焦点移进弹窗。
  // 用 data 属性查询而不是 ref:`ui/Button` 没有 forwardRef,而它被几十处复用,
  // 不值得为这一个弹窗改它的公共签名。
  useEffect(() => {
    if (!current) return;
    // 只在「从没有弹窗到有弹窗」这一刻记录触发元素。连续确认时若每换一个
    // 请求都记一次,记下的会是上一个弹窗那个刚被卸载的按钮,队列排空后
    // isConnected 为假,焦点就掉回 body 了。
    if (!previouslyFocused.current) {
      const active = document.activeElement;
      // body 不是有意义的还原目标;记下它反而会挡住后面真正的触发元素。
      previouslyFocused.current =
        active && active !== document.body ? (active as HTMLElement) : null;
    }
    const root = dialogRef.current;
    if (!root) return;
    if (current.kind === "prompt") {
      const input = root.querySelector<HTMLInputElement>('[data-app-dialog="input"]');
      input?.focus();
      input?.select();
    } else {
      root.querySelector<HTMLButtonElement>('[data-app-dialog="cancel"]')?.focus();
    }
  }, [current?.id, current]);

  // 队列排空后再还焦点。中途每换一个请求都还一次会打断连续确认。
  useEffect(() => {
    if (current) return;
    const target = previouslyFocused.current;
    previouslyFocused.current = null;
    if (target?.isConnected) target.focus();
  }, [current]);

  // 最新值走 ref:键盘监听挂在 window 上且依赖 current,不想让每次输入
  // 都重挂一遍监听器。
  const inputValueRef = useRef(inputValue);
  inputValueRef.current = inputValue;

  useEffect(() => {
    if (!current) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        settle(false, "");
        return;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        settle(true, inputValueRef.current);
        return;
      }
      // Tab 在弹窗内循环:模态期间不该让焦点跑到背后的界面上。
      if (event.key === "Tab") {
        const focusables = dialogRef.current?.querySelectorAll<HTMLElement>("button, input");
        if (!focusables || focusables.length === 0) return;
        const first = focusables[0];
        const last = focusables[focusables.length - 1];
        const active = document.activeElement;
        if (event.shiftKey && active === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && active === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };

    // capture 阶段监听:背后的组件(终端、编辑器)也在 window 上抢 Esc/Enter,
    // 冒泡阶段会被它们先吃掉。
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [current, settle]);

  if (!current) return null;

  const kind = current.kind === "confirm" ? (current.options.kind ?? "info") : "info";
  const { Icon, color, surface } = KIND_PRESENTATION[kind];
  const title = current.options.title ?? t("common.confirm");
  const okLabel = current.options.okLabel ?? t("common.confirm");
  const cancelLabel = current.options.cancelLabel ?? t("common.cancel");
  const destructive = current.kind === "confirm" && (kind === "warning" || kind === "error");

  return createPortal(
    <div style={s.appConfirmOverlay}>
      <div
        ref={dialogRef}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="app-dialog-title"
        aria-describedby="app-dialog-message"
        style={{
          width: "min(480px, calc(100vw - 32px))",
          maxHeight: "calc(100vh - 64px)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          background: "var(--bg-card)",
          border: "1px solid var(--border-medium)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "var(--shadow-popover)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "16px 20px",
            borderBottom: "1px solid var(--border-dim)",
            background: `color-mix(in srgb, ${surface} 60%, transparent)`,
          }}
        >
          <Icon size={20} color={color} aria-hidden />
          <h2
            id="app-dialog-title"
            style={{
              flex: 1,
              margin: 0,
              fontSize: 15,
              fontWeight: 600,
              color: "var(--text-primary)",
            }}
          >
            {title}
          </h2>
        </div>

        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflow: "auto",
            padding: "18px 20px",
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}
        >
          <div
            id="app-dialog-message"
            style={{
              fontSize: 13,
              lineHeight: 1.6,
              color: "var(--text-secondary)",
              // 调用点会拼 `\n\n${sql}`(如生产库确认),不保留换行会糊成一行。
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
            }}
          >
            {current.message}
          </div>

          {isPrompt ? (
            <input
              data-app-dialog="input"
              value={inputValue}
              placeholder={current.options.placeholder}
              onChange={(event) =>
                setDraft({ requestId: current.id, value: event.currentTarget.value })
              }
              style={{
                width: "100%",
                boxSizing: "border-box",
                padding: "8px 10px",
                fontSize: 13,
                fontFamily: "inherit",
                color: "var(--text-primary)",
                background: "var(--bg-input)",
                border: "1px solid var(--border-medium)",
                borderRadius: "var(--radius-sm)",
                outline: "none",
              }}
            />
          ) : null}
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            gap: 10,
            padding: "14px 20px",
            borderTop: "1px solid var(--border-dim)",
          }}
        >
          <Button
            data-app-dialog="cancel"
            variant="outline"
            size="sm"
            onClick={() => settle(false, "")}
          >
            {cancelLabel}
          </Button>
          <Button
            data-app-dialog="ok"
            variant={destructive ? "destructive" : "default"}
            size="sm"
            onClick={() => settle(true, inputValue)}
          >
            {okLabel}
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
