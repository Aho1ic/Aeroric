/**
 * Pure projections for the advanced DeepSeek Harness conversation surfaces.
 * The Web UI normally builds these from its conversation runtime; Aeroric gets
 * the same durable session events over `session/event`, so keeping the fold
 * here makes live updates and history replay deterministic and testable.
 */

import type { DshImageAttachmentRef } from "./dshImageAttachments";
import { collectDshImageAttachments } from "./dshImageAttachments";
import type { DshToolEventView } from "./dshToolViews";
import { parseDshToolEventView } from "./dshToolViews";

export interface DshSessionEvent {
  type: string;
  seq?: number;
  time?: number;
  data?: unknown;
  [key: string]: unknown;
}

export interface DshTrajectoryEntry {
  seq: number;
  time: number;
  type: string;
  turn?: number;
  step?: number;
  title: string;
  detail?: string;
  event: DshSessionEvent;
  /**
   * Host-computed render intent for this delivery of a `tool/call` or
   * `tool/result`, when the Harness produced one. Absent for every other event
   * type and for a tool whose presenter declined, in which case the caller
   * renders the raw event.
   */
  view?: DshToolEventView;
  /**
   * Durable image references this event's content carries, in block order.
   * Absent for the vast majority of events, which are text only.
   */
  images?: readonly DshImageAttachmentRef[];
}

/** Render intents keyed by the event `seq` they accompanied. */
export type DshToolViewsBySeq = Readonly<Record<number, DshToolEventView>>;

export interface DshStats {
  turns: number;
  steps: number;
  llmMs: number;
  toolMs: number;
  ttftMs: number;
  ttftSteps: number;
  decodeMs: number;
  decodeTokens: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
}

export interface DshProducedFile {
  path: string;
  seq: number;
  turn?: number;
}

export type DshWorkflowStatus = "running" | "completed" | "failed" | "cancelled" | "interrupted";

export interface DshWorkflowMember {
  seq: number;
  label: string;
  childId: string;
  phase?: string;
  status: DshWorkflowStatus;
}

export interface DshWorkflowRun {
  runId: string;
  name: string;
  status: DshWorkflowStatus;
  phases: Record<string, { phase?: string; members: DshWorkflowMember[] }>;
}

export interface DshScheduleRecord {
  id: string;
  kind: string;
  prompt: string;
  scheduledAt: string;
  everySeconds?: number;
  afterSeconds?: number;
  state: "scheduled" | "overdue" | "dispatched" | "deleted";
}

export interface DshSessionFeatures {
  events: DshSessionEvent[];
  trajectory: DshTrajectoryEntry[];
  stats: DshStats;
  producedFiles: DshProducedFile[];
  workflows: DshWorkflowRun[];
  schedules: DshScheduleRecord[];
  /** Measured operations for the timing overview, in event order. */
  timeline: DshTimelineRecord[];
}

type Dict = Record<string, unknown>;

function dict(value: unknown): Dict {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Dict)
    : {};
}

function text(value: unknown): string | undefined {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return undefined;
}

