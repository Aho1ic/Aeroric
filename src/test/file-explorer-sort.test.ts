import { describe, expect, it } from "vitest";
import {
  filterFileEntriesByName,
  normalizeFileSortPreference,
  sortFileEntries,
} from "../components/file-explorer/fileEntryUtils";
import type { FsEntry } from "../components/file-explorer/types";
import { entryIconOf } from "../lib/fileIcons";

const entries: FsEntry[] = [
  {
    name: "z.py",
    path: "/repo/z.py",
    is_dir: false,
    extension: "py",
    is_gitignored: false,
    modified_at_ms: 300,
  },
  { name: "src", path: "/repo/src", is_dir: true, is_gitignored: false, modified_at_ms: 100 },
  {
    name: "README.md",
    path: "/repo/README.md",
    is_dir: false,
    extension: "md",
    is_gitignored: false,
    modified_at_ms: 500,
  },
  { name: "docs", path: "/repo/docs", is_dir: true, is_gitignored: false, modified_at_ms: 900 },
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

  it("filters entries by case-insensitive file name query", () => {
    expect(filterFileEntriesByName(entries, "read").map((entry) => entry.name)).toEqual([
      "README.md",
    ]);
    expect(filterFileEntriesByName(entries, " SRC ").map((entry) => entry.name)).toEqual(["src"]);
    expect(filterFileEntriesByName(entries, "").map((entry) => entry.name)).toEqual([
      "z.py",
      "src",
      "README.md",
      "docs",
    ]);
  });

  it("normalizes file sort preferences with modification descending fallback", () => {
    expect(normalizeFileSortPreference({ field: "name", direction: "asc" })).toEqual({
      field: "name",
      direction: "asc",
    });
    expect(normalizeFileSortPreference({ field: "bad", direction: "sideways" })).toEqual({
      field: "modified",
      direction: "desc",
    });
    expect(normalizeFileSortPreference(undefined)).toEqual({
      field: "modified",
      direction: "desc",
    });
  });

  it("classifies database, model, video, and wheel files", () => {
    expect(entryIconOf({ name: "index.db", is_dir: false, extension: "db" }).kind).toBe("database");
    expect(entryIconOf({ name: "model.pt", is_dir: false, extension: "pt" }).kind).toBe("model");
    expect(entryIconOf({ name: "model.pth", is_dir: false, extension: "pth" }).kind).toBe("model");
    expect(entryIconOf({ name: "detector.onnx", is_dir: false, extension: "onnx" }).kind).toBe(
      "model",
    );
    expect(entryIconOf({ name: "clip.mp4", is_dir: false, extension: "mp4" }).kind).toBe("video");
    expect(entryIconOf({ name: "pkg.whl", is_dir: false, extension: "whl" }).kind).toBe("package");
  });
});
