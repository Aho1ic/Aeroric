import { describe, expect, it } from "vitest";
import {
  commandPaletteModeForInput,
  moveCommandPaletteSelection,
  rankCommandPaletteItems,
} from "../components/command-palette/commandPaletteState";
import type { CommandPaletteItem } from "../components/command-palette/types";

const items: CommandPaletteItem[] = [
  { id: "settings", title: "Open Settings", kind: "command" },
  { id: "terminal", title: "Open Terminal", kind: "command", keywords: ["shell"] },
  { id: "src-app", title: "App.tsx", subtitle: "src", kind: "file" },
  { id: "src-project", title: "ProjectPage.tsx", subtitle: "src/components", kind: "file" },
];

describe("command palette state", () => {
  it("uses command mode for > input and strips the prefix from the query", () => {
    expect(commandPaletteModeForInput("> terminal")).toEqual({
      mode: "command",
      query: "terminal",
    });
  });

  it("ranks exact and prefix matches before fuzzy subtitle matches", () => {
    const ranked = rankCommandPaletteItems(items, "app", "file");

    expect(ranked.map((item) => item.id)).toEqual(["src-app"]);
  });

  it("matches command keywords when filtering commands", () => {
    const ranked = rankCommandPaletteItems(items, "shell", "command");

    expect(ranked.map((item) => item.id)).toEqual(["terminal"]);
  });

  it("wraps keyboard selection within result bounds", () => {
    expect(moveCommandPaletteSelection(0, -1, 4)).toBe(3);
    expect(moveCommandPaletteSelection(3, 1, 4)).toBe(0);
    expect(moveCommandPaletteSelection(1, 1, 4)).toBe(2);
  });
});
