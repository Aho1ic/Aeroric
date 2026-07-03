import { describe, expect, it } from "vitest";
import type { Terminal } from "@xterm/xterm";
import { vi } from "vitest";
import {
  colorizePlainTerminalOutput,
  createSmartWriter,
  splitTerminalWriteChunk,
  TERMINAL_WRITE_CHUNK_SIZE,
} from "../components/terminalShared";

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

  it("splits large writes without breaking surrogate pairs", () => {
    const emoji = "😀";
    const data = `${"x".repeat(TERMINAL_WRITE_CHUNK_SIZE - 1)}${emoji}tail`;

    const chunks = splitTerminalWriteChunk(data);

    expect(chunks.length).toBeGreaterThan(1);
    expect(chunks.join("")).toBe(data);
    expect(chunks[0].endsWith("\ud83d")).toBe(false);
  });

  it("briefly defers terminal output after user input", () => {
    vi.useFakeTimers();
    const write = vi.fn((_data: string, callback?: () => void) => callback?.());
    const writer = createSmartWriter({ write } as unknown as Terminal);

    writer.pauseForUserInput(50);
    writer.write("running");

    expect(write).not.toHaveBeenCalled();
    vi.advanceTimersByTime(49);
    expect(write).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);

    expect(write).toHaveBeenCalledWith(expect.stringContaining("running"), expect.any(Function));
    vi.useRealTimers();
  });
});
