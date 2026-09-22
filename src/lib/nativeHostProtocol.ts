/**
 * Client-side reader for the local protocol v1 (plan §2.3, P2-01).
 *
 * The Rust crate `aeroric-protocol` is the source of truth: `pnpm
 * native:contracts --write` publishes `contracts/native/v1/`, and this module, the
 * Swift decoder and the Rust DTOs are all checked against *those* fixtures. Keeping
 * a second copy of the rules in TypeScript is deliberate — a client that cannot
 * parse what it is being told is the failure mode — but every constant here is
 * asserted against the artifact by `src/test/native-host-protocol.test.ts`, so the
 * copy cannot drift.
 *
 * Nothing in this file opens a socket: transport belongs to the host client
 * (P3-02/P4-03), and this is the part both a renderer and a Node test can use.
 */

/** Maximum bytes of one JSONL frame, counted in UTF-8 bytes. */
export const MAX_FRAME_BYTES = 1024 * 1024;
/** Maximum *raw* PTY bytes per terminal chunk; base64 inflates by 4/3 after this. */
export const MAX_TERMINAL_CHUNK_BYTES = 64 * 1024;
export const PROTOCOL_VERSION = 1;

/** Decimal-string quantity. Never a `number`: past 2^53-1 a JS round trip loses bits. */
export type Quantity = string;

export const WIRE_ERROR_CODES = [
  "INVALID_ARGUMENT",
  "UNAUTHENTICATED",
  "UNSUPPORTED_CAPABILITY",
  "REVISION_CONFLICT",
  "NOT_FOUND",
  "BUSY",
  "STREAM_GAP",
  "SNAPSHOT_REQUIRED",
  "HOST_RESTARTED",
  "INTERACTIVE_UI_REQUIRED",
  "IO_ERROR",
] as const;

export type WireErrorCode = (typeof WIRE_ERROR_CODES)[number];
/** A code from a newer host degrades to `UNKNOWN`; the text is kept for display. */
export type ErrorCode = WireErrorCode | "UNKNOWN";

export type TaskStatus =
  | "todo"
  | "pending"
  | "running"
  | "input_required"
  | "detached"
  | "interrupted"
  | "done"
  | "failed"
  | "cancelled";

export type ProtocolFamily = "claude" | "codex" | "dsh" | "omp";

/**
 * The only keys a host may put into `RpcError.detail` (Q12). Closed on purpose:
 * `detail` is where diagnostics go, and diagnostics get pasted into review evidence
 * and `.artifacts` logs. A key we cannot name is not a diagnostic.
 */
export const ERROR_DETAIL_ALLOWED_KEYS = [
  "actualRevision",
  "epoch",
  "expectedRevision",
  "field",
  "fromSeq",
  "generation",
  "lockHolder",
  "reason",
  "taskId",
  "throughSeq",
] as const;

export type ErrorDetailKey = (typeof ERROR_DETAIL_ALLOWED_KEYS)[number];

export function isAllowedErrorDetailKey(key: string): key is ErrorDetailKey {
  return (ERROR_DETAIL_ALLOWED_KEYS as readonly string[]).includes(key);
}

/** Q13: exact major equality. Compatibility inside v1 is carried by capabilities. */
export function handshakeAllowsPeer(version: number): boolean {
  return version === PROTOCOL_VERSION;
}

export function asErrorCode(text: string): ErrorCode {
  return (WIRE_ERROR_CODES as readonly string[]).includes(text) ? (text as WireErrorCode) : "UNKNOWN";
}

export function isQuantity(value: unknown): value is Quantity {
  return typeof value === "string" && /^[0-9]+$/.test(value);
}

export function quantityToBigInt(value: Quantity): bigint {
  return BigInt(value);
}

