/**
 * Pure projections for the advanced DeepSeek Harness conversation surfaces.
 * The Web UI normally builds these from its conversation runtime; Aeroric gets
 * the same durable session events over `session/event`, so keeping the fold
 * here makes live updates and history replay deterministic and testable.
 */

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
}

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
}

type Dict = Record<string, unknown>;

function dict(value: unknown): Dict {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Dict
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
  return value.map((part) => {
    const item = dict(part);
    return text(item.text) ?? text(item.content) ?? "";
  }).join("");
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

function preview(event: DshSessionEvent): { title: string; detail?: string } {
  const data = eventData(event);
  switch (event.type) {
    case "user/message":
      return { title: "User message", detail: contentText(data.content) || contentText(dict(data.message).content) };
    case "assistant/message":
      return { title: "Assistant message", detail: contentText(data.content ?? dict(data.message).content) };
    case "assistant/chunk":
      return { title: "Assistant stream", detail: text(dict(data.chunk).text) ?? text(data.chunk) };
    case "tool/call":
      return { title: `Tool: ${text(data.name) ?? "tool"}`, detail: text(data.arguments) };
    case "tool/result":
      return { title: "Tool result", detail: contentText(data.content ?? dict(data.message).content) };
    case "workflow/run-start":
    case "tool-workflow/run-start":
      return { title: `Workflow: ${text(data.name) ?? "run"}` };
    case "workflow/run-end":
    case "tool-workflow/run-end":
      return { title: `Workflow ${text(data.stopReason) ?? "finished"}` };
    case "schedule/change":
      return { title: `Schedule ${text(data.operation) ?? "change"}`, detail: text(dict(data.schedule).prompt) ?? text(data.id) };
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
  return Array.isArray(message.content)
    && message.content.some((part) => dict(part).isError === true);
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
  return [
    ...candidatePaths(locations),
    ...candidatePaths(diffs),
  ];
}

function mutationCall(name: string): boolean {
  const normalized = name.toLowerCase();
  if (/delete|remove|read|list|search|inspect|cat|grep|bash|shell|terminal|run_code/.test(normalized)) return false;
  return /write|edit|patch|insert|replace|create|move|copy|save|update/.test(normalized);
}

function updateStats(events: DshSessionEvent[]): DshStats {
  const stats: DshStats = {
    turns: 0, steps: 0, llmMs: 0, toolMs: 0, ttftMs: 0, ttftSteps: 0,
    decodeMs: 0, decodeTokens: 0, inputTokens: 0, outputTokens: 0,
    cacheReadTokens: 0, cacheWriteTokens: 0,
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
      const chunk = dict(data.chunk);
      const kind = text(chunk.type) ?? "";
      const hasToken = kind === "text" || kind === "reasoning" || kind === "token" || typeof chunk.text === "string";
      const open = steps.get(stepKey);
      if (hasToken && open && open.firstToken === undefined) open.firstToken = eventTime(event);
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
      stats.cacheReadTokens = Math.max(stats.cacheReadTokens, number(chunkUsage.cacheReadTokens) ?? 0);
      stats.cacheWriteTokens = Math.max(stats.cacheWriteTokens, number(chunkUsage.cacheWriteTokens) ?? 0);
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
      run.status = reason === "completed" ? "completed" : reason === "cancelled" ? "cancelled" : reason === "error" ? "failed" : "interrupted";
    }
    if (event.type === "tool-workflow/agent-start") {
      const phase = text(data.phase);
      const key = phase ?? "(default)";
      const group = run.phases[key] ??= { phase, members: [] };
      group.members.push({ seq: number(data.seq) ?? eventSeq(event, 0), label: text(data.label) ?? "agent", childId: text(data.childId) ?? "", phase, status: "running" });
    }
    if (event.type === "tool-workflow/agent-end") {
      const memberSeq = number(data.seq);
      for (const group of Object.values(run.phases)) {
        const member = group.members.find((candidate) => candidate.seq === memberSeq);
        if (member) {
          const outcome = text(data.outcome);
          member.status = outcome === "completed" ? "completed" : outcome === "cancelled" ? "cancelled" : outcome === "failed" ? "failed" : "interrupted";
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
          const next = anchor + (Math.floor((accepted - anchor) / (current.everySeconds * 1000)) + 1) * current.everySeconds * 1000;
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
      if (id) calls.set(id, { name: text(data.name) ?? "tool", args: parseJson(data.arguments), turn: eventTurn(event) });
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
      files.push({ path, seq: eventSeq(event, files.length), turn: eventTurn(event) ?? call?.turn });
    }
    if (id) calls.delete(id);
  }
  return files;
}

export function projectDshSessionEvents(input: readonly DshSessionEvent[]): DshSessionFeatures {
  const events = [...input]
    .filter((event) => typeof event?.type === "string")
    .sort((a, b) => eventSeq(a, 0) - eventSeq(b, 0));
  const trajectory = events.map((event, index) => ({
    seq: eventSeq(event, index),
    time: eventTime(event),
    type: event.type,
    turn: eventTurn(event),
    step: eventStep(event),
    ...preview(event),
    event,
  }));
  return {
    events,
    trajectory,
    stats: updateStats(events),
    producedFiles: updateProduced(events),
    workflows: updateWorkflows(events),
    schedules: updateSchedules(events),
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
  return [...bySeq.values(), ...withoutSeq]
    .sort((a, b) => (number(a.seq) ?? 0) - (number(b.seq) ?? 0));
}
