import { describe, expect, it } from "vitest";
import {
  isMarkdownFile,
  isPreviewableImageFile,
  isSqliteDatabaseFile,
} from "../components/file-viewer/editorUtils";
import {
  fileExtension,
  fileIconKind,
  isSqliteDatabaseFile as isSqliteDatabaseEntry,
  isSqliteDatabaseFileName,
} from "../components/file-explorer/fileEntryUtils";

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

describe("fileIconKind", () => {
  const entry = (name: string, extension?: string, is_dir = false) => ({
    name,
    extension,
    is_dir,
  });

  it("目录优先于任何后缀", () => {
    expect(fileIconKind(entry("assets.png", undefined, true))).toBe("folder");
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
    ["i.svg", "image"],
    ["d.md", "markdown"],
    ["d.mdx", "markdown"],
    ["d.json", "json"],
    ["d.jsonc", "json"],
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
    ["s.css", "code"],
    ["s.scss", "code"],
    ["s.html", "code"],
    ["s.htm", "code"],
    ["s.yaml", "code"],
    ["s.yml", "code"],
    ["s.toml", "code"],
    ["s.sh", "code"],
    ["s.sql", "code"],
    ["s.java", "code"],
    ["s.c", "code"],
    ["s.cpp", "code"],
    ["s.h", "code"],
    ["s.hpp", "code"],
    ["r.txt", "text"],
    ["r.log", "text"],
    ["r.env", "text"],
    ["r.ini", "text"],
    ["r.conf", "text"],
    ["mystery.xyz", "file"],
    ["LICENSE", "file"],
  ] as const)("%s → %s", (name, kind) => {
    expect(fileIconKind(entry(name))).toBe(kind);
  });

  it("图标表与 isPreviewableImageFile 用的是同一组图片后缀", () => {
    // 这两处目前是两份字面重复的清单。合并时若只改一处,这条会挂。
    for (const ext of IMAGE_EXTS) {
      expect(fileIconKind(entry(`x.${ext}`)), ext).toBe("image");
      expect(isPreviewableImageFile(`x.${ext}`), ext).toBe(true);
    }
  });

  it("图标表与 sqlite 判定用的是同一组后缀", () => {
    for (const ext of SQLITE_EXTS) {
      expect(fileIconKind(entry(`x.${ext}`)), ext).toBe("database");
      expect(isSqliteDatabaseFileName(`x.${ext}`), ext).toBe(true);
    }
  });

  it("`.markdown` 现在拿不到 markdown 图标(与 isMarkdownFile 不一致)", () => {
    // 现状记录:isMarkdownFile("a.markdown") 为 true,但图标表只有 md/mdx。
    // 这条是故意钉住"不一致"本身 —— 改的时候会看到它挂,从而是有意识地改。
    expect(isMarkdownFile("a.markdown")).toBe(true);
    expect(fileIconKind(entry("a.markdown"))).toBe("file");
  });
});
