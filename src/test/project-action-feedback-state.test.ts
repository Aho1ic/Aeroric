import { describe, expect, it } from "vitest";
import {
  appendProjectActionLog,
  finishProjectActionTrace,
  startProjectActionTrace,
  summarizeProjectActionLog,
} from "../components/project-page/actionFeedback";

describe("project action feedback state", () => {
  it("records completed action timing", () => {
    const trace = startProjectActionTrace({
      id: 7,
      action: "open",
      target: "problems",
      now: 1000,
    });

    expect(finishProjectActionTrace(trace, { message: "Opened Problems", now: 1042 })).toEqual({
      id: 7,
      action: "open",
      target: "problems",
      startedAt: 1000,
      finishedAt: 1042,
      durationMs: 42,
      status: "completed",
      message: "Opened Problems",
    });
  });

  it("records failed actions without negative durations", () => {
    const trace = startProjectActionTrace({
      id: 8,
      action: "run",
      target: "preview",
      now: 2000,
    });

    expect(
      finishProjectActionTrace(trace, {
        message: "Preview failed",
        status: "failed",
        error: "port unavailable",
        now: 1990,
      }),
    ).toMatchObject({
      id: 8,
      action: "run",
      target: "preview",
      finishedAt: 1990,
      durationMs: 0,
      status: "failed",
      message: "Preview failed",
      error: "port unavailable",
    });
  });

  it("keeps the most recent action log entries first", () => {
    const first = finishProjectActionTrace(
      startProjectActionTrace({ id: 1, action: "open", target: "search", now: 100 }),
      { message: "Opened Search", now: 120 },
    );
    const second = finishProjectActionTrace(
      startProjectActionTrace({ id: 2, action: "close", target: "search", now: 200 }),
      { message: "Closed Search", now: 230 },
    );

    expect(appendProjectActionLog([first], second, 1)).toEqual([second]);
  });

  it("summarizes action log failures and average duration", () => {
    const opened = finishProjectActionTrace(
      startProjectActionTrace({ id: 1, action: "open", target: "search", now: 100 }),
      { message: "Opened Search", now: 130 },
    );
    const failed = finishProjectActionTrace(
      startProjectActionTrace({ id: 2, action: "open", target: "preview", now: 200 }),
      { message: "Preview failed", status: "failed", error: "boom", now: 250 },
    );

    expect(summarizeProjectActionLog([failed, opened])).toEqual({
      total: 2,
      failed: 1,
      averageDurationMs: 40,
      byAction: { open: 2, close: 0, run: 0 },
      latest: failed,
    });
  });
});
