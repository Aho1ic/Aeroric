import { describe, expect, it } from "vitest";
import {
  sanitizeTerminalHistoryForHandoff,
  stripTerminalControlSequences,
} from "../sessionHandoff";

const ESC = String.fromCharCode(0x1b);
const BEL = String.fromCharCode(0x07);
const CR = String.fromCharCode(0x0d);

describe("session handoff terminal sanitization", () => {
  it("removes CSI, OSC, and other terminal control sequences", () => {
    const value = `${ESC}[31mCodex${ESC}[0m${ESC}]0;Codex${BEL}\n${ESC}[2Jerror: upstream failed`;

    const result = stripTerminalControlSequences(value);

    expect(result).toBe("Codex\nerror: upstream failed");
    expect(result).not.toContain(ESC);
  });

  it("keeps readable terminal errors while dropping redraw fragments", () => {
    const value = [
      `${ESC}[?2026h${ESC}[1;1H${ESC}[2KWelcome to Codex`,
      `spinner${CR}ready`,
      `${ESC}[38;5;1m{"error":{"message":"invalid request"}}${ESC}[0m`,
      `${ESC}[2;1H***`,
    ].join("\n");

    const result = sanitizeTerminalHistoryForHandoff(value);

    expect(result).toContain("Welcome to Codex");
    expect(result).toContain("ready");
    expect(result).toContain("invalid request");
    expect(result).not.toContain(ESC);
    expect(result).not.toContain("spinner");
    expect(result).not.toContain("***");
  });

  it("normalizes carriage returns before the prompt reaches a new PTY", () => {
    expect(stripTerminalControlSequences(`first${CR}second${CR}\nthird`)).toBe("second\nthird");
  });

  it("splits cursor-movement redraw frames instead of welding them into one line", () => {
    // A TUI repaints in place: cursor-up + erase, with no newline between frames.
    const frames = ["W", "Wo", "Wor", "Work", "Worki", "Working"]
      .map((frame) => `${ESC}[1A${ESC}[2K${frame}`)
      .join("");
    const value = `${frames}${ESC}[1A${ESC}[2Kdone: build succeeded`;

    const result = sanitizeTerminalHistoryForHandoff(value);

    expect(result).toBe("done: build succeeded");
    expect(result).not.toContain("WWo");
  });

  it("drops spinner-only rows but keeps real sentences using those words", () => {
    const value = [
      "orking",
      "Workin",
      "Working",
      "Reconnecting... 1/3",
      "Working on src/main.rs now",
    ].join("\n");

    const result = sanitizeTerminalHistoryForHandoff(value);

    // Rows whose only latin words are spinner labels or partial renders of them
    // carry no task context, even when they include counters.
    expect(result.split("\n")).toEqual(["Working on src/main.rs now"]);
  });

  it("removes status-bar rows that carry no task context", () => {
    const value = [
      "applying patch to src/lib.rs",
      "esc to interrupt",
      "esc to view transcript",
    ].join("\n");

    expect(sanitizeTerminalHistoryForHandoff(value)).toBe("applying patch to src/lib.rs");
  });

  it("collapses repeated rows that reappear within the dedupe window", () => {
    const value = ["step one", "noise row", "step two", "noise row", "step three"].join("\n");

    expect(sanitizeTerminalHistoryForHandoff(value).split("\n")).toEqual([
      "step one",
      "noise row",
      "step two",
      "step three",
    ]);
  });

  it("keeps CJK content that contains no latin letters or digits", () => {
    expect(sanitizeTerminalHistoryForHandoff("正在修改配置文件")).toBe("正在修改配置文件");
  });
});
