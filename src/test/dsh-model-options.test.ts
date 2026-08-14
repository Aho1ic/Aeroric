import { describe, expect, it } from "vitest";
import {
  availableReasoningEfforts,
  availableReasoningEffortsForFamily,
  DSH_DEFAULT_MODELS,
  findModelIgnoreCase,
} from "../modelOptions";

describe("dsh model options", () => {
  it("ships the DeepSeek catalog as the default dsh model list", () => {
    expect(DSH_DEFAULT_MODELS).toContain("deepseek-v4-flash");
    expect(DSH_DEFAULT_MODELS).toContain("deepseek-v4-pro");
    expect(findModelIgnoreCase([...DSH_DEFAULT_MODELS], "DeepSeek-V4-Flash")).toBe(
      "deepseek-v4-flash",
    );
  });

  it("hides every effort tier for the dsh family", () => {
    expect(availableReasoningEffortsForFamily("dsh", undefined)).toEqual([]);
    expect(availableReasoningEffortsForFamily("dsh", "deepseek-v4-flash")).toEqual([]);
  });

  it("keeps claude and codex effort tiers unchanged", () => {
    expect(availableReasoningEffortsForFamily("claude", undefined)).toEqual(
      availableReasoningEfforts(false, undefined),
    );
    expect(availableReasoningEffortsForFamily("codex", "gpt-5.6-sol")).toEqual(
      availableReasoningEfforts(true, "gpt-5.6-sol"),
    );
    expect(availableReasoningEffortsForFamily("codex", "gpt-5.6-sol")).toContain("ultra");
    expect(availableReasoningEffortsForFamily("claude", undefined)).not.toContain("minimal");
  });
});
