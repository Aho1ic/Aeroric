import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";
import {
  attachCursorLineHighlight,
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

  it("uses the default background for agent input while preserving diff backgrounds", () => {
    const raw =
      "\x1b[40;38;2;190;190;190minput\x1b[0m " +
      "\x1b[48;2;60;20;20;38;2;180;180;180mremoved\x1b[0m " +
      "\x1b[48;5;22;97madded\x1b[0m";
    const light = remapLightAnsiForeground(raw, "light");

    expect(light).toContain("\x1b[49;39minput");
    expect(light).toContain("\x1b[48;2;255;235;233;39mremoved");
    expect(light).toContain("\x1b[48;2;218;251;225;39madded");
    expect(remapLightAnsiForeground(raw, "dark")).toBe(raw);
  });

  it("normalizes neutral backgrounds before terminal erase commands in light themes", () => {
    const truecolor = "\x1b[48;2;236;238;240m\x1b[K";
    const darkNeutral = "\x1b[48;2;35;36;38mcomposer";
    const indexed = "\x1b[48;5;245mhistory";

    expect(remapLightAnsiForeground(truecolor, "light")).toBe("\x1b[49m\x1b[K");
    expect(remapLightAnsiForeground(darkNeutral, "eyecare")).toBe("\x1b[49mcomposer");
    expect(remapLightAnsiForeground(indexed, "light")).toBe("\x1b[49mhistory");
    expect(remapLightAnsiForeground(truecolor, "dark")).toBe(truecolor);
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

  it("immediately applies plain shell echo when interactive output is enabled", () => {
    vi.useFakeTimers();
    const write = vi.fn((_data: string, callback?: () => void) => callback?.());
    const writer = createSmartWriter({ write } as unknown as Terminal, undefined, {
      resumeOnAnyOutput: true,
    });

    writer.pauseForUserInput(50);
    writer.write("a");

    expect(write).toHaveBeenCalledWith("a", expect.any(Function));
    vi.useRealTimers();
  });
});

describe("cursor line highlight overlay", () => {
  type Listener = () => void;

  function createFakeTerm() {
    const cursorMove: Listener[] = [];
    const render: Listener[] = [];
    const resize: Listener[] = [];
    const state = { rows: 24, cursorY: 0 };
    const term = {
      get rows() {
        return state.rows;
      },
      buffer: {
        active: {
          get cursorY() {
            return state.cursorY;
          },
        },
      },
      onCursorMove: (fn: Listener) => {
        cursorMove.push(fn);
        return { dispose: () => {} };
      },
      onRender: (fn: Listener) => {
        render.push(fn);
        return { dispose: () => {} };
      },
      onResize: (fn: Listener) => {
        resize.push(fn);
        return { dispose: () => {} };
      },
    } as unknown as Terminal;
    const fire = (list: Listener[]) => list.forEach((fn) => fn());
    return {
      term,
      state,
      fireCursorMove: () => fire(cursorMove),
      fireResize: () => fire(resize),
    };
  }

  function createScreenContainer(clientHeight = 480) {
    const container = document.createElement("div");
    const screen = document.createElement("div");
    screen.className = "xterm-screen";
    Object.defineProperty(screen, "clientHeight", { value: clientHeight, configurable: true });
    container.appendChild(screen);
    document.body.appendChild(container);
    return { container, screen };
  }

  it("inserts a cursor-line overlay into the xterm screen and follows the cursor", () => {
    const { term, state, fireCursorMove } = createFakeTerm();
    const { container, screen } = createScreenContainer(480); // 24 rows -> 20px each

    const dispose = attachCursorLineHighlight(term, container);

    const overlay = screen.querySelector<HTMLElement>(".aeroric-cursor-line");
    expect(overlay).not.toBeNull();
    expect(overlay!.style.height).toBe("20px");
    expect(overlay!.style.transform).toBe("translateY(0px)");

    state.cursorY = 5;
    fireCursorMove();
    expect(overlay!.style.transform).toBe("translateY(100px)");

    dispose();
    container.remove();
  });

  it("clamps the cursor row inside the visible range", () => {
    const { term, state, fireCursorMove } = createFakeTerm();
    const { container, screen } = createScreenContainer(480);

    const dispose = attachCursorLineHighlight(term, container);
    const overlay = screen.querySelector<HTMLElement>(".aeroric-cursor-line")!;

    state.cursorY = 999;
    fireCursorMove();
    // 24 rows, last row index 23 -> 23 * 20px
    expect(overlay.style.transform).toBe("translateY(460px)");

    dispose();
    container.remove();
  });

  it("recomputes row height on resize", () => {
    const { term, state, fireResize } = createFakeTerm();
    const { container, screen } = createScreenContainer(480);

    const dispose = attachCursorLineHighlight(term, container);
    const overlay = screen.querySelector<HTMLElement>(".aeroric-cursor-line")!;

    state.rows = 12; // 480 / 12 = 40px
    state.cursorY = 2;
    fireResize();
    expect(overlay.style.height).toBe("40px");
    expect(overlay.style.transform).toBe("translateY(80px)");

    dispose();
    container.remove();
  });

  it("removes the overlay on dispose", () => {
    const { term } = createFakeTerm();
    const { container, screen } = createScreenContainer();

    const dispose = attachCursorLineHighlight(term, container);
    expect(screen.querySelector(".aeroric-cursor-line")).not.toBeNull();

    dispose();
    expect(screen.querySelector(".aeroric-cursor-line")).toBeNull();
    container.remove();
  });

  it("hides the overlay when the screen has no measurable height", () => {
    const { term } = createFakeTerm();
    const { container, screen } = createScreenContainer(0);

    const dispose = attachCursorLineHighlight(term, container);
    const overlay = screen.querySelector<HTMLElement>(".aeroric-cursor-line")!;
    expect(overlay.style.display).toBe("none");

    dispose();
    container.remove();
  });
});
