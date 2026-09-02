import type { FsEntry } from "./types";
import {
  PREVIEWABLE_IMAGE_EXTENSIONS,
  SQLITE_DATABASE_EXTENSIONS,
  fileExtensionOf,
  hasSqliteDatabaseExtension,
} from "../../lib/fileExtensions";

export type FileSortField = "name" | "modified";
export type FileSortDirection = "asc" | "desc";
export type FileSortPreference = {
  field: FileSortField;
  direction: FileSortDirection;
};

export const DEFAULT_FILE_SORT_PREFERENCE: FileSortPreference = {
  field: "modified",
  direction: "desc",
};

export type FileIconKind =
  | "folder"
  | "database"
  | "model"
  | "video"
  | "package"
  | "image"
  | "markdown"
  | "json"
  | "archive"
  | "code"
  | "text"
  | "file";

/** 保留这个名字:file-explorer 内部到处在用。实现已挪到 lib/fileExtensions.ts。 */
export const fileExtension = fileExtensionOf;

export function fileIconKind(entry: Pick<FsEntry, "name" | "extension" | "is_dir">): FileIconKind {
  if (entry.is_dir) return "folder";
  const ext = fileExtension(entry.name, entry.extension);
  if (SQLITE_DATABASE_EXTENSIONS.includes(ext)) return "database";
  if (["pt", "pth", "onnx"].includes(ext)) return "model";
  if (["mp4", "mov", "mkv", "avi", "webm"].includes(ext)) return "video";
  if (["whl"].includes(ext)) return "package";
  if (PREVIEWABLE_IMAGE_EXTENSIONS.includes(ext)) return "image";
  if (["md", "mdx"].includes(ext)) return "markdown";
  if (["json", "jsonc"].includes(ext)) return "json";
  if (["zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar"].includes(ext)) return "archive";
  if (
    [
      "ts",
      "tsx",
      "js",
      "jsx",
      "py",
      "rs",
      "go",
      "css",
      "scss",
      "html",
      "htm",
      "yaml",
      "yml",
      "toml",
      "sh",
      "sql",
      "java",
      "c",
      "cpp",
      "h",
      "hpp",
    ].includes(ext)
  ) {
    return "code";
  }
  if (["txt", "log", "env", "ini", "conf"].includes(ext)) return "text";
  return "file";
}

export function isSqliteDatabaseFileName(name: string, ext?: string | null): boolean {
  return hasSqliteDatabaseExtension(name, ext);
}

export function isSqliteDatabaseFile(
  entry: Pick<FsEntry, "name" | "extension" | "is_dir">,
): boolean {
  if (entry.is_dir) return false;
  return isSqliteDatabaseFileName(entry.name, entry.extension);
}

export function sortFileEntries<T extends Pick<FsEntry, "name" | "is_dir" | "modifiedAtMs">>(
  entries: T[],
  field: FileSortField,
  direction: FileSortDirection,
): T[] {
  const sign = direction === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
    if (field === "modified") {
      const modifiedDiff = ((a.modifiedAtMs ?? 0) - (b.modifiedAtMs ?? 0)) * sign;
      if (modifiedDiff !== 0) return modifiedDiff;
    }
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }) * sign;
  });
}

export function filterFileEntriesByName<T extends Pick<FsEntry, "name">>(
  entries: T[],
  query: string,
): T[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return entries;
  return entries.filter((entry) => entry.name.toLowerCase().includes(normalizedQuery));
}

export function normalizeFileSortPreference(value: unknown): FileSortPreference {
  if (!value || typeof value !== "object") return DEFAULT_FILE_SORT_PREFERENCE;
  const candidate = value as Partial<FileSortPreference>;
  const field =
    candidate.field === "name" || candidate.field === "modified" ? candidate.field : null;
  const direction =
    candidate.direction === "asc" || candidate.direction === "desc" ? candidate.direction : null;
  if (!field || !direction) return DEFAULT_FILE_SORT_PREFERENCE;
  return { field, direction };
}
