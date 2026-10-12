/**
 * P2-01 Action 5/6: the TypeScript half of the three-language contract test. Every
 * assertion reads a committed `contracts/native/v1/` document, so this decoder is
 * checked against the same bytes as the Rust DTOs and the Swift models — the point
 * is to catch a rule two languages agree on and the third one does not.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  MAX_FRAME_BYTES,
  MAX_TERMINAL_CHUNK_BYTES,
  PROTOCOL_VERSION,
  WIRE_ERROR_CODES,
  NativeProtocolError,
  asErrorCode,
  canResume,
  capabilityEnabled,
  decodeBase64Strict,
  decodeLine,
  ERROR_DETAIL_ALLOWED_KEYS,
  STORAGE_REJECTIONS,
  storageRejectionIsRetryable,
  encodeFrame,
  handshakeAllowsPeer,
  hasTaskSession,
  isAllowedErrorDetailKey,
  isQuantity,
  missingCapabilityReason,
  parseFrame,
  parseLine,
  quantityToBigInt,
} from "../lib/nativeHostProtocol";

import type {
  EventFrame,
  FailureFrame,
  SuccessFrame,
  TaskRecord,
  TaskStatus,
} from "../lib/nativeHostProtocol";

/**
 * Contract document shapes. `contracts/native/v1/*.json` is machine-exported by the Rust
 * exporter (`pnpm native:contracts --write`); every field below is copied from what the
 * artifact actually publishes, and only the parts this TypeScript half reads are declared —
 * so a field that drifts out of the artifact stops compiling here instead of turning into a
 * silently `undefined` expectation.
 */

/** `envelope.schema.json` → `constants.storageRejections[]`: one refusal and its answer. */
interface StorageRejectionContract {
  code: string;
  detailKeys: string[];
  rejection: string;
  retryable: boolean;
}

/** `envelope.schema.json` → `frameKinds.failure`: the frozen shape of a failure frame. */
interface FailureKindContract {
  openContent: string;
  properties: {
    error: {
      properties: {
        code: { enum: string[]; unknownFallback: string };
        detail: { mustNotContain: string[]; type: string };
        retryable: { type: string };
      };
      required: string[];
    };
    ok: { const: boolean };
  };
  required: string[];
}

/** `envelope.schema.json`: transport limits, the published constants, the frame rules. */
interface EnvelopeContract {
  cancellation: { disconnect: string; rpcCancel: string; taskCancel: string };
  constants: {
    builtInAgents: string[];
    errorCodes: string[];
    errorDetailAllowedKeys: string[];
    limits: {
      maxFrameBytes: number;
      maxTerminalChunkRawBytes: number;
      replay: { maxBytes: number; maxEvents: number };
    };
    permissionModes: string[];
    protocolFamilies: string[];
    protocolVersion: number;
    storageRejections: StorageRejectionContract[];
    taskStatuses: string[];
  };
  errorDetail: { keys: string[]; mustNotContain: string[]; rule: string };
  frameKinds: { failure: FailureKindContract };
  ownership: { alreadyInUse: string; singleWriter: string };
  resync: { bufferTruncation: string; startAfterCrash: string };
  transport: {
    framing: string;
    maxFrameBytes: number;
    maxTerminalChunkRawBytes: number;
    replay: { beyond: string; maxBytes: number; maxEvents: number };
    socket: string;
  };
  versionPolicy: { acceptsSameMajor: boolean; rejectsAnyOtherVersion: boolean; remote: string };
}

/** `methods.schema.json` → `methods[]`: one method frozen into the v1 surface (ADR-002). */
interface MethodRow {
  callerEvidence: string | null;
  category: string;
  checkModule: string;
  coreService: string;
  featureIds: string[];
  legacyCommand: string;
  mutating: boolean;
  name: string;
  promotion: string;
  requiresCapability: string | null;
  requiresTriageDecision: boolean;
  service: string;
  takesTarget: boolean;
}

/** `methods.schema.json` → `twinAliases[]`: a legacy `remote_`/`wsl_` twin, not a method. */
interface TwinAliasRow {
  comment: string;
  legacyCommand: string;
  method: string;
  paramDiff: string[];
  paramMapping: string;
  requiresCapability: string | null;
  targetKind: string;
  targetParams: string[];
}

