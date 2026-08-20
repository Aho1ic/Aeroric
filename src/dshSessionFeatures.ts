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
 * Whether a streaming chunk carried model output.
 *
 * Time to first token is measured from the first chunk that actually produced
 * something, so a chunk that only reports usage or a tool-call frame must not
 * start the clock.
 */
function isTokenChunk(data: Dict): boolean {
  const chunk = dict(data.chunk);
  const kind = text(chunk.type) ?? "";
  return (
    kind === "text" || kind === "reasoning" || kind === "token" || text(chunk.text) !== undefined
  );
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
    case "assistant/chunk":
      return { title: "Assistant stream", detail: text(dict(data.chunk).text) ?? text(data.chunk) };
    case "tool/call":
      return { title: `Tool: ${text(data.name) ?? "tool"}`, detail: text(data.arguments) };
    case "tool/result":
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

function callId(event: DshSessionEvent): string | undefined {
  const data = eventData(event);
  const source = dict(dict(data.message).source);
  return text(data.callId) ?? text(source.callId);
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
  isTokenChunk as dshIsTokenChunk,
  number as dshNumber,
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
  const steps = new Map<string, { start: number; firstToken?: number }>();
  const calls = new Map<string, number>();
  for (const event of events) {
    const data = eventData(event);
    const turn = eventTurn(event);
    const step = eventStep(event);
    const stepKey = turn !== undefined && step !== undefined ? `${turn}:${step}` : undefined;
    if (event.type === "step/start" && stepKey) steps.set(stepKey, { start: eventTime(event) });
    if (event.type === "assistant/chunk" && stepKey) {
      const open = steps.get(stepKey);
      if (isTokenChunk(data) && open && open.firstToken === undefined)
        open.firstToken = eventTime(event);
    }
    if (event.type === "assistant/message" && stepKey) {
      const open = steps.get(stepKey);
      if (open) {
        stats.llmMs += Math.max(0, eventTime(event) - open.start);
        if (open.firstToken !== undefined) {
          stats.ttftMs += Math.max(0, open.firstToken - open.start);
          stats.ttftSteps += 1;
          const output = usageNumber(data, "outputTokens");
          stats.decodeMs += Math.max(0, eventTime(event) - open.firstToken);
          stats.decodeTokens += output;
        }
        steps.delete(stepKey);
      }
      stats.inputTokens += usageNumber(data, "inputTokens");
      stats.outputTokens += usageNumber(data, "outputTokens");
      stats.cacheReadTokens += usageNumber(data, "cacheReadTokens");
      stats.cacheWriteTokens += usageNumber(data, "cacheWriteTokens");
    }
    if (event.type === "assistant/chunk") {
      const chunkUsage = dict(dict(data.chunk).usage);
      stats.inputTokens = Math.max(stats.inputTokens, number(chunkUsage.inputTokens) ?? 0);
      stats.outputTokens = Math.max(stats.outputTokens, number(chunkUsage.outputTokens) ?? 0);
      stats.cacheReadTokens = Math.max(
        stats.cacheReadTokens,
        number(chunkUsage.cacheReadTokens) ?? 0,
      );
      stats.cacheWriteTokens = Math.max(
        stats.cacheWriteTokens,
        number(chunkUsage.cacheWriteTokens) ?? 0,
      );
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
  if (type === "assistant/message") return "assistant";
  if (type === "tool/call") return "tool";
  if (type === "compaction/summary") return "compacted";
  // Streaming chunks and step boundaries are how the other records are measured,
  // never records themselves; a result belongs to the call it settles.
  if (type === "assistant/chunk" || type === "step/start" || type === "step/end") return undefined;
  if (type === "tool/result") return undefined;
  return "system";
}

/**
 * Fold the event stream into the operations the timing overview plots.
 *
 * Durations come from the same pairings the stats panel sums — `tool/call` to
 * its `tool/result` by call id, `step/start` to the `assistant/message` that
 * closes it — so the overview and the totals can never disagree. An operation
 * still open when the page ends keeps a zero width rather than being stretched
 * to now, which would make a live session's last span grow on every render.
 */
function updateTimeline(events: DshSessionEvent[], titles: ReadonlyMap<number, string>) {
  const records: DshTimelineRecord[] = [];
  const openTools = new Map<string, DshTimelineRecord>();
  const openSteps = new Map<string, { start: number; firstToken?: number }>();
  for (const [index, event] of events.entries()) {
    const seq = eventSeq(event, index);
    const turn = eventTurn(event);
    const step = eventStep(event);
    const stepKey = turn !== undefined && step !== undefined ? `${turn}:${step}` : undefined;
    if (event.type === "step/start" && stepKey) {
      openSteps.set(stepKey, { start: eventTime(event) });
    }
    if (event.type === "assistant/chunk" && stepKey) {
      const open = openSteps.get(stepKey);
      if (isTokenChunk(eventData(event)) && open && open.firstToken === undefined)
        open.firstToken = eventTime(event);
    }
    if (event.type === "tool/result") {
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
      const open = stepKey ? openSteps.get(stepKey) : undefined;
      if (open) {
        record.startedAt = open.start;
        record.durationMs = Math.max(0, eventTime(event) - open.start);
        if (open.firstToken !== undefined) {
          record.ttftMs = Math.max(0, open.firstToken - open.start);
          record.decodeMs = Math.max(0, eventTime(event) - open.firstToken);
        }
        if (stepKey) openSteps.delete(stepKey);
      }
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

export function mergeDshSessionEvents(
  current: readonly DshSessionEvent[],
  incoming: readonly DshSessionEvent[],
): DshSessionEvent[] {
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
