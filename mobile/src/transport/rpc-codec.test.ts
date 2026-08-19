import golden from "@aeroric/remote-contracts/fixtures";
import { describe, expect, it } from "vitest";
import {
  RPC_V2,
  RPC_V3,
  authenticationParams,
  decodeAeroricEnvelope,
  encodeAeroricRequest,
  negotiatedRpcVersion,
  normalizeHostSnapshot,
  normalizeRpcCapabilities,
} from "./rpc-codec";

describe("RPC codec", () => {
  it("matches the shared v3 golden request and push", () => {
    expect(JSON.parse(encodeAeroricRequest(RPC_V3, "rpc-7", "projects.list"))).toEqual(
      golden.request,
    );
    expect(decodeAeroricEnvelope(golden.push)).toEqual({
      kind: "push",
      event: "task-status",
      seq: 42,
      data: { task_id: "task-1", status: "running" },
    });
  });

  it("keeps auth on v2 and defaults missing negotiation to v2", () => {
    expect(authenticationParams({ deviceToken: "token" })).toMatchObject({
      deviceToken: "token",
      supportedRpcVersions: [3, 2],
    });
    expect(authenticationParams({}).capabilities).toEqual(golden.hello.capabilities);
    expect(negotiatedRpcVersion(undefined)).toBe(RPC_V2);
    expect(negotiatedRpcVersion(3)).toBe(RPC_V3);
  });

  it("normalizes additive host capabilities and ignores malformed entries", () => {
    expect(normalizeRpcCapabilities(["tasks.lifecycle", 3, "", null, "custom.feature"])).toEqual([
      "tasks.lifecycle",
      "custom.feature",
    ]);
    expect(
      normalizeHostSnapshot({
        name: "desktop",
        version: "1.2.3",
        platform: "macos",
        rpcVersions: [3, 2, 99],
        capabilities: ["typed-envelope", "files.read", { bad: true }],
        futureField: "ignored",
      }),
    ).toEqual({
      name: "desktop",
      version: "1.2.3",
      platform: "macos",
      rpcVersions: [3, 2],
      capabilities: ["typed-envelope", "files.read"],
    });
  });

  it("loads the shared project, task and session projection fixtures", () => {
    expect(golden.projectProjection).toMatchObject({
      id: "project-1",
      location: { kind: "local" },
      pinned: true,
    });
    expect(golden.taskProjection).toMatchObject({
      id: "task-1",
      dshAgentPreset: "code",
      approval: { requestId: "approval-1" },
    });
    expect(golden.sessionMessage.content.map((part) => part.type)).toEqual([
      "text",
      "thinking",
      "tool_use",
      "tool_result",
      "attachment",
      "opaque",
    ]);
  });

  it("keeps the structured v3 error while accepting additive fields", () => {
    expect(
      decodeAeroricEnvelope({
        v: RPC_V3,
        type: "response",
        id: "rpc-8",
        ok: false,
        error: {
          code: "rate_limited",
          message: "try again",
          retryable: true,
          details: { retryAfterMs: 500 },
          future: true,
        },
      }),
    ).toEqual({
      kind: "response",
      id: "rpc-8",
      ok: false,
      error: "try again",
      errorShape: {
        code: "rate_limited",
        message: "try again",
        retryable: true,
        details: { retryAfterMs: 500 },
      },
    });
  });
});
