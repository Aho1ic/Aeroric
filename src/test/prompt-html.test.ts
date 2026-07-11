import { describe, expect, it } from "vitest";
import { sanitizePromptHtml } from "../components/new-task/promptHtml";

describe("sanitizePromptHtml", () => {
  it("removes executable and unrelated persisted markup", () => {
    const clean = sanitizePromptHtml(
      '<img src=x onerror="alert(1)"><script>alert(2)</script><a href="javascript:alert(3)">x</a>',
    );

    expect(clean).not.toMatch(/img|script|onerror|javascript|href/i);
    expect(clean).toContain("x");
  });

  it("retains file chip metadata and safe display markup", () => {
    const clean = sanitizePromptHtml(
      '<span contenteditable="false" data-file-path="src/App.tsx" data-file-ext="tsx" style="color:var(--accent)"><span>App.tsx</span></span><br>',
    );

    expect(clean).toContain('data-file-path="src/App.tsx"');
    expect(clean).toContain('contenteditable="false"');
    expect(clean).toContain("<br>");
  });
});
