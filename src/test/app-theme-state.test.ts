import { describe, expect, it } from "vitest";
import {
  nativeThemeForVariant,
  nativeWindowBackgroundForVariant,
  resolveThemeVariant,
} from "../appThemeState";

describe("native window chrome theme", () => {
  it("resolves system dark mode before syncing native chrome", () => {
    const variant = resolveThemeVariant("system", true);

    expect(variant).toBe("dark");
    expect(nativeThemeForVariant(variant)).toBe("dark");
    expect(nativeWindowBackgroundForVariant(variant)).toBe("#050607");
  });

  it("keeps explicit dark mode chrome dark", () => {
    expect(nativeThemeForVariant("dark")).toBe("dark");
    expect(nativeWindowBackgroundForVariant("dark")).toBe("#050607");
  });

  it("keeps eye-care chrome in the light family with a matching background", () => {
    expect(nativeThemeForVariant("eyecare")).toBe("light");
    expect(nativeWindowBackgroundForVariant("eyecare")).toBe("#f6eddc");
  });

  it("uses the light surface for light mode", () => {
    expect(nativeThemeForVariant("light")).toBe("light");
    expect(nativeWindowBackgroundForVariant("light")).toBe("#fbfbfc");
  });
});
