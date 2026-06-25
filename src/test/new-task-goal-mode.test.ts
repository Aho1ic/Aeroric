import { describe, expect, it } from "vitest";
import {
  buildPromptWithTaskModes,
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

  it("adds plan mode instructions when plan mode is on", () => {
    const prompt = buildPromptWithTaskModes("修复 SSH 终端显示", {
      planMode: true,
      goalMode: false,
    });

    expect(prompt).toContain("plan mode on");
    expect(prompt).toContain("先给出实现计划");
    expect(prompt).toContain("等待确认后再修改文件");
    expect(prompt).not.toContain("/goal");
  });

  it("adds the requested /goal workflow when goal mode is on", () => {
    const prompt = buildPromptWithTaskModes("修复 SSH 终端显示", {
      planMode: false,
      goalMode: true,
    });

    expect(prompt).toContain("/goal");
    expect(prompt).toContain("先列出 plan");
    expect(prompt).toContain("再进行修改");
    expect(prompt).toContain("修改完成后进行审查");
  });

  it("keeps plan mode before /goal when both modes are on", () => {
    const prompt = buildPromptWithTaskModes("修复 SSH 终端显示", {
      planMode: true,
      goalMode: true,
    });

    expect(prompt.indexOf("plan mode on")).toBeLessThan(prompt.indexOf("/goal"));
  });

  it("leaves prompts unchanged when /goal mode is off", () => {
    expect(
      buildPromptWithTaskModes("修复 SSH 终端显示", {
        planMode: false,
        goalMode: false,
      }),
    ).toBe("修复 SSH 终端显示");
  });
});
