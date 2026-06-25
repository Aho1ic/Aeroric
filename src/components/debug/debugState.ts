import type {
  DebugBreakpoint,
  DebugCallFrame,
  DebugConfig,
  DebugConfigDocument,
  DebugConfigType,
  DebugVariable,
  DebugSessionSnapshot,
} from "../../types";

export type DebugConfigDraft = {
  id: string;
  name: string;
  runtime: DebugConfigType;
  program: string;
  cwd: string;
  argsText: string;
  envText: string;
  breakpointsText: string;
};

export function defaultDebugConfigDraft(): DebugConfigDraft {
  return {
    id: "",
    name: "",
    runtime: "node",
    program: "",
    cwd: ".",
    argsText: "",
    envText: "",
    breakpointsText: "",
  };
}

function slugifyId(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function parseListText(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"));
}

function parseEnvText(value: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const line of value.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const separator = trimmed.indexOf("=");
    if (separator <= 0) continue;
    const key = trimmed.slice(0, separator).trim();
    const envValue = trimmed.slice(separator + 1).trim();
    if (!key) continue;
    env[key] = envValue;
  }
  return env;
}

export function parseBreakpointText(value: string): DebugBreakpoint[] {
  const breakpoints: DebugBreakpoint[] = [];
  for (const line of value.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const match = trimmed.match(/^(.+?):(\d+)(?::(\d+))?$/);
    if (!match) continue;
    const lineNumber = Number(match[2]);
    const columnNumber = Number(match[3] ?? "1");
    if (!Number.isInteger(lineNumber) || lineNumber <= 0) continue;
    if (!Number.isInteger(columnNumber) || columnNumber <= 0) continue;
    breakpoints.push({
      file: match[1].trim(),
      line: lineNumber,
      column: columnNumber,
    });
  }
  return breakpoints;
}

export function buildDebugConfigDraft(draft: DebugConfigDraft): DebugConfig {
  const name = draft.name.trim();
  const id = slugifyId(draft.id || name);
  return {
    id,
    name,
    type: draft.runtime,
    program: draft.program.trim(),
    cwd: draft.cwd.trim() || ".",
    args: parseListText(draft.argsText),
    env: parseEnvText(draft.envText),
    breakpoints: parseBreakpointText(draft.breakpointsText),
  };
}

export function debugConfigToDraft(config: DebugConfig): DebugConfigDraft {
  return {
    id: config.id,
    name: config.name,
    runtime: config.type,
    program: config.program,
    cwd: config.cwd || ".",
    argsText: (config.args ?? []).join("\n"),
    envText: Object.entries(config.env ?? {})
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, value]) => `${key}=${value}`)
      .join("\n"),
    breakpointsText: (config.breakpoints ?? [])
      .map((breakpoint) =>
        breakpoint.column && breakpoint.column !== 1
          ? `${breakpoint.file}:${breakpoint.line}:${breakpoint.column}`
          : `${breakpoint.file}:${breakpoint.line}`,
      )
      .join("\n"),
  };
}

export function upsertDebugConfig(
  document: DebugConfigDocument,
  config: DebugConfig,
): DebugConfigDocument {
  const existingIndex = document.configs.findIndex((item) => item.id === config.id);
  const configs =
    existingIndex === -1
      ? [...document.configs, config]
      : document.configs.map((item, index) => (index === existingIndex ? config : item));
  return {
    version: 1,
    configs,
  };
}

export function removeDebugConfig(document: DebugConfigDocument, id: string): DebugConfigDocument {
  return {
    version: 1,
    configs: document.configs.filter((config) => config.id !== id),
  };
}

export function isDebugSessionActive(snapshot: DebugSessionSnapshot | null): boolean {
  return (
    snapshot?.status === "starting" ||
    snapshot?.status === "running" ||
    snapshot?.status === "paused"
  );
}

export interface DebugSessionControlState {
  canContinue: boolean;
  canStop: boolean;
  canStep: boolean;
}

export function debugSessionControlState(
  snapshot: DebugSessionSnapshot | null,
  requestInFlight: boolean,
): DebugSessionControlState {
  if (requestInFlight) {
    return { canContinue: false, canStop: false, canStep: false };
  }
  const active = isDebugSessionActive(snapshot);
  const paused = snapshot?.status === "paused";
  return {
    canContinue: paused,
    canStop: active,
    canStep: paused,
  };
}

export function isExpandableDebugVariable(variable: DebugVariable): boolean {
  return Boolean(variable.hasChildren && variable.objectId);
}

export interface DebugFrameLocation {
  path: string;
  name: string;
  displayPath: string;
  selection: { line: number; column: number };
}

function normalizeDebugFramePath(file: string): string {
  if (!file.startsWith("file://")) {
    return file;
  }
  try {
    const url = new URL(file);
    if (url.protocol !== "file:") {
      return file;
    }
    return decodeURIComponent(url.pathname);
  } catch {
    return file;
  }
}

function displayDebugPath(path: string, projectPath: string): string {
  if (path === projectPath) {
    return "";
  }
  if (path.startsWith(`${projectPath}/`)) {
    return path.slice(projectPath.length + 1);
  }
  return path;
}

export function resolveDebugFrameLocation(
  frame: DebugCallFrame,
  projectPath: string,
): DebugFrameLocation {
  const path = normalizeDebugFramePath(frame.file);
  const displayPath = displayDebugPath(path, projectPath) || path;
  return {
    path,
    name: frame.functionName || displayPath,
    displayPath,
    selection: {
      line: frame.line,
      column: frame.column,
    },
  };
}
