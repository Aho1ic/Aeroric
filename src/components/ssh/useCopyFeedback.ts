import { useCallback, useEffect, useRef, useState } from "react";

/** 复制成功后保持"已复制"反馈的时长。 */
export const COPY_FEEDBACK_DURATION_MS = 900;

/**
 * 复制按钮的点击反馈状态。
 *
 * SSH 连接卡片有三处"复制密码"入口（列表、工作区、项目对话框），此前各自内联维护
 * 一份 copiedId + setTimeout，卡片列表那处干脆没有任何反馈，点了不知道有没有复制成功。
 * 统一到这里，顺带修掉两个内联实现共同的问题：组件卸载后 setTimeout 仍会 setState。
 */
export function useCopyFeedback(durationMs = COPY_FEEDBACK_DURATION_MS) {
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timerRef.current !== null) clearTimeout(timerRef.current);
    },
    [],
  );

  const markCopied = useCallback(
    (id: string) => {
      if (timerRef.current !== null) clearTimeout(timerRef.current);
      setCopiedId(id);
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        setCopiedId(null);
      }, durationMs);
    },
    [durationMs],
  );

  return { copiedId, markCopied };
}
