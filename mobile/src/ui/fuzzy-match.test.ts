import { describe, expect, it } from "vitest";
import { fuzzyMatch } from "./fuzzy-match";

describe("fuzzyMatch", () => {
  it("空 query 命中一切", () => {
    expect(fuzzyMatch("OpenAI Router", "")).toBe(true);
    expect(fuzzyMatch("OpenAI Router", "   ")).toBe(true);
  });

  it("完整子串命中", () => {
    expect(fuzzyMatch("OpenAI Router", "Router")).toBe(true);
  });

  it("跨字符子序列命中", () => {
    expect(fuzzyMatch("OpenAI Router", "oai")).toBe(true);
    expect(fuzzyMatch("Claude Code Pro", "ccp")).toBe(true);
  });

  it("大小写无关", () => {
    expect(fuzzyMatch("OpenAI Router", "OPENAI")).toBe(true);
    expect(fuzzyMatch("openai router", "OaR")).toBe(true);
  });

  it("query 中的空格被忽略", () => {
    expect(fuzzyMatch("OpenAI Router", "open router")).toBe(true);
    expect(fuzzyMatch("OpenAIRouter", "open router")).toBe(true);
  });

  it("乱序不命中", () => {
    expect(fuzzyMatch("OpenAI Router", "iao")).toBe(false);
    expect(fuzzyMatch("OpenAI Router", "routeropen")).toBe(false);
  });

  it("缺失字符不命中", () => {
    expect(fuzzyMatch("OpenAI Router", "oaix")).toBe(false);
  });

  it("支持中文名称", () => {
    expect(fuzzyMatch("公司内网中转", "内网")).toBe(true);
    expect(fuzzyMatch("公司内网中转", "公转")).toBe(true);
    expect(fuzzyMatch("公司内网中转", "转公")).toBe(false);
  });
});
