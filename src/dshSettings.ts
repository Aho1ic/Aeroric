export interface DshAgentPreset {
  id: string;
  name?: string;
  description?: string;
}

export interface DshSettingsSnapshot {
  shell: {
    timeoutMs: number;
    maxOutputBytes: number;
  };
  agentLoop: {
    maxParallelToolCalls: number;
  };
  webSearch: {
    baseUrl: string;
    maxUses: number;
    apiKeyConfigured: boolean;
  };
  defaultPreset: string;
  customPresets: DshAgentPreset[];
}

export const DSH_BUILT_IN_PRESET_IDS = ["standard", "code", "minimal", "cordis"] as const;

export function normalizeDshDefaultPreset(snapshot: DshSettingsSnapshot): string {
  const customPresets = Array.isArray(snapshot.customPresets) ? snapshot.customPresets : [];
  const known = new Set<string>([
    ...DSH_BUILT_IN_PRESET_IDS,
    ...customPresets.map((preset) => preset.id).filter((id) => id.trim().length > 0),
  ]);
  return typeof snapshot.defaultPreset === "string" && known.has(snapshot.defaultPreset)
    ? snapshot.defaultPreset
    : "standard";
}
