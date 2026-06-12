import { describe, expect, it } from "vitest";
import {
  buildPromptWithGoalMode,
  shouldShowInstructionsBanner,
} from "../components/new-task/goalMode";

describe("new task goal mode", () => {
  it("hides the missing CLAUDE.md banner for Claude Code projects", () => {
    expect(shouldShowInstructionsBanner("claude", false)).toBe(false);
  });

  it("keeps the missing AGENTS.md banner for codex-compatible agents", () => {
    expect(shouldShowInstructionsBanner("codex", false)).toBe(true);
    expect(shouldShowInstructionsBanner("claude_gpt55", false)).toBe(true);
  });

  it("adds the requested /goal workflow to submitted prompts", () => {
    const prompt = buildPromptWithGoalMode("修复 SSH 终端显示", true);

    expect(prompt).toContain("/goal");
    expect(prompt).toContain("先列出 plan");
    expect(prompt).toContain("再进行修改");
    expect(prompt).toContain("修改完成后进行审查");
  });

  it("leaves prompts unchanged when /goal mode is off", () => {
    expect(buildPromptWithGoalMode("修复 SSH 终端显示", false)).toBe("修复 SSH 终端显示");
  });
});
