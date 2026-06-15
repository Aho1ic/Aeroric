import { describe, expect, it } from "vitest";
import { createSshShellId, shouldAttemptSshAutoConnect } from "../components/ssh/session";

describe("createSshShellId", () => {
  it("prefixes SSH terminal ids and includes connection id plus timestamp", () => {
    expect(createSshShellId("prod", 1700000000000)).toBe("ssh:prod:1700000000000");
  });

  it("allows SSH auto-connect again after re-entering a disconnected remote project", () => {
    expect(
      shouldAttemptSshAutoConnect({
        autoConnect: true,
        active: true,
        hasActiveSession: false,
        connectionId: "prod",
        lastStartedConnectionId: null,
      }),
    ).toBe(true);

    expect(
      shouldAttemptSshAutoConnect({
        autoConnect: true,
        active: true,
        hasActiveSession: false,
        connectionId: "prod",
        lastStartedConnectionId: "prod",
      }),
    ).toBe(false);

    expect(
      shouldAttemptSshAutoConnect({
        autoConnect: true,
        active: false,
        hasActiveSession: false,
        connectionId: "prod",
        lastStartedConnectionId: null,
      }),
    ).toBe(false);
  });
});
