/**
 * Ledger fold for the DeepSeek Harness trajectory panel.
 *
 * The panel reads a session as a list of operations rather than of events: a
 * tool call and the result that settles it are one row, and the streaming chunks
 * that make up a reply are evidence for that reply's timing instead of rows of
 * their own. That is the same set the timing overview plots, so clicking a bar
 * always lands on a row that exists.
 *
 * A tool's schema is stated only in a `request/header` event, and stated once
 * per request, so the fold carries the newest header it has walked past and
 * resolves a call against it by tool name — the same window upstream uses
 * (`ui-trajectory/src/client/trajectory-snapshot-builder.ts:224`), and by name
 * because Aeroric's `tool/call` carries `name` while the header carries only
 * definitions. The Harness emits no parent call id
 * (`packages/core/agent-loop/src/tool-calls.ts:263`), so a call nests under the
 * assistant message that ordered it and never deeper.
 */

import type { DshDict, DshTrajectoryEntry, DshSessionEvent } from "./dshSessionFeatures";
import {
  dshDict,
  dshEventCallId,
  dshEventData,
  dshEventIsError,
  dshIsTokenChunk,
  dshNumber,
  dshText,
  dshUsage,
} from "./dshSessionFeatures";

/** The coloured kind chip a row carries, derived from the event type. */
export type DshLedgerTag =
  | "USER"
  | "ASSISTANT"
  | "TOOL"
  | "TURN"
  | "STEP"
  | "REQUEST"
  | "WORKFLOW"
  | "COMMAND"
  | "SCHEDULE"
  | "COMPACT"
  | "FEEDBACK"
  | "SYSTEM";

/** How a row ended, which is what the detail column reports as `Status`. */
export type DshLedgerStatus = "complete" | "running" | "error";

/** The four groups the ledger's segmented filter offers. */
export type DshLedgerCategory = "message" | "tool" | "lifecycle" | "system";

/** A tool definition as one `request/header` stated it. */
export interface DshLedgerToolSchema {
  name: string;
  description: string;
  parameters: unknown;
}

/** The token counts an assistant message reported. */
export interface DshLedgerUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
}

/** One operation of the session, as the ledger draws it. */
export interface DshLedgerRow {
  /** The anchoring event's seq, and the row's identity. */
  seq: number;
  /** Every event folded into this row, in event order. */
  seqs: readonly number[];
  tag: DshLedgerTag;
  title: string;
  /** One line for the row list; empty when the event carried no detail. */
  preview: string;
  startedAt: number;
  /** Measured span; absent means no boundary closed the operation. */
  durationMs?: number;
  status: DshLedgerStatus;
  turn?: number;
  step?: number;
  /** 0 a turn boundary, 1 a turn's own row, 2 a call under an assistant reply. */
  depth: 0 | 1 | 2;
  /** The assistant reply that ordered this call, for the `Hierarchy` link. */
  parentSeq?: number;
  callId?: string;
  toolName?: string;
  /** Formatted input detail; absent means the `Payload` tab is not offered. */
  payload?: string;
  /** Formatted output detail; absent means the `Result` tab is not offered. */
  result?: string;
  schema?: DshLedgerToolSchema;
  usage?: DshLedgerUsage;
  ttftMs?: number;
  decodeMs?: number;
  /** The projected entry, for the image gallery, tool card and mention prose. */
  entry: DshTrajectoryEntry;
}

/** One turn's rows, in event order. */
export interface DshLedgerGroup {
  /** Absent for the rows a session recorded outside any turn. */
  turn?: number;
  startedAt: number;
  rows: readonly DshLedgerRow[];
}

