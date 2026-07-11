import { afterEach, describe, expect, it, vi } from "vitest";
import {
  buildPostCompositionIgnoredCandidates,
  normalizeCommittedCompositionText,
  POST_COMPOSITION_REPLAY_IGNORE_MS,
  applyTerminalTextareaInputAttributes,
  shouldSuppressPrintableKeyRepeat,
  shouldIgnorePostCompositionCandidate,
  shouldIgnorePostCompositionInsert,
  shouldDeferRomanizedCompositionCommit,
  shouldPreserveBrowserCompositionPreview,
} from "../components/terminalInputFix";
import { normalizeEditorCompositionText } from "../components/new-task/PromptEditor";

afterEach(() => {
  vi.restoreAllMocks();
  vi.resetModules();
});

describe("terminal input fixes", () => {
  it("normalizes macOS WebKit pinyin text committed while IME is closing", () => {
    expect(normalizeCommittedCompositionText("s'dsd")).toBe("sd");
    expect(normalizeCommittedCompositionText("sds'dsd")).toBe("sd");
    expect(normalizeCommittedCompositionText("shuo'huashuohua")).toBe("shuohua");
    expect(normalizeCommittedCompositionText("shuohuashuo'huashuohua")).toBe("shuohua");
    expect(normalizeCommittedCompositionText("s'd's'd")).toBe("sdsd");
    expect(normalizeCommittedCompositionText("s'd's'ds'd's'd")).toBe("sdsdsdsd");
  });

  it("leaves normal composition text unchanged", () => {
    expect(normalizeCommittedCompositionText("你好")).toBe("你好");
    expect(normalizeCommittedCompositionText("hello")).toBe("hello");
  });

  it("ignores WebKit text replay after a composition has already been committed", () => {
    expect(shouldIgnorePostCompositionInsert("s'dsd", "s'd", "sd")).toBe(true);
    expect(shouldIgnorePostCompositionInsert("shuo'huashuohua", "shuo'hua", "shuohua")).toBe(true);
    expect(shouldIgnorePostCompositionInsert("shuohua", "shuo'hua", "shuohua")).toBe(true);
    expect(shouldIgnorePostCompositionInsert("abc", "s'd", "sd")).toBe(false);
  });

  it("ignores raw pinyin replay after committing Chinese text", () => {
    const candidates = buildPostCompositionIgnoredCandidates("测试", "ceshi");

    expect(shouldIgnorePostCompositionCandidate("ceshi", candidates)).toBe(true);
    expect(shouldIgnorePostCompositionCandidate("测试", candidates)).toBe(true);
    expect(shouldIgnorePostCompositionCandidate("测试ceshi", candidates)).toBe(true);
    expect(shouldIgnorePostCompositionCandidate("hello", candidates)).toBe(false);
  });

  it("removes pinyin preedit text appended after committed Chinese text", () => {
    expect(normalizeCommittedCompositionText("是的shi'de")).toBe("是的");
    expect(normalizeCommittedCompositionText("测试ce'shi")).toBe("测试");
    expect(normalizeCommittedCompositionText("你好ni'hao")).toBe("你好");
  });

  it("keeps intentional Chinese and English mixed input", () => {
    expect(normalizeCommittedCompositionText("测试abc")).toBe("测试abc");
  });

  it("keeps the post-composition replay guard long enough for delayed macOS WebKit insertText", () => {
    expect(POST_COMPOSITION_REPLAY_IGNORE_MS).toBeGreaterThanOrEqual(3000);
  });

  it("ignores pinyin replay when switching IME to English mid-composition", () => {
    const candidates = buildPostCompositionIgnoredCandidates("shuo'hua", "shuo'hua");

    expect(shouldIgnorePostCompositionCandidate("shuohua", candidates)).toBe(true);
    expect(shouldIgnorePostCompositionCandidate("shuo'huashuohua", candidates)).toBe(true);
    expect(shouldIgnorePostCompositionCandidate("shuo", candidates)).toBe(false);
  });

  it("preserves WebKit live IME composition input for xterm's preview", () => {
    expect(shouldPreserveBrowserCompositionPreview("insertCompositionText", true)).toBe(true);
    expect(shouldPreserveBrowserCompositionPreview("insertText", true)).toBe(false);
    expect(shouldPreserveBrowserCompositionPreview("insertCompositionText", false)).toBe(false);
    expect(shouldPreserveBrowserCompositionPreview("insertText", false)).toBe(false);
  });

  it("shows pinyin preedit text within the remaining terminal width", async () => {
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const terminalElement = document.createElement("div");
    terminalElement.className = "xterm";
    const screen = document.createElement("div");
    screen.className = "xterm-screen";
    const textarea = document.createElement("textarea");
    const compositionView = document.createElement("div");
    compositionView.className = "composition-view";
    terminalElement.append(screen, textarea, compositionView);
    document.body.appendChild(terminalElement);
    vi.spyOn(screen, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      width: 120,
      height: 100,
      top: 0,
      right: 120,
      bottom: 100,
      left: 0,
      toJSON: () => ({}),
    });
    textarea.addEventListener("compositionupdate", (event) => {
      const compositionEvent = event as CompositionEvent;
      compositionView.textContent = compositionEvent.data;
      compositionView.style.left = "80px";
      compositionView.classList.toggle("active", Boolean(compositionEvent.data));
    });
    const term = {
      textarea,
      onData: () => ({ dispose: vi.fn() }),
    };

    const disposable = attachLinuxIMEFix(term as never, vi.fn());
    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ce'shi" }));
    textarea.value = "ce'shi";
    const beforeInput = new InputEvent("beforeinput", {
      inputType: "insertCompositionText",
      data: "ce'shi",
      isComposing: true,
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(beforeInput);
    textarea.value = "ce'shi";
    const input = new InputEvent("input", {
      inputType: "insertCompositionText",
      data: "ce'shi",
      isComposing: true,
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(input);
    await Promise.resolve();

    expect(beforeInput.defaultPrevented).toBe(false);
    expect(input.defaultPrevented).toBe(false);
    expect(textarea.value).toBe("ce'shi");
    expect(compositionView.classList.contains("active")).toBe(true);
    expect(compositionView.textContent).toBe("ce'shi");
    expect(compositionView.style.getPropertyValue("--aeroric-composition-max-width")).toBe("40px");

    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "" }));
    await Promise.resolve();

    expect(compositionView.classList.contains("active")).toBe(false);
    expect(compositionView.textContent).toBe("");

    disposable.dispose();
    terminalElement.remove();
  });

  it("defers romanized composition commits that may be followed by committed Chinese text", () => {
    expect(shouldDeferRomanizedCompositionCommit("ceshi", "ceshi")).toBe(true);
    expect(shouldDeferRomanizedCompositionCommit("ce'shi", "ce'shi")).toBe(true);
    expect(shouldDeferRomanizedCompositionCommit("测试", "ceshi")).toBe(false);
    expect(shouldDeferRomanizedCompositionCommit("hello world", "ceshi")).toBe(false);
  });

  it("sends committed Chinese from WebKit beforeinput instead of stale pinyin from compositionend", async () => {
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const listeners: Array<{
      event: string;
      listener: EventListenerOrEventListenerObject;
      options?: boolean | AddEventListenerOptions;
    }> = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };
    const originalAddEventListener = textarea.addEventListener.bind(textarea);
    vi.spyOn(textarea, "addEventListener").mockImplementation((event, listener, options) => {
      listeners.push({ event, listener, options });
      originalAddEventListener(event, listener, options);
    });

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ceshi" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "ceshi" }));
    expect(sent).toEqual([]);

    const beforeInput = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "测试",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(beforeInput);

    expect(beforeInput.defaultPrevented).toBe(true);
    expect(sent).toEqual(["测试"]);
    expect(listeners.some((item) => item.event === "compositionend")).toBe(true);
  });

  it("commits Chinese text from insertText while composition is still active", async () => {
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ceshi" }));
    const beforeInput = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "测试",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(beforeInput);

    expect(beforeInput.defaultPrevented).toBe(true);
    expect(sent).toEqual(["测试"]);
  });

  it("does not send growing pinyin preedit text before Chinese composition commits", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "s" }));
    const firstPreedit = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "s",
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(firstPreedit, "isComposing", { value: true });
    textarea.dispatchEvent(firstPreedit);

    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "sd" }));
    const secondPreedit = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "sd",
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(secondPreedit, "isComposing", { value: true });
    textarea.dispatchEvent(secondPreedit);

    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "sda" }));
    const thirdPreedit = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "sda",
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(thirdPreedit, "isComposing", { value: true });
    textarea.dispatchEvent(thirdPreedit);

    const committedChinese = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "是的啊",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(committedChinese);
    vi.runOnlyPendingTimers();

    expect(firstPreedit.defaultPrevented).toBe(false);
    expect(secondPreedit.defaultPrevented).toBe(false);
    expect(thirdPreedit.defaultPrevented).toBe(false);
    expect(committedChinese.defaultPrevented).toBe(true);
    expect(sent).toEqual(["是的啊"]);
    vi.useRealTimers();
  });

  it("ignores split pinyin replay after committed Chinese text", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "s'd" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "是的" }));
    const first = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "s",
      bubbles: true,
      cancelable: true,
    });
    const second = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "'",
      bubbles: true,
      cancelable: true,
    });
    const third = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "d",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(first);
    textarea.dispatchEvent(second);
    textarea.dispatchEvent(third);
    vi.runOnlyPendingTimers();

    expect(sent).toEqual(["是的"]);
    expect(first.defaultPrevented).toBe(true);
    expect(second.defaultPrevented).toBe(true);
    expect(third.defaultPrevented).toBe(true);
    vi.useRealTimers();
  });

  it("ignores delayed full pinyin tail after committing Chinese text", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ni'hao" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "你好" }));
    vi.advanceTimersByTime(1800);
    const replay = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "ni'hao",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(replay);
    vi.runOnlyPendingTimers();

    expect(sent).toEqual(["你好"]);
    expect(replay.defaultPrevented).toBe(true);
    vi.useRealTimers();
  });

  it("clears stale pinyin from compositionend after committed Chinese beforeinput", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ni'hao" }));
    textarea.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertText",
        data: "你好",
        bubbles: true,
        cancelable: true,
      }),
    );
    vi.runOnlyPendingTimers();
    textarea.value = "ni'hao";
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "ni'hao" }));
    vi.runOnlyPendingTimers();

    expect(sent).toEqual(["你好"]);
    expect(textarea.value).toBe("");
    vi.useRealTimers();
  });

  it("keeps pinyin replay candidates after ignoring a committed Chinese replay", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "s'd" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "是的" }));
    textarea.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertText",
        data: "是的",
        bubbles: true,
        cancelable: true,
      }),
    );
    textarea.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertText",
        data: "s",
        bubbles: true,
        cancelable: true,
      }),
    );
    textarea.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertText",
        data: "'",
        bubbles: true,
        cancelable: true,
      }),
    );
    textarea.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertText",
        data: "d",
        bubbles: true,
        cancelable: true,
      }),
    );
    vi.runOnlyPendingTimers();

    expect(sent).toEqual(["是的"]);
    vi.useRealTimers();
  });

  it("normalizes xterm data that contains committed Chinese plus a pinyin tail", async () => {
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const dataListeners: Array<(data: string) => void> = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        dataListeners.push(listener);
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));
    expect(dataListeners).toHaveLength(1);
    dataListeners[0]("你好ni'hao");

    expect(sent).toEqual(["你好"]);
  });

  it("waits for delayed committed Chinese before sending stale romanized composition text", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const dataListeners: Array<(data: string) => void> = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        dataListeners.push(listener);
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "shi'de" }));
    textarea.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: " ",
        code: "Space",
        bubbles: true,
        cancelable: true,
      }),
    );
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "shi'de" }));
    vi.advanceTimersByTime(80);
    const beforeInput = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "是的shi'de",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(beforeInput);
    dataListeners[0]("是的shi'de");
    vi.runOnlyPendingTimers();

    expect(beforeInput.defaultPrevented).toBe(true);
    expect(sent).toEqual(["是的"]);
    vi.useRealTimers();
  });

  it("clears delayed WebKit textarea pinyin tail after committed Chinese input", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "shi'de" }));
    textarea.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: " ",
        code: "Space",
        bubbles: true,
        cancelable: true,
      }),
    );
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "shi'de" }));
    vi.advanceTimersByTime(80);
    textarea.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertText",
        data: "是的shi'de",
        bubbles: true,
        cancelable: true,
      }),
    );

    globalThis.setTimeout(() => {
      textarea.value = "shi'de";
      textarea.setSelectionRange(0, textarea.value.length);
    }, 40);
    vi.advanceTimersByTime(100);

    expect(sent).toEqual(["是的"]);
    expect(textarea.value).toBe("");
    expect(textarea.selectionStart).toBe(0);
    expect(textarea.selectionEnd).toBe(0);
    vi.useRealTimers();
  });

  it("temporarily resets the WebKit textarea input client after committed Chinese input", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const focus = vi.spyOn(textarea, "focus").mockImplementation(() => {});
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "shi'de" }));
    textarea.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertText",
        data: "是的shi'de",
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(sent).toEqual(["是的"]);
    expect(textarea.disabled).toBe(true);

    vi.advanceTimersByTime(40);

    expect(textarea.disabled).toBe(false);
    expect(focus).toHaveBeenCalledWith({ preventScroll: true });
    vi.useRealTimers();
  });

  it("hides xterm composition view when suppressing compositionend replay", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const terminalElement = document.createElement("div");
    terminalElement.className = "xterm";
    const textarea = document.createElement("textarea");
    const compositionView = document.createElement("div");
    compositionView.className = "composition-view active";
    compositionView.textContent = "shi'de";
    terminalElement.append(textarea, compositionView);
    document.body.appendChild(terminalElement);
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    const disposable = attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "shi'de" }));
    textarea.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: " ",
        code: "Space",
        bubbles: true,
        cancelable: true,
      }),
    );
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "shi'de" }));
    textarea.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertText",
        data: "是的shi'de",
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(sent).toEqual(["是的"]);
    expect(compositionView.classList.contains("active")).toBe(false);
    expect(compositionView.textContent).toBe("");

    disposable.dispose();
    terminalElement.remove();
    vi.useRealTimers();
  });

  it("clears the xterm composition view when preedit text is deleted to empty", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const terminalElement = document.createElement("div");
    terminalElement.className = "xterm";
    const textarea = document.createElement("textarea");
    const compositionView = document.createElement("div");
    compositionView.className = "composition-view active";
    terminalElement.append(textarea, compositionView);
    document.body.appendChild(terminalElement);
    const term = {
      textarea,
      onData: () => ({ dispose: vi.fn() }),
    };
    const disposable = attachLinuxIMEFix(term as never, vi.fn());

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    compositionView.textContent = "c";
    textarea.value = "c";
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "c" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "" }));
    compositionView.classList.add("active");
    compositionView.textContent = "c";
    textarea.value = "c";

    await Promise.resolve();
    vi.runOnlyPendingTimers();

    expect(compositionView.classList.contains("active")).toBe(false);
    expect(compositionView.textContent).toBe("");
    expect(textarea.value).toBe("");

    disposable.dispose();
    terminalElement.remove();
    vi.useRealTimers();
  });

  it("commits normalized romanized text when switching IME to English mid-composition", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "s'd" }));
    const beforeInput = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "s'd",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(beforeInput);
    vi.runOnlyPendingTimers();

    expect(beforeInput.defaultPrevented).toBe(true);
    expect(sent).toEqual(["sd"]);
    vi.useRealTimers();
  });

  it("promptly commits romanized text when switching IME to English", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ye's" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "ye's" }));

    expect(sent).toEqual(["yes"]);
    vi.useRealTimers();
  });

  it("commits normalized pinyin immediately when switching IME to English", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ce'shi" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "ce'shi" }));

    expect(sent).toEqual(["ceshi"]);
    vi.useRealTimers();
  });

  it("promptly commits separator-less pinyin when switching IME without a candidate key", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ceshi" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "ceshi" }));

    // 切换瞬间尚未 flush（0ms 定时器在下一个宏任务）
    expect(sent).toEqual([]);
    vi.runOnlyPendingTimers();
    expect(sent).toEqual(["ceshi"]);
    vi.useRealTimers();
  });

  it("suppresses the first English space and flushes a pending non-candidate romanized commit", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const downstreamKeydown = vi.fn();
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));
    textarea.addEventListener("keydown", downstreamKeydown);

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ceshi" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "ceshi" }));

    // 0ms 定时器尚未触发时按下空格：应立即 flush ceshi 并抑制空格
    const space = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(space, "keyCode", { value: 32 });
    textarea.dispatchEvent(space);

    expect(space.defaultPrevented).toBe(true);
    expect(downstreamKeydown).not.toHaveBeenCalled();
    expect(sent).toEqual(["ceshi"]);
    vi.runOnlyPendingTimers();
    expect(sent).toEqual(["ceshi"]);
    vi.useRealTimers();
  });

  it("commits romanized text left in textarea when IME switch ends composition without event data", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.value = "ce'shi";
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "" }));

    expect(sent).toEqual(["ceshi"]);
    expect(textarea.value).toBe("");
    vi.useRealTimers();
  });

  it("lets the next English keystroke pass after immediate romanized IME switch", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ye's" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "ye's" }));
    const nextInput = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "a",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(nextInput);

    expect(nextInput.defaultPrevented).toBe(false);
    expect(sent).toEqual(["yes"]);
    vi.useRealTimers();
  });

  it("commits stale romanized composition and suppresses the first English space after IME switch", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const downstreamKeydown = vi.fn();
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));
    textarea.addEventListener("keydown", downstreamKeydown);

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ce'shi" }));

    const space = new KeyboardEvent("keydown", {
      key: " ",
      code: "Space",
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(space, "keyCode", { value: 32 });
    textarea.dispatchEvent(space);

    expect(space.defaultPrevented).toBe(true);
    expect(downstreamKeydown).not.toHaveBeenCalled();
    expect(sent).toEqual(["ceshi"]);
    vi.useRealTimers();
  });

  it("commits romanized text from a non-composing input event during IME switch", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ce'shi" }));
    textarea.value = "ce'shi";
    textarea.dispatchEvent(
      new InputEvent("input", {
        inputType: "insertText",
        data: "ce'shi",
        bubbles: true,
      }),
    );

    expect(sent).toEqual(["ceshi"]);
    expect(textarea.value).toBe("");
    vi.useRealTimers();
  });

  it("recovers romanized composition when IME switches without compositionend", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "shi'de" }));
    const nextInput = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "a",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(nextInput);

    expect(nextInput.defaultPrevented).toBe(true);
    expect(sent).toEqual(["shide", "a"]);
    vi.useRealTimers();
  });

  it("commits romanized composition on blur during IME switch", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "shi'de" }));
    textarea.dispatchEvent(new FocusEvent("blur"));

    expect(sent).toEqual(["shide"]);
    vi.useRealTimers();
  });

  it("commits romanized composition on window blur when clicking input source switcher", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    const disposable = attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "ce'shi" }));
    window.dispatchEvent(new FocusEvent("blur"));

    expect(sent).toEqual(["ceshi"]);
    disposable.dispose();
    vi.useRealTimers();
  });

  it("lets xterm observe compositionend so its internal IME state is released", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));
    let xtermCompositionEnded = false;
    textarea.addEventListener("compositionend", () => {
      xtermCompositionEnded = true;
    });

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "shi'de" }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "shi'de" }));

    expect(xtermCompositionEnded).toBe(true);
    vi.runOnlyPendingTimers();
    expect(sent).toEqual(["shide"]);
    vi.useRealTimers();
  });

  it("synthesizes compositionend for xterm when IME switch blurs without native compositionend", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: (listener: (data: string) => void) => {
        void listener;
        return { dispose: vi.fn() };
      },
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));
    let xtermCompositionEnded = false;
    textarea.addEventListener("compositionend", () => {
      xtermCompositionEnded = true;
    });

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "shi'de" }));
    textarea.dispatchEvent(new FocusEvent("blur"));

    expect(sent).toEqual(["shide"]);
    expect(xtermCompositionEnded).toBe(true);
    vi.useRealTimers();
  });

  it("ignores replayed pinyin fragments after a Chinese commit", async () => {
    const candidates = buildPostCompositionIgnoredCandidates("是的", "s'd");

    expect(shouldIgnorePostCompositionCandidate("s", candidates)).toBe(false);
    expect(shouldIgnorePostCompositionCandidate("s'", candidates)).toBe(false);
    expect(shouldIgnorePostCompositionCandidate("d", candidates)).toBe(false);
    expect(shouldIgnorePostCompositionCandidate("s'd", candidates)).toBe(true);
  });

  it("normalizes committed pinyin text inside the new-task editor", () => {
    const editor = document.createElement("div");
    editor.textContent = "shuo'huashuohua";

    expect(normalizeEditorCompositionText(editor)).toBe(true);
    expect(editor.textContent).toBe("shuohua");
  });

  it("keeps terminal textarea compatible with Chinese IME composition", () => {
    const textarea = document.createElement("textarea");

    applyTerminalTextareaInputAttributes({ textarea });

    expect(textarea.getAttribute("autocomplete")).toBe("off");
    expect(textarea.getAttribute("autocorrect")).toBe("off");
    expect(textarea.getAttribute("autocapitalize")).toBe("off");
    expect(textarea.getAttribute("spellcheck")).toBe("false");
    expect(textarea.getAttribute("inputmode")).not.toBe("none");
  });

  it("suppresses repeated printable keydown events while preserving navigation repeats", () => {
    expect(
      shouldSuppressPrintableKeyRepeat(new KeyboardEvent("keydown", { key: "s", repeat: true })),
    ).toBe(true);
    expect(
      shouldSuppressPrintableKeyRepeat(
        new KeyboardEvent("keydown", { key: "ArrowLeft", repeat: true }),
      ),
    ).toBe(false);
    expect(
      shouldSuppressPrintableKeyRepeat(new KeyboardEvent("keydown", { key: "s", repeat: false })),
    ).toBe(false);
  });

  it("suppresses repeated IME process keydown events from Chinese input", () => {
    const event = new KeyboardEvent("keydown", { key: "Process", repeat: true });
    Object.defineProperty(event, "keyCode", { value: 229 });

    expect(shouldSuppressPrintableKeyRepeat(event)).toBe(true);
  });

  it("prevents repeated printable keydown events before xterm receives them", async () => {
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const downstream = vi.fn();
    const term = {
      textarea,
      onData: () => ({ dispose: vi.fn() }),
    };

    attachLinuxIMEFix(term as never, vi.fn());
    textarea.addEventListener("keydown", downstream);

    const event = new KeyboardEvent("keydown", {
      key: "s",
      repeat: true,
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(downstream).not.toHaveBeenCalled();
  });

  it("suppresses text insertion that follows a repeated IME keydown", async () => {
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: () => ({ dispose: vi.fn() }),
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    const repeatKeydown = new KeyboardEvent("keydown", {
      key: "Process",
      repeat: true,
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(repeatKeydown, "keyCode", { value: 229 });
    textarea.dispatchEvent(repeatKeydown);

    const repeatedInput = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "w",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(repeatedInput);

    expect(repeatKeydown.defaultPrevented).toBe(true);
    expect(repeatedInput.defaultPrevented).toBe(true);
    expect(sent).toEqual([]);
  });

  it("suppresses repeated IME insertText while composition is still active", async () => {
    vi.resetModules();
    vi.doMock("../platform", () => ({
      APP_PLATFORM: "macos",
      ENABLE_USAGE_INSIGHTS: true,
      IS_MAC_WEBKIT: true,
      IS_OTHER_WEBKIT: false,
      detectAppPlatform: () => "macos",
      isAppleWebKit: () => true,
    }));
    const { attachLinuxIMEFix } = await import("../components/terminalInputFix");
    const textarea = document.createElement("textarea");
    const sent: string[] = [];
    const term = {
      textarea,
      onData: () => ({ dispose: vi.fn() }),
    };

    attachLinuxIMEFix(term as never, (data) => sent.push(data));

    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "w" }));

    const repeatKeydown = new KeyboardEvent("keydown", {
      key: "Process",
      repeat: true,
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(repeatKeydown, "keyCode", { value: 229 });
    textarea.dispatchEvent(repeatKeydown);

    const repeatedInput = new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "w",
      bubbles: true,
      cancelable: true,
    });
    textarea.dispatchEvent(repeatedInput);

    expect(repeatKeydown.defaultPrevented).toBe(true);
    expect(repeatedInput.defaultPrevented).toBe(true);
    expect(sent).toEqual([]);
  });
});
