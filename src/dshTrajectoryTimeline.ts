/**
 * Timeline projections for the DeepSeek Harness trajectory overview.
 *
 * The Harness plots one session four ways, from two independent choices: whether
 * an operation is drawn at its recorded width or as an equal-width slot, and
 * whether operations sit on the real clock or in operation order. The overview
 * is a focusing instrument, so a selected interval resolves back to the ledger
 * rows it covers rather than to spans.
 */

import type { DshTimelineKind, DshTimelineRecord } from "./dshSessionFeatures";

/** Which projection the overview is drawn in. */
export interface DshTimelineMode {
  /** Draw each operation at its recorded duration instead of one equal slot. */
  actualDuration: boolean;
  /** Place operations on the real clock instead of in operation order. */
  actualTime: boolean;
}

/** An inclusive interval in the active projection's domain. */
export interface DshTimelineRange {
  start: number;
  end: number;
}

/** One record placed in the active domain. */
export interface DshTimelineSpan extends DshTimelineRange {
  record: DshTimelineRecord;
  /** Row the span is drawn in: 0 context, 1 model, 2 tools. */
  lane: number;
}

/** One turn's first operation, for the boundary markers. */
export interface DshTimelineBoundary {
  turn: number;
  at: number;
}

/** The full domain of one projection. */
export interface DshTimelineModel extends DshTimelineRange {
  spans: readonly DshTimelineSpan[];
  boundaries: readonly DshTimelineBoundary[];
}

/** Three stable lanes, so a session's shape stays comparable across pages. */
function laneFor(kind: DshTimelineKind): number {
  if (kind === "tool") return 2;
  if (kind === "assistant" || kind === "compacted") return 1;
  return 0;
}

/** One marker per turn, at the earliest operation that reported it. */
function boundariesOf(spans: readonly DshTimelineSpan[]): DshTimelineBoundary[] {
  const at = new Map<number, number>();
  for (const span of spans) {
    const turn = span.record.turn;
    if (turn === undefined) continue;
    const current = at.get(turn);
    if (current === undefined || span.start < current) at.set(turn, span.start);
  }
  return [...at.entries()]
    .map(([turn, start]) => ({ turn, at: start }))
    .sort((left, right) => left.at - right.at || left.turn - right.turn);
}

/** Equal-width slots in operation order: the shape of a session, not its clock. */
function sequenceProjection(records: readonly DshTimelineRecord[]): DshTimelineModel | null {
  if (records.length === 0) return null;
  const spans = records.map((record, index) => ({
    start: index,
    end: index + 1,
    lane: laneFor(record.kind),
    record,
  }));
  return { start: 0, end: records.length, spans, boundaries: boundariesOf(spans) };
}

/**
 * Place records on the recorded clock.
 *
 * `actualDuration` decides whether a span keeps its measured width or collapses
 * to the instant it began. Idle is squeezed out only in the compressed
 * projection, and only for gaps no operation covers, so a session that spent an
 * hour waiting for input still reads as the few seconds of work it did.
 */
function timedProjection(
  records: readonly DshTimelineRecord[],
  { actualDuration, compressIdle }: { actualDuration: boolean; compressIdle: boolean },
): DshTimelineModel | null {
  if (records.length === 0) return null;
  const ordered = [...records].sort(
    (left, right) => left.startedAt - right.startedAt || left.durationMs - right.durationMs,
  );
  const removedBefore = new Map<DshTimelineRecord, number>();
  let removed = 0;
  let covered: number | null = null;
  for (const record of ordered) {
    if (compressIdle && covered !== null && record.startedAt > covered) {
      removed += record.startedAt - covered;
    }
    removedBefore.set(record, removed);
    const end = record.startedAt + record.durationMs;
    covered = covered === null ? end : Math.max(covered, end);
  }
  const spans = records.map((record): DshTimelineSpan => {
    const offset = removedBefore.get(record) ?? 0;
    const start = record.startedAt - offset;
    return {
      start,
      end: actualDuration ? start + record.durationMs : start,
      lane: laneFor(record.kind),
      record,
    };
  });
  return {
    start: Math.min(...spans.map((span) => span.start)),
    end: Math.max(...spans.map((span) => span.end)),
    spans,
    boundaries: boundariesOf(spans),
  };
}

/**
 * Project the session's operations into the active domain.
 *
 * @param records - The fold's timeline records, in event order.
 * @param mode - The two independent projection choices.
 * @returns The model, or null when the session recorded no operation.
 */
export function deriveDshTimeline(
  records: readonly DshTimelineRecord[],
  mode: DshTimelineMode,
): DshTimelineModel | null {
  if (!mode.actualDuration && !mode.actualTime) return sequenceProjection(records);
  return timedProjection(records, {
    actualDuration: mode.actualDuration,
    // Compressing only when duration is shown keeps the fourth projection — real
    // widths on the real clock — able to show the waiting the others hide.
    compressIdle: mode.actualDuration && !mode.actualTime,
  });
}

/**
 * The ledger rows an interval covers.
 *
 * A span counts when it overlaps the interval at all, so a selection thinner
 * than a long operation still finds it.
 *
 * @param model - The active projection.
 * @param range - The selected interval, in the same domain.
 * @returns Event seqs to focus; empty when nothing overlaps.
 */
export function dshTimelineFocus(
  model: DshTimelineModel | null,
  range: DshTimelineRange,
): ReadonlySet<number> {
  const seqs = new Set<number>();
  const start = Math.min(range.start, range.end);
  const end = Math.max(range.start, range.end);
  for (const span of model?.spans ?? []) {
    if (span.start > end || span.end < start) continue;
    for (const seq of span.record.seqs) seqs.add(seq);
  }
  return seqs;
}

/** Format a projected offset the way the Harness labels timeline durations. */
export function formatDshTimelineOffset(ms: number): string {
  return `${Math.round(ms).toLocaleString()} ms`;
}

/**
 * The operation nearest a point in the active domain, for click-to-locate.
 *
 * A span that contains the point wins; otherwise the nearest one does. Ties break
 * toward the earlier start and then the lower first row, so clicking the same
 * pixel twice always lands on the same operation — including in the default
 * projection, where every span is one slot wide and a click on a boundary is
 * equidistant from two of them.
 *
 * @param model - The active projection.
 * @param at - A point in that projection's domain.
 * @returns The located span, or null when the session recorded no operation.
 */
export function dshTimelineLocate(
  model: DshTimelineModel | null,
  at: number,
): DshTimelineSpan | null {
  let best: DshTimelineSpan | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const span of model?.spans ?? []) {
    const low = Math.min(span.start, span.end);
    const high = Math.max(span.start, span.end);
    const distance = at < low ? low - at : at > high ? at - high : 0;
    if (distance > bestDistance) continue;
    if (
      best !== null &&
      distance === bestDistance &&
      (span.start > best.start ||
        (span.start === best.start && (span.record.seqs[0] ?? 0) >= (best.record.seqs[0] ?? 0)))
    ) {
      continue;
    }
    best = span;
    bestDistance = distance;
  }
  return best;
}