/** The chip an event type carries. `SYSTEM` keeps an unknown event renderable. */
export function dshLedgerTag(type: string): DshLedgerTag {
  if (type.startsWith("user/")) return "USER";
  if (type.startsWith("assistant/")) return "ASSISTANT";
  // `tool-workflow/` is a fan-out lifecycle, not a call, so it is checked first.
  if (type.startsWith("tool-workflow/") || type.startsWith("workflow/")) return "WORKFLOW";
  if (type.startsWith("tool/")) return "TOOL";
  if (type.startsWith("turn/")) return "TURN";
  if (type.startsWith("step/")) return "STEP";
  if (type.startsWith("request/")) return "REQUEST";
  if (type.startsWith("command/")) return "COMMAND";
  if (type.startsWith("schedule/")) return "SCHEDULE";
  if (type.startsWith("compaction/")) return "COMPACT";
  if (type.startsWith("feedback/")) return "FEEDBACK";
  return "SYSTEM";
}

/** The filter group a chip belongs to, so chip and segment can never disagree. */
export function dshLedgerCategory(tag: DshLedgerTag): DshLedgerCategory {
  if (tag === "USER" || tag === "ASSISTANT") return "message";
  if (tag === "TOOL" || tag === "WORKFLOW") return "tool";
  if (tag === "TURN" || tag === "STEP") return "lifecycle";
  return "system";
}

const PREVIEW_CHARS = 160;

/** A mutable row, so the fold can settle a call once its result arrives. */
interface Draft extends DshLedgerRow {
  seqs: number[];
}

function eventData(entry: DshTrajectoryEntry): DshDict {
  return dshEventData(entry.event);
}

function stepKeyOf(entry: DshTrajectoryEntry): string | undefined {
  return entry.turn !== undefined && entry.step !== undefined
    ? `${entry.turn}:${entry.step}`
    : undefined;
}

/** The detail's first non-blank line, clipped to one row's worth of text. */
function previewOf(detail: string | undefined): string {
  if (detail === undefined) return "";
  const line =
    detail
      .split(/\r?\n/)
      .find((candidate) => candidate.trim() !== "")
      ?.trim() ?? "";
  return line.length > PREVIEW_CHARS ? `${line.slice(0, PREVIEW_CHARS)}…` : line;
}

function formatJson(value: unknown): string | undefined {
  if (value === undefined || value === null) return undefined;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    // A cyclic payload has no readable form; the other tabs still work.
    return undefined;
  }
}

/**
 * Pretty-print a call's arguments, leaving a malformed one readable.
 *
 * `arguments` is the raw model output: JSON for a well-formed call and an
 * arbitrary string when the model got it wrong, which is exactly the case where
 * the user needs to see it verbatim rather than as an empty payload.
 */
function formatArguments(value: unknown): string | undefined {
  if (typeof value !== "string") return formatJson(value);
  const trimmed = value.trim();
  if (trimmed === "") return undefined;
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return value;
  }
}

/** Index one `request/header`'s tool definitions by name. */
function headerSchemas(event: DshSessionEvent): Map<string, DshLedgerToolSchema> {
  const index = new Map<string, DshLedgerToolSchema>();
  const tools = dshDict(dshEventData(event).header).tools;
  if (!Array.isArray(tools)) return index;
  for (const item of tools) {
    const tool = dshDict(item);
    // The Harness' own shape is flat; an OpenAI-shaped definition nests the tool
    // under `function`. Reading both keeps `Schema` populated either way.
    const inner = dshDict(tool.function);
    const name = dshText(tool.name) ?? dshText(inner.name);
    if (name === undefined || name === "") continue;
    index.set(name, {
      name,
      description: dshText(tool.description) ?? dshText(inner.description) ?? "",
      parameters: tool.parameters ?? inner.parameters ?? tool.inputSchema ?? inner.inputSchema,
    });
  }
  return index;
}

function usageOf(data: DshDict): DshLedgerUsage | undefined {
  const values = dshUsage(data);
  const read = (key: string) => dshNumber(values[key]) ?? 0;
  const usage: DshLedgerUsage = {
    inputTokens: read("inputTokens"),
    outputTokens: read("outputTokens"),
    cacheReadTokens: read("cacheReadTokens"),
    cacheWriteTokens: read("cacheWriteTokens"),
  };
  return Object.values(usage).some((value) => value > 0) ? usage : undefined;
}

