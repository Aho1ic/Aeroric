import { describe, expect, it } from "vitest";
import {
  AGENT_OPTIONS,
  agentFamily,
  agentOptionsFromProfiles,
  customAgentToOption,
  familyFromCodexLike,
  isBuiltInAgent,
  isCodexLikeAgent,
  isDshAgent,
  normalizeAgentConfigLang,
  normalizeProtocolFamily,
  profileFamily,
  sanitizeAgentId,
  type CustomAgentProfile,
} from "../agents";

function profile(overrides: Partial<CustomAgentProfile> = {}): CustomAgentProfile {
  return {
    id: "my_agent",
    label: "My Agent",
    path: "/tmp/agent.sh",
    codex_like: true,
    config_lang: "shellscript",
    ...overrides,
  };
}

describe("protocol family", () => {
  it("registers dsh as a built-in agent option with yaml config", () => {
    const dsh = AGENT_OPTIONS.find((option) => option.value === "dsh");
    expect(dsh).toBeDefined();
    expect(dsh?.label).toBe("DeepSeek Harness");
    expect(dsh?.configLang).toBe("yaml");
    expect(dsh?.family).toBe("dsh");
    expect(dsh?.codexLike).toBe(false);
  });

  it("treats dsh as built-in and prefixes colliding custom ids", () => {
    expect(isBuiltInAgent("dsh")).toBe(true);
    expect(sanitizeAgentId("dsh")).toBe("local_dsh");
    expect(sanitizeAgentId("DSH")).toBe("local_dsh");
  });

  it("derives family for the three built-ins", () => {
    expect(agentFamily("claude")).toBe("claude");
    expect(agentFamily("codex")).toBe("codex");
    expect(agentFamily("claude_gpt55")).toBe("codex");
    expect(agentFamily("dsh")).toBe("dsh");
  });

  it("keeps isCodexLikeAgent semantics (dsh is not codex-like)", () => {
    expect(isCodexLikeAgent("claude")).toBe(false);
    expect(isCodexLikeAgent("codex")).toBe(true);
    expect(isCodexLikeAgent("dsh")).toBe(false);
    expect(isDshAgent("dsh")).toBe(true);
    expect(isDshAgent("codex")).toBe(false);
  });

  it("derives custom profile family from codex_like when family is absent", () => {
    expect(profileFamily(profile({ codex_like: true }))).toBe("codex");
    expect(profileFamily(profile({ codex_like: false }))).toBe("claude");
    expect(profileFamily(profile({ codex_like: true, family: "" }))).toBe("codex");
  });

  it("prefers the explicit family over codex_like", () => {
    expect(profileFamily(profile({ codex_like: true, family: "dsh" }))).toBe("dsh");
    expect(profileFamily(profile({ codex_like: false, family: "codex" }))).toBe("codex");
    const option = customAgentToOption(profile({ codex_like: true, family: "dsh" }));
    expect(option.family).toBe("dsh");
    expect(option.codexLike).toBe(false);
  });

  it("keeps codexLike derived for custom options built from profiles", () => {
    const options = agentOptionsFromProfiles([
      profile({ id: "alpha", codex_like: false }),
      profile({ id: "beta", codex_like: true, family: "dsh" }),
    ]);
    const alpha = options.find((item) => item.value === "alpha");
    const beta = options.find((item) => item.value === "beta");
    expect(alpha?.family).toBe("claude");
    expect(beta?.family).toBe("dsh");
    expect(beta?.codexLike).toBe(false);
  });

  it("normalizes yaml config lang and protocol family values", () => {
    expect(normalizeAgentConfigLang("yaml")).toBe("yaml");
    expect(normalizeAgentConfigLang("nope")).toBe("shellscript");
    expect(normalizeProtocolFamily("dsh")).toBe("dsh");
    expect(normalizeProtocolFamily("x")).toBeUndefined();
    expect(familyFromCodexLike(true)).toBe("codex");
    expect(familyFromCodexLike(false)).toBe("claude");
  });

  it("falls back to codex family for unknown agents (legacy behavior)", () => {
    expect(agentFamily("mystery")).toBe("codex");
    expect(isCodexLikeAgent("mystery")).toBe(true);
  });
});
