import { describe, expect, it } from "vitest";
import { renderSessionMarkdown } from "../components/SessionView";
import { renderMarkdownWithToc } from "../components/file-viewer/markdownPreview";

describe("renderSessionMarkdown", () => {
  it("sanitizes HTML generated from session markdown", () => {
    const html = renderSessionMarkdown(
      `<img src=x onerror="window.__xss = true"><script>window.__xss = true</script>`,
    );

    expect(html).toContain("<img");
    expect(html).not.toContain("onerror");
    expect(html).not.toContain("<script");
  });

  it("sanitizes the file markdown preview before it reaches innerHTML", () => {
    const { html } = renderMarkdownWithToc(
      `<img src=x onerror="window.__xss = true"><script>alert(1)</script>\n\n# Safe heading`,
    );

    expect(html).not.toMatch(/onerror|<script/i);
    expect(html).toContain("Safe heading");
  });
});