/** `methods.schema.json`: the method surface plus the counts every client asserts. */
interface MethodSurface {
  aliasSummary: {
    pending: number;
    pendingBlocksHost: boolean;
    proven: number;
    rule: string;
    total: number;
  };
  byService: Record<string, number>;
  capabilityGatedCount: number;
  classificationRule: {
    advisory: string;
    default: string;
    how: string;
    readVerbIsANoun: string[];
    readVerbs: string[];
    why: string;
    writeVerbs: string[];
  };
  count: number;
  excluded: { hostLocalOsUi: string; prefixTwins: string };
  methods: MethodRow[];
  mutatingCount: number;
  protocolVersion: number;
  readCount: number;
  source: string;
  surfaceProvenance: {
    adr: string;
    "caller-literal": number;
    how: string;
    prefixDerivedAliases: number;
    requiresTriageDecision: number;
    "stated-evidence": number;
    "uncalled-no-evidence": number;
  };
  twinAliases: TwinAliasRow[];
}

/** `events.schema.json` → `events[]`: one legacy channel with its emitter/listener evidence. */
interface EventRow {
  disposition: string;
  emitters: number;
  listeners: number;
  name: string;
  orphan: string | null;
}

/** `events.schema.json`: the legacy channels the topic bridge still has to cover. */
interface EventSurface {
  contract: { controlQueue: string; seq: string; unknownEvent: string };
  count: number;
  events: EventRow[];
  title: string;
}

/** `event-topics.schema.json` → `rows[]`: one v1 topic and the channels it owns (ADR-007). */
interface TopicRow {
  evidence: { emitters: number; listeners: number };
  legacyChannels: string[];
  legacyDirection: string;
  legacyOrphan?: string | null;
  payload: string;
  plannedDirection: string;
  /** Only a decided (`mapped`) row states where its name came from. */
  source?: string;
  status: string;
  topic: string;
}

/** `event-topics.schema.json`: the topic table and the counts the gate asserts. */
interface TopicSurface {
  adr: string;
  contract: { pending: string; renameIsProtocolChange: string; unknownTopic: string };
  legacyChannels: number;
  legacyDirectionUnproven: number;
  mapped: number;
  notForwarded: number;
  pending: number;
  protocolVersion: number;
  reshapedPayloads: number;
  rows: TopicRow[];
  title: string;
  topics: number;
}

/** `fixtures/terminal.json` → `base64.valid[]`: a vector and the exact bytes it must yield. */
interface Base64Vector {
  bytes: number[];
  text: string;
}

/** A published event frame: dotted topic, decimal-string cursor, payload of its own. */
interface EventVector<TData> {
  data: TData;
  epoch: string;
  event: string;
  seq: string;
  v: number;
}

interface TerminalOutputData {
  bytesBase64: string;
  generation: string;
  nextOffset: string;
  offset: string;
  taskId: string;
}

interface TerminalGapData {
  generation: string;
  reason: string;
  reset: boolean;
  taskId: string;
  truncated: boolean;
}

/** `fixtures/terminal.json` → `frameVectors`: the strictness table all three readers share. */
interface TerminalFrameVectors {
  accepted: { maxU64Seq: EventVector<Record<string, never>> };
  comment: string;
  rejected: {
    empty: string;
    noMethod: { id: string; v: number };
    noVersion: { id: string; ok: boolean };
    numericSeq: {
      data: Record<string, never>;
      epoch: string;
      event: string;
      seq: number;
      v: number;
    };
    unknownStatus: { id: string; ok: boolean; result: { status: string }; v: number };
  };
}

/** `fixtures/terminal.json`: PTY bytes, the frame table and the single-writer lease. */
interface TerminalFixture {
  base64: { comment: string; invalid: string[]; valid: Base64Vector[] };
  comment: string;
  frameVectors: TerminalFrameVectors;
  gap: EventVector<TerminalGapData>;
  lease: { acquiredAt: number; generation: string; holder: string; taskId: string };
  output: EventVector<TerminalOutputData>;
}

/** `fixtures/success.json`: a mutating request and the authoritative snapshot it returns. */
interface SuccessFixture {
  comment: string;
  request: {
    id: string;
    idempotencyKey: string;
    method: string;
    params: { expectedRevision: string; projectId: string; taskId: string };
    v: number;
  };
  response: {
    id: string;
    ok: boolean;
    result: { generation: string; revision: string; taskId: string };
    v: number;
  };
}

