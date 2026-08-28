import { describe, expect, it } from "vitest";
import { deriveTitle, fileStem, joinNote, splitNote } from "../components/notebook/noteFrontmatter";

describe("splitNote", () => {
  it("extracts the title and keeps the body", () => {
    const source = '---\ntitle: "Deploy notes"\nupdated: 2026-01-01T00:00:00Z\n---\n\n# Release\n';
    const { frontmatter, body } = splitNote(source);

    expect(frontmatter.title).toBe("Deploy notes");
    expect(body).toBe("# Release\n");
    // 不认识的字段要留着 —— 可能是别的工具写的。
    expect(frontmatter.extra).toContain("updated: 2026-01-01T00:00:00Z");
  });

  it("treats a document without frontmatter as pure body", () => {
    const source = "# Just a heading\n\ntext\n";
    const { frontmatter, body } = splitNote(source);

    expect(frontmatter.title).toBeNull();
    expect(body).toBe(source);
  });

  it("does not treat an unterminated fence as frontmatter", () => {
    // 开了 `---` 却没闭合,整篇都是正文 —— 否则会把用户的内容当元数据吃掉。
    const source = "---\ntitle: broken\n\nstill body\n";
    const { frontmatter, body } = splitNote(source);

    expect(frontmatter.title).toBeNull();
    expect(body).toBe(source);
  });

  it("unescapes quoted scalars", () => {
    const source = '---\ntitle: "He said \\"hi\\": really"\n---\n\nbody\n';
    expect(splitNote(source).frontmatter.title).toBe('He said "hi": really');
  });

  it("reads single-quoted scalars", () => {
    const source = "---\ntitle: 'it''s here'\n---\n\nbody\n";
    expect(splitNote(source).frontmatter.title).toBe("it's here");
  });

  it("reads bare scalars", () => {
    const source = "---\ntitle: plain title\n---\n\nbody\n";
    expect(splitNote(source).frontmatter.title).toBe("plain title");
  });

  it("handles CRLF files written by external editors", () => {
    const source = '---\r\ntitle: "Win"\r\n---\r\n\r\n# Body\r\n';
    const { frontmatter, body } = splitNote(source);

    expect(frontmatter.title).toBe("Win");
    expect(body).toContain("# Body");
  });

  it("does not mistake a horizontal rule mid-document for frontmatter", () => {
    const source = "# Title\n\n---\n\nafter the rule\n";
    const { frontmatter, body } = splitNote(source);

    expect(frontmatter.title).toBeNull();
    expect(body).toBe(source);
  });
});

describe("joinNote", () => {
  it("round-trips through split without losing unknown fields", () => {
    const source = '---\ntitle: "Keep"\ncustom: value\n---\n\nbody text\n';
    const { frontmatter, body } = splitNote(source);
    const rebuilt = joinNote(frontmatter, body);

    expect(rebuilt).toContain('title: "Keep"');
    expect(rebuilt).toContain("custom: value");
    expect(rebuilt).toContain("body text");

    // 再拆一次结果必须一致。
    const again = splitNote(rebuilt);
    expect(again.frontmatter.title).toBe("Keep");
    expect(again.frontmatter.extra).toContain("custom: value");
    expect(again.body.trim()).toBe("body text");
  });

  it("escapes titles that would break YAML", () => {
    const rebuilt = joinNote({ title: 'a "b": c', extra: [] }, "body");
    expect(rebuilt).toContain('title: "a \\"b\\": c"');
    // 关键:重新解析得回原值。
    expect(splitNote(rebuilt).frontmatter.title).toBe('a "b": c');
  });

  it("flattens newlines in a title so the block stays parseable", () => {
    // frontmatter 是单行 key: value,标题里的换行会截断这一项。
    const rebuilt = joinNote({ title: "line1\nline2", extra: [] }, "body");
    expect(splitNote(rebuilt).frontmatter.title).toBe("line1 line2");
  });

  it("omits the block entirely when there is nothing to write", () => {
    // 空标题不值得往文件里塞一个空的 `---\n---`。
    expect(joinNote({ title: null, extra: [] }, "just body")).toBe("just body");
    expect(joinNote({ title: "   ", extra: [] }, "just body")).toBe("just body");
  });

  it("survives a title made of backslashes", () => {
    const rebuilt = joinNote({ title: "C:\\path\\to", extra: [] }, "b");
    expect(splitNote(rebuilt).frontmatter.title).toBe("C:\\path\\to");
  });

  it("stays byte-stable across repeated read-write cycles", () => {
    // 每次保存都会走一遍 split → join。空行处理只要不对称,反复编辑就会在
    // 正文顶部累积空行 —— 用户看到的是文件慢慢「长胖」。
    const start = '---\ntitle: "Stable"\n---\n\n# Body\n\ntext\n';
    let current = start;
    for (let round = 0; round < 5; round += 1) {
      const { frontmatter, body } = splitNote(current);
      current = joinNote(frontmatter, body);
    }
    expect(current).toBe(start);
  });

  it("preserves intentional blank lines inside the body", () => {
    // 只吃 frontmatter 后面的分隔空行,正文内部的空行是用户写的排版。
    const source = '---\ntitle: "T"\n---\n\npara one\n\n\npara two\n';
    const { body } = splitNote(source);
    expect(body).toBe("para one\n\n\npara two\n");
  });
});

describe("deriveTitle", () => {
  it("prefers frontmatter title", () => {
    const source = '---\ntitle: "From matter"\n---\n\n# From heading\n';
    expect(deriveTitle(source, "/v/file.md")).toBe("From matter");
  });

  it("falls back to the first heading", () => {
    expect(deriveTitle("# From heading\n\ntext", "/v/file.md")).toBe("From heading");
  });

  it("falls back to the file stem for a bare markdown file", () => {
    // 用户从 Obsidian 拖进来的裸 md 也要有个合理的名字。
    expect(deriveTitle("no heading, no matter\n", "/v/My Note.md")).toBe("My Note");
  });

  it("ignores an empty frontmatter title and uses the heading", () => {
    const source = '---\ntitle: ""\n---\n\n# Real title\n';
    expect(deriveTitle(source, "/v/file.md")).toBe("Real title");
  });
});

describe("fileStem", () => {
  it("strips directories and the extension", () => {
    expect(fileStem("/a/b/c.md")).toBe("c");
    expect(fileStem("C:\\notes\\d.markdown")).toBe("d");
  });

  it("keeps dotfiles intact", () => {
    // `.gitignore` 的 stem 不该是空字符串。
    expect(fileStem("/a/.gitignore")).toBe(".gitignore");
  });

  it("handles a name without an extension", () => {
    expect(fileStem("/a/README")).toBe("README");
  });
});
