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

type Row = Record<string, any>;

// `process.cwd()`, not `import.meta.url`: under jsdom the module URL is an http one
// and `readFileSync` refuses it (same convention as `z-layers.test.ts`).
const contract = (relative: string) =>
  JSON.parse(readFileSync(resolve(process.cwd(), "contracts/native/v1", relative), "utf8")) as Row;
const fixture = (name: string) => contract(`fixtures/${name}`);

const envelope = contract("envelope.schema.json");
const methodSurface = contract("methods.schema.json");
const eventSurface = contract("events.schema.json");
const terminalFixture = fixture("terminal.json");
const legacyFixture = fixture("legacy-task.json");

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
    const success = fixture("success.json");
    expect(parseFrame(success.request)).toMatchObject({
      method: "terminal.run_task",
      idempotencyKey: "op-1",
    });
    expect(isQuantity(success.request.params.expectedRevision)).toBe(true);

    const response = parseFrame(success.response) as Row;
    expect(response.ok).toBe(true);
    expect(response.result).toMatchObject({ revision: "8", generation: "2" });

    for (const key of Object.keys(fixture("error.json").response.error.detail)) {
      expect(isAllowedErrorDetailKey(key), `${key} is not an allowed detail key`).toBe(true);
    }
    expect(isAllowedErrorDetailKey("connectionString")).toBe(false);

    const failure = parseFrame(fixture("error.json").response) as Row;
    expect(failure.ok).toBe(false);
    expect(asErrorCode(failure.error.code)).toBe("REVISION_CONFLICT");
    expect(failure.error.retryable).toBe(false);
    expect(failure.error.detail.actualRevision).toBe("9");
  });

  it("keeps unknown fields and degrades unknown codes and topics", () => {
    const unknown = fixture("unknown.json");
    expect((parseFrame(unknown.frameWithUnknownFields) as Row).hostFeature).toEqual({
      enabled: true,
    });
    expect(asErrorCode(unknown.unknownCode.code)).toBe("UNKNOWN");
    expect(asErrorCode("REVISION_CONFLICT")).toBe("REVISION_CONFLICT");
    const event = parseFrame(unknown.unknownEvent) as Row;
    expect(event).toMatchObject({ event: "some.newer.topic", seq: "42" });
    expect(unknown.refusedUnknownMutating.method).toBe("terminal.frobnicate_task");
    expect(asErrorCode(unknown.refusedUnknownMutating.error.code)).toBe("INVALID_ARGUMENT");
  });

  it("refuses the shapes the published table marks as rejected", () => {
    const rejected = terminalFixture.frameVectors.rejected as Record<string, string | Row>;
    // `empty` is a *line*, so it goes in verbatim; re-encoding it would make it a
    // JSON string literal and the refusal would be for the wrong reason.
    expect(failureOf(() => parseLine(rejected.empty as string))).toBe("empty");
    expect(failureOf(() => parseLine(JSON.stringify(rejected.noMethod)))).toBe("malformed");
    expect(failureOf(() => parseLine(JSON.stringify(rejected.noVersion)))).toBe("malformed");
    // A numeric seq is not "close enough": 2^53+1 is exactly where a JS number stops
    // round-tripping, so the frame must be refused instead of accepted.
    expect(failureOf(() => parseLine(JSON.stringify(rejected.numericSeq)))).toBe("malformed");
    // The unknown status inside an otherwise valid frame stays verbatim: a client
    // must not invent a default for a state it has never seen.
    expect((parseFrame(rejected.unknownStatus) as Row).result.status).toBe("vibrating");
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
    const frame = parseFrame(accepted) as Row;
    expect(frame.seq).toBe("18446744073709551615");
    expect(quantityToBigInt(frame.seq)).toBe(BigInt("18446744073709551615"));
    // Reading the same cursor as a JS number has already rounded it upward.
    expect(Number(frame.seq)).toBe(18446744073709551616);
    expect(encodeFrame(frame as never).trimEnd()).toBe(JSON.stringify(accepted));
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
    const output = parseFrame(terminalFixture.output) as Row;
    expect(output).toMatchObject({ event: "terminal.output", seq: "19" });
    expect(Array.from(decodeBase64Strict(output.data.bytesBase64) ?? [])).toEqual([97, 98, 99]);
    const gap = parseFrame(terminalFixture.gap) as Row;
    expect(gap.data).toMatchObject({ reason: "SNAPSHOT_REQUIRED", reset: true, truncated: true });
    expect(Number.isSafeInteger(terminalFixture.lease.acquiredAt)).toBe(true);
  });
});

