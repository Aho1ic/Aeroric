import { describe, expect, it } from "vitest";
import {
  isMarkdownFile,
  isPreviewableImageFile,
  isSqliteDatabaseFile,
} from "../components/file-viewer/editorUtils";
import {
  fileExtension,
  isSqliteDatabaseFile as isSqliteDatabaseEntry,
  isSqliteDatabaseFileName,
} from "../components/file-explorer/fileEntryUtils";
import { entryIconOf } from "../lib/fileIcons";

/**
 * 后缀判定的现状固化测试。
 *
 * 写在合并之前:`file-viewer/editorUtils` 与 `file-explorer/fileEntryUtils` 各自
 * 维护了一份后缀表(sqlite 三个、图片七个完全重复)。这里先把两边**当前**的行为
 * 逐条钉住,之后把表抽到公共模块时,这个文件就是"没改行为"的凭据。
 *
 * 包括那些看起来不像特意设计的行为(例如名字就叫 `png` 的无后缀文件被当成图片):
 * 它们是现状,合并不该顺手改掉。
 */

const IMAGE_EXTS = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];
const SQLITE_EXTS = ["db", "sqlite", "sqlite3"];
const MARKDOWN_EXTS = ["md", "mdx", "markdown"];

describe("fileExtension", () => {
  it("没给 ext 时从名字里取最后一段", () => {
    expect(fileExtension("a/b/c.TS")).toBe("ts");
    expect(fileExtension("archive.tar.gz")).toBe("gz");
  });

  it("给了 ext 就用 ext,不看名字", () => {
    expect(fileExtension("weird.name", "TSX")).toBe("tsx");
  });

  it("空字符串的 ext 会盖掉名字(?? 只挡 null/undefined)", () => {
    // 后端把 extension 报成 "" 的目录/无后缀文件走的就是这条路。
    expect(fileExtension("Makefile", "")).toBe("");
  });

  it("ext 为 null 时回落到名字", () => {
    expect(fileExtension("script.SH", null)).toBe("sh");
  });

  it("没有点的名字整体当后缀", () => {
    expect(fileExtension("Dockerfile")).toBe("dockerfile");
  });
});

describe("editorUtils 的三个后缀判定", () => {
  it("markdown 认 md / mdx / markdown,大小写无关", () => {
    for (const ext of MARKDOWN_EXTS) {
      expect(isMarkdownFile(`note.${ext}`), ext).toBe(true);
      expect(isMarkdownFile(`note.${ext.toUpperCase()}`), ext).toBe(true);
    }
    expect(isMarkdownFile("note.txt")).toBe(false);
    expect(isMarkdownFile("note")).toBe(false);
  });

  it("图片认这七个", () => {
    for (const ext of IMAGE_EXTS) {
      expect(isPreviewableImageFile(`shot.${ext}`), ext).toBe(true);
      expect(isPreviewableImageFile(`shot.${ext.toUpperCase()}`), ext).toBe(true);
    }
    expect(isPreviewableImageFile("shot.tiff")).toBe(false);
    expect(isPreviewableImageFile("shot.ico")).toBe(false);
  });

  it("sqlite 认这三个", () => {
    for (const ext of SQLITE_EXTS) {
      expect(isSqliteDatabaseFile(`app.${ext}`), ext).toBe(true);
      expect(isSqliteDatabaseFile(`app.${ext.toUpperCase()}`), ext).toBe(true);
    }
    expect(isSqliteDatabaseFile("app.db3")).toBe(false);
    expect(isSqliteDatabaseFile("app.sql")).toBe(false);
  });

  it("名字整体等于后缀的无后缀文件也会命中(现状,不是设计)", () => {
    // `"png".split(".").pop()` 就是 "png"。合并后缀表时别顺手"修"掉这个。
    expect(isPreviewableImageFile("png")).toBe(true);
    expect(isMarkdownFile("markdown")).toBe(true);
    expect(isSqliteDatabaseFile("db")).toBe(true);
  });

  it("带路径也按最后一段判", () => {
    expect(isSqliteDatabaseFile("/var/data/app.sqlite3")).toBe(true);
    expect(isPreviewableImageFile("C:\\pics\\a.PNG")).toBe(true);
  });
});

