import { describe, expect, it } from "vitest";
import {
  nativeThemeForVariant,
  nativeWindowBackgroundForVariant,
  resolveThemeVariant,
} from "../appThemeState";

describe("native application theme", () => {
  it("resolves system dark mode before syncing native chrome", () => {
    const variant = resolveThemeVariant("system", true);

    expect(variant).toBe("dark");
    expect(nativeThemeForVariant(variant)).toBe("dark");
    expect(nativeWindowBackgroundForVariant(variant)).toBe("#09090b");
  });

  it("keeps eye-care native chrome in the light family with a matching background", () => {
    expect(nativeThemeForVariant("eyecare")).toBe("light");
    expect(nativeWindowBackgroundForVariant("eyecare")).toBe("#f6eddc");
  });
});