export interface RpcErrorPayload {
  code: string;
  message: string;
  retryable: boolean;
  detail?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface RequestFrame {
  v: number;
  id: string;
  method: string;
  idempotencyKey?: string;
  params?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface SuccessFrame {
  v: number;
  id: string;
  ok: true;
  result: Record<string, unknown>;
  [key: string]: unknown;
}

export interface FailureFrame {
  v: number;
  id: string;
  ok: false;
  error: RpcErrorPayload;
  [key: string]: unknown;
}

export interface EventFrame {
  v: number;
  event: string;
  epoch: string;
  seq: Quantity;
  data: Record<string, unknown>;
  [key: string]: unknown;
}

export type Frame = RequestFrame | SuccessFrame | FailureFrame | EventFrame;

/** Transport faults are not protocol errors: the bytes never formed a frame. */
export type FrameFailure = "empty" | "tooLarge" | "notUtf8" | "malformed";

export class NativeProtocolError extends Error {
  constructor(
    readonly failure: FrameFailure,
    message: string,
  ) {
    super(message);
    this.name = "NativeProtocolError";
  }
}

function fail(failure: FrameFailure, message: string): never {
  throw new NativeProtocolError(failure, message);
}

function record(value: unknown, what: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail("malformed", `${what} must be an object`);
  }
  return value as Record<string, unknown>;
}

function str(value: unknown, what: string): string {
  if (typeof value !== "string") fail("malformed", `${what} must be a string`);
  return value;
}

/**
 * Parse one already-decoded line. Order matches the Rust decoder: empty, then JSON,
 * then frame shape. Byte limits and UTF-8 validity are `decodeLine`'s job.
 */
export function parseLine(line: string): Frame {
  if (line.trim().length === 0) fail("empty", "frame is empty");
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch (error) {
    fail("malformed", `frame is not valid JSON: ${(error as Error).message}`);
  }
  return parseFrame(value);
}

export function decodeLine(bytes: Uint8Array): Frame {
  if (bytes.byteLength > MAX_FRAME_BYTES) {
    fail("tooLarge", `frame of ${bytes.byteLength} B exceeds the ${MAX_FRAME_BYTES} B limit`);
  }
  try {
    // `fatal` is the point: a lossy decode would hand a corrupt frame to the parser,
    // and its replacement characters are indistinguishable from real content.
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return parseLine(text);
  } catch (error) {
    if (error instanceof NativeProtocolError) throw error;
    return fail("notUtf8", "frame is not valid UTF-8");
  }
}

export function parseFrame(value: unknown): Frame {
  const frame = record(value, "frame");
  if (frame.v !== PROTOCOL_VERSION) {
    fail("malformed", `frame carries v=${JSON.stringify(frame.v)}, this build speaks v${PROTOCOL_VERSION}`);
  }
  if (typeof frame.event === "string") {
    if (!isQuantity(frame.seq)) fail("malformed", "event seq must be a decimal string");
    return {
      ...frame,
      v: PROTOCOL_VERSION,
      event: frame.event,
      epoch: str(frame.epoch, "event epoch"),
      seq: frame.seq,
      data: record(frame.data ?? {}, "event data"),
    } as EventFrame;
  }
  if (typeof frame.method === "string") {
    if (frame.idempotencyKey !== undefined && typeof frame.idempotencyKey !== "string") {
      fail("malformed", "idempotencyKey must be a string when present");
    }
    return {
      ...frame,
      v: PROTOCOL_VERSION,
      id: str(frame.id, "request id"),
      method: frame.method,
      params: record(frame.params ?? {}, "request params"),
    } as RequestFrame;
  }
  const id = str(frame.id, "response id");
  if (typeof frame.ok !== "boolean") fail("malformed", "a response frame needs a boolean ok");
  if (!frame.ok) {
    const error = record(frame.error, "failure error");
    return {
      ...frame,
      v: PROTOCOL_VERSION,
      id,
      ok: false,
      error: {
        ...error,
        code: str(error.code, "error code"),
        message: str(error.message, "error message"),
        retryable: typeof error.retryable === "boolean" ? error.retryable : false,
      },
    } as FailureFrame;
  }
  return {
    ...frame,
    v: PROTOCOL_VERSION,
    id,
    ok: true,
    result: record(frame.result ?? {}, "success result"),
  } as SuccessFrame;
}

export function encodeFrame(frame: Frame): string {
  const line = `${JSON.stringify(frame)}\n`;
  const bytes = new TextEncoder().encode(line).byteLength;
  if (bytes > MAX_FRAME_BYTES) {
    fail("tooLarge", `encoded frame is ${bytes} B, over the ${MAX_FRAME_BYTES} B limit`);
  }
  return line;
}

const BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/**
 * Strict base64. `Buffer.from(text, "base64")` and `atob` both *tolerate* garbage —
 * they drop invalid characters and truncate — so a mangled PTY chunk would decode to
 * plausible-looking bytes and corrupt a screen the user cannot see. Same verdicts as
 * `BytesBase64::decode` in Rust, checked against the published vector list.
 */
export function decodeBase64Strict(text: string): Uint8Array | null {
  if (text.length === 0) return new Uint8Array(0);
  if (text.length % 4 !== 0) return null;
  let padding = 0;
  while (padding < 2 && text[text.length - 1 - padding] === "=") padding += 1;
  const body = text.slice(0, text.length - padding);
  if (body.includes("=")) return null;
  // A full quad is 3 bytes minus its padding; sizing from `body.length` instead of
  // the whole input truncates "YQ==" to an empty result.
  const out = new Uint8Array((text.length / 4) * 3 - padding);
  let acc = 0;
  let bits = 0;
  let written = 0;
  for (const char of body) {
    const value = BASE64_ALPHABET.indexOf(char);
    if (value < 0) return null;
    acc = (acc << 6) | value;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out[written++] = (acc >> bits) & 0xff;
    }
  }
  if (bits >= 6 || (acc & ((1 << bits) - 1)) !== 0) return null;
  return written === out.length ? out : null;
}