describe("两处 sqlite 判定等价", () => {
  // 一份在 file-viewer,一份在 file-explorer。抽公共表之前先证明它们一致,
  // 否则合并就是在悄悄改其中一侧的行为。
  const corpus = [
    ...SQLITE_EXTS.flatMap((e) => [`a.${e}`, `a.${e.toUpperCase()}`, e]),
    "a.db3",
    "a.sql",
    "a.sqlite.bak",
    "backup.sqlite3",
    "Makefile",
    "",
    "a.",
    ".db",
    "/x/y/z.db",
    "no-dot-here",
  ];

  it.each(corpus)("%o 两侧结论相同", (name) => {
    expect(isSqliteDatabaseFileName(name)).toBe(isSqliteDatabaseFile(name));
  });

  it("entry 版对目录一律返回 false(名字像 db 也不算)", () => {
    expect(isSqliteDatabaseEntry({ name: "data.db", extension: undefined, is_dir: true })).toBe(
      false,
    );
    expect(isSqliteDatabaseEntry({ name: "data.db", extension: undefined, is_dir: false })).toBe(
      true,
    );
  });

  it("entry 版优先用后端给的 extension", () => {
    expect(isSqliteDatabaseEntry({ name: "opaque", extension: "sqlite", is_dir: false })).toBe(
      true,
    );
    expect(isSqliteDatabaseEntry({ name: "real.db", extension: "txt", is_dir: false })).toBe(false);
  });
});

