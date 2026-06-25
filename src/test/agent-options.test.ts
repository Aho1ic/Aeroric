import { describe, expect, it } from "vitest";
import {
  AGENT_OPTIONS,
  agentDisplayLabel,
  agentOptionsFromProfiles,
  isCodexLikeAgent,
} from "../agents";
import {
  composeControlOrder,
  composePermissionLabel,
  nextComposeMenuState,
} from "../components/new-task/AgentPermSelector";

describe("agent options", () => {
  it("exposes only clean release launch profiles in dropdown order", () => {
    expect(AGENT_OPTIONS.map((agent) => agent.value)).toEqual(["claude", "codex"]);
  });

  it("labels the release launch profiles clearly", () => {
    expect(agentDisplayLabel("claude")).toBe("Claude Code");
    expect(agentDisplayLabel("codex")).toBe("Codex");
  });

  it("ships release profiles without local config file paths", () => {
    expect(AGENT_OPTIONS.map((agent) => [agent.value, agent.configFile])).toEqual([
      ["claude", ""],
      ["codex", ""],
    ]);
  });

  it("applies local display-name overrides without changing release defaults", () => {
    const options = agentOptionsFromProfiles([], { claude: "Local Claude", codex: "Local Codex" });

    expect(agentDisplayLabel("claude", options)).toBe("Local Claude");
    expect(agentDisplayLabel("codex", options)).toBe("Local Codex");
    expect(agentDisplayLabel("claude")).toBe("Claude Code");
    expect(agentDisplayLabel("codex")).toBe("Codex");
  });

  it("treats the GPT55 script as codex-compatible because it execs codex", () => {
    expect(isCodexLikeAgent("claude")).toBe(false);
    expect(isCodexLikeAgent("claude_gpt55")).toBe(true);
    expect(isCodexLikeAgent("codex")).toBe(true);
  });

  it("uses concise permission labels in the compose toolbar", () => {
    expect(composePermissionLabel("ask")).toBe("请求确认");
    expect(composePermissionLabel("auto_edit")).toBe("替我审批");
    expect(composePermissionLabel("full_access")).toBe("完全访问");
  });

  it("keeps compose controls in the requested order", () => {
    expect(composeControlOrder()).toEqual([
      "more",
      "agent",
      "permission",
      "launch",
      "branch",
      "send",
    ]);
  });

  it("keeps only one compose dropdown open at a time", () => {
    expect(nextComposeMenuState("agent", "launch", true)).toBe("launch");
    expect(nextComposeMenuState("launch", "branch", true)).toBe("branch");
    expect(nextComposeMenuState("branch", "permission", true)).toBe("permission");
    expect(nextComposeMenuState("send", "send", false)).toBe(null);
  });
});
