import { describe, expect, it } from "vitest";
import { availableReasoningEfforts } from "../modelOptions";

describe("model reasoning options", () => {
  it("keeps legacy minimal for Codex but not Claude", () => {
    expect(availableReasoningEfforts(true, "gpt-5.6")).toContain("minimal");
    expect(availableReasoningEfforts(false, "sonnet")).not.toContain("minimal");
  });

  it("only exposes Codex Ultra for the supported model", () => {
    expect(availableReasoningEfforts(true, "gpt-5.6-sol")).toContain("ultra");
    expect(availableReasoningEfforts(true, "gpt-5.6-terra")).not.toContain("ultra");
  });
});
