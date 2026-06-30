import type {
  DebugBreakpoint,
  DebugCallFrame,
  DebugConfig,
  DebugConfigDocument,
  DebugConfigType,
  DebugRequestType,
  DebugVariable,
  DebugSessionSnapshot,
} from "../../types";

export type DebugConfigDraft = {
  id: string;
  name: string;
  runtime: DebugConfigType;
  request: DebugRequestType;
  program: string;
  cwd: string;
  attachHost: string;
  attachPort: string;
  argsText: string;
  envText: string;
  breakpointsText: string;
};

export function defaultDebugConfigDraft(): DebugConfigDraft {
  return {
    id: "",
    name: "",
    runtime: "node",
    request: "launch",
    program: "",
    cwd: ".",
    attachHost: "127.0.0.1",
    attachPort: "9229",
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

function parseAttachPort(value: string): number | null {
  const port = Number(value.trim());
  return Number.isInteger(port) && port > 0 && port <= 65535 ? port : null;
}

export function parseBreakpointText(value: string): DebugBreakpoint[] {
  const breakpoints: DebugBreakpoint[] = [];
  for (const line of value.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const match = trimmed.match(/^(.+?):(\d+)(?::(\d+))?(?:\s+(if|log)\s+(.+))?$/);
    if (!match) continue;
    const lineNumber = Number(match[2]);
    const columnNumber = Number(match[3] ?? "1");
    if (!Number.isInteger(lineNumber) || lineNumber <= 0) continue;
    if (!Number.isInteger(columnNumber) || columnNumber <= 0) continue;
    const modifier = match[4];
    const modifierValue = match[5]?.trim();
    const breakpoint: DebugBreakpoint = {
      file: match[1].trim(),
      line: lineNumber,
      column: columnNumber,
    };
    if (modifier === "if" && modifierValue) breakpoint.condition = modifierValue;
    if (modifier === "log" && modifierValue) breakpoint.logMessage = modifierValue;
    breakpoints.push(breakpoint);
  }
  return breakpoints;
}

export function formatBreakpointText(breakpoints: DebugBreakpoint[]): string {
  return breakpoints
    .map((breakpoint) => {
      const base =
        breakpoint.column && breakpoint.column !== 1
          ? `${breakpoint.file}:${breakpoint.line}:${breakpoint.column}`
          : `${breakpoint.file}:${breakpoint.line}`;
      if (breakpoint.condition) return `${base} if ${breakpoint.condition}`;
      if (breakpoint.logMessage) return `${base} log ${breakpoint.logMessage}`;
      return base;
    })
    .join("\n");
}

export function buildDebugConfigDraft(draft: DebugConfigDraft): DebugConfig {
  const name = draft.name.trim();
  const id = slugifyId(draft.id || name);
  const config: DebugConfig = {
    id,
    name,
    type: draft.runtime,
    request: draft.request,
    program: draft.program.trim(),
    cwd: draft.cwd.trim() || ".",
    attachHost: draft.attachHost.trim() || "127.0.0.1",
    args: parseListText(draft.argsText),
    env: parseEnvText(draft.envText),
    breakpoints: parseBreakpointText(draft.breakpointsText),
  };
  const attachPort = parseAttachPort(draft.attachPort);
  if (draft.request === "attach" && attachPort !== null) {
    config.attachPort = attachPort;
  }
  return config;
}

export function debugConfigToDraft(config: DebugConfig): DebugConfigDraft {
  const request = config.request ?? "launch";
  return {
    id: config.id,
    name: config.name,
    runtime: config.type,
    request,
    program: config.program,
    cwd: config.cwd || ".",
    attachHost: config.attachHost ?? "127.0.0.1",
    attachPort:
      typeof config.attachPort === "number"
        ? String(config.attachPort)
        : request === "attach"
          ? ""
          : "9229",
    argsText: (config.args ?? []).join("\n"),
    envText: Object.entries(config.env ?? {})
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, value]) => `${key}=${value}`)
      .join("\n"),
    breakpointsText: formatBreakpointText(config.breakpoints ?? []),
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

export function canEvaluateDebugSession(
  snapshot: DebugSessionSnapshot | null,
  requestInFlight: boolean,
): boolean {
  return Boolean(snapshot?.status === "paused" && !requestInFlight);
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
