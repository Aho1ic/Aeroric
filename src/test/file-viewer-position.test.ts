import { describe, expect, it } from "vitest";
import { lineColumnToOffset } from "../components/file-viewer/position";

describe("file viewer position helpers", () => {
  it("converts one-based line and column into a document offset", () => {
    expect(lineColumnToOffset("one\ntwo\nthree", { line: 3, column: 2 })).toBe(9);
  });

  it("clamps missing or out-of-range columns and lines", () => {
    expect(lineColumnToOffset("abc\ndef", { line: 0, column: 20 })).toBe(3);
    expect(lineColumnToOffset("abc\ndef", { line: 9, column: 2 })).toBe(5);
    expect(lineColumnToOffset("abc\ndef", { line: 2 })).toBe(4);
  });

  it("does not count carriage returns as visible columns in CRLF files", () => {
    expect(lineColumnToOffset("abc\r\ndef", { line: 1, column: 20 })).toBe(3);
  });
});
