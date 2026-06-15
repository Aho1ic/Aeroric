import { describe, expect, it } from "vitest";
import { renderSessionMarkdown } from "../components/SessionView";

describe("renderSessionMarkdown", () => {
  it("sanitizes HTML generated from session markdown", () => {
    const html = renderSessionMarkdown(
      `<img src=x onerror="window.__xss = true"><script>window.__xss = true</script>`,
    );

    expect(html).toContain("<img");
    expect(html).not.toContain("onerror");
    expect(html).not.toContain("<script");
  });
});
