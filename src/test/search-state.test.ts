import { describe, expect, it } from "vitest";
import {
  buildTextSearchOptions,
  canApplyReplacementPreview,
  flattenReplacePreview,
  groupSearchMatches,
  searchMatchPreview,
} from "../components/search/searchState";
import type { ReplacePreview, TextSearchMatch } from "../types";

const matches: TextSearchMatch[] = [
  {
    path: "/repo/src/App.tsx",
    name: "App.tsx",
    line: 10,
    column: 5,
    lineText: "const title = 'Aeroric';",
    matchText: "title",
  },
  {
    path: "/repo/src/App.tsx",
    name: "App.tsx",
    line: 12,
    column: 9,
    lineText: "return <h1>{title}</h1>;",
    matchText: "title",
  },
  {
    path: "/repo/src/main.tsx",
    name: "main.tsx",
    line: 3,
    column: 1,
    lineText: "import App from './App';",
    matchText: "App",
  },
];

describe("search state", () => {
  it("groups matches by file path while preserving match order", () => {
    const groups = groupSearchMatches(matches);

    expect(groups).toHaveLength(2);
    expect(groups[0].path).toBe("/repo/src/App.tsx");
    expect(groups[0].matches.map((match) => match.line)).toEqual([10, 12]);
    expect(groups[1].path).toBe("/repo/src/main.tsx");
  });

  it("builds a compact preview around the matching column", () => {
    expect(
      searchMatchPreview({
        path: "/repo/a.ts",
        name: "a.ts",
        line: 1,
        column: 41,
        lineText: "0123456789012345678901234567890123456789MATCHtail",
        matchText: "MATCH",
      }),
    ).toBe("...2345678901234567890123456789MATCHtail");
  });

  it("builds search options with trimmed include and exclude globs", () => {
    expect(
      buildTextSearchOptions({
        caseSensitive: true,
        regex: false,
        wholeWord: true,
        includeGlob: "  src/**/*.ts  ",
        excludeGlob: "  src/generated/**  ",
        limit: 300,
      }),
    ).toEqual({
      caseSensitive: true,
      regex: false,
      wholeWord: true,
      includeGlob: "src/**/*.ts",
      excludeGlob: "src/generated/**",
      limit: 300,
    });
  });

  it("flattens replace preview matches into an apply payload", () => {
    const preview: ReplacePreview = {
      query: "old",
      replacement: "new",
      totalMatches: 2,
      truncated: false,
      files: [
        {
          path: "/repo/src/App.tsx",
          name: "App.tsx",
          matches: [
            {
              path: "/repo/src/App.tsx",
              name: "App.tsx",
              line: 1,
              column: 7,
              lineText: "const old = 1;",
              matchText: "old",
              replacementText: "new",
              start: 6,
              end: 9,
            },
            {
              path: "/repo/src/App.tsx",
              name: "App.tsx",
              line: 2,
              column: 8,
              lineText: "return old;",
              matchText: "old",
              replacementText: "new",
              start: 22,
              end: 25,
            },
          ],
        },
      ],
    };

    expect(flattenReplacePreview(preview)).toEqual([
      {
        path: "/repo/src/App.tsx",
        start: 6,
        end: 9,
        matchText: "old",
        replacementText: "new",
      },
      {
        path: "/repo/src/App.tsx",
        start: 22,
        end: 25,
        matchText: "old",
        replacementText: "new",
      },
    ]);
  });

  it("flattens only selected replace preview files when paths are provided", () => {
    const preview: ReplacePreview = {
      query: "old",
      replacement: "new",
      totalMatches: 2,
      truncated: false,
      files: [
        {
          path: "/repo/src/App.tsx",
          name: "App.tsx",
          matches: [
            {
              path: "/repo/src/App.tsx",
              name: "App.tsx",
              line: 1,
              column: 7,
              lineText: "const old = 1;",
              matchText: "old",
              replacementText: "new",
              start: 6,
              end: 9,
            },
          ],
        },
        {
          path: "/repo/src/utils.ts",
          name: "utils.ts",
          matches: [
            {
              path: "/repo/src/utils.ts",
              name: "utils.ts",
              line: 2,
              column: 8,
              lineText: "return old;",
              matchText: "old",
              replacementText: "new",
              start: 22,
              end: 25,
            },
          ],
        },
      ],
    };

    expect(flattenReplacePreview(preview, new Set(["/repo/src/utils.ts"]))).toEqual([
      {
        path: "/repo/src/utils.ts",
        start: 22,
        end: 25,
        matchText: "old",
        replacementText: "new",
      },
    ]);
  });

  it("requires a current preview before replacements can be applied", () => {
    const preview: ReplacePreview = {
      query: "old",
      replacement: "new",
      totalMatches: 1,
      truncated: false,
      files: [
        {
          path: "/repo/a.ts",
          name: "a.ts",
          matches: [
            {
              path: "/repo/a.ts",
              name: "a.ts",
              line: 1,
              column: 1,
              lineText: "old",
              matchText: "old",
              replacementText: "new",
              start: 0,
              end: 3,
            },
          ],
        },
      ],
    };

    expect(canApplyReplacementPreview(null, "old", "new")).toBe(false);
    expect(canApplyReplacementPreview(preview, "changed", "new")).toBe(false);
    expect(canApplyReplacementPreview(preview, "old", "updated")).toBe(false);
    expect(canApplyReplacementPreview(preview, " old ", "new")).toBe(true);
  });
});
