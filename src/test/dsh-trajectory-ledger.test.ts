import { describe, expect, it } from "vitest";
import { projectDshSessionEvents } from "../dshSessionFeatures";
import type { DshSessionEvent } from "../dshSessionFeatures";
import {
  deriveDshLedger,
  dshLedgerCategory,
  dshLedgerRows,
  dshLedgerTag,
} from "../dshTrajectoryLedger";
import type { DshLedgerRow } from "../dshTrajectoryLedger";

function ledger(events: DshSessionEvent[]) {
  return deriveDshLedger(projectDshSessionEvents(events).trajectory);
}

function rows(events: DshSessionEvent[]): readonly DshLedgerRow[] {
  return dshLedgerRows(ledger(events));
}

function rowAt(events: DshSessionEvent[], seq: number): DshLedgerRow {
  const row = rows(events).find((candidate) => candidate.seq === seq);
  if (row === undefined) throw new Error(`no ledger row for seq ${seq}`);
  return row;
}

/** A tool definition as one `request/header` states it. */
const bashTool = {
  name: "bash",
  description: "Run a bash command in the workspace.",
  parameters: {
    type: "object",
    properties: { command: { type: "string", description: "The bash command to run." } },
    required: ["command"],
  },
};

/** One turn: a request header, a reply, and the tool call it ordered. */
function turnEvents(): DshSessionEvent[] {
  return [
    { type: "turn/start", seq: 1, time: 1_000, data: { turn: 1 } },
    { type: "user/message", seq: 2, time: 1_010, data: { turn: 1, content: "list the files" } },
    {
      type: "request/header",
      seq: 3,
      time: 1_020,
      data: { header: { system: "be brief", tools: [bashTool] }, reason: "initial" },
    },
    { type: "step/start", seq: 4, time: 1_100, data: { turn: 1, step: 1 } },
    {
      type: "assistant/chunk",
      seq: 5,
      time: 1_200,
      data: { turn: 1, step: 1, chunk: { type: "text", text: "on it" } },
    },
    {
      type: "assistant/message",
      seq: 6,
      time: 1_400,
      data: { turn: 1, step: 1, content: "on it", usage: { inputTokens: 30, outputTokens: 10 } },
    },
    {
      type: "tool/call",
      seq: 7,
      time: 1_500,
      data: { turn: 1, step: 1, callId: "c1", name: "bash", arguments: '{"command":"ls -1"}' },
    },
    {
      type: "tool/result",
      seq: 8,
      time: 5_228,
      data: { turn: 1, step: 1, callId: "c1", content: "README.md\nsrc" },
    },
    { type: "step/end", seq: 9, time: 5_300, data: { turn: 1, step: 1 } },
    { type: "turn/end", seq: 10, time: 5_400, data: { turn: 1 } },
  ];
}

describe("dshLedgerTag", () => {
  it("maps every kind the panel colours, and falls back to SYSTEM", () => {
    expect(dshLedgerTag("user/message")).toBe("USER");
    expect(dshLedgerTag("assistant/message")).toBe("ASSISTANT");
    expect(dshLedgerTag("assistant/chunk")).toBe("ASSISTANT");
    expect(dshLedgerTag("tool/call")).toBe("TOOL");
    expect(dshLedgerTag("tool/result")).toBe("TOOL");
    expect(dshLedgerTag("turn/start")).toBe("TURN");
    expect(dshLedgerTag("step/end")).toBe("STEP");
    expect(dshLedgerTag("request/header")).toBe("REQUEST");
    expect(dshLedgerTag("tool-workflow/run-start")).toBe("WORKFLOW");
    expect(dshLedgerTag("workflow/run-end")).toBe("WORKFLOW");
    expect(dshLedgerTag("command/run")).toBe("COMMAND");
    expect(dshLedgerTag("schedule/change")).toBe("SCHEDULE");
    expect(dshLedgerTag("compaction/summary")).toBe("COMPACT");
    expect(dshLedgerTag("feedback/record")).toBe("FEEDBACK");
    expect(dshLedgerTag("session/end")).toBe("SYSTEM");
  });

  it("reads a workflow fan-out as a tool rather than as a call", () => {
    expect(dshLedgerCategory(dshLedgerTag("tool-workflow/agent-start"))).toBe("tool");
    expect(dshLedgerCategory(dshLedgerTag("user/message"))).toBe("message");
    expect(dshLedgerCategory(dshLedgerTag("step/start"))).toBe("lifecycle");
    expect(dshLedgerCategory(dshLedgerTag("session/end"))).toBe("system");
  });
});