/** `fixtures/error.json`: the revision conflict the second writer is refused with. */
interface ErrorFixture {
  comment: string;
  request: {
    id: string;
    idempotencyKey: string;
    method: string;
    params: { expectedRevision: string; taskId: string };
    v: number;
  };
  response: {
    error: {
      code: string;
      detail: { actualRevision: string; expectedRevision: string };
      message: string;
      retryable: boolean;
    };
    id: string;
    ok: boolean;
    v: number;
  };
}

/** `fixtures/unknown.json`: forward-compatibility vectors for a newer host. */
interface UnknownFixture {
  comment: string;
  frameWithUnknownFields: {
    hostFeature: { enabled: boolean };
    id: string;
    ok: boolean;
    result: Record<string, never>;
    v: number;
  };
  refusedUnknownMutating: {
    error: { code: string; message: string; retryable: boolean };
    method: string;
  };
  unknownCode: { code: string; message: string; retryable: boolean };
  unknownEvent: EventVector<{ kept: number }>;
}

/** `fixtures/legacy-task.json`: a task written before `sessionFamily` existed (ADR-028/029). */
interface LegacyTaskFixture {
  comment: string;
  expectations: { canResume: boolean; familyFromAgent: string; unknownFieldSurvives: boolean };
  familySplit: {
    builtIn: Record<string, { launch: string; resume: string; session: string }>;
    comment: string;
    fields: { launch: string; resume: string; session: string };
    missingFieldRule: string;
    resumeIsNotSession: string;
  };
  resumeRule: {
    comment: string;
    discardedWorktreeWithTranscript: boolean;
    withTranscript: Record<TaskStatus, boolean>;
    withoutTranscript: Record<TaskStatus, boolean>;
  };
  stored: {
    agent: { builtIn: string };
    createdAt: number;
    id: string;
    projectId: string;
    sessionCodexLike: boolean;
    status: string;
  };
}

/**
 * `parseFrame` returns the whole `Frame` union because a live host may send any arm. Each
 * published vector already names the arm it is, and the decoder has validated it, so the
 * assertions below narrow that union to the fixture's documented frame instead of reading
 * it blind. `TerminalOutputFrame` also pins `data`, which `EventFrame` keeps opaque.
 */
type TerminalOutputFrame = EventFrame & { data: TerminalOutputData };

// `process.cwd()`, not `import.meta.url`: under jsdom the module URL is an http one
// and `readFileSync` refuses it (same convention as `z-layers.test.ts`).
const contract = <T>(relative: string): T =>
  JSON.parse(readFileSync(resolve(process.cwd(), "contracts/native/v1", relative), "utf8")) as T;
const fixture = <T>(name: string): T => contract<T>(`fixtures/${name}`);

const envelope = contract<EnvelopeContract>("envelope.schema.json");
const methodSurface = contract<MethodSurface>("methods.schema.json");
const eventSurface = contract<EventSurface>("events.schema.json");
const terminalFixture = fixture<TerminalFixture>("terminal.json");
const legacyFixture = fixture<LegacyTaskFixture>("legacy-task.json");

function failureOf(run: () => unknown): string | null {
  try {
    run();
    return null;
  } catch (error) {
    if (error instanceof NativeProtocolError) return error.failure;
    throw error;
  }
}

