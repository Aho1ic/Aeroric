import { describe, expect, it } from "vitest";
import {
  branchGraphSummary,
  inlineBlameText,
  inlineBlameTitle,
  isGitConflictResolution,
  projectRelativeGitPath,
  stashDisplayTitle,
  summarizeBlameAuthors,
} from "../components/git-advanced/gitAdvancedState";

describe("git advanced state", () => {
  it("derives a project-relative blame path from the active editor file", () => {
    expect(projectRelativeGitPath("/repo/app", "/repo/app/src/index.ts")).toBe("src/index.ts");
    expect(projectRelativeGitPath("/repo/app/", "/repo/app/src/index.ts")).toBe("src/index.ts");
    expect(projectRelativeGitPath("C:\\repo\\app", "C:\\repo\\app\\src\\index.ts")).toBe(
      "src/index.ts",
    );
    expect(projectRelativeGitPath("/repo/app", "src/index.ts")).toBe("src/index.ts");
    expect(projectRelativeGitPath("/repo/app", "/other/app/src/index.ts")).toBe("");
    expect(projectRelativeGitPath("/repo/app", null)).toBe("");
  });

  it("summarizes blame authors by line count", () => {
    expect(
      summarizeBlameAuthors([
        {
          line: 1,
          commit: "a",
          shortCommit: "a",
          author: "Ada",
          authorTime: 1,
          summary: "",
          content: "",
        },
        {
          line: 2,
          commit: "b",
          shortCommit: "b",
          author: "Grace",
          authorTime: 1,
          summary: "",
          content: "",
        },
        {
          line: 3,
          commit: "c",
          shortCommit: "c",
          author: "Ada",
          authorTime: 1,
          summary: "",
          content: "",
        },
      ]),
    ).toEqual([
      { author: "Ada", lines: 2 },
      { author: "Grace", lines: 1 },
    ]);
  });

  it("builds compact inline blame labels", () => {
    const line = {
      line: 7,
      commit: "abcdef123456",
      shortCommit: "abcdef1",
      author: "Ada",
      authorTime: 1,
      summary: "Add parser",
      content: "const value = 1;",
    };

    expect(inlineBlameText(line)).toBe("abcdef1 Ada - Add parser");
    expect(inlineBlameTitle(line)).toBe("abcdef123456 Ada - Add parser");
  });

  it("builds compact stash labels", () => {
    expect(
      stashDisplayTitle({
        name: "stash@{0}",
        index: 0,
        commit: "abc",
        date: "2 hours ago",
        message: "WIP on main",
      }),
    ).toBe("stash@{0} WIP on main");
  });

  it("accepts only supported conflict resolution actions", () => {
    expect(isGitConflictResolution("ours")).toBe(true);
    expect(isGitConflictResolution("theirs")).toBe(true);
    expect(isGitConflictResolution("both")).toBe(true);
    expect(isGitConflictResolution("delete")).toBe(false);
  });

  it("summarizes branch graph refs and current branch", () => {
    expect(
      branchGraphSummary({
        commits: [
          {
            hash: "abcdef123456",
            shortHash: "abcdef1",
            parents: ["1111111"],
            refs: ["HEAD -> main", "origin/main", "tag: v1.0.0"],
            subject: "Add graph",
            author: "Ada",
            relativeTime: "2 minutes ago",
          },
          {
            hash: "111111111111",
            shortHash: "1111111",
            parents: [],
            refs: [],
            subject: "Initial",
            author: "Grace",
            relativeTime: "1 hour ago",
          },
        ],
        truncated: false,
      }),
    ).toEqual({
      totalCommits: 2,
      currentBranch: "main",
      refs: ["main", "origin/main", "v1.0.0"],
    });
  });
});
