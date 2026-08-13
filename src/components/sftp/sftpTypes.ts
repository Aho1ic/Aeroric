import type { SshConnection } from "../../types";
import type { StorageConnection } from "../../types/storage";

export type SftpEndpoint =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connectionId: string; connectionName: string; path: string }
  | { kind: "storage"; connectionId: string; connectionName: string; path: string };

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

export const SFTP_SORT_FIELDS = ["name", "modified", "size", "type"] as const;
export type SftpSortField = (typeof SFTP_SORT_FIELDS)[number];
export type SftpSortDirection = "asc" | "desc";
export type SftpSortPreference = {
  field: SftpSortField;
  direction: SftpSortDirection;
};

export type SftpConnectionGroup = {
  label: string;
  connections: SshConnection[];
};

export const DEFAULT_SFTP_SORT_PREFERENCE: SftpSortPreference = {
  field: "modified",
  direction: "desc",
};

export interface SftpRelativeDayLabels {
  today: string;
  yesterday: string;
  dayBeforeYesterday: string;
}

export function formatSftpModifiedTime(
  modifiedAtMs: number | null | undefined,
  labels: SftpRelativeDayLabels,
  nowMs = Date.now(),
): string {
  if (typeof modifiedAtMs !== "number" || !Number.isFinite(modifiedAtMs)) return "";
  const date = new Date(modifiedAtMs);
  const now = new Date(nowMs);
  if (Number.isNaN(date.getTime()) || Number.isNaN(now.getTime())) return "";

  const dateDay = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
  const nowDay = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
  const daysAgo = Math.round((nowDay - dateDay) / 86_400_000);
  const time = `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
  if (daysAgo === 0) return `${labels.today} ${time}`;
  if (daysAgo === 1) return `${labels.yesterday} ${time}`;
  if (daysAgo === 2) return `${labels.dayBeforeYesterday} ${time}`;
  return `${date.getFullYear()}/${date.getMonth() + 1}/${date.getDate()} ${time}`;
}

export type SftpTauriEndpoint =
  | { kind: "local"; path: string }
  | { kind: "ssh"; connection: SshConnection; path: string }
  // 存储连接只传 id,凭据由后端从 0600 的 secrets 文件读取,不经过前端。
  | { kind: "storage"; connectionId: string; path: string };

export function sftpEndpointKey(endpoint: SftpEndpoint): string {
  if (endpoint.kind === "local") return `local:${endpoint.path}`;
  return `${endpoint.kind}:${endpoint.connectionId}:${endpoint.path}`;
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
  // storage 的路径前缀由后端 `root` 配置解析,前端始终从 "/" 开始。
  if (kind === "storage") return "/";
  return connection?.remotePath?.trim() || "/";
}

export type SftpStorageConnectionGroup = {
  label: string;
  connections: StorageConnection[];
};

/** 与 `groupSftpSshConnections` 同构,保持下拉框两类连接的分组体验一致。 */
export function groupSftpStorageConnections(
  connections: StorageConnection[],
  defaultGroupLabel: string,
): SftpStorageConnectionGroup[] {
  const groups: SftpStorageConnectionGroup[] = [];
  const byLabel = new Map<string, SftpStorageConnectionGroup>();
  for (const connection of connections) {
    const label = connection.group?.trim() || defaultGroupLabel;
    const existing = byLabel.get(label);
    if (existing) {
      existing.connections.push(connection);
      continue;
    }
    const group = { label, connections: [connection] };
    byLabel.set(label, group);
    groups.push(group);
  }
  return groups;
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
    if (field === "size") {
      const sizeDiff = ((a.size ?? 0) - (b.size ?? 0)) * sign;
      if (sizeDiff !== 0) return sizeDiff;
    }
    if (field === "type") {
      const typeDiff =
        inferFileType(a).localeCompare(inferFileType(b), undefined, { sensitivity: "base" }) * sign;
      if (typeDiff !== 0) return typeDiff;
    }
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }) * sign;
  });
}

export function normalizeSftpSortPreference(value: unknown): SftpSortPreference {
  if (!value || typeof value !== "object") return DEFAULT_SFTP_SORT_PREFERENCE;
  const candidate = value as Partial<SftpSortPreference>;
  const field = SFTP_SORT_FIELDS.find((item) => item === candidate.field) ?? null;
  const direction =
    candidate.direction === "asc" || candidate.direction === "desc" ? candidate.direction : null;
  if (!field || !direction) return DEFAULT_SFTP_SORT_PREFERENCE;
  return { field, direction };
}

export function groupSftpSshConnections(
  connections: SshConnection[],
  defaultGroupLabel: string,
): SftpConnectionGroup[] {
  const groups: SftpConnectionGroup[] = [];
  const byLabel = new Map<string, SftpConnectionGroup>();
  for (const connection of connections) {
    const label = connection.group?.trim() || defaultGroupLabel;
    const existing = byLabel.get(label);
    if (existing) {
      existing.connections.push(connection);
      continue;
    }
    const group = { label, connections: [connection] };
    byLabel.set(label, group);
    groups.push(group);
  }
  return groups;
}

export function formatSftpTransferPercent(completed: number, total: number): number {
  if (!Number.isFinite(completed) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((completed / total) * 100)));
}

export function sftpProgressRingBackground(
  percent: number,
  color: string = "var(--accent)",
): string {
  const value = Math.max(0, Math.min(100, Math.round(percent)));
  return `conic-gradient(${color} ${value}%, var(--border-dim) 0)`;
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

/**
 * 推断条目类型:目录返回 "folder",文件返回小写扩展名,无扩展名文件返回 "file"。
 * 用于按类型排序与类型展示,保证同类文件聚在一起。
 */
export function inferFileType(entry: Pick<SftpEntry, "name" | "isDir" | "extension">): string {
  if (entry.isDir) return "folder";
  const explicit = entry.extension?.trim().replace(/^\.+/, "").toLowerCase();
  if (explicit) return explicit;
  const name = entry.name.replace(/\/+$/, "");
  const dotIndex = name.lastIndexOf(".");
  if (dotIndex <= 0 || dotIndex === name.length - 1) return "file";
  return name.slice(dotIndex + 1).toLowerCase();
}

export function isSftpImageFile(name: string): boolean {
  const ext = name.split(".").pop()?.toLowerCase();
  return ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].includes(ext ?? "");
}
