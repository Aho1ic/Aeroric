import { describe, expect, it } from "vitest";
import {
  openFileInEditorGroup,
  openFileTab,
  splitEditorGroupRight,
} from "../hooks/projectPanelsState";

describe("project panel file opening", () => {
  it("adds a new tab and stores the requested editor selection", () => {
    const next = openFileTab(
      { tabs: [], activePath: null },
      { path: "/repo/src/App.tsx", name: "App.tsx", selection: { line: 12, column: 5 } },
    );

    expect(next.activePath).toBe("/repo/src/App.tsx");
    expect(next.tabs).toEqual([
      { path: "/repo/src/App.tsx", name: "App.tsx", selection: { line: 12, column: 5 } },
    ]);
  });

  it("reuses an existing tab and refreshes its requested selection", () => {
    const next = openFileTab(
      {
        activePath: "/repo/README.md",
        tabs: [
          { path: "/repo/src/App.tsx", name: "App.tsx", selection: { line: 1, column: 1 } },
          { path: "/repo/README.md", name: "README.md" },
        ],
      },
      { path: "/repo/src/App.tsx", name: "App.tsx", selection: { line: 40, column: 3 } },
    );

    expect(next.activePath).toBe("/repo/src/App.tsx");
    expect(next.tabs).toEqual([
      { path: "/repo/src/App.tsx", name: "App.tsx", selection: { line: 40, column: 3 } },
      { path: "/repo/README.md", name: "README.md" },
    ]);
  });

  it("clears an existing requested selection when reopening without one", () => {
    const next = openFileTab(
      {
        activePath: "/repo/src/App.tsx",
        tabs: [{ path: "/repo/src/App.tsx", name: "App.tsx", selection: { line: 40, column: 3 } }],
      },
      { path: "/repo/src/App.tsx", name: "App.tsx" },
    );

    expect(next.tabs).toEqual([{ path: "/repo/src/App.tsx", name: "App.tsx" }]);
  });

  it("opens a file in the requested editor group and makes that group active", () => {
    const next = openFileInEditorGroup(
      {
        activeGroupId: "main",
        groups: [
          {
            id: "main",
            tabs: [{ path: "/repo/src/App.tsx", name: "App.tsx" }],
            activePath: "/repo/src/App.tsx",
          },
          { id: "side", tabs: [], activePath: null },
        ],
      },
      { path: "/repo/README.md", name: "README.md" },
      "side",
    );

    expect(next.activeGroupId).toBe("side");
    expect(next.groups).toEqual([
      {
        id: "main",
        tabs: [{ path: "/repo/src/App.tsx", name: "App.tsx" }],
        activePath: "/repo/src/App.tsx",
      },
      {
        id: "side",
        tabs: [{ path: "/repo/README.md", name: "README.md" }],
        activePath: "/repo/README.md",
      },
    ]);
  });

  it("splits the active tab into a right editor group", () => {
    const next = splitEditorGroupRight({
      activeGroupId: "main",
      groups: [
        {
          id: "main",
          tabs: [
            { path: "/repo/src/App.tsx", name: "App.tsx" },
            { path: "/repo/README.md", name: "README.md" },
          ],
          activePath: "/repo/README.md",
        },
      ],
    });

    expect(next.activeGroupId).toBe("side");
    expect(next.groups).toEqual([
      {
        id: "main",
        tabs: [
          { path: "/repo/src/App.tsx", name: "App.tsx" },
          { path: "/repo/README.md", name: "README.md" },
        ],
        activePath: "/repo/README.md",
      },
      {
        id: "side",
        tabs: [{ path: "/repo/README.md", name: "README.md" }],
        activePath: "/repo/README.md",
      },
    ]);
  });
});
