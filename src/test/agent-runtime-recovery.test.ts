import { describe, expect, it } from "vitest";
import { shouldOfferWindowsNodeInstaller } from "../components/agentRuntimeRecovery";

describe("Windows Claude runtime recovery", () => {
  it("recognizes the localized PowerShell error from older launchers", () => {
    expect(
      shouldOfferWindowsNodeInstaller(
        "windows",
        "& : 无法将“claude”项识别为 cmdlet、函数、脚本文件或可运行程序的名称。\n+CategoryInfo: ObjectNotFound: (claude:String) [], CommandNotFoundException",
      ),
    ).toBe(true);
  });

  it("recognizes the stable marker from refreshed launchers", () => {
    expect(
      shouldOfferWindowsNodeInstaller(
        "windows",
        "AERORIC_CLAUDE_CLI_NOT_FOUND: Claude Code CLI was not found.",
      ),
    ).toBe(true);
  });

  it("does not offer the Windows installer for unrelated or non-Windows failures", () => {
    expect(shouldOfferWindowsNodeInstaller("macos", "AERORIC_CLAUDE_CLI_NOT_FOUND")).toBe(false);
    expect(
      shouldOfferWindowsNodeInstaller("windows", "CommandNotFoundException: git is unavailable"),
    ).toBe(false);
  });
});
