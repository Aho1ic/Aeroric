import { describe, expect, it } from "vitest";
import {
  sanitizeTerminalHistoryForHandoff,
  stripTerminalControlSequences,
} from "../sessionHandoff";

describe("session handoff terminal sanitization", () => {
  it("removes CSI, OSC, and other terminal control sequences", () => {
    const value = "\u001b[31mCodex\u001b[0m\u001b]0;Codex\u0007\n\u001b[2Jerror: upstream failed";

    const result = stripTerminalControlSequences(value);

    expect(result).toBe("Codex\nerror: upstream failed");
    expect(result).not.toContain("\u001b");
  });

  it("keeps readable terminal errors while dropping redraw fragments", () => {
    const value = [
      "\u001b[?2026h\u001b[1;1H\u001b[2KWelcome to Codex",
      "spinner\rready",
      '\u001b[38;5;1m{"error":{"message":"invalid request"}}\u001b[0m',
      "\u001b[2;1H***",
    ].join("\n");

    const result = sanitizeTerminalHistoryForHandoff(value);

    expect(result).toContain("Welcome to Codex");
    expect(result).toContain("ready");
    expect(result).toContain("invalid request");
    expect(result).not.toContain("\u001b");
    expect(result).not.toContain("spinner");
    expect(result).not.toContain("***");
  });

  it("normalizes carriage returns before the prompt reaches a new PTY", () => {
    expect(stripTerminalControlSequences("first\rsecond\r\nthird")).toBe("second\nthird");
  });
});
