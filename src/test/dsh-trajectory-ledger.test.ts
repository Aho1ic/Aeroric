import { describe, expect, it } from "vitest";
import { projectDshSessionEvents } from "../dshSessionFeatures";
import type { DshSessionEvent } from "../dshSessionFeatures";
import {
  DSH_KNOWN_SESSION_EVENT_TYPES,
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

/**
 * One packed run of text deltas, as an attempt's compact stream records it.
 *
 * `time0` is the first fragment's own timestamp and `dt` holds the gaps to the
 * ones after it, so the run states when the model produced each fragment without
 * an event per token (`packages/llm/llm/src/assistant-stream.ts:21`).
 */
function textRun(time0: number, texts: readonly string[], dt: readonly number[] = []) {
  return { type: "text-chunks", time0, index: 0, dt, texts };
}

/**
 * One turn: a header, an attempt that committed nothing, the reply that did, and
 * the tool call that reply ordered.
 */
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
      type: "assistant/attempt",
      seq: 5,
      time: 1_250,
      data: { turn: 1, step: 1, stream: [textRun(1_150, ["par"])] },
    },
    {
      type: "assistant/message",
      seq: 6,
      time: 1_400,
      data: {
        turn: 1,
        step: 1,
        content: "on it",
        stream: [textRun(1_200, ["on ", "it"], [40])],
        usage: { inputTokens: 30, outputTokens: 10 },
      },
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
    expect(dshLedgerTag("assistant/attempt")).toBe("ASSISTANT");
    expect(dshLedgerTag("system/message")).toBe("SYSTEM");
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

describe("DSH_KNOWN_SESSION_EVENT_TYPES", () => {
  it("states the vocabulary of the Harness build Aeroric mirrors", () => {
    // Mirrored from `packages/core/session/src/known-event-types.ts:23` at
    // `c291e7961a`: 56 declared types.
    expect(Object.keys(DSH_KNOWN_SESSION_EVENT_TYPES)).toHaveLength(56);
  });

  it("carries the renamed sub-dispatch pair and not the name it replaced", () => {
    expect(DSH_KNOWN_SESSION_EVENT_TYPES["tool/ptc-dispatch"]).toBe(true);
    expect(DSH_KNOWN_SESSION_EVENT_TYPES["tool/ptc-dispatch-start"]).toBe(true);
    expect(DSH_KNOWN_SESSION_EVENT_TYPES["tool/code-dispatch"]).toBeUndefined();
    expect(DSH_KNOWN_SESSION_EVENT_TYPES["tool/code-dispatch-start"]).toBeUndefined();
  });

  it("drops the retired per-token event and carries what replaced it", () => {
    expect(DSH_KNOWN_SESSION_EVENT_TYPES["assistant/chunk"]).toBeUndefined();
    expect(DSH_KNOWN_SESSION_EVENT_TYPES["assistant/attempt"]).toBe(true);
    expect(DSH_KNOWN_SESSION_EVENT_TYPES["system/message"]).toBe(true);
  });

  it("carries every type the release added, so a mirrored event is never raw", () => {
    for (const type of [
      "model/selection",
      "subagent/catalog",
      "subagent/model-selection-policy",
      "deliverables/presented",
      "feedback/message-put",
      "feedback/message-delete",
      "session-log-deepseek/delivery-accepted",
    ]) {
      expect(DSH_KNOWN_SESSION_EVENT_TYPES[type]).toBe(true);
    }
  });

  it("shows an event outside the vocabulary raw rather than as nothing", () => {
    // A newer Harness' event and a downstream plugin's are both outside the
    // mirrored list by construction, and the panel is the only place their data
    // can be read.
    const unknown = rowAt(
      [{ type: "tool/code-dispatch", seq: 1, time: 1, data: { name: "bash", subCallId: "s1" } }],
      1,
    );
    expect(unknown.payload).toBe('{\n  "name": "bash",\n  "subCallId": "s1"\n}');
    // A mirrored type states its own detail, so it is never dumped raw.
    expect(rowAt(turnEvents(), 2).payload).toBe("list the files");
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

  it("reads the reply's timing from the stream the reply itself carries", () => {
    expect(rows(turnEvents()).map((row) => row.seq)).toEqual([1, 2, 3, 4, 5, 6, 7, 9, 10]);
    expect(rowAt(turnEvents(), 6)).toMatchObject({
      startedAt: 1_100,
      durationMs: 300,
      ttftMs: 100,
      decodeMs: 200,
    });
  });

  it("stands an attempt that committed no message up as its own failed row", () => {
    // The attempt streamed `par` at 1_150 and gave up at 1_250, both inside the
    // step the request opened at 1_100.
    expect(rowAt(turnEvents(), 5)).toMatchObject({
      tag: "ASSISTANT",
      title: "Assistant attempt",
      preview: "par",
      payload: "par",
      status: "error",
      startedAt: 1_100,
      durationMs: 150,
      ttftMs: 50,
      decodeMs: 100,
    });
    expect(rowAt(turnEvents(), 5).usage).toBeUndefined();
  });

  it("leaves the step open for the reply that follows a failed attempt", () => {
    // Both are measured from the request at 1_100: an attempt that committed
    // nothing must not make the reply look like it started when the attempt
    // gave up.
    expect(rowAt(turnEvents(), 6).startedAt).toBe(1_100);
    expect(rowAt(turnEvents(), 5).startedAt).toBe(1_100);
  });

  it("does not let an attempt adopt the calls the reply ordered", () => {
    expect(rowAt(turnEvents(), 7).parentSeq).toBe(6);
  });

  it("reads an attempt's reasoning when it produced no text at all", () => {
    const row = rowAt(
      [
        { type: "step/start", seq: 1, time: 100, data: { turn: 1, step: 1 } },
        {
          type: "assistant/attempt",
          seq: 2,
          time: 400,
          data: {
            turn: 1,
            step: 1,
            stream: [
              { type: "reasoning-chunks", time0: 200, index: 0, dt: [50], texts: ["hm", "mm"] },
            ],
          },
        },
      ],
      2,
    );
    expect(row).toMatchObject({ preview: "hmmm", ttftMs: 100, decodeMs: 200 });
  });

  it("starts the clock at the first token, not at a usage or finish frame", () => {
    const row = rowAt(
      [
        { type: "step/start", seq: 1, time: 100, data: { turn: 1, step: 1 } },
        {
          type: "assistant/message",
          seq: 2,
          time: 500,
          data: {
            turn: 1,
            step: 1,
            content: "hi",
            stream: [
              { type: "chunk", time: 150, chunk: { type: "block-start", blockType: "text" } },
              { type: "text-chunks", time0: 200, index: 0, dt: [], texts: [""] },
              { type: "text-chunks", time0: 300, index: 0, dt: [], texts: ["hi"] },
              { type: "chunk", time: 480, chunk: { type: "usage", usage: { outputTokens: 1 } } },
            ],
          },
        },
      ],
      2,
    );
    expect(row).toMatchObject({ ttftMs: 200, decodeMs: 200 });
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

  it("folds a run_code sub-dispatch into one row under the call that ran it", () => {
    const events: DshSessionEvent[] = [
      {
        type: "assistant/message",
        seq: 1,
        time: 100,
        data: { turn: 1, step: 1, content: "running" },
      },
      {
        type: "tool/call",
        seq: 2,
        time: 200,
        data: { turn: 1, step: 1, callId: "root", name: "run_code" },
      },
      {
        type: "tool/ptc-dispatch-start",
        seq: 3,
        time: 300,
        data: {
          turn: 1,
          step: 1,
          rootCallId: "root",
          parentCallId: "root",
          subCallId: "root:ptc:1",
          name: "read_file",
          arguments: { path: "src/main.ts" },
        },
      },
      {
        type: "tool/ptc-dispatch",
        seq: 4,
        time: 450,
        data: {
          turn: 1,
          step: 1,
          rootCallId: "root",
          parentCallId: "root",
          subCallId: "root:ptc:1",
          name: "read_file",
          arguments: { path: "src/main.ts" },
          isError: false,
          content: "export {}",
        },
      },
    ];
    // The pair settles on `subCallId`, so the sub-call is one row rather than a
    // second settlement of the `run_code` call that ran it.
    expect(rows(events).map((row) => row.seq)).toEqual([1, 2, 3]);
    expect(rowAt(events, 3)).toMatchObject({
      tag: "TOOL",
      seqs: [3, 4],
      toolName: "read_file",
      callId: "root:ptc:1",
      parentSeq: 2,
      depth: 2,
      durationMs: 150,
      status: "complete",
      result: "export {}",
      payload: '{\n  "path": "src/main.ts"\n}',
    });
    expect(rowAt(events, 2)).toMatchObject({ status: "running", parentSeq: 1 });
  });

  it("leaves a sub-dispatch running until the event settling it arrives", () => {
    const events: DshSessionEvent[] = [
      {
        type: "tool/ptc-dispatch-start",
        seq: 1,
        time: 100,
        data: { parentCallId: "root", subCallId: "root:ptc:1", name: "read_file" },
      },
    ];
    expect(rowAt(events, 1)).toMatchObject({ status: "running", callId: "root:ptc:1" });
  });

  it("marks a failed sub-dispatch an error, the same as a native call", () => {
    const events: DshSessionEvent[] = [
      {
        type: "tool/ptc-dispatch-start",
        seq: 1,
        time: 100,
        data: { parentCallId: "root", subCallId: "s1", name: "read_file" },
      },
      {
        type: "tool/ptc-dispatch",
        seq: 2,
        time: 200,
        data: { parentCallId: "root", subCallId: "s1", name: "read_file", isError: true },
      },
    ];
    expect(rowAt(events, 1)).toMatchObject({ status: "error", durationMs: 100 });
  });

  it("folds a replaced system prompt into the node that replaced it", () => {
    const events: DshSessionEvent[] = [
      {
        type: "system/message",
        seq: 1,
        time: 100,
        data: { turn: 1, step: 1, content: "be brief" },
        surfaceOp: "append",
      },
      { type: "user/message", seq: 2, time: 200, data: { turn: 1, content: "go" } },
      {
        type: "system/message",
        seq: 3,
        time: 300,
        data: { turn: 1, step: 1, content: "be brief and cite files" },
        surfaceOp: { op: "replace", startSeq: 1, endSeq: 1 },
        sourceEventSeqs: [1],
      },
    ];
    // The replacement takes the shadowed node's place on the surface, so the
    // panel shows one prompt rather than two it would both call current.
    expect(rows(events).map((row) => row.seq)).toEqual([2, 3]);
    expect(rowAt(events, 3)).toMatchObject({
      tag: "SYSTEM",
      title: "System prompt",
      seqs: [1, 3],
      payload: "be brief and cite files",
    });
  });

  it("keeps both prompts when the replacement cites neither of them", () => {
    const events: DshSessionEvent[] = [
      {
        type: "system/message",
        seq: 1,
        time: 100,
        data: { turn: 1, step: 1, content: "be brief" },
        surfaceOp: "append",
      },
      {
        type: "system/message",
        seq: 2,
        time: 200,
        data: { turn: 1, step: 1, content: "cite files" },
        surfaceOp: { op: "replace", startSeq: 1, endSeq: 1 },
        sourceEventSeqs: [99],
      },
    ];
    expect(rows(events).map((row) => row.seq)).toEqual([1, 2]);
  });

  it("keeps every row a compaction checkpoint shadowed, so the log stays readable", () => {
    const events: DshSessionEvent[] = [
      { type: "user/message", seq: 1, time: 100, data: { turn: 1, content: "first" } },
      {
        type: "assistant/message",
        seq: 2,
        time: 200,
        data: { turn: 1, step: 1, content: "reply" },
      },
      {
        type: "compaction/summary",
        seq: 3,
        time: 300,
        data: { compactionId: "c1", text: "summary" },
      },
      {
        // Compaction replaces the span it summarised, which on a log surface is
        // every row of it: folding those away would leave nothing to read.
        type: "user/message",
        seq: 4,
        time: 400,
        data: { turn: 1, content: "checkpoint" },
        surfaceOp: { op: "replace", startSeq: 1, endSeq: 2 },
        sourceEventSeqs: [1, 2, 3],
      },
    ];
    expect(rows(events).map((row) => row.seq)).toEqual([1, 2, 3, 4]);
  });
});
