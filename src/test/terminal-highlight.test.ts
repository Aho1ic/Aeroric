import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";
import {
  colorizePlainTerminalOutput,
  createSmartWriter,
  remapLightAnsiForeground,
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

  it("preserves distinct ANSI foreground colors", () => {
    const raw = "\x1b[31mred\x1b[0m \x1b[32mgreen\x1b[0m \x1b[34mblue\x1b[0m";

    expect(remapLightAnsiForeground(raw, "light")).toBe(raw);
    expect(remapLightAnsiForeground(raw, "dark")).toBe(raw);
  });

  it("remaps explicit white ANSI foregrounds in light themes", () => {
    const raw = "\x1b[1;97mbold white\x1b[0m \x1b[38;2;255;255;255mtruecolor\x1b[0m";

    expect(remapLightAnsiForeground(raw, "light")).toContain("\x1b[1;39m");
    expect(remapLightAnsiForeground(raw, "light")).toContain("\x1b[39mtruecolor");
    expect(remapLightAnsiForeground(raw, "dark")).toBe(raw);
  });

  it("lightens agent input and diff backgrounds while darkening pale code text", () => {
    const raw =
      "\x1b[40;38;2;190;190;190minput\x1b[0m " +
      "\x1b[48;2;60;20;20;38;2;180;180;180mremoved\x1b[0m " +
      "\x1b[48;5;22;97madded\x1b[0m";
    const light = remapLightAnsiForeground(raw, "light");

    expect(light).toContain("\x1b[48;2;234;238;242;39minput");
    expect(light).toContain("\x1b[48;2;255;235;233;39mremoved");
    expect(light).toContain("\x1b[48;2;218;251;225;39madded");
    expect(remapLightAnsiForeground(raw, "dark")).toBe(raw);
  });

  it("splits large writes without breaking surrogate pairs", () => {
    const emoji = "😀";
    const data = `${"x".repeat(TERMINAL_WRITE_CHUNK_SIZE - 1)}${emoji}tail`;

    const chunks = splitTerminalWriteChunk(data);

    expect(chunks.length).toBeGreaterThan(1);
    expect(chunks.join("")).toBe(data);
    expect(chunks[0].endsWith("\ud83d")).toBe(false);
  });

  it("does not split CSI control sequences", () => {
    const data = `abcde\x1b[38;2;255;255;255mwhite\x1b[0m`;

    const chunks = splitTerminalWriteChunk(data, 8);

    expect(chunks.join("")).toBe(data);
    expect(chunks[0]).toBe("abcde");
    expect(chunks[1].startsWith("\x1b[38;2;255;255;255m")).toBe(true);
  });

  it("does not split OSC control sequences", () => {
    const data = `abc\x1b]0;${"title".repeat(8)}\x07tail`;

    const chunks = splitTerminalWriteChunk(data, 10);

    expect(chunks.join("")).toBe(data);
    expect(chunks[0]).toBe("abc");
    expect(chunks[1].startsWith("\x1b]0;")).toBe(true);
    expect(chunks[1].endsWith("\x07")).toBe(true);
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

  it("immediately applies interactive redraws after user input", () => {
    vi.useFakeTimers();
    const write = vi.fn((_data: string, callback?: () => void) => callback?.());
    const writer = createSmartWriter({ write } as unknown as Terminal);

    writer.pauseForUserInput(50);
    writer.write("\x1b[2K\r12");

    expect(write).toHaveBeenCalledWith(
      expect.stringContaining("\x1b[2K\r12"),
      expect.any(Function),
    );
    vi.useRealTimers();
  });
});
