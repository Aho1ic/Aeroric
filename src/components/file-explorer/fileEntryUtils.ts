import type { FsEntry } from "./types";
import { fileExtensionOf, hasSqliteDatabaseExtension } from "../../lib/fileExtensions";

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

/** 保留这个名字:file-explorer 内部到处在用。实现已挪到 lib/fileExtensions.ts。 */
export const fileExtension = fileExtensionOf;

export function isSqliteDatabaseFileName(name: string, ext?: string | null): boolean {
  return hasSqliteDatabaseExtension(name, ext);
}

export function isSqliteDatabaseFile(
  entry: Pick<FsEntry, "name" | "extension" | "is_dir">,
): boolean {
  if (entry.is_dir) return false;
  return isSqliteDatabaseFileName(entry.name, entry.extension);
}

export function sortFileEntries<T extends Pick<FsEntry, "name" | "is_dir" | "modified_at_ms">>(
  entries: T[],
  field: FileSortField,
  direction: FileSortDirection,
): T[] {
  const sign = direction === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
    if (field === "modified") {
      const modifiedDiff = ((a.modified_at_ms ?? 0) - (b.modified_at_ms ?? 0)) * sign;
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