describe("constants published by the Rust exporter", () => {
  it("match the numbers this decoder enforces", () => {
    expect(MAX_FRAME_BYTES).toBe(envelope.transport.maxFrameBytes);
    expect(MAX_TERMINAL_CHUNK_BYTES).toBe(envelope.transport.maxTerminalChunkRawBytes);
    expect(PROTOCOL_VERSION).toBe(envelope.constants.protocolVersion);
    expect([...WIRE_ERROR_CODES]).toEqual(envelope.constants.errorCodes);
    expect(envelope.constants.taskStatuses).toHaveLength(9);
    expect(envelope.constants.protocolFamilies).toEqual(["claude", "codex", "dsh", "omp"]);
    expect(envelope.constants.builtInAgents).toContain("claude_gpt55");
    expect(envelope.constants.permissionModes).toEqual(["ask", "auto_edit", "full_access"]);
  });

  it("describe the replay window the client has to survive", () => {
    expect(envelope.constants.limits.replay).toEqual({ maxEvents: 4096, maxBytes: 8388608 });
    expect(envelope.transport.replay.beyond).toBe("SNAPSHOT_REQUIRED");
  });

  it("keep the error-detail allowlist closed and the major version strict (Q12/Q13)", () => {
    // The allowlist is published twice on purpose: as the fixture's authority and as
    // the host's constructor surface. Either side drifting must show up here.
    expect([...ERROR_DETAIL_ALLOWED_KEYS]).toEqual(envelope.constants.errorDetailAllowedKeys);
    expect(envelope.errorDetail.keys).toEqual(envelope.constants.errorDetailAllowedKeys);
    expect(envelope.constants.errorDetailAllowedKeys).not.toContain("token");
    expect(envelope.errorDetail.mustNotContain).toContain("connection string");
    expect(envelope.errorDetail.rule).toContain("closed");
    // Q13: an exact major match, or nothing. No version arithmetic, no degradation.
    expect(envelope.versionPolicy.acceptsSameMajor).toBe(true);
    expect(envelope.versionPolicy.rejectsAnyOtherVersion).toBe(true);
    expect(handshakeAllowsPeer(PROTOCOL_VERSION)).toBe(true);
    for (const other of [0, PROTOCOL_VERSION + 1, 255]) {
      expect(handshakeAllowsPeer(other)).toBe(false);
    }
    expect(envelope.versionPolicy.remote).toContain("separate version space");
  });

  it("publish the single-writer and cancellation rules the UI has to honour", () => {
    expect(envelope.ownership.alreadyInUse).toContain("never kill a PID");
    expect(envelope.cancellation.disconnect).toBe("a socket close is not a task end");
    expect(envelope.cancellation.taskCancel).toContain("terminal.cancel_task");
    expect(envelope.resync.startAfterCrash).toContain("terminal.run_task");
    const failureShape = envelope.frameKinds.failure;
    expect(failureShape.required).toEqual(["v", "id", "ok", "error"]);
    const detail = failureShape.properties.error.properties.detail;
    expect(detail.mustNotContain).toContain("token");
    expect(detail.mustNotContain).toContain("raw session content");
    expect(failureShape.properties.error.properties.code.unknownFallback).toBe("UNKNOWN");
    expect(failureShape.openContent).toContain("Carried forward");
  });
});

