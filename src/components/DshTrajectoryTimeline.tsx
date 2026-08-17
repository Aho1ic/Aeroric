/**
 * The trajectory timing overview: a three-lane, drag-to-focus timeline over one
 * session's measured operations.
 *
 * Drawn like a network panel's overview — context on the top lane, model replies
 * in the middle, tool calls below — so a slow turn is visible before any row is
 * read. Dragging selects an interval and the ledger narrows to the rows it
 * covers; the wheel zooms the visible window, which is navigation and never
 * changes the selection.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { Clock, Timer, X, ZoomOut } from "lucide-react";
import { useI18n } from "../i18n";
import {
  deriveDshTimeline,
  formatDshTimelineOffset,
  type DshTimelineMode,
  type DshTimelineRange,
} from "../dshTrajectoryTimeline";
import type { DshTimelineKind, DshTimelineRecord } from "../dshSessionFeatures";

/** A drag thinner than this is a click, which clears the selection. */
const MINIMUM_DRAG_FRACTION = 0.004;
/** Never zoom past four equal-width operations, where the shape stops reading. */
const MINIMUM_SEQUENCE_WINDOW = 4;
/** Never zoom past 20 ms on a clock projection, for the same reason. */
const MINIMUM_TIMED_WINDOW = 20;

/** Context, model, tools — the lane order the spans are placed in. */
const LANES: readonly DshTimelineKind[] = ["user", "assistant", "tool"];

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}

/** Wall-clock stamp with milliseconds, the resolution the spans are measured at. */
function stamp(time: number): string {
  const at = new Date(time);
  const clock = at.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
  return `${clock}.${String(at.getMilliseconds()).padStart(3, "0")}`;
}

function recordTitle(record: DshTimelineRecord, kindLabel: string): string {
  const lines = [`${kindLabel} · ${record.label}`];
  lines.push(
    record.durationMs > 0
      ? `${stamp(record.startedAt)} → ${stamp(record.startedAt + record.durationMs)} (${formatDshTimelineOffset(record.durationMs)})`
      : stamp(record.startedAt),
  );
  if (record.ttftMs !== undefined) {
    lines.push(
      `TTFT ${formatDshTimelineOffset(record.ttftMs)} · ${formatDshTimelineOffset(record.decodeMs ?? 0)}`,
    );
  }
  return lines.join("\n");
}

