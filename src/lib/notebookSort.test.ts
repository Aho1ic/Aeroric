import { describe, expect, it } from "vitest";
import { compareNotebookPath, compareNotebookText } from "./notebookSort";

describe("notebook sorting", () => {
  it("uses the explicit Chinese pinyin policy for visible text", () => {
    // Keep this small smoke test tied to the user-facing policy used by the
    // query view: it should not silently fall back to Unicode code-point order
    // (which would put 甲 after the other two characters).
    expect(["乙", "甲", "丙"].sort(compareNotebookText)).toEqual(["丙", "甲", "乙"]);
  });

  it("breaks collator ties by code point instead of input order", () => {
    const sorted = ["done", "Done", "DONE"].sort(compareNotebookText);
    expect(sorted).toEqual(["DONE", "Done", "done"]);
    expect(compareNotebookText("done", "Done")).not.toBe(0);
    expect(compareNotebookText("Done", "done")).not.toBe(0);
  });

  it("orders paths by code point and is antisymmetric", () => {
    expect(["/v/z.md", "/v/a.md"].sort(compareNotebookPath)).toEqual(["/v/a.md", "/v/z.md"]);
    for (const [left, right] of [
      ["/v/a.md", "/v/b.md"],
      ["/v/2.md", "/v/10.md"],
    ] as const) {
      expect(compareNotebookPath(left, right)).toBe(-compareNotebookPath(right, left));
    }
  });
});
