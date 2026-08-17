import { describe, expect, it, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { I18nProvider } from "../i18n";
import { projectDshSessionEvents } from "../dshSessionFeatures";
import type { DshSessionEvent, DshTimelineRecord } from "../dshSessionFeatures";
import { deriveDshTimeline, dshTimelineFocus } from "../dshTrajectoryTimeline";
import { DshSessionInsights } from "../components/DshSessionInsights";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** A turn that thinks for 300 ms and then runs one 500 ms tool call. */
function turnEvents(): DshSessionEvent[] {
  return [
    { type: "user/message", seq: 1, time: 1_000, data: { turn: 1, content: "go" } },
    { type: "step/start", seq: 2, time: 1_100, data: { turn: 1, step: 1 } },
    {
      type: "assistant/chunk",
      seq: 3,
      time: 1_200,
      data: { turn: 1, step: 1, chunk: { type: "text", text: "hi" } },
    },
    { type: "assistant/message", seq: 4, time: 1_400, data: { turn: 1, step: 1, content: "hi" } },
    {
      type: "tool/call",
      seq: 5,
      time: 1_500,
      data: { turn: 1, step: 1, callId: "c1", name: "write_file" },
    },
    { type: "tool/result", seq: 6, time: 2_000, data: { turn: 1, step: 1, callId: "c1" } },
  ];
}

function records(events = turnEvents()): DshTimelineRecord[] {
  return projectDshSessionEvents(events).timeline;
}

describe("DSH timeline records", () => {
  it("measures a tool call from its call to its result and folds both rows into it", () => {
    const tool = records().find((record) => record.kind === "tool");
    expect(tool).toMatchObject({ durationMs: 500, startedAt: 1_500, seqs: [5, 6] });
  });

  it("measures a reply from its step start and reports its first-token split", () => {
    const assistant = records().find((record) => record.kind === "assistant");
    expect(assistant).toMatchObject({
      startedAt: 1_100,
      durationMs: 300,
      ttftMs: 100,
      decodeMs: 200,
    });
  });

  it("leaves the streaming chunks and step boundaries out of the ledger", () => {
    expect(records().map((record) => record.kind)).toEqual(["user", "assistant", "tool"]);
  });

  it("keeps a still-open call at zero width instead of stretching it to now", () => {
    const open = records(turnEvents().slice(0, 5)).find((record) => record.kind === "tool");
    expect(open).toMatchObject({ durationMs: 0, seqs: [5] });
  });

  it("marks a failed call so the overview can show it as an error", () => {
    const failed = records([
      { type: "tool/call", seq: 1, time: 1_000, data: { callId: "c1", name: "bash" } },
      { type: "tool/result", seq: 2, time: 1_200, data: { callId: "c1", isError: true } },
    ]);
    expect(failed[0]).toMatchObject({ isError: true, durationMs: 200 });
  });
});

describe("deriveDshTimeline", () => {
  const mode = (actualDuration: boolean, actualTime: boolean) => ({ actualDuration, actualTime });

  it("gives every operation an equal slot in the default projection", () => {
    const model = deriveDshTimeline(records(), mode(false, false));
    expect(model).toMatchObject({ start: 0, end: 3 });
    expect(model?.spans.map((span) => [span.start, span.end])).toEqual([
      [0, 1],
      [1, 2],
      [2, 3],
    ]);
  });

  it("lays the lanes out as context, model, tools", () => {
    const lanes = deriveDshTimeline(records(), mode(false, false))?.spans.map((span) => span.lane);
    expect(lanes).toEqual([0, 1, 2]);
  });

  it("draws recorded widths and squeezes out the gaps nothing covers", () => {
    // user@1000 (a point) → reply 1100–1400 → tool 1500–2000. Two 100 ms gaps
    // nothing covers are removed, so the domain is the 800 ms of real work.
    const model = deriveDshTimeline(records(), mode(true, false));
    expect(model).toMatchObject({ start: 1_000, end: 1_800 });
    expect(model?.spans.map((span) => [span.start, span.end])).toEqual([
      [1_000, 1_000],
      [1_000, 1_300],
      [1_300, 1_800],
    ]);
  });

  it("keeps the idle gap when the clock is shown as it happened", () => {
    const model = deriveDshTimeline(records(), mode(true, true));
    expect(model).toMatchObject({ start: 1_000, end: 2_000 });
    expect(model?.spans.map((span) => [span.start, span.end])).toEqual([
      [1_000, 1_000],
      [1_100, 1_400],
      [1_500, 2_000],
    ]);
  });

  it("collapses each operation to its start instant in the time-only projection", () => {
    const model = deriveDshTimeline(records(), mode(false, true));
    expect(model?.spans.every((span) => span.start === span.end)).toBe(true);
  });

  it("marks each turn once, at its earliest operation", () => {
    const model = deriveDshTimeline(records(), mode(false, false));
    expect(model?.boundaries).toEqual([{ turn: 1, at: 0 }]);
  });

  it("has no model for a session that recorded no operation", () => {
    expect(deriveDshTimeline([], mode(false, false))).toBeNull();
    expect(deriveDshTimeline([], mode(true, true))).toBeNull();
  });
});

describe("dshTimelineFocus", () => {
  const model = deriveDshTimeline(records(), { actualDuration: false, actualTime: false });

  it("returns every ledger row an interval overlaps", () => {
    expect([...dshTimelineFocus(model, { start: 2.2, end: 2.5 })]).toEqual([5, 6]);
  });

  it("reads a backwards drag the same as a forwards one", () => {
    expect(dshTimelineFocus(model, { start: 2.5, end: 2.2 })).toEqual(
      dshTimelineFocus(model, { start: 2.2, end: 2.5 }),
    );
  });

  it("finds an operation wider than the selection inside it", () => {
    const timed = deriveDshTimeline(records(), { actualDuration: true, actualTime: true });
    expect([...dshTimelineFocus(timed, { start: 1_700, end: 1_710 })]).toEqual([5, 6]);
  });

  it("focuses nothing when the interval covers no operation", () => {
    expect(dshTimelineFocus(model, { start: 9, end: 10 }).size).toBe(0);
    expect(dshTimelineFocus(null, { start: 0, end: 1 }).size).toBe(0);
  });
});

async function openInsights() {
  invokeMock.mockImplementation((command) =>
    command === "get_dsh_session_history"
      ? Promise.resolve({
          events: turnEvents().map((event) => ({ event })),
          hasMore: false,
          projections: { values: {} },
        })
      : Promise.resolve(null),
  );
  render(
    <I18nProvider>
      <DshSessionInsights sessionId="session-1" />
    </I18nProvider>,
  );
  await userEvent.click(screen.getByRole("button", { name: /Session details/ }));
  return await screen.findByLabelText(/Timeline overview/);
}

/** jsdom has no layout and no pointer capture; a drag needs both. */
function measureTrack(track: HTMLElement, width = 400) {
  track.setPointerCapture = vi.fn();
  vi.spyOn(track, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: width,
    bottom: 42,
    width,
    height: 42,
    toJSON: () => ({}),
  });
}

