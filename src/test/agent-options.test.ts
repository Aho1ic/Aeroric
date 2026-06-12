import { describe, expect, it } from "vitest";
import { AGENT_OPTIONS, agentDisplayLabel, isCodexLikeAgent } from "../agents";

describe("agent options", () => {
  it("exposes the three configured launch profiles in dropdown order", () => {
    expect(AGENT_OPTIONS.map((agent) => agent.value)).toEqual([
      "claude",
      "claude_gpt55",
      "codex",
    ]);
  });

  it("labels the configured launch profiles clearly", () => {
    expect(agentDisplayLabel("claude")).toBe("Claude Code");
    expect(agentDisplayLabel("claude_gpt55")).toBe("Claude GPT55");
    expect(agentDisplayLabel("codex")).toBe("Codex");
  });

  it("treats the GPT55 script as codex-compatible because it execs codex", () => {
    expect(isCodexLikeAgent("claude")).toBe(false);
    expect(isCodexLikeAgent("claude_gpt55")).toBe(true);
    expect(isCodexLikeAgent("codex")).toBe(true);
  });
});