describe("frame decoding", () => {
  it("reads the success and failure fixtures the way Rust does", () => {
    const success = fixture<SuccessFixture>("success.json");
    expect(parseFrame(success.request)).toMatchObject({
      method: "terminal.run_task",
      idempotencyKey: "op-1",
    });
    expect(isQuantity(success.request.params.expectedRevision)).toBe(true);

    const response = parseFrame(success.response) as SuccessFrame;
    expect(response.ok).toBe(true);
    expect(response.result).toMatchObject({ revision: "8", generation: "2" });

    const errorFixture = fixture<ErrorFixture>("error.json");
    for (const key of Object.keys(errorFixture.response.error.detail)) {
      expect(isAllowedErrorDetailKey(key), `${key} is not an allowed detail key`).toBe(true);
    }
    expect(isAllowedErrorDetailKey("connectionString")).toBe(false);

    const failure = parseFrame(errorFixture.response) as FailureFrame;
    expect(failure.ok).toBe(false);
    expect(asErrorCode(failure.error.code)).toBe("REVISION_CONFLICT");
    expect(failure.error.retryable).toBe(false);
    expect(failure.error.detail?.actualRevision).toBe("9");
  });

  it("keeps unknown fields and degrades unknown codes and topics", () => {
    const unknown = fixture<UnknownFixture>("unknown.json");
    expect((parseFrame(unknown.frameWithUnknownFields) as SuccessFrame).hostFeature).toEqual({
      enabled: true,
    });
    expect(asErrorCode(unknown.unknownCode.code)).toBe("UNKNOWN");
    expect(asErrorCode("REVISION_CONFLICT")).toBe("REVISION_CONFLICT");
    const event = parseFrame(unknown.unknownEvent) as EventFrame;
    expect(event).toMatchObject({ event: "some.newer.topic", seq: "42" });
    expect(unknown.refusedUnknownMutating.method).toBe("terminal.frobnicate_task");
    expect(asErrorCode(unknown.refusedUnknownMutating.error.code)).toBe("INVALID_ARGUMENT");
  });

  it("refuses the shapes the published table marks as rejected", () => {
    const rejected = terminalFixture.frameVectors.rejected;
    // `empty` is a *line*, so it goes in verbatim; re-encoding it would make it a
    // JSON string literal and the refusal would be for the wrong reason.
    expect(failureOf(() => parseLine(rejected.empty))).toBe("empty");
    expect(failureOf(() => parseLine(JSON.stringify(rejected.noMethod)))).toBe("malformed");
    expect(failureOf(() => parseLine(JSON.stringify(rejected.noVersion)))).toBe("malformed");
    // A numeric seq is not "close enough": 2^53+1 is exactly where a JS number stops
    // round-tripping, so the frame must be refused instead of accepted.
    expect(failureOf(() => parseLine(JSON.stringify(rejected.numericSeq)))).toBe("malformed");
    // The unknown status inside an otherwise valid frame stays verbatim: a client
    // must not invent a default for a state it has never seen.
    expect((parseFrame(rejected.unknownStatus) as SuccessFrame).result.status).toBe("vibrating");
  });

  it("enforces the byte limits the artifact advertises", () => {
    expect(failureOf(() => decodeLine(new Uint8Array(MAX_FRAME_BYTES + 1).fill(0x20)))).toBe(
      "tooLarge",
    );
    expect(
      failureOf(() =>
        encodeFrame({ v: 1, id: "x", ok: true, result: { pad: "a".repeat(MAX_FRAME_BYTES) } }),
      ),
    ).toBe("tooLarge");
    expect(failureOf(() => decodeLine(new Uint8Array([0xff, 0xfe, 0x00, 0x01])))).toBe("notUtf8");
    // exactly at the limit is transport-legal; the parser then decides it is blank
    expect(failureOf(() => decodeLine(new Uint8Array(MAX_FRAME_BYTES).fill(0x20)))).toBe("empty");
  });

  it("carries a 64-bit cursor without losing a bit", () => {
    const accepted = terminalFixture.frameVectors.accepted.maxU64Seq;
    const frame = parseFrame(accepted) as EventFrame;
    expect(frame.seq).toBe("18446744073709551615");
    expect(quantityToBigInt(frame.seq)).toBe(BigInt("18446744073709551615"));
    // Reading the same cursor as a JS number has already rounded it upward.
    expect(Number(frame.seq)).toBe(18446744073709551616);
    expect(encodeFrame(frame).trimEnd()).toBe(JSON.stringify(accepted));
  });
});

describe("base64 terminal bytes", () => {
  it("decodes the published vectors to exactly the published bytes", () => {
    for (const vector of terminalFixture.base64.valid) {
      expect(Array.from(decodeBase64Strict(vector.text) ?? [])).toEqual(vector.bytes);
    }
  });

  it("rejects every vector the other decoders reject", () => {
    for (const text of terminalFixture.base64.invalid) {
      // A named loop keeps the failing vector visible without a second expect arg.
      const rejected = text;
      expect(decodeBase64Strict(rejected)).toBeNull();
    }
  });

  it("is stricter than the runtime's own reader, which is the whole point", () => {
    // `atob` strips embedded whitespace and silently truncates short unpadded input
    // into plausible bytes. A PTY chunk that "works" that way corrupts a screen
    // nobody is watching, which is why the vectors are shared, not paraphrased.
    expect(atob("Y WJj")).toBe("abc");
    expect(atob("YWJ")).toBe("ab");
    expect(decodeBase64Strict("Y WJj")).toBeNull();
    expect(decodeBase64Strict("YWJ")).toBeNull();
  });

  it("survives the terminal frames", () => {
    const output = parseFrame(terminalFixture.output) as TerminalOutputFrame;
    expect(output).toMatchObject({ event: "terminal.output", seq: "19" });
    expect(Array.from(decodeBase64Strict(output.data.bytesBase64) ?? [])).toEqual([97, 98, 99]);
    const gap = parseFrame(terminalFixture.gap) as EventFrame;
    expect(gap.data).toMatchObject({ reason: "SNAPSHOT_REQUIRED", reset: true, truncated: true });
    expect(Number.isSafeInteger(terminalFixture.lease.acquiredAt)).toBe(true);
  });
});