describe("deriveDshLedger", () => {
  it("folds a call and its result into one measured row", () => {
    const call = rowAt(turnEvents(), 7);
    expect(call).toMatchObject({
      tag: "TOOL",
      seqs: [7, 8],
      startedAt: 1_500,
      durationMs: 3_728,
      status: "complete",
      toolName: "bash",
      callId: "c1",
      result: "README.md\nsrc",
    });
  });

  it("pretty-prints a call's arguments as its payload", () => {
    expect(rowAt(turnEvents(), 7).payload).toBe('{\n  "command": "ls -1"\n}');
  });

  it("passes a malformed argument string through verbatim", () => {
    const row = rowAt(
      [
        {
          type: "tool/call",
          seq: 1,
          time: 1,
          data: { callId: "c1", name: "bash", arguments: "ls" },
        },
      ],
      1,
    );
    expect(row.payload).toBe("ls");
  });

  it("leaves a call with no result running and unmeasured", () => {
    const row = rowAt(turnEvents().slice(0, 7), 7);
    expect(row).toMatchObject({ status: "running", seqs: [7] });
    expect(row.durationMs).toBeUndefined();
    expect(row.result).toBeUndefined();
  });

  it("marks a failed call as an error", () => {
    const events = turnEvents();
    events[7] = { type: "tool/result", seq: 8, time: 1_600, data: { callId: "c1", isError: true } };
    expect(rowAt(events, 7)).toMatchObject({ status: "error", durationMs: 100 });
  });

  it("stands an orphan result up as its own row so the call is not lost", () => {
    const orphan = rows([
      { type: "tool/result", seq: 9, time: 2_000, data: { callId: "gone", content: "done" } },
    ]);
    expect(orphan).toHaveLength(1);
    expect(orphan[0]).toMatchObject({ tag: "TOOL", seq: 9, result: "done" });
  });

  it("keeps the streaming chunks out of the rows and in the reply's timing", () => {
    expect(rows(turnEvents()).map((row) => row.seq)).toEqual([1, 2, 3, 4, 6, 7, 9, 10]);
    expect(rowAt(turnEvents(), 6)).toMatchObject({
      startedAt: 1_100,
      durationMs: 300,
      ttftMs: 100,
      decodeMs: 200,
    });
  });

  it("reports the same reply timing and token counts the stats panel sums", () => {
    const stats = projectDshSessionEvents(turnEvents()).stats;
    const reply = rowAt(turnEvents(), 6);
    expect(reply.durationMs).toBe(stats.llmMs);
    expect(reply.ttftMs).toBe(stats.ttftMs);
    expect(reply.decodeMs).toBe(stats.decodeMs);
    expect(reply.usage).toEqual({
      inputTokens: 30,
      outputTokens: 10,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    });
  });

  it("measures a turn and a step from the boundary that closed them", () => {
    expect(rowAt(turnEvents(), 1)).toMatchObject({ tag: "TURN", depth: 0, durationMs: 4_400 });
    expect(rowAt(turnEvents(), 4)).toMatchObject({ tag: "STEP", depth: 1, durationMs: 4_200 });
  });

  it("nests a call under the reply that ordered it", () => {
    expect(rowAt(turnEvents(), 7)).toMatchObject({ depth: 2, parentSeq: 6 });
    expect(rowAt(turnEvents(), 6)).toMatchObject({ depth: 1 });
    expect(rowAt(turnEvents(), 6).parentSeq).toBeUndefined();
  });

  it("leaves a call with no preceding reply at the turn's own depth", () => {
    const row = rowAt(
      [{ type: "tool/call", seq: 1, time: 1, data: { turn: 1, callId: "c1", name: "bash" } }],
      1,
    );
    expect(row).toMatchObject({ depth: 1 });
    expect(row.parentSeq).toBeUndefined();
  });

  it("does not carry a nesting parent across a turn boundary", () => {
    const events: DshSessionEvent[] = [
      { type: "assistant/message", seq: 1, time: 1, data: { turn: 1, content: "hi" } },
      { type: "tool/call", seq: 2, time: 2, data: { turn: 2, callId: "c1", name: "bash" } },
    ];
    expect(rowAt(events, 2)).toMatchObject({ depth: 1 });
    expect(rowAt(events, 2).parentSeq).toBeUndefined();
  });

  it("resolves a tool's schema from the newest header that precedes the call", () => {
    expect(rowAt(turnEvents(), 7).schema).toEqual(bashTool);
  });

  it("leaves the schema unresolved when no header precedes the call", () => {
    const events = turnEvents().filter((event) => event.type !== "request/header");
    expect(rowAt(events, 7).schema).toBeUndefined();
  });

  it("ignores a header that only arrives after the call it would describe", () => {
    const events: DshSessionEvent[] = [
      { type: "tool/call", seq: 1, time: 1, data: { callId: "c1", name: "bash" } },
      { type: "request/header", seq: 2, time: 2, data: { header: { tools: [bashTool] } } },
    ];
    expect(rowAt(events, 1).schema).toBeUndefined();
  });

  it("reads an OpenAI-shaped tool definition as well as the flat one", () => {
    const events: DshSessionEvent[] = [
      {
        type: "request/header",
        seq: 1,
        time: 1,
        data: {
          header: {
            tools: [
              { type: "function", function: { name: "bash", description: "Run.", parameters: {} } },
            ],
          },
        },
      },
      { type: "tool/call", seq: 2, time: 2, data: { callId: "c1", name: "bash" } },
    ];
    expect(rowAt(events, 2).schema).toEqual({ name: "bash", description: "Run.", parameters: {} });
  });

  it("groups rows by turn and keeps an untimed event in the group it arrived in", () => {
    const events: DshSessionEvent[] = [
      { type: "session/start", seq: 1, time: 1, data: {} },
      { type: "user/message", seq: 2, time: 2, data: { turn: 1, content: "a" } },
      { type: "schedule/change", seq: 3, time: 3, data: { operation: "delete", id: "s1" } },
      { type: "user/message", seq: 4, time: 4, data: { turn: 2, content: "b" } },
    ];
    const groups = ledger(events);
    expect(groups.map((group) => [group.turn, group.rows.map((row) => row.seq)])).toEqual([
      [undefined, [1]],
      [1, [2, 3]],
      [2, [4]],
    ]);
    expect(groups[1]).toMatchObject({ startedAt: 2 });
  });

  it("has no groups for a session that recorded nothing", () => {
    expect(deriveDshLedger([])).toEqual([]);
  });

  it("summarises a row with the detail's first non-blank line", () => {
    const events: DshSessionEvent[] = [
      { type: "user/message", seq: 1, time: 1, data: { content: "\n\n  first line \nsecond" } },
    ];
    expect(rowAt(events, 1).preview).toBe("first line");
  });
});