/** Drag across the track from one fraction of its width to another. */
function drag(track: HTMLElement, from: number, to: number) {
  const box = track.getBoundingClientRect();
  fireEvent.pointerDown(track, { button: 0, pointerId: 1, clientX: box.width * from });
  fireEvent.pointerMove(track, { pointerId: 1, clientX: box.width * to });
  fireEvent.pointerUp(track, { pointerId: 1, clientX: box.width * to });
}

describe("DSH trajectory timing overview", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("plots one bar per measured operation with its timing on the hover label", async () => {
    const track = await openInsights();
    const bars = track.querySelectorAll(".dsh-timeline-bar");
    expect(bars).toHaveLength(3);
    expect(within(track).getByTitle(/Tools · Tool: write_file/)).toHaveAttribute(
      "title",
      expect.stringContaining("500 ms"),
    );
  });

  it("narrows the ledger to the rows a dragged interval covers, and restores it", async () => {
    const track = await openInsights();
    measureTrack(track);
    const list = document.querySelector(".dsh-trajectory-list") as HTMLElement;
    expect(list.querySelectorAll(".dsh-trajectory-entry")).toHaveLength(6);

    // The last third of the equal-width domain is the tool call and its result.
    drag(track, 0.8, 0.95);
    expect(list.querySelectorAll(".dsh-trajectory-entry")).toHaveLength(2);
    expect(within(list).getByText("Tool: write_file")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Clear focus/ }));
    expect(list.querySelectorAll(".dsh-trajectory-entry")).toHaveLength(6);
  });

  it("treats a click as clearing the focus rather than an empty selection", async () => {
    const track = await openInsights();
    measureTrack(track);
    drag(track, 0.8, 0.95);
    expect(screen.getByRole("button", { name: /Clear focus/ })).toBeInTheDocument();
    drag(track, 0.5, 0.5);
    expect(screen.queryByRole("button", { name: /Clear focus/ })).not.toBeInTheDocument();
  });

  it("drops the selection when the projection it was measured in changes", async () => {
    const track = await openInsights();
    measureTrack(track);
    drag(track, 0.8, 0.95);
    await userEvent.click(screen.getByRole("button", { name: /Duration/ }));
    expect(screen.queryByRole("button", { name: /Clear focus/ })).not.toBeInTheDocument();
    expect(document.querySelectorAll(".dsh-trajectory-entry")).toHaveLength(6);
  });

  it("reports the domain in operations by default and in milliseconds once timed", async () => {
    await openInsights();
    expect(screen.getByRole("status")).toHaveTextContent("3 operations");
    // Points on the clock: first start to last start. Adding the recorded widths
    // extends the domain to where the last operation actually ended.
    await userEvent.click(screen.getByRole("button", { name: /Actual time/ }));
    expect(screen.getByRole("status")).toHaveTextContent("500 ms");
    await userEvent.click(screen.getByRole("button", { name: /Duration/ }));
    expect(screen.getByRole("status")).toHaveTextContent("1,000 ms");
  });

  it("zooms the visible window on the wheel without touching the selection", async () => {
    const track = await openInsights();
    measureTrack(track);
    // Three equal-width slots are already the closest the overview will zoom, so
    // the window is exercised on a clock projection, which has room to narrow.
    await userEvent.click(screen.getByRole("button", { name: /Actual time/ }));
    drag(track, 0.1, 0.3);
    expect(screen.getByRole("button", { name: /Clear focus/ })).toBeInTheDocument();
    fireEvent.wheel(track, { deltaY: -100, clientX: 200 });
    expect(screen.getByRole("button", { name: /Reset zoom/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Clear focus/ })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Reset zoom/ }));
    expect(screen.queryByRole("button", { name: /Reset zoom/ })).not.toBeInTheDocument();
  });
});
