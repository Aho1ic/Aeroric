import { describe, expect, it } from "vitest";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";

describe("translation catalogs", () => {
  it("keep the English and Chinese key sets aligned", () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(zh).sort());
  });
});