export interface TaskRecord {
  status?: string;
  sessionOwner?: { sessionId?: string; path?: string } | null;
  worktreeDiscarded?: boolean | null;
  [key: string]: unknown;
}

/** Mirrors `hasTaskSession` in `src/taskSession.ts`: an id *or* a path counts. */
export function hasTaskSession(task: TaskRecord): boolean {
  const owner = task.sessionOwner;
  if (!owner) return false;
  return Boolean(owner.sessionId || owner.path);
}

/**
 * Mirrors `canResumeTask` in `src/components/task-panel/TaskList.tsx` and
 * `TaskSnapshot::can_resume` in Rust. A missing transcript is refused here rather
 * than resumed as a brand-new session behind the user's back.
 */
export function canResume(task: TaskRecord): boolean {
  const status = task.status as TaskStatus | undefined;
  if (!status || status === "todo") return false;
  if (["pending", "running", "input_required", "detached"].includes(status)) return false;
  if (task.worktreeDiscarded === true) return false;
  return hasTaskSession(task);
}

export interface CapabilityEntry {
  name: string;
  enabled: boolean;
  since?: number;
  note?: string;
  [key: string]: unknown;
}

export interface HostCapabilityReport {
  protocolVersion: number;
  buildVersion: string;
  dataFormat?: string;
  capabilities?: Record<string, CapabilityEntry>;
  agents?: Record<string, unknown>;
  [key: string]: unknown;
}

/** The host declares capabilities; a client must never infer one from its own OS. */
export function capabilityEnabled(report: HostCapabilityReport, name: string): boolean {
  return report.capabilities?.[name]?.enabled === true;
}

export function missingCapabilityReason(report: HostCapabilityReport, name: string): string {
  const entry = report.capabilities?.[name];
  if (!entry) return `${name} is not reported by this host`;
  return entry.note ?? `${name} is disabled on this host`;
}
