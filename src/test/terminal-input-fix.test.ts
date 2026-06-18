import { afterEach, describe, expect, it, vi } from "vitest";
import {
  buildPostCompositionIgnoredCandidates,
  normalizeCommittedCompositionText,
  POST_COMPOSITION_REPLAY_IGNORE_MS,
  applyTerminalTextareaInputAttributes,
  shouldIgnorePostCompositionCandidate,
  shouldIgnorePostCompositionInsert,
  shouldDeferRomanizedCompositionCommit,
  shouldSuppressBrowserCompositionPreview,
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

  it("suppresses WebKit live IME composition preview in terminal textarea", () => {
    expect(shouldSuppressBrowserCompositionPreview("insertCompositionText", true)).toBe(true);
    expect(shouldSuppressBrowserCompositionPreview("insertText", true)).toBe(false);
    expect(shouldSuppressBrowserCompositionPreview("insertCompositionText", false)).toBe(false);
    expect(shouldSuppressBrowserCompositionPreview("insertText", false)).toBe(false);
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
    const listeners: Array<{ event: string; listener: EventListenerOrEventListenerObject; options?: boolean | AddEventListenerOptions }> = [];
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
    textarea.dispatchEvent(new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "你好",
      bubbles: true,
      cancelable: true,
    }));
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
    textarea.dispatchEvent(new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "是的",
      bubbles: true,
      cancelable: true,
    }));
    textarea.dispatchEvent(new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "s",
      bubbles: true,
      cancelable: true,
    }));
    textarea.dispatchEvent(new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "'",
      bubbles: true,
      cancelable: true,
    }));
    textarea.dispatchEvent(new InputEvent("beforeinput", {
      inputType: "insertText",
      data: "d",
      bubbles: true,
      cancelable: true,
    }));
    vi.runOnlyPendingTimers();

    expect(sent).toEqual(["是的"]);
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
});