describe("the rules every client shares", () => {
  it("resumes exactly the statuses the published table allows", () => {
    const rule = legacyFixture.resumeRule;
    // A pre-`sessionFamily` record may still carry a `family` field the shared
    // `TaskRecord` deliberately does not model (`unknownFieldSurvives: true` in the
    // fixture), so the local shape widens `sessionOwner` instead of weakening the
    // assertion to `any` or dropping the field the fixture is actually about.
    type LegacyTaskRecord = TaskRecord & {
      sessionOwner?: { family?: string; sessionId?: string; path?: string } | null;
    };
    const record = (status: string, transcript: boolean, discarded = false): LegacyTaskRecord => ({
      status,
      ...(transcript ? { sessionOwner: { family: "codex", sessionId: "s1" } } : {}),
      ...(discarded ? { worktreeDiscarded: true } : {}),
    });
    let checked = 0;
    for (const key of ["withTranscript", "withoutTranscript"] as const) {
      const transcript = key === "withTranscript";
      for (const [status, expected] of Object.entries(rule[key])) {
        expect(canResume(record(status, transcript))).toBe(expected);
        checked += 1;
      }
    }
    expect(checked).toBeGreaterThanOrEqual(13);
    expect(canResume(record("done", true, true))).toBe(rule.discardedWorktreeWithTranscript);
    // The table must cover every published status, not the ones someone remembered.
    expect(envelope.constants.taskStatuses).toHaveLength(Object.keys(rule.withTranscript).length);
  });

  it("treats a path as a transcript, like the shipping client does", () => {
    expect(hasTaskSession({ sessionOwner: { sessionId: "", path: "/s.jsonl" } })).toBe(true);
    expect(hasTaskSession({ sessionOwner: { sessionId: "", path: "" } })).toBe(false);
    expect(hasTaskSession({})).toBe(false);
    expect(canResume(legacyFixture.stored)).toBe(legacyFixture.expectations.canResume);
  });

  it("gates platform methods on a host-declared capability, never on its own OS", () => {
    const report = {
      protocolVersion: 1,
      buildVersion: "1.4.9",
      capabilities: {
        wsl: { name: "wsl", enabled: false, note: "本机没有 WSL" },
        ssh: { name: "ssh", enabled: true },
      },
    };
    expect(capabilityEnabled(report, "wsl")).toBe(false);
    expect(capabilityEnabled(report, "ssh")).toBe(true);
    expect(capabilityEnabled(report, "never_reported")).toBe(false);
    expect(missingCapabilityReason(report, "wsl")).toBe("本机没有 WSL");
    expect(missingCapabilityReason(report, "never_reported")).toContain("not reported");

    const gated = methodSurface.methods.filter((m) => m.requiresCapability);
    expect(gated).toHaveLength(methodSurface.capabilityGatedCount);
    expect(gated.length).toBeGreaterThan(0);
    expect(methodSurface.aliasSummary.pendingBlocksHost).toBe(true);
    // The alias plane is where the ssh/wsl spelling lives: the same behaviour, one
    // method, and the target's own capability gate.
    const aliases = methodSurface.twinAliases;
    expect(aliases).toHaveLength(methodSurface.surfaceProvenance.prefixDerivedAliases);
    const published = new Set(methodSurface.methods.map((m) => m.name));
    const byStatus = { proven: 0, pending: 0 } as Record<string, number>;
    for (const alias of aliases) {
      const status = String(alias.paramMapping).startsWith("proven") ? "proven" : "pending";
      byStatus[status] += 1;
      expect(
        published.has(alias.method),
        `${alias.legacyCommand} aliases an unpublished method`,
      ).toBe(true);
      expect(alias.paramDiff.length === 0).toBe(status === "proven");
    }
    expect(byStatus).toEqual({
      proven: methodSurface.aliasSummary.proven,
      pending: methodSurface.aliasSummary.pending,
    });
    expect(gated.every((m) => m.requiresCapability === "wsl")).toBe(true);
    expect(gated.every((m) => m.category === "platform_specific")).toBe(true);
    expect(methodSurface.capabilityGatedCount).toBe(21);
    // the two cross-platform platform commands must stay callable on every host
    const ungatedPlatform = methodSurface.methods
      .filter((m) => m.category === "platform_specific" && !m.requiresCapability)
      .map((m) => m.name);
    expect(ungatedPlatform).toEqual([
      "platform.get_platform_runtime_info",
      "platform.build_runnable_file_command",
    ]);
  });

  it("publishes the method surface ADR-002 froze", () => {
    const names: string[] = methodSurface.methods.map((m) => m.name);
    expect(methodSurface.count).toBe(532);
    expect(methodSurface.surfaceProvenance.prefixDerivedAliases).toBe(109);
    // Q10: every name on the surface either has a caller literal or states who decides.
    expect(methodSurface.surfaceProvenance["uncalled-no-evidence"]).toBe(0);
    expect(methodSurface.surfaceProvenance["stated-evidence"]).toBe(83);
    expect(names).toHaveLength(methodSurface.count);
    expect(new Set(names).size).toBe(names.length);
    expect(names.some((n) => n.includes("/"))).toBe(false);
    expect(names.every((n) => /^[a-z0-9_]+\.[A-Za-z0-9_]+$/.test(n))).toBe(true);
    expect(names).toContain("terminal.run_task");
    expect(names).toContain("git.git_status");
    const services = new Set(names.map((n) => n.split(".")[0]));
    expect(services.size).toBe(Object.keys(methodSurface.byService).length);
    expect(Object.values(methodSurface.byService).reduce((t, n) => t + n, 0)).toBe(
      methodSurface.count,
    );
    expect(methodSurface.excluded.hostLocalOsUi).toContain("19");
    expect(methodSurface.excluded.prefixTwins).toContain("ADR-006");
    expect(methodSurface.classificationRule.default).toBe("mutating");
    expect(methodSurface.methods.filter((m) => !m.mutating)).toHaveLength(methodSurface.readCount);
    // A Redis key type is not a read verb.
    expect(
      methodSurface.methods
        .filter((m) => !m.mutating)
        .some((m) => ["dbx_redis_list_remove", "dbx_redis_list_push"].includes(m.legacyCommand)),
    ).toBe(false);
  });

  it("keeps the event surface countable", () => {
    expect(eventSurface.events).toHaveLength(eventSurface.count);
    // P8-04 刀三：`weekly.report_progress` 与两条 P8-03 遗留的 v1 字面量一起进面
    // （38 → 41）；旧通道数仍是 38（v1 主题名不是旧通道，见下一条用例）。
    expect(eventSurface.count).toBe(41);
    expect(eventSurface.contract.seq).toContain("publish point");
    const names = eventSurface.events.map((e) => e.name);
    expect(new Set(names).size).toBe(names.length);
    expect(names).toContain("task-status");
  });

  it("bridges every legacy channel to exactly one v1 topic (ADR-007)", () => {
    const topics = contract<TopicSurface>("event-topics.schema.json");
expect(topics.topics).toBe(39);
    // P8-04 刀二/刀三:`usage.statistics_updated` 与 `weekly.report_progress` 双发接线,
    // mapped 5 → 11(P8-02/03 的 9 条 + 本轮 2 条)。P8-05 切片九十七/九十九/一百零三
    // 再把 dbx.production_confirm_requested、notebook.rag_index_progress、
    // dbx.transfer_progress 接线成 mapped(11 → 14),pending 27 → 24。
    expect(topics.mapped).toBe(14);
    // P8-04 刀三:两条 pending: P8-04 接线成 mapped,pending 33 → 27。
    expect(topics.pending).toBe(24); // P8-03 刀二:remote.task_request 转 not-forwarded(RPC 应答即结果)
    expect(topics.reshapedPayloads).toBe(5); // P8-02 刀六起 agent 两条 mapped（reshaped payload）
    const claimed = new Map<string, number>();
    for (const row of topics.rows) {
      expect(String(row.topic)).toMatch(/^[a-z0-9_]+\.[a-z0-9_]+$/);
      for (const channel of row.legacyChannels) {
        claimed.set(channel, (claimed.get(channel) ?? 0) + 1);
      }
      if (row.status === "mapped") {
        // A decided row must say where the name came from and what the payload becomes.
        expect(String(row.source)).toContain("§");
        expect(
          ["verbatim", "reshaped:"].some((prefix) => String(row.payload).startsWith(prefix)),
        ).toBe(true);
      } else {
        expect(String(row.plannedDirection).startsWith("pending")).toBe(true);
        expect(String(row.payload).startsWith("pending")).toBe(true);
      }
    }
    expect(claimed.size).toBe(38);
    // `events[]` 里混着**旧通道**与**以 v1 主题名发出的字面量**（P8-03 刀六起的
    // 双发接线）。后者的 v1 一侧就是它自己，不在"旧通道要由本表承接"的判据里——
    // 与 `aeroric-protocol` 的 `export-schema` 用同一条口径（P8-04 刀三修正）。
    const topicNames = new Set(topics.rows.map((row) => String(row.topic)));
    for (const event of eventSurface.events) {
      if (topicNames.has(event.name)) continue;
      expect(claimed.get(event.name), `${event.name} is uncovered`).toBe(1);
    }
    expect(topics.contract.unknownTopic).toContain("must not fail");
    // §2.3 的三条＋P8-02 刀六的两条 agent 行（§Task P8-02 具名移交项）
    // ＋P8-03 刀六的四条 remote 行＋P8-04 刀三的两条（usage.statistics_updated /
    // weekly.report_progress，都是宿主双发接线的 v1 名字）。
    expect(
      topics.rows
        .filter((r) => r.status === "mapped")
        .map((r) => r.topic)
        .sort(),
    ).toEqual([
      "aeroric.app_settings_changed",
      "agent.operation_changed",
      "agent.tool_install_progress",
      "dbx.production_confirm_requested",
      "dbx.transfer_progress",
      "notebook.rag_index_progress",
      "project.pinned_changed",
      "remote.device_paired",
      "remote.terminal_resized",
      "task.updated",
      "terminal.gap",
      "terminal.output",
      "usage.statistics_updated",
      "weekly.report_progress",
    ]);
  });

  it("publishes the same three storage refusals the Rust enum generated (ADR-011)", () => {
    const published = envelope.constants.storageRejections;
    expect(published.map((row) => row.rejection)).toEqual(
      STORAGE_REJECTIONS.map((row) => row.rejection),
    );
    expect(published).toEqual(STORAGE_REJECTIONS);
    for (const row of STORAGE_REJECTIONS) {
      for (const key of row.detailKeys) {
        expect(isAllowedErrorDetailKey(key)).toBe(true);
      }
      expect(WIRE_ERROR_CODES).toContain(row.code);
    }
    // The whole point of the table: three refusals, two answers.
    expect(storageRejectionIsRetryable("BUSY")).toBe(true);
    expect(storageRejectionIsRetryable("IO_ERROR")).toBe(false);
    expect(storageRejectionIsRetryable("REVISION_CONFLICT")).toBe(false);
    expect(storageRejectionIsRetryable("SOMETHING_NEW")).toBe(false);
  });

  it("publishes two distinct families per agent, and no fake family value", () => {
    // ADR-028. The client already lives this split: `agentOption("claude_gpt55")` reports
    // codex while its transcript is Claude. The contract now names both halves so no side
    // can quietly answer one question with the other field.
    const split = legacyFixture.familySplit;
    expect(split.fields.launch).toBe("launchFamily");
    expect(split.fields.session).toBe("sessionFamily");
    expect(split.fields.resume).toBe("resumeFamily");
    expect(new Set(Object.values(split.fields)).size).toBe(3);

    const families: string[] = envelope.constants.protocolFamilies;
    const builtIn = split.builtIn;
    // Every built-in must carry both answers: a missing row would let one family stand in
    // for the other, which is the collapse ADR-028 exists to prevent.
    expect(Object.keys(builtIn).sort()).toEqual([...envelope.constants.builtInAgents].sort());
    for (const row of Object.values(builtIn)) {
      for (const value of [row.launch, row.session]) {
        expect(value === "content-derived" || families.includes(value)).toBe(true);
      }
    }
    expect(builtIn.claude_gpt55.session).toBe("claude");
    expect(builtIn.claude_gpt55.launch).toBe("content-derived");
    expect(families).not.toContain("content-derived");
    expect(split.missingFieldRule).toBe(
      "derive-from-host then backfill-on-write; never a batch rewrite of stored tasks",
    );
  });
});
