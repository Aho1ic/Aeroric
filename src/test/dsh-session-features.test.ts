import { describe, expect, it } from "vitest";
import {
  mergeDshSessionEvents,
  projectDshSessionEvents,
  type DshSessionEvent,
} from "../dshSessionFeatures";

const event = (type: string, seq: number, time: number, data: unknown): DshSessionEvent => ({ type, seq, time, data });

describe("DSH advanced session projections", () => {
  it("folds Harness step/tool timing, usage, and produced paths", () => {
    const features = projectDshSessionEvents([
      event("step/start", 1, 100, { turn: 1, step: 1 }),
      event("assistant/chunk", 2, 125, { turn: 1, step: 1, chunk: { type: "text", text: "hi" } }),
      event("tool/call", 3, 140, { turn: 1, step: 1, callId: "call-1", name: "fs_edit", arguments: JSON.stringify({ file_path: "src/main.ts" }) }),
      event("tool/result", 4, 190, { turn: 1, step: 1, message: { source: { callId: "call-1" }, content: [{ type: "text", text: "ok" }] }, meta: { diffs: [{ path: "src/main.ts" }] } }),
      event("assistant/message", 5, 220, { turn: 1, step: 1, message: { content: [{ type: "text", text: "done" }] }, usage: { inputTokens: 30, outputTokens: 10, cacheReadTokens: 5 } }),
      event("step/end", 6, 230, { turn: 1, step: 1 }),
    ]);
    expect(features.stats).toMatchObject({ turns: 1, steps: 1, llmMs: 120, toolMs: 50, ttftMs: 25, ttftSteps: 1, decodeMs: 95, decodeTokens: 10, inputTokens: 30, outputTokens: 10, cacheReadTokens: 5 });
    expect(features.producedFiles).toEqual([{ path: "src/main.ts", seq: 4, turn: 1 }]);
    expect(features.trajectory.map((entry) => entry.type)).toEqual([
      "step/start", "assistant/chunk", "tool/call", "tool/result", "assistant/message", "step/end",
    ]);
  });

  it("projects workflow phases and recurring schedules", () => {
    const features = projectDshSessionEvents([
      event("tool-workflow/run-start", 1, 1, { runId: "run-1", name: "Release" }),
      event("tool-workflow/agent-start", 2, 2, { runId: "run-1", seq: 1, label: "Build", phase: "Build", childId: "child-1" }),
      event("tool-workflow/agent-end", 3, 3, { runId: "run-1", seq: 1, outcome: "completed" }),
      event("tool-workflow/run-end", 4, 4, { runId: "run-1", stopReason: "completed" }),
      event("schedule/change", 5, 5, { operation: "create", schedule: { id: "schedule-1", kind: "every", prompt: "check", everySeconds: 300, scheduledAt: "2999-01-01T00:00:00.000Z" } }),
      event("schedule/change", 6, 6, { operation: "dispatch", id: "schedule-1", acceptedAt: "2999-01-01T00:05:00.000Z" }),
    ]);
    expect(features.workflows[0]).toMatchObject({ name: "Release", status: "completed", phases: { Build: { members: [{ status: "completed", childId: "child-1" }] } } });
    expect(features.schedules[0]).toMatchObject({ id: "schedule-1", state: "scheduled", scheduledAt: "2999-01-01T00:10:00.000Z" });
  });

  it("deduplicates live and history events by sequence", () => {
    const original = event("turn/start", 1, 1, { turn: 1 });
    const replacement = event("turn/start", 1, 2, { turn: 1, source: "live" });
    expect(mergeDshSessionEvents([original], [replacement, event("turn/end", 2, 3, { turn: 1 })])).toEqual([replacement, event("turn/end", 2, 3, { turn: 1 })]);
  });
});
