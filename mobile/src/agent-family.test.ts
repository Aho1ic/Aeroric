import { describe, expect, it } from "vitest";
import { agentFamilyOf, reasoningOptionsForFamily } from "./agent-family";

describe("mobile agent family contract", () => {
  it("prefers explicit DSH family and keeps the legacy codexLike fallback", () => {
    expect(agentFamilyOf({ family: "dsh", codexLike: false })).toBe("dsh");
    expect(agentFamilyOf({ codexLike: true })).toBe("codex");
    expect(agentFamilyOf({ codexLike: false })).toBe("claude");
  });

  it("limits DSH reasoning to Off, High, and Max", () => {
    expect(reasoningOptionsForFamily("dsh", "deepseek-v4-pro")).toEqual(["off", "high", "max"]);
  });
});