function number(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function eventData(event: DshSessionEvent): Dict {
  return dict(event.data);
}

function contentText(value: unknown): string {
  if (typeof value === "string") return value;
  if (!Array.isArray(value)) return "";
  return value
    .map((part) => {
      const item = dict(part);
      return text(item.text) ?? text(item.content) ?? "";
    })
    .join("");
}

/**
 * The content array of a message-shaped event.
 *
 * The Harness delivers content either inline on `data` or wrapped in
 * `data.message`, depending on the event; the inline array wins so an event that
 * carries both is not counted twice.
 */
function messageContent(data: Dict): unknown {
  return Array.isArray(data.content) ? data.content : dict(data.message).content;
}

/** Durable image references one event's content carries, in block order. */
function eventImages(event: DshSessionEvent): DshImageAttachmentRef[] {
  return collectDshImageAttachments(messageContent(eventData(event)));
}

function eventTurn(event: DshSessionEvent): number | undefined {
  return number(eventData(event).turn);
}

function eventStep(event: DshSessionEvent): number | undefined {
  return number(eventData(event).step);
}

function eventTime(event: DshSessionEvent): number {
  return number(event.time) ?? 0;
}

function eventSeq(event: DshSessionEvent, fallback: number): number {
  return number(event.seq) ?? fallback;
}

function usage(data: Dict): Dict {
  return dict(data.usage ?? dict(data.message).usage);
}

function usageNumber(data: Dict, key: string): number {
  return number(usage(data)[key]) ?? 0;
}

/**
 * The fragments one packed delta run carries, in stream order.
 *
 * A run packs one block's deltas without joining their boundaries, so the
 * fragments are the original chunks' texts — `args` for a tool call's arguments
 * and `texts` for model output
 * (`packages/llm/llm/src/assistant-stream.ts:21`).
 */
function runFragments(record: Dict): string[] {
  const list = record.type === "tool-call-chunks" ? record.args : record.texts;
  if (!Array.isArray(list)) return [];
  return list.map((item) => text(item) ?? "");
}

/**
 * Whether one raw stream chunk carried model output.
 *
 * Mirrors `isTokenDelta` (`packages/llm/llm/src/assistant-stream.ts:246`): a
 * usage, finish, or block frame reports the call's shape rather than its output,
 * so it must not start the first-token clock. A tool-call delta counts as soon
 * as it names the tool, which is output even before any arguments arrive.
 */
function isTokenDelta(chunk: Dict): boolean {
  switch (chunk.type) {
    case "text-delta":
    case "reasoning-delta":
      return (text(chunk.text) ?? "") !== "";
    case "tool-call-delta":
      return (text(chunk.argumentsDelta) ?? "") !== "" || text(chunk.name) !== undefined;
    default:
      return false;
  }
}

/**
 * When one packed run first produced a token.
 *
 * A run's members are not events of their own, so each member's time is
 * reconstructed from the run's `time0` and the gaps `dt` holds — the same walk
 * `runFirstTokenTime` does upstream
 * (`packages/llm/llm/src/assistant-stream.ts:299`).
 */
function runFirstTokenTime(record: Dict): number | undefined {
  const time0 = number(record.time0);
  if (time0 === undefined) return undefined;
  if (record.type === "tool-call-chunks" && text(record.name) !== undefined) return time0;
  const gaps = Array.isArray(record.dt) ? record.dt : [];
  let at = time0;
  for (const [index, fragment] of runFragments(record).entries()) {
    if (index > 0) at += number(gaps[index - 1]) ?? 0;
    if (fragment !== "") return at;
  }
  return undefined;
}

/**
 * When an assistant attempt's stream first produced a token.
 *
 * The Harness retired the per-token `assistant/chunk` event: an attempt's exact
 * timed stream now rides the event that settles it — `assistant/message.stream`
 * for a committed reply and `assistant/attempt.stream` for one that committed
 * nothing (`packages/core/session/src/types.ts:335`) — as compact records. So
 * time to first token is read from those records rather than from the arrival of
 * a chunk event, which is also why it no longer depends on which events a page
 * happened to load.
 */
function streamFirstTokenTime(value: unknown): number | undefined {
  if (!Array.isArray(value)) return undefined;
  for (const item of value) {
    const record = dict(item);
    const at =
      record.type === "chunk"
        ? isTokenDelta(dict(record.chunk))
          ? number(record.time)
          : undefined
        : runFirstTokenTime(record);
    if (at !== undefined) return at;
  }
  return undefined;
}

/**
 * The text one compact stream carried, for the one kind of delta asked for.
 *
 * An attempt that committed no message left its stream as the only trace of
 * what the model produced, and a failed attempt often produced reasoning and no
 * text at all — so the two are read separately rather than concatenated into one
 * misleading reply.
 */
function streamText(value: unknown, kind: "text" | "reasoning"): string {
  if (!Array.isArray(value)) return "";
  const run = kind === "text" ? "text-chunks" : "reasoning-chunks";
  const delta = kind === "text" ? "text-delta" : "reasoning-delta";
  let out = "";
  for (const item of value) {
    const record = dict(item);
    if (record.type === run) {
      out += runFragments(record).join("");
      continue;
    }
    const chunk = dict(record.chunk);
    if (record.type === "chunk" && chunk.type === delta) out += text(chunk.text) ?? "";
  }
  return out;
}

function preview(event: DshSessionEvent): { title: string; detail?: string } {
  const data = eventData(event);
  switch (event.type) {
    case "user/message":
      return {
        title: "User message",
        detail: contentText(data.content) || contentText(dict(data.message).content),
      };
    case "assistant/message":
      return {
        title: "Assistant message",
        detail: contentText(data.content ?? dict(data.message).content),
      };
    case "assistant/attempt": {
      // An attempt event exists only because the model committed no message, so
      // its stream is the whole record of what that attempt produced. Reasoning
      // stands in for text because a failed attempt often produced only that.
      const streamed = streamText(data.stream, "text") || streamText(data.stream, "reasoning");
      return { title: "Assistant attempt", detail: streamed || undefined };
    }
    case "system/message":
      return {
        title: "System prompt",
        detail: contentText(data.content ?? dict(data.message).content),
      };
    case "tool/call":
      return { title: `Tool: ${text(data.name) ?? "tool"}`, detail: text(data.arguments) };
    case "tool/result":
      return {
        title: "Tool result",
        detail: contentText(data.content ?? dict(data.message).content),
      };
    // A `run_code` program dispatches tools of its own. The pair reads in
    // `tool/call`'s vocabulary so a sub-call renders through the same path as a
    // native one (`packages/core/tools/src/types.ts:40`).
    case "tool/ptc-dispatch-start":
      return { title: `Tool: ${text(data.name) ?? "tool"}` };
    case "tool/ptc-dispatch":
      return {
        title: "Tool result",
        detail: contentText(data.content ?? dict(data.message).content),
      };
    case "workflow/run-start":
    case "tool-workflow/run-start":
      return { title: `Workflow: ${text(data.name) ?? "run"}` };
    case "workflow/run-end":
    case "tool-workflow/run-end":
      return { title: `Workflow ${text(data.stopReason) ?? "finished"}` };
    case "schedule/change":
      return {
        title: `Schedule ${text(data.operation) ?? "change"}`,
        detail: text(dict(data.schedule).prompt) ?? text(data.id),
      };
    case "feedback/record":
      return { title: "Feedback recorded", detail: text(data.text) };
    case "command/run":
      return { title: `/${text(data.name) ?? "command"}`, detail: text(data.args) };
    case "command/done":
      return { title: "Command completed", detail: text(data.text) ?? contentText(data.content) };
    case "compaction/summary":
      return { title: "Compaction summary", detail: text(data.text) ?? contentText(data.content) };
    default:
      return { title: event.type };
  }
}

function isErrorResult(event: DshSessionEvent): boolean {
  const data = eventData(event);
  if (data.isError === true) return true;
  const message = dict(data.message);
  if (message.isError === true) return true;
  return (
    Array.isArray(message.content) && message.content.some((part) => dict(part).isError === true)
  );
}

/**
 * The id that pairs a call with the event settling it.
 *
 * A `run_code` sub-dispatch pairs on `subCallId` instead, because `callId` on
 * that pair names the parent program's own call
 * (`packages/core/tools/src/types.ts:43`); reading `subCallId` first keeps the
 * sub-call a call of its own rather than a second settlement of its parent.
 */
function callId(event: DshSessionEvent): string | undefined {
  const data = eventData(event);
  const source = dict(dict(data.message).source);
  return text(data.subCallId) ?? text(data.callId) ?? text(source.callId);
}

function parseJson(value: unknown): Dict {
  if (typeof value !== "string") return dict(value);
  try {
    return dict(JSON.parse(value));
  } catch {
    return {};
  }
}

/**
 * Readers the ledger fold in `dshTrajectoryLedger.ts` shares with this one.
 *
 * Both folds read the same durable events, so where a call id, an error flag or
 * a usage block lives has to be stated once: a second copy would drift the first
 * time the Harness moves a field, and the two surfaces would then disagree about
 * the same session.
 */
export {
  callId as dshEventCallId,
  contentText as dshContentText,
  dict as dshDict,
  eventData as dshEventData,
  isErrorResult as dshEventIsError,
  number as dshNumber,
  streamFirstTokenTime as dshStreamFirstTokenTime,
  text as dshText,
  usage as dshUsage,
};
export type { Dict as DshDict };

function candidatePaths(value: unknown): string[] {
  const paths: string[] = [];
  const add = (candidate: unknown) => {
    if (typeof candidate === "string" && candidate.trim()) paths.push(candidate.trim());
  };
  const walk = (node: unknown, key = "") => {
    if (typeof node === "string") {
      if (/^(path|file[_-]?path|filename|target)$/i.test(key)) add(node);
      return;
    }
    if (Array.isArray(node)) {
      node.forEach((item) => walk(item, key));
      return;
    }
    if (typeof node !== "object" || node === null) return;
    for (const [name, item] of Object.entries(node)) walk(item, name);
  };
  walk(value);
  return [...new Set(paths)];
}

function resultLocations(event: DshSessionEvent): string[] {
  const data = eventData(event);
  const meta = dict(data.meta);
  const locations = meta.locations ?? data.locations;
  const diffs = meta.diffs ?? data.diffs;
  return [...candidatePaths(locations), ...candidatePaths(diffs)];
}

function mutationCall(name: string): boolean {
  const normalized = name.toLowerCase();
  if (
    /delete|remove|read|list|search|inspect|cat|grep|bash|shell|terminal|run_code/.test(normalized)
  )
    return false;
  return /write|edit|patch|insert|replace|create|move|copy|save|update/.test(normalized);
}

function updateStats(events: DshSessionEvent[]): DshStats {
  const stats: DshStats = {
    turns: 0,
    steps: 0,
    llmMs: 0,
    toolMs: 0,
    ttftMs: 0,
    ttftSteps: 0,
    decodeMs: 0,
    decodeTokens: 0,
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
  };
  const turns = new Set<number>();
  const steps = new Map<string, number>();
  const calls = new Map<string, number>();
  for (const event of events) {
    const data = eventData(event);
    const turn = eventTurn(event);
    const step = eventStep(event);
    const stepKey = turn !== undefined && step !== undefined ? `${turn}:${step}` : undefined;
    if (event.type === "step/start" && stepKey) steps.set(stepKey, eventTime(event));
    if (event.type === "assistant/message" && stepKey) {
      const start = steps.get(stepKey);
      if (start !== undefined) {
        stats.llmMs += Math.max(0, eventTime(event) - start);
        // The reply carries the attempt's own timed stream, so the first token is
        // read from it rather than from a separately delivered chunk event.
        const firstToken = streamFirstTokenTime(data.stream);
        if (firstToken !== undefined) {
          stats.ttftMs += Math.max(0, firstToken - start);
          stats.ttftSteps += 1;
          stats.decodeMs += Math.max(0, eventTime(event) - firstToken);
          stats.decodeTokens += usageNumber(data, "outputTokens");
        }
        steps.delete(stepKey);
      }
      // Token accounting travels with the reply it belongs to: the Harness keeps
      // no separate usage record (`packages/core/session/src/types.ts:325`).
      stats.inputTokens += usageNumber(data, "inputTokens");
      stats.outputTokens += usageNumber(data, "outputTokens");
      stats.cacheReadTokens += usageNumber(data, "cacheReadTokens");
      stats.cacheWriteTokens += usageNumber(data, "cacheWriteTokens");
    }
    if (event.type === "tool/call") {
      const id = callId(event);
      if (id) calls.set(id, eventTime(event));
    }
    if (event.type === "tool/result") {
      const id = callId(event);
      const start = id ? calls.get(id) : undefined;
      if (start !== undefined) stats.toolMs += Math.max(0, eventTime(event) - start);
      if (id) calls.delete(id);
    }
    if (event.type === "step/end" && turn !== undefined) {
      stats.steps += 1;
      turns.add(turn);
    }
  }
  stats.turns = turns.size;
  return stats;
}

function updateWorkflows(events: DshSessionEvent[]): DshWorkflowRun[] {
  const runs = new Map<string, DshWorkflowRun>();
  for (const event of events) {
    const data = eventData(event);
    if (!event.type.startsWith("tool-workflow/")) continue;
    const id = text(data.runId);
    if (!id) continue;
    let run = runs.get(id);
    if (!run) {
      run = { runId: id, name: text(data.name) ?? id, status: "running", phases: {} };
      runs.set(id, run);
    }
    if (event.type === "tool-workflow/run-start") run.name = text(data.name) ?? run.name;
    if (event.type === "tool-workflow/run-end") {
      const reason = text(data.stopReason);
      run.status =
        reason === "completed"
          ? "completed"
          : reason === "cancelled"
            ? "cancelled"
            : reason === "error"
              ? "failed"
              : "interrupted";
    }
    if (event.type === "tool-workflow/agent-start") {
      const phase = text(data.phase);
      const key = phase ?? "(default)";
      const group = (run.phases[key] ??= { phase, members: [] });
      group.members.push({
        seq: number(data.seq) ?? eventSeq(event, 0),
        label: text(data.label) ?? "agent",
        childId: text(data.childId) ?? "",
        phase,
        status: "running",
      });
    }
    if (event.type === "tool-workflow/agent-end") {
      const memberSeq = number(data.seq);
      for (const group of Object.values(run.phases)) {
        const member = group.members.find((candidate) => candidate.seq === memberSeq);
        if (member) {
          const outcome = text(data.outcome);
          member.status =
            outcome === "completed"
              ? "completed"
              : outcome === "cancelled"
                ? "cancelled"
                : outcome === "failed"
                  ? "failed"
                  : "interrupted";
        }
      }
    }
  }
  return [...runs.values()];
}

function updateSchedules(events: DshSessionEvent[]): DshScheduleRecord[] {
  const schedules = new Map<string, DshScheduleRecord>();
  for (const event of events) {
    if (event.type !== "schedule/change") continue;
    const data = eventData(event);
    const operation = text(data.operation);
    if (operation === "create") {
      const schedule = dict(data.schedule);
      const id = text(schedule.id);
      const at = text(schedule.scheduledAt);
      if (!id || !at) continue;
      schedules.set(id, {
        id,
        kind: text(schedule.kind) ?? "after",
        prompt: text(schedule.prompt) ?? "",
        scheduledAt: at,
        everySeconds: number(schedule.everySeconds),
        afterSeconds: number(schedule.afterSeconds),
        state: Date.parse(at) <= Date.now() ? "overdue" : "scheduled",
      });
    } else if (operation === "delete") {
      const id = text(data.id);
      if (id && schedules.has(id)) schedules.get(id)!.state = "deleted";
    } else if (operation === "dispatch") {
      const id = text(data.id);
      const current = id ? schedules.get(id) : undefined;
      if (!current) continue;
      if (current.kind === "every" && current.everySeconds && current.everySeconds > 0) {
        const anchor = Date.parse(current.scheduledAt);
        const accepted = Date.parse(text(data.acceptedAt) ?? "");
        if (Number.isFinite(anchor) && Number.isFinite(accepted)) {
          const next =
            anchor +
            (Math.floor((accepted - anchor) / (current.everySeconds * 1000)) + 1) *
              current.everySeconds *
              1000;
          current.scheduledAt = new Date(next).toISOString();
          current.state = next <= Date.now() ? "overdue" : "scheduled";
        }
      } else {
        current.state = "dispatched";
      }
    }
  }
  return [...schedules.values()];
}

function updateProduced(events: DshSessionEvent[]): DshProducedFile[] {
  const calls = new Map<string, { name: string; args: Dict; turn?: number }>();
  const files: DshProducedFile[] = [];
  const seen = new Set<string>();
  for (const event of events) {
    if (event.type === "tool/call") {
      const id = callId(event);
      const data = eventData(event);
      if (id)
        calls.set(id, {
          name: text(data.name) ?? "tool",
          args: parseJson(data.arguments),
          turn: eventTurn(event),
        });
      continue;
    }
    if (event.type !== "tool/result" || isErrorResult(event)) continue;
    const id = callId(event);
    const call = id ? calls.get(id) : undefined;
    const paths = resultLocations(event);
    if (call && mutationCall(call.name)) paths.push(...candidatePaths(call.args));
    for (const path of [...new Set(paths)]) {
      if (seen.has(path)) continue;
      seen.add(path);
      files.push({
        path,
        seq: eventSeq(event, files.length),
        turn: eventTurn(event) ?? call?.turn,
      });
    }
    if (id) calls.delete(id);
  }
  return files;
}

/** What a timeline record is, which fixes the lane it is drawn in. */
export type DshTimelineKind = "user" | "assistant" | "tool" | "compacted" | "system";

/**
 * One measured operation of the session, as the timing overview sees it.
 *
 * A record is an operation rather than an event: a tool call and its result are
 * one span whose width is the time between them, and an assistant reply is one
 * span from its step start. The streaming chunks that make up a reply carry no
 * duration of their own and are left out, as the Harness' own ledger leaves out
 * its request-only rows.
 */
export interface DshTimelineRecord {
  /** Ledger rows this record covers — what focusing it selects. */
  seqs: readonly number[];
  kind: DshTimelineKind;
  label: string;
  isError: boolean;
  /** Wall-clock start in ms. */
  startedAt: number;
  /** Measured span in ms; zero for an operation observed at one instant. */
  durationMs: number;
  turn?: number;
  /** Time to first token, when the whole step was observed. */
  ttftMs?: number;
  /** Decode time after the first token, when the whole step was observed. */
  decodeMs?: number;
}

function timelineKind(type: string): DshTimelineKind | undefined {
  if (type === "user/message") return "user";
  // An attempt is a model call that committed no message, so it is plotted in
  // the same lane: the time it spent is the session's time either way.
  if (type === "assistant/message" || type === "assistant/attempt") return "assistant";
  if (type === "tool/call" || type === "tool/ptc-dispatch-start") return "tool";
  if (type === "compaction/summary") return "compacted";
  // Step boundaries are how the other records are measured, never records
  // themselves; a result belongs to the call it settles.
  if (type === "step/start" || type === "step/end") return undefined;
  if (type === "tool/result" || type === "tool/ptc-dispatch") return undefined;
  return "system";
}

/**
 * Fold the event stream into the operations the timing overview plots.
 *
 * Durations come from the same pairings the stats panel sums — a call to the
 * event settling it by call id, `step/start` to the `assistant/message` that
 * closes it — so the overview and the totals can never disagree. An operation
 * still open when the page ends keeps a zero width rather than being stretched
 * to now, which would make a live session's last span grow on every render.
 */
function updateTimeline(events: DshSessionEvent[], titles: ReadonlyMap<number, string>) {
  const records: DshTimelineRecord[] = [];
  const openTools = new Map<string, DshTimelineRecord>();
  const openSteps = new Map<string, number>();
  for (const [index, event] of events.entries()) {
    const seq = eventSeq(event, index);
    const turn = eventTurn(event);
    const step = eventStep(event);
    const stepKey = turn !== undefined && step !== undefined ? `${turn}:${step}` : undefined;
    if (event.type === "step/start" && stepKey) {
      openSteps.set(stepKey, eventTime(event));
    }
    if (event.type === "tool/result" || event.type === "tool/ptc-dispatch") {
      const id = callId(event);
      const open = id === undefined ? undefined : openTools.get(id);
      if (id !== undefined && open) {
        open.durationMs = Math.max(0, eventTime(event) - open.startedAt);
        open.seqs = [...open.seqs, seq];
        if (isErrorResult(event)) open.isError = true;
        openTools.delete(id);
      }
      continue;
    }
    const kind = timelineKind(event.type);
    if (kind === undefined) continue;
    const record: DshTimelineRecord = {
      seqs: [seq],
      kind,
      label: titles.get(seq) ?? event.type,
      isError: kind === "system" ? isErrorResult(event) : false,
      startedAt: eventTime(event),
      durationMs: 0,
      ...(turn === undefined ? {} : { turn }),
    };
    if (kind === "assistant") {
      const start = stepKey === undefined ? undefined : openSteps.get(stepKey);
      if (start !== undefined) {
        record.startedAt = start;
        record.durationMs = Math.max(0, eventTime(event) - start);
        const firstToken = streamFirstTokenTime(eventData(event).stream);
        if (firstToken !== undefined) {
          record.ttftMs = Math.max(0, firstToken - start);
          record.decodeMs = Math.max(0, eventTime(event) - firstToken);
        }
        // Only a committed reply closes the step: a retried attempt leaves it
        // open so the reply that finally settles it is still measured from the
        // request, rather than from the moment the failed attempt gave up.
        if (event.type === "assistant/message" && stepKey !== undefined) openSteps.delete(stepKey);
      }
      // An attempt committed nothing, so there is no reply to blame for it.
      if (event.type === "assistant/attempt") record.isError = true;
    }
    if (kind === "tool") {
      const id = callId(event);
      if (id) openTools.set(id, record);
    }
    records.push(record);
  }
  return records;
}

export function projectDshSessionEvents(
  input: readonly DshSessionEvent[],
  views: DshToolViewsBySeq = {},
): DshSessionFeatures {
  const events = [...input]
    .filter((event) => typeof event?.type === "string")
    .sort((a, b) => eventSeq(a, 0) - eventSeq(b, 0));
  const trajectory = events.map((event, index) => {
    const seq = eventSeq(event, index);
    const view = views[seq];
    const images = eventImages(event);
    return {
      seq,
      time: eventTime(event),
      type: event.type,
      turn: eventTurn(event),
      step: eventStep(event),
      ...preview(event),
      event,
      ...(view ? { view } : {}),
      ...(images.length > 0 ? { images } : {}),
    };
  });
  return {
    events,
    trajectory,
    stats: updateStats(events),
    producedFiles: updateProduced(events),
    workflows: updateWorkflows(events),
    schedules: updateSchedules(events),
    timeline: updateTimeline(events, new Map(trajectory.map((entry) => [entry.seq, entry.title]))),
  };
}

/**
 * Merge newly delivered events into the ordered event list.
 *
 * Live streaming delivers events in ascending `seq`, one per model token, so the
 * common case is "everything incoming sits after everything current". Rebuilding
 * a Map over the concatenation and re-sorting it on every token made this
 * O(n log n) per token, i.e. Θ(n² log n) across a turn — the reason a long
 * session got progressively slower at everything. That case is now a plain
 * concat. The Map rebuild is kept as the fallback for history pages (which
 * prepend) and for out-of-order or duplicated deliveries.
 */
export function mergeDshSessionEvents(
  current: readonly DshSessionEvent[],
  incoming: readonly DshSessionEvent[],
): DshSessionEvent[] {
  if (incoming.length === 0) return current as DshSessionEvent[];
  if (current.length === 0 && incoming.length === 1) return [...incoming];

  let currentMax = -Infinity;
  let currentOrdered = true;
  for (const event of current) {
    const seq = number(event.seq);
    if (seq === undefined) {
      currentOrdered = false;
      break;
    }
    if (seq <= currentMax) {
      currentOrdered = false;
      break;
    }
    currentMax = seq;
  }

  if (currentOrdered) {
    let appendOnly = true;
    let incomingMax = currentMax;
    for (const event of incoming) {
      const seq = number(event.seq);
      if (seq === undefined || seq <= incomingMax) {
        appendOnly = false;
        break;
      }
      incomingMax = seq;
    }
    if (appendOnly) return [...current, ...incoming];
  }

  const bySeq = new Map<number, DshSessionEvent>();
  const withoutSeq: DshSessionEvent[] = [];
  for (const event of [...current, ...incoming]) {
    const seq = number(event.seq);
    if (seq === undefined) withoutSeq.push(event);
    else bySeq.set(seq, event);
  }
  return [...bySeq.values(), ...withoutSeq].sort(
    (a, b) => (number(a.seq) ?? 0) - (number(b.seq) ?? 0),
  );
}

/**
 * Split one `session.history` page into its events and their render intents.
 *
 * The Harness serves a page as `HistoryEntry[]` — `{ event, view? }` wrappers,
 * not bare events — so a reader that looks for `type` on the page item finds
 * nothing and silently drops the whole page. Bare events are still accepted so
 * a page from a deployment without the wrapper (and Aeroric's own live merge
 * path, which already holds unwrapped events) reads the same way.
 */
export function readDshHistoryPage(entries: readonly unknown[]): {
  events: DshSessionEvent[];
  views: Record<number, DshToolEventView>;
} {
  const events: DshSessionEvent[] = [];
  const views: Record<number, DshToolEventView> = {};
  for (const entry of entries) {
    const record = dict(entry);
    // A wrapper carries the event under `event`; a bare event carries its own
    // `type`. Checking the wrapper first keeps an event that happens to have an
    // `event` field from being misread, since a wrapper never has a `type`.
    const inner = typeof record.type === "string" ? record : dict(record.event);
    if (typeof inner.type !== "string") continue;
    const event = inner as DshSessionEvent;
    events.push(event);
    const view = parseDshToolEventView(record.view);
    const seq = number(event.seq);
    if (view !== undefined && seq !== undefined) views[seq] = view;
  }
  return { events, views };
}

/**
 * Whether a history failure means the Harness has no such session.
 *
 * The RPC answers with the raw `session "<id>" not found`, which reads as a
 * crash in the session detail view. Recognising it lets the UI show a plain
 * "this session is gone" line instead, and leaves every other failure — a dead
 * `dsh web`, a transport error — reported verbatim so it stays diagnosable.
 */
export function isDshSessionMissingError(message: string): boolean {
  return /session\b[\s\S]*\bnot found/i.test(message);
}
