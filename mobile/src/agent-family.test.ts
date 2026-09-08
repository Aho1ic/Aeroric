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

  it("gives omp the seven thinking levels, without ultra", () => {
    expect(reasoningOptionsForFamily("omp", "gpt-5.3-codex")).toEqual([
      "off",
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
  });

  it("groups omp profiles into the omp family, not claude", () => {
    /* 手机新建任务页按 family 分列。缺 omp 时档案即使返回也不进任何分组,用户永远选不到。 */
    const agents = [
      { id: "claude", label: "Claude Code", codexLike: false, family: "claude" as const },
      { id: "omp", label: "oh-my-pi", codexLike: false, family: "omp" as const },
      { id: "my-omp", label: "My Pi", codexLike: false, family: "omp" as const },
      { id: "dsh", label: "DeepSeek Harness", codexLike: false, family: "dsh" as const },
    ];
    const of = (family: "claude" | "codex" | "dsh" | "omp") =>
      agents.filter((choice) => agentFamilyOf(choice) === family).map((choice) => choice.id);

    expect(of("omp")).toEqual(["omp", "my-omp"]);
    expect(of("claude")).toEqual(["claude"]);
    expect(of("dsh")).toEqual(["dsh"]);
  });
});
