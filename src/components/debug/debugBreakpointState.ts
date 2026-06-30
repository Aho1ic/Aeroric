import type { DebugBreakpoint, DebugConfig } from "../../types";

function normalizePath(value: string): string {
  return value.replace(/\\/g, "/").replace(/\/+/g, "/");
}

function trimTrailingSlash(value: string): string {
  return value.length > 1 ? value.replace(/\/+$/g, "") : value;
}

function normalizeBreakpoint(breakpoint: DebugBreakpoint): DebugBreakpoint {
  const normalized: DebugBreakpoint = {
    file: normalizePath(breakpoint.file.trim()),
    line: Math.max(1, Math.floor(breakpoint.line)),
    column: Math.max(1, Math.floor(breakpoint.column || 1)),
  };
  const condition = breakpoint.condition?.trim();
  const logMessage = breakpoint.logMessage?.trim();
  if (condition) normalized.condition = condition;
  if (logMessage) normalized.logMessage = logMessage;
  return normalized;
}

function hasBreakpointMetadata(breakpoint: DebugBreakpoint): boolean {
  return Boolean(breakpoint.condition || breakpoint.logMessage);
}

function lineKey(breakpoint: DebugBreakpoint): string {
  const normalized = normalizeBreakpoint(breakpoint);
  return `${normalized.file}:${normalized.line}`;
}

function locationKey(breakpoint: DebugBreakpoint): string {
  const normalized = normalizeBreakpoint(breakpoint);
  return `${normalized.file}:${normalized.line}:${normalized.column}`;
}

export function debugBreakpointFileForProject(projectPath: string, filePath: string): string {
  const root = trimTrailingSlash(normalizePath(projectPath));
  const file = normalizePath(filePath);
  if (file === root) return "";
  if (file.startsWith(`${root}/`)) return file.slice(root.length + 1);
  return file;
}

export function sortDebugBreakpoints(breakpoints: DebugBreakpoint[]): DebugBreakpoint[] {
  return [...breakpoints].map(normalizeBreakpoint).sort((a, b) => {
    const fileOrder = a.file.localeCompare(b.file);
    if (fileOrder !== 0) return fileOrder;
    if (a.line !== b.line) return a.line - b.line;
    return a.column - b.column;
  });
}

export function toggleLineDebugBreakpoint(
  breakpoints: DebugBreakpoint[],
  breakpoint: DebugBreakpoint,
): DebugBreakpoint[] {
  const nextBreakpoint = normalizeBreakpoint({ ...breakpoint, column: 1 });
  const key = lineKey(nextBreakpoint);
  if (breakpoints.some((item) => lineKey(item) === key)) {
    return sortDebugBreakpoints(breakpoints.filter((item) => lineKey(item) !== key));
  }
  return sortDebugBreakpoints([...breakpoints, nextBreakpoint]);
}

export function debugBreakpointLinesForFile(
  breakpoints: DebugBreakpoint[],
  projectPath: string,
  filePath: string,
): Set<number> {
  const relative = debugBreakpointFileForProject(projectPath, filePath);
  const files = new Set([normalizePath(filePath), normalizePath(relative)]);
  const lines = new Set<number>();
  for (const breakpoint of breakpoints.map(normalizeBreakpoint)) {
    if (files.has(breakpoint.file)) {
      lines.add(breakpoint.line);
    }
  }
  return lines;
}

export function mergeDebugConfigBreakpoints(
  config: DebugConfig,
  editorBreakpoints: DebugBreakpoint[],
): DebugConfig {
  const merged = new Map<string, DebugBreakpoint>();
  for (const breakpoint of [...config.breakpoints, ...editorBreakpoints].map(normalizeBreakpoint)) {
    const key = locationKey(breakpoint);
    const existing = merged.get(key);
    if (existing && hasBreakpointMetadata(existing) && !hasBreakpointMetadata(breakpoint)) {
      continue;
    }
    merged.set(key, breakpoint);
  }
  return {
    ...config,
    breakpoints: sortDebugBreakpoints([...merged.values()]),
  };
}