describe("entryIconOf", () => {
  const entry = (name: string, extension?: string, is_dir = false) => ({
    name,
    extension,
    is_dir,
  });
  const kindOf = (name: string, extension?: string, is_dir = false) =>
    entryIconOf(entry(name, extension, is_dir)).kind;

  it("目录优先于任何后缀:名字带 .png 的目录仍是目录", () => {
    expect(kindOf("assets.png", undefined, true)).toBe("folder");
    expect(kindOf("assets", undefined, true)).toBe("folder-assets");
  });

  it.each([
    ["app.db", "database"],
    ["app.sqlite", "database"],
    ["app.sqlite3", "database"],
    ["m.pt", "model"],
    ["m.pth", "model"],
    ["m.onnx", "model"],
    ["v.mp4", "video"],
    ["v.mov", "video"],
    ["v.mkv", "video"],
    ["v.avi", "video"],
    ["v.webm", "video"],
    ["p.whl", "package"],
    ["i.png", "image"],
    ["i.jpg", "image"],
    ["i.jpeg", "image"],
    ["i.gif", "image"],
    ["i.webp", "image"],
    ["i.bmp", "image"],
    ["i.svg", "vector"],
    ["d.md", "markdown"],
    ["d.mdx", "markdown"],
    ["d.json", "data"],
    ["d.jsonc", "data"],
    ["a.zip", "archive"],
    ["a.tar", "archive"],
    ["a.gz", "archive"],
    ["a.tgz", "archive"],
    ["a.bz2", "archive"],
    ["a.xz", "archive"],
    ["a.7z", "archive"],
    ["a.rar", "archive"],
    ["s.ts", "code"],
    ["s.tsx", "code"],
    ["s.js", "code"],
    ["s.jsx", "code"],
    ["s.py", "code"],
    ["s.rs", "code"],
    ["s.go", "code"],
    ["s.css", "style"],
    ["s.scss", "style"],
    ["s.html", "markup"],
    ["s.htm", "markup"],
    ["s.yaml", "data"],
    ["s.yml", "data"],
    ["s.toml", "data"],
    ["s.sh", "shell"],
    ["s.sql", "database"],
    ["s.java", "code"],
    ["s.c", "code"],
    ["s.cpp", "code"],
    ["s.h", "code"],
    ["s.hpp", "code"],
    ["r.txt", "text"],
    ["r.log", "text"],
    ["r.ini", "config"],
    ["r.conf", "config"],
    ["mystery.xyz", "file"],
  ] as const)("%s → %s", (name, kind) => {
    expect(kindOf(name)).toBe(kind);
  });

  it("图标表与 isPreviewableImageFile 用的是同一组图片后缀", () => {
    // svg 在图标表里是 vector(矢量,画的是调色板而不是照片框),仍然可预览。
    for (const ext of IMAGE_EXTS) {
      expect(["image", "vector"], ext).toContain(kindOf(`x.${ext}`));
      expect(isPreviewableImageFile(`x.${ext}`), ext).toBe(true);
    }
  });

  it("图标表与 sqlite 判定用的是同一组后缀", () => {
    for (const ext of SQLITE_EXTS) {
      expect(kindOf(`x.${ext}`), ext).toBe("database");
      expect(isSqliteDatabaseFileName(`x.${ext}`), ext).toBe(true);
    }
  });

  it("`.markdown` 现在拿不到 markdown 图标(与 isMarkdownFile 不一致)", () => {
    // 现状记录:isMarkdownFile("a.markdown") 为 true,但图标表只有 md/mdx/rst/adoc。
    expect(isMarkdownFile("a.markdown")).toBe(true);
    expect(kindOf("a.markdown")).toBe("file");
  });

  // ── 兜底与优先级(NZ-1 的核心语义) ────────────────────────────────────────

  it("符号链接优先于目录与后缀", () => {
    // is_dir 走的是跟随链接后的类型,所以指向目录的链接两个标记同时为真;
    // 先判目录就再也画不出"这是链接"。
    expect(entryIconOf({ name: "src", is_dir: true, is_symlink: true }).kind).toBe("symlink");
    expect(entryIconOf({ name: "main.rs", is_dir: false, is_symlink: true }).kind).toBe("symlink");
    expect(entryIconOf({ name: "main.rs", is_dir: false, is_symlink: false }).kind).toBe("code");
  });

  it("精确文件名优先于后缀,且大小写不敏感", () => {
    // vite.config.ts:精确名(build)胜过后缀 ts(code)。
    expect(kindOf("vite.config.ts")).toBe("build");
    expect(kindOf("package.json")).toBe("manifest");
    expect(kindOf("Dockerfile")).toBe("docker");
    expect(kindOf("DOCKERFILE")).toBe("docker");
    expect(kindOf("CMakeLists.txt")).toBe("build");
    expect(kindOf("LICENSE")).toBe("license");
  });

  it("名字前缀族在精确名之后、后缀之前命中", () => {
    // .env.production 的后缀是 production(表里没有),靠前缀族才落到 config。
    expect(kindOf(".env")).toBe("config");
    expect(kindOf(".env.production")).toBe("config");
    // Dockerfile.prod 的后缀是 prod(表里没有)。
    expect(kindOf("Dockerfile.prod")).toBe("docker");
    expect(kindOf(".gitignore")).toBe("git");
  });

  it("多段名字按最后一段后缀命中", () => {
    expect(kindOf("types.d.ts")).toBe("code");
    expect(kindOf("bundle.tar.gz")).toBe("archive");
    expect(kindOf("bundle.tar.zst")).toBe("archive");
  });

  it("后端的 extension 优先于名字里推出的后缀", () => {
    // 名字里没有点:靠 extension 认出 rust。
    expect(kindOf("weird", "rs")).toBe("code");
    // ext 覆盖名字推出的后缀 —— 与 fileExtension / isSqliteDatabaseFile 同一约定:
    // 后端报的类型比名字权威(它对 `opaque` 报 `sqlite` 时必须认)。
    expect(kindOf("opaque", "sqlite")).toBe("database");
    expect(kindOf("foo.ts", "rs")).toBe("code");
  });

  it("未知后缀 / 未知目录 / 名叫 constructor 的文件都落兜底,不命中 Object.prototype", () => {
    expect(kindOf("mystery.qqq")).toBe("file");
    expect(kindOf("NOTICE.unknownext")).toBe("file");
    expect(kindOf("constructor")).toBe("file");
    expect(kindOf("toString")).toBe("file");
    expect(kindOf("constructor", undefined, true)).toBe("folder");
    expect(kindOf("zzz-unknown", undefined, true)).toBe("folder");
  });

  it("每个 kind 都带一个 CSS 变量颜色", () => {
    for (const name of ["main.rs", "a.png", "unknown.qqq", "package.json"]) {
      expect(entryIconOf(entry(name)).color, name).toMatch(/^var\(--icon-/);
    }
    expect(entryIconOf({ name: "src", is_dir: true }).color).toMatch(/^var\(--icon-/);
    expect(entryIconOf({ name: "link", is_dir: false, is_symlink: true }).color).toMatch(
      /^var\(--icon-/,
    );
  });
});
