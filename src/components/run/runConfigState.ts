import type {
  DebugConfig,
  DebugRunConfig,
  RunConfig,
  RunConfigDocument,
  RunDebugConfigType,
  RunConfigType,
  RunProcessSnapshot,
} from "../../types";
import { parseBreakpointText } from "../debug/debugState";

export type RunConfigDraft = {
  type: RunConfigType;
  id: string;
  name: string;
  debugRuntime: RunDebugConfigType;
  command: string;
  program: string;
  cwd: string;
  argsText: string;
  envText: string;
  breakpointsText: string;
};

export function defaultRunConfigDraft(): RunConfigDraft {
  return {
    type: "shell",
    id: "",
    name: "",
    debugRuntime: "node",
    command: "",
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

function parseListText(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"));
}

export function buildRunConfigDraft(draft: RunConfigDraft): RunConfig {
  const name = draft.name.trim();
  const id = slugifyId(draft.id || name);
  if (draft.type === "debug") {
    return {
      id,
      name,
      type: "debug",
      debugType: draft.debugRuntime,
      program: draft.program.trim(),
      cwd: draft.cwd.trim() || ".",
      args: parseListText(draft.argsText),
      env: parseEnvText(draft.envText),
      breakpoints: parseBreakpointText(draft.breakpointsText),
    };
  }
  return {
    id,
    name,
    type: "shell",
    command: draft.command.trim(),
    cwd: draft.cwd.trim() || ".",
    env: parseEnvText(draft.envText),
  };
}

export function runConfigToDraft(config: RunConfig): RunConfigDraft {
  const envText = Object.entries(config.env ?? {})
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
  if (config.type === "debug") {
    return {
      type: "debug",
      id: config.id,
      name: config.name,
      debugRuntime: config.debugType,
      command: "",
      program: config.program,
      cwd: config.cwd || ".",
      argsText: (config.args ?? []).join("\n"),
      envText,
      breakpointsText: (config.breakpoints ?? [])
        .map((breakpoint) =>
          breakpoint.column && breakpoint.column !== 1
            ? `${breakpoint.file}:${breakpoint.line}:${breakpoint.column}`
            : `${breakpoint.file}:${breakpoint.line}`,
        )
        .join("\n"),
    };
  }
  return {
    type: "shell",
    id: config.id,
    name: config.name,
    debugRuntime: "node",
    command: config.command,
    program: "",
    cwd: config.cwd || ".",
    argsText: "",
    envText,
    breakpointsText: "",
  };
}

export function runConfigSummary(config: RunConfig): string {
  return config.type === "debug" ? config.program : config.command;
}

export function isRunConfigLaunchable(config: RunConfig): boolean {
  if (!config.id.trim() || !config.name.trim()) return false;
  return config.type === "debug" ? Boolean(config.program.trim()) : Boolean(config.command.trim());
}

export function isRunConfigDraftLaunchable(draft: RunConfigDraft): boolean {
  return isRunConfigLaunchable(buildRunConfigDraft(draft));
}

export function runConfigToDebugConfig(config: RunConfig): DebugConfig | null {
  if (config.type !== "debug") return null;
  return {
    id: config.id,
    name: config.name,
    type: config.debugType,
    program: config.program,
    cwd: config.cwd,
    args: config.args,
    env: config.env,
    breakpoints: config.breakpoints,
  };
}

export function isDebugRunConfig(config: RunConfig): config is DebugRunConfig {
  return config.type === "debug";
}

export function upsertRunConfig(document: RunConfigDocument, config: RunConfig): RunConfigDocument {
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

export function removeRunConfig(document: RunConfigDocument, id: string): RunConfigDocument {
  return {
    version: 1,
    configs: document.configs.filter((config) => config.id !== id),
  };
}

export function isRunProcessActive(snapshot: RunProcessSnapshot | null): boolean {
  return snapshot?.status === "running";
}
