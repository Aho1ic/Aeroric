import golden from "@aeroric/remote-contracts/fixtures";
import { describe, expect, it } from "vitest";
import {
  RPC_V2,
  RPC_V3,
  authenticationParams,
  decodeAeroricEnvelope,
  encodeAeroricRequest,
  negotiatedRpcVersion,
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
    expect(negotiatedRpcVersion(undefined)).toBe(RPC_V2);
    expect(negotiatedRpcVersion(3)).toBe(RPC_V3);
  });
});
