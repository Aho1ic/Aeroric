/* 随手记面板的三档布局。
 *
 * 按面板**自己量出来的宽度**分档,不看 `width` prop —— ProjectPage 里传的一直是
 * "100%",真实宽度由外面的容器决定,prop 读不出来。
 *
 * 三档:
 * - compact  (<560px)  列表收起,正文吃满整宽。当前 400px 的面板是 170 列表 +
 *                      230 正文,正文那半几乎没法用。
 * - standard (560–900) 现状:170px 列表 + 正文。
 * - wide     (≥900px)  列表加宽到 220px,大纲有地方开。
 */

import { useEffect, useRef, useState } from "react";

export const TIER_COMPACT_MAX = 560;
export const TIER_WIDE_MIN = 900;

/** 切档回差。阈值上反复横跳会让面板疯狂重排(拖分隔条时很容易停在边界上)。 */
export const TIER_HYSTERESIS = 24;

export type NoteLayoutTier = "compact" | "standard" | "wide";

/**
 * 按宽度定档。`current` 是当前档位,用来加回差 —— 回差只在**离开**当前档时生效,
 * 所以从 standard 出去要多走 24px,进来不用。
 *
 * 宽度量不到(0 / NaN,首帧或 display:none)时退回 standard:那是现状的布局,
 * 猜错的代价最小。
 */
export function tierForWidth(width: number, current: NoteLayoutTier | null): NoteLayoutTier {
  if (!Number.isFinite(width) || width <= 0) return current ?? "standard";
  const compactMax = current === "compact" ? TIER_COMPACT_MAX + TIER_HYSTERESIS : TIER_COMPACT_MAX;
  const wideMin = current === "wide" ? TIER_WIDE_MIN - TIER_HYSTERESIS : TIER_WIDE_MIN;
  if (width < compactMax) return "compact";
  if (width >= wideMin) return "wide";
  return "standard";
}

export type NoteLayoutTierResult<T extends HTMLElement> = {
  /** 挂到要量的那个元素上。 */
  ref: React.RefObject<T | null>;
  tier: NoteLayoutTier;
};

export function useNoteLayoutTier<T extends HTMLElement>(): NoteLayoutTierResult<T> {
  const ref = useRef<T | null>(null);
  const [tier, setTier] = useState<NoteLayoutTier>("standard");

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => {
      // 用 getBoundingClientRect 而不是 clientWidth:后者取整,在阈值上会因为
      // 亚像素宽度来回抖。
      const width = element.getBoundingClientRect().width;
      setTier((current) => tierForWidth(width, current));
    };
    measure();
    // 老 WebView 没有 ResizeObserver —— 量一次就停在那一档,不崩。
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return { ref, tier };
}
