/* 按大纲重排章节。
 *
 * 从 Markio 的 Outline.tsx 移植(`computeHeadingSpans` / `moveSection`)。
 *
 * 与 Markio 的关键差别:段落范围由 `analyzeNote` 给出的偏移算,不再自己重扫源码。
 * Markio 的大纲来自 Rust、偏移来自前端正则,两个扫描器判定不一致时索引就对不齐,
 * 会把段落移到错位置、静默损坏文档 —— 它只能在"数量不等"时整体拒绝重排。这里
 * 两者出自同一次扫描,那类错位在结构上不可能发生,也就不需要那道兜底。
 */

import type { OutlineItem } from "./noteOutline";

/** 一个章节在源码里的字符范围 `[from, to)`。 */
export type SectionSpan = {
  from: number;
  to: number;
  level: number;
};

/**
 * 算出每个标题所辖章节的范围。
 *
 * `to` 是后面第一个**层级不深于自己**的标题的起点,没有就到文末 —— 也就是说
 * 子标题连同正文一起算进父章节,拖动父标题会带走整棵子树。
 */
export function sectionSpans(outline: OutlineItem[], sourceLength: number): SectionSpan[] {
  return outline.map((item, index) => {
    let to = sourceLength;
    for (let next = index + 1; next < outline.length; next += 1) {
      if (outline[next]!.level <= item.level) {
        to = outline[next]!.offset;
        break;
      }
    }
    return { from: item.offset, to, level: item.level };
  });
}

/**
 * 把 `[from, to)` 这段挪到 `insertBefore` 之前。条件不成立时原样返回。
 *
 * `insertBefore === to`(把 A 拖到紧邻的下一段 B 之前)本就是 no-op。Markio 原来
 * 用 `< to` 判定,漏掉了相等这一档,导致偏移修正落在错位置,把 B 的内容黏到 A 的
 * 标题上损坏文档。这里保留它修好之后的 `<= to`。
 */
export function moveSection(
  source: string,
  from: number,
  to: number,
  insertBefore: number,
): string {
  if (insertBefore >= from && insertBefore <= to) return source;
  const section = source.slice(from, to);
  const without = source.slice(0, from) + source.slice(to);
  // 插入点在被移走的段之后:段拿掉之后它整体前移了 (to - from)。
  const adjusted = insertBefore > to ? insertBefore - (to - from) : insertBefore;
  return without.slice(0, adjusted) + section + without.slice(adjusted);
}

/**
 * 按大纲下标把一个章节移到另一个章节之前。
 *
 * 返回新的源码;移不动(下标非法、拖到自己身上、拖到自己内部)时返回 `null`,
 * 让调用方能区分"没变化"和"变了但恰好一样"。
 */
export function reorderSection(
  source: string,
  outline: OutlineItem[],
  sourceIndex: number,
  targetIndex: number,
): string | null {
  if (sourceIndex === targetIndex) return null;
  const spans = sectionSpans(outline, source.length);
  const from = spans[sourceIndex];
  const target = spans[targetIndex];
  if (!from || !target) return null;
  const next = moveSection(source, from.from, from.to, target.from);
  return next === source ? null : next;
}
