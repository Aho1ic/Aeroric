import type { SshConnection } from "../../types";

export type SftpEndpoint =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connectionId: string; connectionName: string; path: string };

export type SftpOperation = "copy" | "move";

export type SftpConflictStrategy = "fail" | "merge" | "replace";

export type SftpEntry = {
  name: string;
  path: string;
  isDir: boolean;
  extension?: string | null;
  size?: number | null;
  modifiedAtMs?: number | null;
};

export type SftpTreeRow = {
  entry: SftpEntry;
  depth: number;
};

export type SftpBreadcrumbSegment = {
  label: string;
  path: string;
};

export type SftpFileIconKind =
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

export type SftpSortField = "name" | "modified";
export type SftpSortDirection = "asc" | "desc";
export type SftpSortPreference = {
  field: SftpSortField;
  direction: SftpSortDirection;
};

export type SftpSshConnectionGroup = {
  label: string;
  connections: SshConnection[];
};

export const DEFAULT_SFTP_SORT_PREFERENCE: SftpSortPreference = {
  field: "modified",
  direction: "desc",
};

export type SftpTauriEndpoint =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connection: SshConnection; path: string };

export function sftpEndpointKey(endpoint: SftpEndpoint): string {
  return endpoint.kind === "local"
    ? `local:${endpoint.path}`
    : `ssh:${endpoint.connectionId}:${endpoint.path}`;
}

export function sftpDropOperation(source: SftpEndpoint, target: SftpEndpoint): SftpOperation {
  void source;
  void target;
  return "move";
}

export function sftpClickAction({
  isDir,
  isSelected,
}: {
  isDir: boolean;
  isSelected: boolean;
}): "select" | "toggle" {
  return isDir && isSelected ? "toggle" : "select";
}

export function sftpKeyAction(event: {
  metaKey?: boolean;
  altKey?: boolean;
  ctrlKey?: boolean;
  key: string;
  code?: string;
}): "copy" | "copyPath" | "paste" | "delete" | "preview" | null {
  if (event.code === "Space" || event.key === " ") return "preview";
  const mod = Boolean(event.metaKey || event.ctrlKey);
  if (!mod) return null;
  if (event.altKey && (event.code === "KeyC" || event.key.toLowerCase() === "c")) return "copyPath";
  if (event.code === "KeyC" || event.key.toLowerCase() === "c") return "copy";
  if (event.code === "KeyV" || event.key.toLowerCase() === "v") return "paste";
  if (event.key === "Backspace" || event.key === "Delete") return "delete";
  return null;
}

export function defaultSftpPathForEndpoint(
  kind: SftpEndpoint["kind"],
  connection: SshConnection | undefined,
  localDefaultPath: string,
): string {
  if (kind === "local") return localDefaultPath;
  return connection?.remotePath?.trim() || "/";
}

export function groupSftpSshConnections(
  connections: SshConnection[],
  defaultGroupLabel: string,
): SftpSshConnectionGroup[] {
  const groups: SftpSshConnectionGroup[] = [];
  const byLabel = new Map<string, SftpSshConnectionGroup>();
  for (const connection of connections) {
    const label = connection.group?.trim() || defaultGroupLabel;
    let group = byLabel.get(label);
    if (!group) {
      group = { label, connections: [] };
      byLabel.set(label, group);
      groups.push(group);
    }
    group.connections.push(connection);
  }
  return groups;
}

export function formatSftpTransferPercent(completed: number, total: number): number {
  if (total <= 0) return 100;
  const boundedCompleted = Math.min(Math.max(completed, 0), total);
  return Math.round((boundedCompleted / total) * 100);
}

export function sftpProgressRingBackground(
  percent: number,
  color: string = "var(--accent)",
): string {
  const boundedPercent = Math.min(Math.max(percent, 0), 100);
  const degrees = Math.round((boundedPercent / 100) * 360);
  return `conic-gradient(${color} ${degrees}deg, var(--border-dim) ${degrees}deg)`;
}

export function sftpFileName(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  return trimmed.split("/").pop() || path;
}

export function shouldPromptForSftpConflict(paths: string[], targetEntries: SftpEntry[]): boolean {
  const existingNames = new Set(targetEntries.map((entry) => entry.name));
  return paths.some((path) => existingNames.has(sftpFileName(path)));
}

export function shouldPromptForUnknownSftpConflict(
  paths: string[],
  targetEntries: SftpEntry[] | undefined,
): boolean {
  if (paths.length === 0) return false;
  if (!targetEntries) return true;
  return shouldPromptForSftpConflict(paths, targetEntries);
}

export function sftpParentPath(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  if (!trimmed || trimmed === "/") return "/";
  const idx = trimmed.lastIndexOf("/");
  if (idx <= 0) return "/";
  return trimmed.slice(0, idx);
}

export function sftpJoinPath(parent: string, name: string): string {
  if (!parent || parent === "/") return `/${name}`;
  return `${parent.replace(/\/+$/, "")}/${name}`;
}

