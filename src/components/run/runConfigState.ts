import type { RunConfig, RunConfigDocument, RunProcessSnapshot } from "../../types";

export type RunConfigDraft = {
  id: string;
  name: string;
  command: string;
  cwd: string;
  envText: string;
};

export function defaultRunConfigDraft(): RunConfigDraft {
  return {
    id: "",
    name: "",
    command: "",
    cwd: ".",
    envText: "",
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

export function buildRunConfigDraft(draft: RunConfigDraft): RunConfig {
  const name = draft.name.trim();
  const id = slugifyId(draft.id || name);
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
  return {
    id: config.id,
    name: config.name,
    command: config.command,
    cwd: config.cwd || ".",
    envText: Object.entries(config.env ?? {})
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, value]) => `${key}=${value}`)
      .join("\n"),
  };
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
