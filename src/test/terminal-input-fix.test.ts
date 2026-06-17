import { describe, expect, it } from "vitest";
import {
  buildPostCompositionIgnoredCandidates,
  normalizeCommittedCompositionText,
  POST_COMPOSITION_REPLAY_IGNORE_MS,
  shouldIgnorePostCompositionCandidate,
  shouldIgnorePostCompositionInsert,
  shouldSuppressBrowserCompositionPreview,
} from "../components/terminalInputFix";
import { normalizeEditorCompositionText } from "../components/new-task/PromptEditor";

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
    expect(POST_COMPOSITION_REPLAY_IGNORE_MS).toBeGreaterThanOrEqual(1000);
  });

  it("ignores pinyin replay when switching IME to English mid-composition", () => {
    const candidates = buildPostCompositionIgnoredCandidates("shuo'hua", "shuo'hua");

    expect(shouldIgnorePostCompositionCandidate("shuohua", candidates)).toBe(true);
    expect(shouldIgnorePostCompositionCandidate("shuo'huashuohua", candidates)).toBe(true);
    expect(shouldIgnorePostCompositionCandidate("shuo", candidates)).toBe(false);
  });

  it("suppresses WebKit live IME composition preview in terminal textarea", () => {
    expect(shouldSuppressBrowserCompositionPreview("insertCompositionText", true)).toBe(true);
    expect(shouldSuppressBrowserCompositionPreview("insertText", true)).toBe(true);
    expect(shouldSuppressBrowserCompositionPreview("insertCompositionText", false)).toBe(false);
    expect(shouldSuppressBrowserCompositionPreview("insertText", false)).toBe(false);
  });

  it("normalizes committed pinyin text inside the new-task editor", () => {
    const editor = document.createElement("div");
    editor.textContent = "shuo'huashuohua";

    expect(normalizeEditorCompositionText(editor)).toBe(true);
    expect(editor.textContent).toBe("shuohua");
  });
});
