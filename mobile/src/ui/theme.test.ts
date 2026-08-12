import { describe, expect, it } from "vitest";
import { theme } from "./theme";

describe("mobile glass theme", () => {
  it("uses a solid atmospheric canvas with translucent shared surfaces", () => {
    expect(theme.canvas).toMatch(/^#/);
    expect(theme.bg).toContain("rgba(");
    expect(theme.bgCard).toContain("rgba(");
    expect(theme.bgElevated).toContain("rgba(");
    expect(theme.border).toContain("rgba(");
  });
});
