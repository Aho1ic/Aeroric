import { describe, expect, it } from "vitest";
import { zh } from "../i18n/zh";
import {
  getCommandPaletteIdeTools,
  getToolbarIdeTools,
  IDE_TOOL_REGISTRY,
  isIdeToolDisabled,
} from "../plugins/ideToolRegistry";

describe("ide tool registry", () => {
  it("registers IDE tools as metadata only", () => {
    expect(IDE_TOOL_REGISTRY.map((tool) => [tool.id, tool.titleKey, tool.commandId])).toEqual([
      ["git-advanced", "gitAdvanced.title", "git-advanced"],
      ["problems", "problems.title", "problems"],
      ["tests", "tests.title", "test-explorer"],
      ["debug", "debug.title", "debug"],
      ["run", "run.title", "run-configurations"],
      ["preview", "preview.title", "web-preview"],
      ["search", "toolbar.search", "search-files"],
      ["skills", "skills.installedSkills", "project-skills"],
    ]);
    for (const tool of IDE_TOOL_REGISTRY) {
      expect(zh[tool.titleKey], tool.titleKey).toEqual(expect.any(String));
    }
  });

  it("uses a distinct icon for Git Advanced instead of the Git Changes branch icon", () => {
    const gitAdvanced = IDE_TOOL_REGISTRY.find((tool) => tool.id === "git-advanced");

    expect(gitAdvanced?.icon as string | undefined).toBe("git-graph");
    expect(gitAdvanced?.icon as string | undefined).not.toBe("git-branch");
  });

  it("keeps toolbar tools ordered while preserving disabled entries", () => {
    const tools = getToolbarIdeTools({
      gitDisabled: true,
      problemsDisabled: true,
      testsDisabled: true,
      runDisabled: true,
      searchDisabled: true,
    });

    expect(tools.map((tool) => tool.id)).toEqual([
      "git-advanced",
      "problems",
      "tests",
      "debug",
      "run",
      "preview",
      "search",
      "skills",
    ]);
    expect(Object.fromEntries(tools.map((tool) => [tool.id, tool.disabled]))).toMatchObject({
      "git-advanced": true,
      problems: true,
      tests: true,
      debug: false,
      run: true,
      preview: false,
      search: true,
      skills: false,
    });
  });

  it("filters disabled tools out of command palette registration", () => {
    const commandTools = getCommandPaletteIdeTools({
      debugDisabled: true,
      previewDisabled: true,
      searchDisabled: true,
    });

    expect(commandTools.map((tool) => tool.commandId)).toEqual([
      "git-advanced",
      "problems",
      "test-explorer",
      "run-configurations",
      "project-skills",
    ]);
  });

  it("checks disable flags conservatively", () => {
    const searchTool = IDE_TOOL_REGISTRY.find((tool) => tool.id === "search");
    expect(searchTool).toBeTruthy();
    expect(isIdeToolDisabled(searchTool!, {})).toBe(false);
    expect(isIdeToolDisabled(searchTool!, { searchDisabled: true })).toBe(true);
  });
});
