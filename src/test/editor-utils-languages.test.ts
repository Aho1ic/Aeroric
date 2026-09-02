import { describe, expect, it } from "vitest";
import { loadLanguageExtension } from "../components/file-viewer/editorUtils";

/**
 * `loadLanguageExtension` 的分派表。
 *
 * 断言的是「有没有解析到一个语言扩展」,不去断言具体是哪个 parser ——
 * 那属于 CodeMirror 自己的实现细节,钉住它会让升级依赖时无谓地挂。
 * 未知后缀必须回 `[]`(空扩展),这一点要断言死:回 undefined 会让
 * FileViewer 的 extensions 数组里出现空洞。
 */

/** 已识别 = 返回了非空扩展;未识别 = 返回空数组。 */
async function resolves(fileName: string): Promise<boolean> {
  const ext = await loadLanguageExtension(fileName);
  if (Array.isArray(ext)) return ext.length > 0;
  return ext !== null && ext !== undefined;
}

describe("按整个文件名匹配的(nameMap)", () => {
  it.each([
    "Dockerfile",
    "dockerfile",
    "DOCKERFILE",
    "Dockerfile.dev",
    "Dockerfile.prod",
    "Makefile",
    "GNUmakefile",
    "Justfile",
    "Gemfile",
    "Rakefile",
    "Vagrantfile",
    "Procfile",
    "CMakeLists.txt",
    ".gitignore",
    ".dockerignore",
    ".env",
    ".env.local",
    ".env.example",
    ".npmrc",
    ".yarnrc",
    "CHANGELOG.md",
    "README",
  ])("%s 能识别", async (name) => {
    expect(await resolves(name)).toBe(true);
  });

  it("nameMap 按整个文件名匹配,所以调用方必须传 basename", async () => {
    // `nameMap["/app/dockerfile"]` 不存在,于是落到后缀分支:
    // "/app/Dockerfile".split(".").pop() === 整串,不在 switch 里 → []。
    // FileViewer 传的是 fileName / activeTab.name,符合要求;别改成传全路径。
    expect(await resolves("Dockerfile")).toBe(true);
    expect(await resolves("/app/Dockerfile")).toBe(false);
  });

  it(".env.production 不在表里(现状,不是本次要补的)", async () => {
    // .env.local / .env.example 都在,production 漏了。
    expect(await resolves(".env.local")).toBe(true);
    expect(await resolves(".env.production")).toBe(false);
  });
});

describe("按后缀匹配的", () => {
  it.each([
    "a.ts",
    "a.tsx",
    "a.js",
    "a.mjs",
    "a.cjs",
    "a.jsx",
    "a.json",
    "a.jsonc",
    "a.rs",
    "a.html",
    "a.htm",
    "a.css",
    "a.scss",
    "a.sass",
    "a.md",
    "a.mdx",
    "a.yaml",
    "a.yml",
    "a.toml",
    "a.sh",
    "a.bash",
    "a.zsh",
    "a.fish",
    "a.py",
    "a.go",
    "a.java",
    "a.c",
    "a.h",
    "a.cpp",
    "a.cc",
    "a.hpp",
    "a.sql",
    "a.xml",
    "a.swift",
    "a.kt",
    "a.rb",
    "a.lua",
    "a.r",
    "a.proto",
  ])("%s 能识别", async (name) => {
    expect(await resolves(name)).toBe(true);
  });

  it("后缀大小写无关", async () => {
    expect(await resolves("A.TS")).toBe(true);
    expect(await resolves("A.PY")).toBe(true);
  });

  it("带路径时按最后一段的后缀判", async () => {
    expect(await resolves("/deep/nested/path/main.rs")).toBe(true);
  });

  it.each(["a.unknownext", "a.docx", "a.bin", "noextension", "", "a."])(
    "%o 未识别时回空数组",
    async (name) => {
      const ext = await loadLanguageExtension(name);
      expect(Array.isArray(ext)).toBe(true);
      expect(ext).toEqual([]);
    },
  );

  it(".markdown 拿不到高亮(与 isMarkdownFile 不一致)", async () => {
    // 现状记录。isMarkdownFile("a.markdown") 是 true,但 switch 只有 md/mdx。
    // 详见 HANDOFF.md §4 第 1 条。
    expect(await resolves("a.md")).toBe(true);
    expect(await resolves("a.markdown")).toBe(false);
  });
});
