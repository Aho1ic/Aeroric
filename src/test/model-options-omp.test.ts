import { describe, expect, it } from "vitest";
import {
  OMP_THINKING_LEVELS,
  OMP_THINKING_LEVEL_MAP,
  availableReasoningEffortsForFamily,
  ompThinkingLevelFor,
} from "../modelOptions";

describe("omp thinking levels", () => {
  it("exposes the omp thinking-level vocabulary without ultra", () => {
    expect(OMP_THINKING_LEVELS).toEqual([
      "off",
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(OMP_THINKING_LEVELS).not.toContain("ultra");
  });

  it("maps the unified effort vocabulary onto omp thinking levels", () => {
    // omp 原生档位恒等透传;仅 ultra(omp 无此档)封顶为 max。
    expect(OMP_THINKING_LEVEL_MAP.off).toBe("off");
    expect(OMP_THINKING_LEVEL_MAP.minimal).toBe("minimal");
    expect(OMP_THINKING_LEVEL_MAP.low).toBe("low");
    expect(OMP_THINKING_LEVEL_MAP.medium).toBe("medium");
    expect(OMP_THINKING_LEVEL_MAP.high).toBe("high");
    expect(OMP_THINKING_LEVEL_MAP.xhigh).toBe("xhigh");
    expect(OMP_THINKING_LEVEL_MAP.max).toBe("max");
    expect(OMP_THINKING_LEVEL_MAP.ultra).toBe("max");
  });

  it("falls back to medium for unknown effort values", () => {
    expect(ompThinkingLevelFor(undefined)).toBe("medium");
    expect(ompThinkingLevelFor("bogus")).toBe("medium");
    expect(ompThinkingLevelFor("constructor")).toBe("medium");
    expect(ompThinkingLevelFor("high")).toBe("high");
  });

  it("offers the omp vocabulary via the family-aware effort lookup", () => {
    expect(availableReasoningEffortsForFamily("omp", "anthropic/claude-sonnet-4-5")).toEqual(
      OMP_THINKING_LEVELS,
    );
  });
});