/** The row's input detail, or undefined when the event carried no input. */
function payloadOf(entry: DshTrajectoryEntry): string | undefined {
  const data = dshEventData(entry.event);
  switch (entry.type) {
    case "tool/call":
      return formatArguments(data.arguments);
    case "user/message":
    case "assistant/message":
      return entry.detail || undefined;
    case "command/run":
      return dshText(data.args) || undefined;
    case "feedback/record":
      return dshText(data.text) || undefined;
    case "schedule/change":
      return formatJson(data.schedule);
    case "request/header":
      return formatJson(data.header);
    default:
      return undefined;
  }
}

/** The row's own output, for the rows that carry one without being paired. */
function resultOf(entry: DshTrajectoryEntry): string | undefined {
  switch (entry.type) {
    // Only reached for a result whose call is on a page not yet loaded; a paired
    // result is folded into its call instead.
    case "tool/result":
    case "command/done":
    case "compaction/summary":
      return entry.detail || undefined;
    default:
      return undefined;
  }
}

function draftRow(
  entry: DshTrajectoryEntry,
  schemas: ReadonlyMap<string, DshLedgerToolSchema>,
  parent: Draft | undefined,
): Draft {
  const event = entry.event;
  const data = dshEventData(event);
  const tag = dshLedgerTag(entry.type);
  const toolName = tag === "TOOL" ? dshText(data.name) : undefined;
  const id = tag === "TOOL" ? dshEventCallId(event) : undefined;
  const schema = toolName === undefined ? undefined : schemas.get(toolName);
  const payload = payloadOf(entry);
  const result = resultOf(entry);
  const usage = entry.type === "assistant/message" ? usageOf(data) : undefined;
  const nested = tag === "TOOL" && parent !== undefined;
  return {
    seq: entry.seq,
    seqs: [entry.seq],
    tag,
    title: entry.title,
    preview: previewOf(entry.detail),
    startedAt: entry.time,
    // A call is running until its result arrives; nothing else waits on a later
    // event to be readable.
    status: dshEventIsError(event) ? "error" : entry.type === "tool/call" ? "running" : "complete",
    depth: tag === "TURN" ? 0 : nested ? 2 : 1,
    ...(entry.turn === undefined ? {} : { turn: entry.turn }),
    ...(entry.step === undefined ? {} : { step: entry.step }),
    ...(nested && parent !== undefined ? { parentSeq: parent.seq } : {}),
    ...(id === undefined ? {} : { callId: id }),
    ...(toolName === undefined ? {} : { toolName }),
    ...(payload === undefined ? {} : { payload }),
    ...(result === undefined ? {} : { result }),
    ...(schema === undefined ? {} : { schema }),
    ...(usage === undefined ? {} : { usage }),
    entry,
  };
}

/** Give a lifecycle row the width of the boundary event that closed it. */
function closeRow(row: Draft | undefined, at: number): void {
  if (row === undefined || row.durationMs !== undefined) return;
  row.durationMs = Math.max(0, at - row.startedAt);
}

/**
 * Cut the flat rows into turns.
 *
 * A row that reports no turn joins the group it arrived in, so an out-of-band
 * event such as a schedule change stays where it happened instead of opening a
 * group of its own.
 */
function groupRows(rows: readonly DshLedgerRow[]): DshLedgerGroup[] {
  const groups: DshLedgerGroup[] = [];
  let current: { turn?: number; startedAt: number; rows: DshLedgerRow[] } | undefined;
  for (const row of rows) {
    if (current === undefined || (row.turn !== undefined && row.turn !== current.turn)) {
      current = {
        ...(row.turn === undefined ? {} : { turn: row.turn }),
        startedAt: row.startedAt,
        rows: [],
      };
      groups.push(current);
    }
    current.rows.push(row);
  }
  return groups;
}