export function sftpBreadcrumbSegments(path: string): SftpBreadcrumbSegment[] {
  const trimmed = path.trim().replace(/\/+$/, "");
  if (!trimmed || trimmed === "/") return [{ label: "/", path: "/" }];
  const parts = trimmed.split("/").filter(Boolean);
  return parts.map((part, index) => ({
    label: part,
    path: `/${parts.slice(0, index + 1).join("/")}`,
  }));
}

export function flattenSftpTreeEntries(
  entries: SftpEntry[],
  expandedPaths: Set<string>,
  childrenByPath: Map<string, SftpEntry[]>,
  sortField: SftpSortField = "name",
  sortDirection: SftpSortDirection = "asc",
): SftpTreeRow[] {
  const rows: SftpTreeRow[] = [];
  const append = (items: SftpEntry[], depth: number) => {
    for (const entry of sortSftpEntries(items, sortField, sortDirection)) {
      rows.push({ entry, depth });
      if (!entry.isDir || !expandedPaths.has(entry.path)) continue;
      append(childrenByPath.get(entry.path) ?? [], depth + 1);
    }
  };
  append(entries, 0);
  return rows;
}

export function sortSftpEntries(
  entries: SftpEntry[],
  field: SftpSortField,
  direction: SftpSortDirection,
): SftpEntry[] {
  const sign = direction === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    if (field === "modified") {
      const modifiedDiff = ((a.modifiedAtMs ?? 0) - (b.modifiedAtMs ?? 0)) * sign;
      if (modifiedDiff !== 0) return modifiedDiff;
    }
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }) * sign;
  });
}

export function normalizeSftpSortPreference(value: unknown): SftpSortPreference {
  if (!value || typeof value !== "object") return DEFAULT_SFTP_SORT_PREFERENCE;
  const candidate = value as Partial<SftpSortPreference>;
  const field =
    candidate.field === "name" || candidate.field === "modified" ? candidate.field : null;
  const direction =
    candidate.direction === "asc" || candidate.direction === "desc" ? candidate.direction : null;
  if (!field || !direction) return DEFAULT_SFTP_SORT_PREFERENCE;
  return { field, direction };
}

export function filterSftpTreeEntriesByName(
  entries: SftpEntry[],
  childrenByPath: Map<string, SftpEntry[]>,
  query: string,
): { entries: SftpEntry[]; childrenByPath: Map<string, SftpEntry[]> } {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return { entries, childrenByPath };

  const filteredChildrenByPath = new Map<string, SftpEntry[]>();
  const filterItems = (items: SftpEntry[]): SftpEntry[] => {
    const visible: SftpEntry[] = [];
    for (const entry of items) {
      const childEntries = childrenByPath.get(entry.path) ?? [];
      const visibleChildren = entry.isDir ? filterItems(childEntries) : [];
      if (visibleChildren.length > 0) {
        filteredChildrenByPath.set(entry.path, visibleChildren);
      }
      if (entry.name.toLowerCase().includes(normalizedQuery) || visibleChildren.length > 0) {
        visible.push(entry);
      }
    }
    return visible;
  };

  return {
    entries: filterItems(entries),
    childrenByPath: filteredChildrenByPath,
  };
}

function normalizeTreePath(path: string): string {
  const normalized = path.replace(/\/+$/, "");
  return normalized || "/";
}

function isSameOrAncestorPath(candidate: string, path: string): boolean {
  const normalizedCandidate = normalizeTreePath(candidate);
  const normalizedPath = normalizeTreePath(path);
  if (normalizedCandidate === "/") return true;
  return (
    normalizedCandidate === normalizedPath || normalizedPath.startsWith(`${normalizedCandidate}/`)
  );
}

function isSameOrDescendantPath(candidate: string, path: string): boolean {
  const normalizedCandidate = normalizeTreePath(candidate);
  const normalizedPath = normalizeTreePath(path);
  if (normalizedPath === "/") return true;
  return (
    normalizedCandidate === normalizedPath || normalizedCandidate.startsWith(`${normalizedPath}/`)
  );
}

export function pruneExpandedPathsForFolderSelection(
  expandedPaths: Set<string>,
  selectedFolderPath: string,
): Set<string> {
  const next = new Set<string>();
  for (const path of expandedPaths) {
    if (
      isSameOrAncestorPath(path, selectedFolderPath) ||
      isSameOrDescendantPath(path, selectedFolderPath)
    ) {
      next.add(path);
    }
  }
  return next;
}

export function sftpFileIconKind(entry: SftpEntry): SftpFileIconKind {
  if (entry.isDir) return "folder";
  const ext = (entry.extension ?? entry.name.split(".").pop() ?? "").toLowerCase();
  if (["db", "sqlite", "sqlite3"].includes(ext)) return "database";
  if (["pt", "pth", "onnx"].includes(ext)) return "model";
  if (["mp4", "mov", "mkv", "avi", "webm"].includes(ext)) return "video";
  if (["whl"].includes(ext)) return "package";
  if (["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].includes(ext)) return "image";
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

export function isSftpImageFile(name: string): boolean {
  const ext = name.split(".").pop()?.toLowerCase();
  return ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].includes(ext ?? "");
}
