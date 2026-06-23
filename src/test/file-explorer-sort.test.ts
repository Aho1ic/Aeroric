import { describe, expect, it } from "vitest";
import { fileIconKind, sortFileEntries } from "../components/file-explorer/fileEntryUtils";
import type { FsEntry } from "../components/file-explorer/types";

const entries: FsEntry[] = [
  { name: "z.py", path: "/repo/z.py", is_dir: false, extension: "py", is_gitignored: false, modifiedAtMs: 300 },
  { name: "src", path: "/repo/src", is_dir: true, is_gitignored: false, modifiedAtMs: 100 },
  { name: "README.md", path: "/repo/README.md", is_dir: false, extension: "md", is_gitignored: false, modifiedAtMs: 500 },
  { name: "docs", path: "/repo/docs", is_dir: true, is_gitignored: false, modifiedAtMs: 900 },
];

describe("file explorer sorting", () => {
  it("keeps folders above files when sorting by name ascending or descending", () => {
    expect(sortFileEntries(entries, "name", "asc").map((entry) => entry.name)).toEqual([
      "docs",
      "src",
      "README.md",
      "z.py",
    ]);
    expect(sortFileEntries(entries, "name", "desc").map((entry) => entry.name)).toEqual([
      "src",
      "docs",
      "z.py",
      "README.md",
    ]);
  });

  it("keeps folders above files when sorting by modification time", () => {
    expect(sortFileEntries(entries, "modified", "asc").map((entry) => entry.name)).toEqual([
      "src",
      "docs",
      "z.py",
      "README.md",
    ]);
    expect(sortFileEntries(entries, "modified", "desc").map((entry) => entry.name)).toEqual([
      "docs",
      "src",
      "README.md",
      "z.py",
    ]);
  });

  it("classifies database, model, video, and wheel files", () => {
    expect(fileIconKind({ name: "index.db", is_dir: false, extension: "db" })).toBe("database");
    expect(fileIconKind({ name: "model.pt", is_dir: false, extension: "pt" })).toBe("model");
    expect(fileIconKind({ name: "model.pth", is_dir: false, extension: "pth" })).toBe("model");
    expect(fileIconKind({ name: "detector.onnx", is_dir: false, extension: "onnx" })).toBe("model");
    expect(fileIconKind({ name: "clip.mp4", is_dir: false, extension: "mp4" })).toBe("video");
    expect(fileIconKind({ name: "pkg.whl", is_dir: false, extension: "whl" })).toBe("package");
  });
});