describe("the rules every client shares", () => {
  it("resumes exactly the statuses the published table allows", () => {
    const rule = legacyFixture.resumeRule;
    const record = (status: string, transcript: boolean, discarded = false): Row => ({
      status,
      ...(transcript ? { sessionOwner: { family: "codex", sessionId: "s1" } } : {}),
      ...(discarded ? { worktreeDiscarded: true } : {}),
    });
    let checked = 0;
    for (const key of ["withTranscript", "withoutTranscript"] as const) {
      const transcript = key === "withTranscript";
      for (const [status, expected] of Object.entries(rule[key] as Record<string, boolean>)) {
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

    const gated = methodSurface.methods.filter((m: Row) => m.requiresCapability);
    expect(gated).toHaveLength(methodSurface.capabilityGatedCount);
    expect(gated.length).toBeGreaterThan(0);
    expect(methodSurface.aliasSummary.pendingBlocksHost).toBe(true);
    // The alias plane is where the ssh/wsl spelling lives: the same behaviour, one
    // method, and the target's own capability gate.
    const aliases = methodSurface.twinAliases as Row[];
    expect(aliases).toHaveLength(methodSurface.surfaceProvenance.prefixDerivedAliases);
    const published = new Set((methodSurface.methods as Row[]).map((m) => m.name as string));
    const byStatus = { proven: 0, pending: 0 } as Record<string, number>;
    for (const alias of aliases) {
      const status = String(alias.paramMapping).startsWith("proven") ? "proven" : "pending";
      byStatus[status] += 1;
      expect(published.has(alias.method as string), `${alias.legacyCommand} aliases an unpublished method`).toBe(true);
      expect((alias.paramDiff as unknown[]).length === 0).toBe(status === "proven");
    }
    expect(byStatus).toEqual({ proven: methodSurface.aliasSummary.proven, pending: methodSurface.aliasSummary.pending });
    expect(gated.every((m: Row) => m.requiresCapability === "wsl")).toBe(true);
    expect(gated.every((m: Row) => m.category === "platform_specific")).toBe(true);
    expect(methodSurface.capabilityGatedCount).toBe(21);
    // the two cross-platform platform commands must stay callable on every host
    const ungatedPlatform = methodSurface.methods
      .filter((m: Row) => m.category === "platform_specific" && !m.requiresCapability)
      .map((m: Row) => m.name);
    expect(ungatedPlatform).toEqual(["platform.get_platform_runtime_info", "platform.build_runnable_file_command"]);
  });

  it("publishes the method surface ADR-002 froze", () => {
    const names: string[] = methodSurface.methods.map((m: Row) => m.name);
    expect(methodSurface.count).toBe(532);
    expect(methodSurface.surfaceProvenance.prefixDerivedAliases).toBe(109);
    // Q10: every name on the surface either has a caller literal or states who decides.
    expect(methodSurface.surfaceProvenance["uncalled-no-evidence"]).toBe(0);
    expect(methodSurface.surfaceProvenance["stated-evidence"]).toBe(82);
    expect(names).toHaveLength(methodSurface.count);
    expect(new Set(names).size).toBe(names.length);
    expect(names.some((n) => n.includes("/"))).toBe(false);
    expect(names.every((n) => /^[a-z0-9_]+\.[A-Za-z0-9_]+$/.test(n))).toBe(true);
    expect(names).toContain("terminal.run_task");
    expect(names).toContain("git.git_status");
    const services = new Set(names.map((n) => n.split(".")[0]));
    expect(services.size).toBe(Object.keys(methodSurface.byService).length);
    expect(
      Object.values(methodSurface.byService as Record<string, number>).reduce((t, n) => t + n, 0),
    ).toBe(methodSurface.count);
    expect(methodSurface.excluded.hostLocalOsUi).toContain("19");
    expect(methodSurface.excluded.prefixTwins).toContain("ADR-006");
    expect(methodSurface.classificationRule.default).toBe("mutating");
    expect(methodSurface.methods.filter((m: Row) => !m.mutating)).toHaveLength(
      methodSurface.readCount,
    );
    // A Redis key type is not a read verb.
    expect(
      methodSurface.methods
        .filter((m: Row) => !m.mutating)
        .some((m: Row) => ["dbx_redis_list_remove", "dbx_redis_list_push"].includes(m.legacyCommand)),
    ).toBe(false);
  });

  it("keeps the event surface countable", () => {
    expect(eventSurface.events).toHaveLength(eventSurface.count);
    expect(eventSurface.count).toBe(38);
    expect(eventSurface.contract.seq).toContain("publish point");
    const names = eventSurface.events.map((e: Row) => e.name);
    expect(new Set(names).size).toBe(names.length);
    expect(names).toContain("task-status");
  });

  it("bridges every legacy channel to exactly one v1 topic (ADR-007)", () => {
    const topics = contract("event-topics.schema.json");
    expect(topics.topics).toBe(39);
    expect(topics.mapped).toBe(3);
    expect(topics.pending).toBe(36);
    expect(topics.reshapedPayloads).toBe(3);
    const claimed = new Map<string, number>();
    for (const row of topics.rows as Row[]) {
      expect(String(row.topic)).toMatch(/^[a-z0-9_]+\.[a-z0-9_]+$/);
      for (const channel of row.legacyChannels as string[]) {
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
    for (const event of eventSurface.events as Row[]) {
      expect(claimed.get(event.name as string), `${event.name} is uncovered`).toBe(1);
    }
    expect(topics.contract.unknownTopic).toContain("must not fail");
    // §2.3's own spellings are the three decided rows.
    expect((topics.rows as Row[]).filter((r) => r.status === "mapped").map((r) => r.topic).sort())
      .toEqual(["task.updated", "terminal.gap", "terminal.output"]);
  });
});