export function DshTrajectoryTimeline({
  records,
  mode,
  onModeChange,
  focus,
  onFocusChange,
}: {
  records: readonly DshTimelineRecord[];
  mode: DshTimelineMode;
  onModeChange: (mode: DshTimelineMode) => void;
  /** The active selection in the current domain, or null when the ledger is whole. */
  focus: DshTimelineRange | null;
  onFocusChange: (range: DshTimelineRange | null) => void;
}) {
  const { t } = useI18n();
  const trackRef = useRef<HTMLDivElement>(null);
  const model = useMemo(() => deriveDshTimeline(records, mode), [mode, records]);
  const [view, setView] = useState<DshTimelineRange | null>(null);
  const [drag, setDrag] = useState<DshTimelineRange | null>(null);

  // A projection change re-scales the domain, so a window or a drag measured in
  // the old one would point somewhere arbitrary in the new one.
  useEffect(() => {
    setView(null);
    setDrag(null);
  }, [mode.actualDuration, mode.actualTime]);

  const sequenceMode = !mode.actualDuration && !mode.actualTime;
  const minimumWindow = sequenceMode ? MINIMUM_SEQUENCE_WINDOW : MINIMUM_TIMED_WINDOW;

  useEffect(() => {
    const track = trackRef.current;
    if (track === null || model === null) return;
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY === 0) return;
      // The overview owns the vertical wheel inside its own box: the insights
      // panel scrolls, so letting the page take it would make zooming impossible.
      event.preventDefault();
      const box = track.getBoundingClientRect();
      const at = box.width === 0 ? 0.5 : clamp((event.clientX - box.left) / box.width, 0, 1);
      const current = view ?? { start: model.start, end: model.end };
      const width = Math.max(1e-6, current.end - current.start);
      const full = Math.max(1e-6, model.end - model.start);
      const anchor = current.start + at * width;
      const next = clamp(
        width * (event.deltaY > 0 ? 1.25 : 0.8),
        Math.min(minimumWindow, full),
        full,
      );
      const start = clamp(anchor - at * next, model.start, Math.max(model.start, model.end - next));
      setView(next >= full ? null : { start, end: start + next });
    };
    track.addEventListener("wheel", onWheel, { passive: false });
    return () => track.removeEventListener("wheel", onWheel);
  }, [minimumWindow, model, view]);

  if (model === null) return null;

  const domain = view ?? { start: model.start, end: model.end };
  const span = Math.max(1e-6, domain.end - domain.start);
  const percent = (value: number) => `${clamp((value - domain.start) / span, 0, 1) * 100}%`;
  const width = (from: number, to: number) => `${clamp((to - from) / span, 0, 1) * 100}%`;
  const atFraction = (fraction: number) => domain.start + clamp(fraction, 0, 1) * span;
  const fractionOf = (event: { clientX: number }) => {
    const box = trackRef.current?.getBoundingClientRect();
    return !box || box.width === 0 ? 0 : clamp((event.clientX - box.left) / box.width, 0, 1);
  };

  const visible = model.spans.filter(
    (entry) => entry.end >= domain.start && entry.start <= domain.end,
  );
  const selection = drag ?? focus;
  const wide = (range: DshTimelineRange) =>
    Math.abs(range.end - range.start) / span >= MINIMUM_DRAG_FRACTION;

  return (
    <div className="dsh-timeline">
      <div className="dsh-timeline-controls">
        <button
          type="button"
          data-active={mode.actualDuration}
          aria-pressed={mode.actualDuration}
          title={
            mode.actualDuration ? t("dsh.timeline.useEqualWidth") : t("dsh.timeline.useDuration")
          }
          onClick={() => onModeChange({ ...mode, actualDuration: !mode.actualDuration })}
        >
          <Timer size={12} aria-hidden="true" />
          {t("dsh.timeline.duration")}
        </button>
        <button
          type="button"
          data-active={mode.actualTime}
          aria-pressed={mode.actualTime}
          title={
            mode.actualTime ? t("dsh.timeline.useCompressed") : t("dsh.timeline.useActualTime")
          }
          onClick={() => onModeChange({ ...mode, actualTime: !mode.actualTime })}
        >
          <Clock size={12} aria-hidden="true" />
          {t("dsh.timeline.actualTime")}
        </button>
        <span className="dsh-timeline-span" role="status">
          {sequenceMode
            ? t("dsh.timeline.operations", { count: String(Math.round(span)) })
            : formatDshTimelineOffset(span)}
        </span>
        {view !== null && (
          <button type="button" onClick={() => setView(null)}>
            <ZoomOut size={12} aria-hidden="true" />
            {t("dsh.timeline.resetZoom")}
          </button>
        )}
        {focus !== null && (
          <button type="button" onClick={() => onFocusChange(null)}>
            <X size={12} aria-hidden="true" />
            {t("dsh.timeline.clearFocus")}
          </button>
        )}
      </div>
      <div className="dsh-timeline-plot">
        <div className="dsh-timeline-labels" aria-hidden="true">
          {LANES.map((kind) => (
            <span key={kind}>{t(`dsh.timeline.lane.${kind}`)}</span>
          ))}
        </div>
        <div
          ref={trackRef}
          className="dsh-timeline-track"
          tabIndex={0}
          aria-label={t("dsh.timeline.aria")}
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.currentTarget.setPointerCapture(event.pointerId);
            const at = atFraction(fractionOf(event));
            setDrag({ start: at, end: at });
          }}
          onPointerMove={(event) => {
            if (drag === null) return;
            setDrag({ start: drag.start, end: atFraction(fractionOf(event)) });
          }}
          onPointerUp={(event) => {
            if (drag === null) return;
            const range = { start: drag.start, end: atFraction(fractionOf(event)) };
            setDrag(null);
            onFocusChange(wide(range) ? range : null);
          }}
          onPointerCancel={() => setDrag(null)}
          onDoubleClick={() => onFocusChange(null)}
          onKeyDown={(event) => {
            if (event.key !== "Escape" || (focus === null && view === null)) return;
            event.preventDefault();
            onFocusChange(null);
            setView(null);
          }}
        >
          {LANES.map((kind) => (
            <div key={kind} className="dsh-timeline-lane" aria-hidden="true" />
          ))}
          {model.boundaries.map((boundary) => (
            <span
              key={boundary.turn}
              className="dsh-timeline-boundary"
              style={{ left: percent(boundary.at) }}
              aria-hidden="true"
            >
              <em>T{boundary.turn}</em>
            </span>
          ))}
          {visible.map((entry) => (
            // A bar is a mark, not a control: every pointer gesture belongs to
            // the track, so a drag that starts on one still selects an interval.
            // Hovering it names the operation and its measured timing.
            <span
              key={entry.record.seqs[0]}
              className="dsh-timeline-bar"
              data-kind={entry.record.kind}
              data-error={entry.record.isError || undefined}
              style={{
                left: percent(entry.start),
                width: width(entry.start, Math.max(entry.end, entry.start)),
                top: `calc(${entry.lane} * (100% / ${LANES.length}))`,
              }}
              title={recordTitle(entry.record, t(`dsh.timeline.lane.${entry.record.kind}`))}
            />
          ))}
          {selection !== null && wide(selection) && (
            <span
              className="dsh-timeline-selection"
              style={{
                left: percent(Math.min(selection.start, selection.end)),
                width: width(
                  Math.min(selection.start, selection.end),
                  Math.max(selection.start, selection.end),
                ),
              }}
              aria-hidden="true"
            />
          )}
        </div>
      </div>
    </div>
  );
}
