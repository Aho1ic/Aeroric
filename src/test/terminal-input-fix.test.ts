import { describe, expect, it } from "vitest";
import {
  normalizeCommittedCompositionText,
  shouldLetBrowserRenderCompositionPreview,
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

  it("lets WebKit render live IME composition text before commit", () => {
    expect(shouldLetBrowserRenderCompositionPreview("insertCompositionText", true)).toBe(true);
    expect(shouldLetBrowserRenderCompositionPreview("insertText", true)).toBe(false);
    expect(shouldLetBrowserRenderCompositionPreview("insertCompositionText", false)).toBe(false);
  });

  it("normalizes committed pinyin text inside the new-task editor", () => {
    const editor = document.createElement("div");
    editor.textContent = "shuo'huashuohua";

    expect(normalizeEditorCompositionText(editor)).toBe(true);
    expect(editor.textContent).toBe("shuohua");
  });
});