/**
 * Fold the projected entries into the ledger the trajectory panel draws.
 *
 * @param entries - The session projection's trajectory, in event order.
 * @returns One group per turn, each holding that turn's rows.
 */
export function deriveDshLedger(entries: readonly DshTrajectoryEntry[]): readonly DshLedgerGroup[] {
  const rows: Draft[] = [];
  const openCalls = new Map<string, Draft>();
  const openTurns = new Map<number, Draft>();
  const openStepRows = new Map<string, Draft>();
  const openSteps = new Map<string, { start: number; firstToken?: number }>();
  let schemas: ReadonlyMap<string, DshLedgerToolSchema> = new Map();
  let activeTurn: number | undefined;
  let lastAssistant: Draft | undefined;

  for (const entry of entries) {
    const event = entry.event;
    const stepKey = stepKeyOf(entry);
    // A call belongs to the reply that ordered it, so the nesting parent resets
    // with the turn rather than carrying across a turn boundary.
    if (entry.turn !== undefined && entry.turn !== activeTurn) {
      activeTurn = entry.turn;
      lastAssistant = undefined;
    }

    if (entry.type === "step/start" && stepKey !== undefined) {
      openSteps.set(stepKey, { start: entry.time });
    }

    if (entry.type === "assistant/chunk") {
      const open = stepKey === undefined ? undefined : openSteps.get(stepKey);
      if (
        open !== undefined &&
        open.firstToken === undefined &&
        dshIsTokenChunk(eventData(entry))
      ) {
        open.firstToken = entry.time;
      }
      continue;
    }

    if (entry.type === "tool/result") {
      const id = dshEventCallId(event);
      const open = id === undefined ? undefined : openCalls.get(id);
      if (id !== undefined && open !== undefined) {
        open.seqs = [...open.seqs, entry.seq];
        open.durationMs = Math.max(0, entry.time - open.startedAt);
        open.status = dshEventIsError(event) ? "error" : "complete";
        const result = entry.detail || undefined;
        if (result !== undefined) open.result = result;
        openCalls.delete(id);
        continue;
      }
      // The call is on a page that has not been loaded: stand the orphan result
      // up as its own row rather than dropping the only trace of the call.
    }

    if (entry.type === "turn/end" && entry.turn !== undefined) {
      closeRow(openTurns.get(entry.turn), entry.time);
      openTurns.delete(entry.turn);
    }
    if (entry.type === "step/end" && stepKey !== undefined) {
      closeRow(openStepRows.get(stepKey), entry.time);
      openStepRows.delete(stepKey);
    }

    const row = draftRow(entry, schemas, lastAssistant);
    rows.push(row);

    // The header applies to the calls that follow it, never to itself.
    if (entry.type === "request/header") schemas = headerSchemas(event);
    if (entry.type === "turn/start" && entry.turn !== undefined) openTurns.set(entry.turn, row);
    if (entry.type === "step/start" && stepKey !== undefined) openStepRows.set(stepKey, row);
    if (entry.type === "tool/call") {
      const id = dshEventCallId(event);
      if (id !== undefined) openCalls.set(id, row);
    }
    if (entry.type === "assistant/message") {
      lastAssistant = row;
      const open = stepKey === undefined ? undefined : openSteps.get(stepKey);
      if (open !== undefined && stepKey !== undefined) {
        // A reply is measured from the request that asked for it, the same
        // pairing the stats panel sums, so the two can never disagree.
        row.startedAt = open.start;
        row.durationMs = Math.max(0, entry.time - open.start);
        if (open.firstToken !== undefined) {
          row.ttftMs = Math.max(0, open.firstToken - open.start);
          row.decodeMs = Math.max(0, entry.time - open.firstToken);
        }
        openSteps.delete(stepKey);
      }
    }
  }
  return groupRows(rows);
}

/** Every row of a ledger in event order, for seq lookup and range filtering. */
export function dshLedgerRows(groups: readonly DshLedgerGroup[]): readonly DshLedgerRow[] {
  return groups.flatMap((group) => group.rows);
}
