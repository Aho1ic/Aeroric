import type { Terminal } from "@xterm/xterm";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyTerminalTheme,
  attachCursorLineHighlight,
  colorizePlainTerminalOutput,
  createSmartWriter,
  DARK_THEME,
  EYECARE_THEME,
  LIGHT_THEME,
  remapAnsiForTheme,
  scheduleTerminalFrame,
  splitTerminalWriteChunk,
  TERMINAL_WRITE_CHUNK_SIZE,
} from "../components/terminalShared";

/* 文件级兜底:本文件有十处 vi.useFakeTimers(),绝大多数把 useRealTimers() 写在用例
   末尾而不是 finally 里 —— 断言一抛出就还不回真实时钟,后续用例全在冻结时钟下跑,
   失败会连片出现且指向无辜的用例。同理清掉 navigator.scheduling(见 :264 那条用例)。
   用例内已经显式还原的地方重复调用是幂等的。 */
afterEach(() => {
  vi.useRealTimers();
  Reflect.deleteProperty(navigator, "scheduling");
});

describe("terminal output highlighting", () => {
  it("raises contrast only for the light terminal while keeping ANSI hues distinct", () => {
    const term = { options: {} } as unknown as Terminal;

    applyTerminalTheme(term, "light");
    expect(term.options.minimumContrastRatio).toBe(4.5);
    expect(term.options.theme).toBe(LIGHT_THEME);
    expect(new Set([LIGHT_THEME.red, LIGHT_THEME.green, LIGHT_THEME.blue]).size).toBe(3);

    applyTerminalTheme(term, "dark");
    expect(term.options.minimumContrastRatio).toBe(1);
    expect(term.options.theme).toBe(DARK_THEME);

    applyTerminalTheme(term, "eyecare");
    expect(term.options.minimumContrastRatio).toBe(1);
    expect(term.options.theme).toBe(EYECARE_THEME);
  });

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

    expect(remapAnsiForTheme(raw, "light")).toBe(raw);
    expect(remapAnsiForTheme(raw, "dark")).toBe(raw);
    expect(remapAnsiForTheme(raw, "eyecare")).toBe(raw);
  });

  it("remaps explicit white ANSI foregrounds in light themes", () => {
    const raw = "\x1b[1;97mbold white\x1b[0m \x1b[38;2;255;255;255mtruecolor\x1b[0m";

    expect(remapAnsiForTheme(raw, "light")).toContain("\x1b[1;39m");
    expect(remapAnsiForTheme(raw, "light")).toContain("\x1b[39mtruecolor");
    expect(remapAnsiForTheme(raw, "dark")).toContain("\x1b[1;39m");
    expect(remapAnsiForTheme(raw, "eyecare")).toContain("\x1b[39mtruecolor");
  });

  it("uses the default background for agent input while preserving diff backgrounds", () => {
    const raw =
      "\x1b[40;38;2;190;190;190minput\x1b[0m " +
      "\x1b[48;2;60;20;20;38;2;180;180;180mremoved\x1b[0m " +
      "\x1b[48;5;22;97madded\x1b[0m";
    const light = remapAnsiForTheme(raw, "light");

    expect(light).toContain("\x1b[48;2;241;243;245;39minput");
    expect(light).toContain("\x1b[48;2;255;235;233;39mremoved");
    expect(light).toContain("\x1b[48;2;218;251;225;39madded");
    const dark = remapAnsiForTheme(raw, "dark");
    expect(dark).toContain("\x1b[48;2;17;21;26;39minput");
    expect(dark).toContain("\x1b[48;2;60;20;20;39mremoved");
    expect(dark).toContain("\x1b[48;5;22;39madded");
    expect(remapAnsiForTheme(raw, "eyecare")).toContain("\x1b[48;2;238;232;213;39minput");
  });

  it("normalizes neutral backgrounds before terminal erase commands in light themes", () => {
    const truecolor = "\x1b[48;2;236;238;240m\x1b[K";
    const darkNeutral = "\x1b[48;2;35;36;38mcomposer";
    const indexed = "\x1b[48;5;245mhistory";

    expect(remapAnsiForTheme(truecolor, "light")).toBe("\x1b[48;2;241;243;245m\x1b[K");
    expect(remapAnsiForTheme(darkNeutral, "eyecare")).toBe("\x1b[48;2;238;232;213mcomposer");
    expect(remapAnsiForTheme(indexed, "light")).toBe("\x1b[48;2;241;243;245mhistory");
    expect(remapAnsiForTheme(truecolor, "dark")).toBe("\x1b[48;2;17;21;26m\x1b[K");
  });

  it("keeps extended color channels atomic instead of turning them into text attributes", () => {
    const foreground = "\x1b[38;2;204;100;81mtext";
    const background = "\x1b[48;2;2;40;0mline";
    const underlineColor = "\x1b[58;2;40;100;21mdecorated";

    expect(remapAnsiForTheme(foreground, "dark")).toBe(foreground);
    expect(remapAnsiForTheme(background, "dark")).toBe(background);
    expect(remapAnsiForTheme(underlineColor, "dark")).toBe(underlineColor);
  });

  it("keeps full theme remapping opt-in for Agent terminal writers", () => {
    const raw = "\x1b[48;2;35;36;38mcomposer";
    const defaultWrite = vi.fn((_data: string, callback?: () => void) => callback?.());
    const agentWrite = vi.fn((_data: string, callback?: () => void) => callback?.());
    const defaultWriter = createSmartWriter(
      { write: defaultWrite } as unknown as Terminal,
      () => "dark",
    );
    const agentWriter = createSmartWriter(
      { write: agentWrite } as unknown as Terminal,
      () => "dark",
      { themeAwareAnsiRemap: true },
    );

    defaultWriter.writeImmediate(raw);
    agentWriter.writeImmediate(raw);

    expect(defaultWrite).toHaveBeenCalledWith(raw, expect.any(Function));
    expect(agentWrite).toHaveBeenCalledWith("\x1b[48;2;17;21;26mcomposer", expect.any(Function));
  });

  it("keeps fragmented cursor controls raw across PTY chunks", () => {
    const writes: string[] = [];
    const write = vi.fn((data: string, callback?: () => void) => {
      writes.push(data);
      callback?.();
    });
    const writer = createSmartWriter({ write } as unknown as Terminal);

    writer.write("\x1b[");
    writer.write("12;40Herror 42");

    expect(writes.join("")).toBe("\x1b[12;40Herror 42");
  });

  it("does not add semantic highlights to Agent TUI output", () => {
    const write = vi.fn((_data: string, callback?: () => void) => callback?.());
    const writer = createSmartWriter({ write } as unknown as Terminal, () => "dark", {
      themeAwareAnsiRemap: true,
    });

    writer.writeImmediate("running 42");

    expect(write).toHaveBeenCalledWith("running 42", expect.any(Function));
  });

  it("reports pending live and immediate writes until xterm acknowledges them", () => {
    const callbacks: Array<() => void> = [];
    const write = vi.fn((_data: string, callback?: () => void) => {
      if (callback) callbacks.push(callback);
    });
    const writer = createSmartWriter({ write } as unknown as Terminal);

    expect(writer.isIdle()).toBe(true);
    writer.write("live");
    expect(writer.isIdle()).toBe(false);
    callbacks.shift()?.();
    expect(writer.isIdle()).toBe(true);

    writer.writeImmediate("history");
    expect(writer.isIdle()).toBe(false);
    callbacks.shift()?.();
    expect(writer.isIdle()).toBe(true);
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

  it("defers queued rendering during composition and resumes on commit", () => {
    vi.useFakeTimers();
    const write = vi.fn((_data: string, callback?: () => void) => callback?.());
    const writer = createSmartWriter({ write } as unknown as Terminal, undefined, {
      resumeOnAnyOutput: true,
    });

    writer.setCompositionPaused(true);
    writer.write("background output");
    expect(write).not.toHaveBeenCalled();

    writer.setCompositionPaused(false);
    vi.runAllTimers();
    expect(write).toHaveBeenCalledWith("background output", expect.any(Function));
    vi.useRealTimers();
  });

  it("yields queued rendering while the browser has pending input", () => {
    let inputPending = true;
    /* `navigator.scheduling` 必须在 finally 里清掉:断言一失败就跳过收尾,这个属性会
       泄漏到本文件后面每一个 describe。带 `isInputPending` 时 `createSmartWriter` 走的是
       "有待处理输入就让路"的分支而不是默认分支,于是后面的用例单跑绿、整文件跑红 ——
       症状还会指向那些无辜的用例。同理 fake timers 也要还回去。 */
    try {
      vi.useFakeTimers();
      Object.defineProperty(navigator, "scheduling", {
        configurable: true,
        value: { isInputPending: () => inputPending },
      });
      const write = vi.fn((_data: string, callback?: () => void) => callback?.());
      const writer = createSmartWriter({ write } as unknown as Terminal);

      writer.write("background output");
      expect(write).not.toHaveBeenCalled();

      inputPending = false;
      vi.runAllTimers();
      expect(write).toHaveBeenCalledWith("background output", expect.any(Function));
    } finally {
      Reflect.deleteProperty(navigator, "scheduling");
      vi.useRealTimers();
    }
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
    container.dataset.terminalTheme = "light";
    const screen = document.createElement("div");
    screen.className = "xterm-screen";
    Object.defineProperty(screen, "clientHeight", { value: clientHeight, configurable: true });
    container.appendChild(screen);
    document.body.appendChild(container);
    return { container, screen };
  }

  it("cursor moves schedule a render asynchronously", () => {
    vi.useFakeTimers();
    const { term, state, fireCursorMove } = createFakeTerm();
    const { container, screen } = createScreenContainer(480); // 24 rows -> 20px each

    const dispose = attachCursorLineHighlight(term, container);

    const overlay = screen.querySelector<HTMLElement>(".aeroric-cursor-line");
    expect(overlay).not.toBeNull();
    expect(overlay!.style.height).toBe("20px");
    expect(overlay!.style.transform).toBe("translateY(0px)");
    expect(overlay!.dataset.terminalTheme).toBe("light");
    expect(overlay!.style.background).toBe("transparent");

    state.cursorY = 5;
    fireCursorMove();
    // 光标移动通过 scheduleRender 汇入 rAF,所以同步看不到变化。
    expect(overlay!.style.transform).toBe("translateY(0px)");
    vi.runOnlyPendingTimers();
    expect(overlay!.style.transform).toBe("translateY(100px)");

    container.dataset.terminalTheme = "dark";
    fireCursorMove();
    // rAF 合并后主题也会跟着更新。
    expect(overlay!.dataset.terminalTheme).toBe("light");
    vi.runOnlyPendingTimers();
    expect(overlay!.dataset.terminalTheme).toBe("dark");
    expect(overlay!.style.background).toBe("transparent");

    dispose();
    container.remove();
    vi.useRealTimers();
  });

  it("clamps the cursor row inside the visible range", () => {
    vi.useFakeTimers();
    const { term, state, fireCursorMove } = createFakeTerm();
    const { container, screen } = createScreenContainer(480);

    const dispose = attachCursorLineHighlight(term, container);
    const overlay = screen.querySelector<HTMLElement>(".aeroric-cursor-line")!;

    state.cursorY = 999;
    fireCursorMove();
    vi.runOnlyPendingTimers();
    // 24 rows, last row index 23 -> 23 * 20px
    expect(overlay.style.transform).toBe("translateY(460px)");

    dispose();
    container.remove();
    vi.useRealTimers();
  });

  it("recomputes row height on resize", () => {
    vi.useFakeTimers();
    const { term, state, fireResize } = createFakeTerm();
    const { container, screen } = createScreenContainer(480);

    const dispose = attachCursorLineHighlight(term, container);
    const overlay = screen.querySelector<HTMLElement>(".aeroric-cursor-line")!;

    state.rows = 12; // 480 / 12 = 40px
    state.cursorY = 2;
    fireResize();
    vi.runOnlyPendingTimers();
    expect(overlay.style.height).toBe("40px");
    expect(overlay.style.transform).toBe("translateY(80px)");

    dispose();
    container.remove();
    vi.useRealTimers();
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

  /* overlay 的重绘走共享的 `scheduleTerminalFrame`。窗口不可见时浏览器把 rAF 降到近乎
     停止,而 xterm 仍在写入、光标仍在移动 —— 如果只靠 rAF,overlay 会一直停在切走前那
     一行,直到用户切回来。下面钉住"隐藏时仍然落地"和"两条路径都能取消"。 */
  function stubVisibility(state: "visible" | "hidden") {
    const original = Object.getOwnPropertyDescriptor(Document.prototype, "visibilityState");
    Object.defineProperty(document, "visibilityState", { value: state, configurable: true });
    return () => {
      Reflect.deleteProperty(document, "visibilityState");
      if (original) Object.defineProperty(Document.prototype, "visibilityState", original);
    };
  }

  it("document 隐藏时仍然通过 setTimeout 分支落地重绘", () => {
    /* rAF 存在但永不回调 —— 真机上窗口不可见时就是这个样子。所以这条测试能区分
       "退到了 setTimeout" 和 "只是 jsdom 把 rAF 实现成了 setTimeout"。
       用赋值而不是 spyOn:jsdom 的 rAF 经常不是 own property,spyOn 再 mockRestore
       会把属性删掉,后面依赖 rAF 的用例就会炸。 */
    const restoreVisibility = stubVisibility("hidden");
    const origRaf = window.requestAnimationFrame;
    const raf = vi.fn().mockReturnValue(1);
    window.requestAnimationFrame = raf as typeof window.requestAnimationFrame;
    try {
      vi.useFakeTimers();
      const { term, state, fireCursorMove } = createFakeTerm();
      const { container, screen } = createScreenContainer(480); // 24 rows -> 20px each

      const dispose = attachCursorLineHighlight(term, container);
      const overlay = screen.querySelector<HTMLElement>(".aeroric-cursor-line")!;

      state.cursorY = 5;
      fireCursorMove();
      expect(overlay.style.transform).toBe("translateY(0px)");

      vi.runOnlyPendingTimers();
      expect(overlay.style.transform).toBe("translateY(100px)");
      expect(raf).not.toHaveBeenCalled();

      dispose();
      container.remove();
    } finally {
      window.requestAnimationFrame = origRaf;
      restoreVisibility();
      vi.useRealTimers();
    }
  });

  it("卸载时会撤掉已排的那一帧 —— rAF 与 setTimeout 两条路径都算", () => {
    // 可见:走 rAF,dispose 必须调 cancelAnimationFrame,否则切走后那一帧还会落地。
    {
      const restoreVisibility = stubVisibility("visible");
      const origRaf = window.requestAnimationFrame;
      const origCancel = window.cancelAnimationFrame;
      const raf = vi.fn().mockReturnValue(77);
      const cancelRaf = vi.fn();
      window.requestAnimationFrame = raf as typeof window.requestAnimationFrame;
      window.cancelAnimationFrame = cancelRaf as typeof window.cancelAnimationFrame;
      try {
        const { term, fireCursorMove } = createFakeTerm();
        const { container } = createScreenContainer(480);
        const dispose = attachCursorLineHighlight(term, container);
        fireCursorMove();
        expect(raf).toHaveBeenCalledTimes(1);
        dispose();
        expect(cancelRaf).toHaveBeenCalledWith(77);
        container.remove();
      } finally {
        window.requestAnimationFrame = origRaf;
        window.cancelAnimationFrame = origCancel;
        restoreVisibility();
      }
    }
    // 隐藏:走 setTimeout,dispose 必须调 clearTimeout。
    {
      const restoreVisibility = stubVisibility("hidden");
      const origSet = globalThis.setTimeout;
      const origClear = globalThis.clearTimeout;
      const setT = vi.fn().mockReturnValue(88);
      const clearT = vi.fn();
      globalThis.setTimeout = setT as unknown as typeof setTimeout;
      globalThis.clearTimeout = clearT as unknown as typeof clearTimeout;
      try {
        const { term, fireCursorMove } = createFakeTerm();
        const { container } = createScreenContainer(480);
        const dispose = attachCursorLineHighlight(term, container);
        fireCursorMove();
        expect(setT).toHaveBeenCalled();
        dispose();
        expect(clearT).toHaveBeenCalledWith(88);
        container.remove();
      } finally {
        globalThis.setTimeout = origSet;
        globalThis.clearTimeout = origClear;
        restoreVisibility();
      }
    }
  });
});

describe("scheduleTerminalFrame", () => {
  function stubVisibility(state: "visible" | "hidden") {
    const original = Object.getOwnPropertyDescriptor(Document.prototype, "visibilityState");
    Object.defineProperty(document, "visibilityState", { value: state, configurable: true });
    return () => {
      Reflect.deleteProperty(document, "visibilityState");
      if (original) Object.defineProperty(Document.prototype, "visibilityState", original);
    };
  }

  it("取消函数拦住回调 —— rAF 与 setTimeout 两条路径都算", () => {
    vi.useFakeTimers();
    try {
      for (const visibility of ["visible", "hidden"] as const) {
        const restoreVisibility = stubVisibility(visibility);
        const cb = vi.fn();
        const cancel = scheduleTerminalFrame(cb);
        cancel();
        vi.runAllTimers();
        expect(cb, `${visibility} 路径取消后回调仍跑了`).not.toHaveBeenCalled();
        restoreVisibility();
      }
    } finally {
      vi.useRealTimers();
    }
  });
});
