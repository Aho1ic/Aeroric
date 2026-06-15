import { describe, expect, it } from "vitest";
import { colorizePlainTerminalOutput } from "../components/terminalShared";

describe("terminal output highlighting", () => {
  it("adds ANSI colors for plain keyword and numeric output", () => {
    const highlighted = colorizePlainTerminalOutput("error line 42 passed\n");

    expect(highlighted).toContain("\x1b[31merror\x1b[39m");
    expect(highlighted).toContain("\x1b[36m42\x1b[39m");
    expect(highlighted).toContain("\x1b[32mpassed\x1b[39m");
  });

  it("does not rewrite output that already contains terminal control sequences", () => {
    const raw = "\x1b[31merror\x1b[0m 42";

    expect(colorizePlainTerminalOutput(raw)).toBe(raw);
  });
});
