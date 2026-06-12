import { describe, expect, it } from "vitest";
import { createSshShellId } from "../components/ssh/session";

describe("createSshShellId", () => {
  it("prefixes SSH terminal ids and includes connection id plus timestamp", () => {
    expect(createSshShellId("prod", 1700000000000)).toBe("ssh:prod:1700000000000");
  });
});
