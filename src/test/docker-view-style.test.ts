import { describe, expect, it } from "vitest";
import { dockerActionButtonStyle } from "../components/docker/DockerServiceView";

describe("DockerServiceView action button styles", () => {
  it("uses transparent backgrounds for container action buttons", () => {
    for (const tone of ["start", "restart", "stop", "logs"] as const) {
      const style = dockerActionButtonStyle(tone);

      expect(style.background).toBe("transparent");
      expect(String(style.border)).toContain("color-mix");
      expect(style.color).toBeTruthy();
    }
  });

  it("uses a lighter green for the start button", () => {
    expect(dockerActionButtonStyle("start").color).toBe("#4ade80");
  });
});
