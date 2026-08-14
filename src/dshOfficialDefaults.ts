export type DshFiberPhase = "pending" | "loading" | "active" | "failed" | "unloading" | null;

export interface DshPluginInventoryEntry {
  entryId: string;
  moduleName: string;
  enabled: boolean;
  fiberPhase: DshFiberPhase;
  builtIn: boolean;
  version?: string;
}

// Mirrored from deepseek-harness/packages/bundle/{base,web-app}/cordis.patch.yml.
// The last column is the effective default state of the Web profile on macOS/Linux.
const OFFICIAL_WEB_PLUGIN_MANIFEST = `
timer|@deepseek-ai/cordis-plugin-timer|1
hmr|@deepseek-ai/cordis-plugin-hmr|0
llm|@deepseek-ai/dsh-llm|1
session|@deepseek-ai/dsh-session|1
typert|@deepseek-ai/dsh-typert-registry|1
typert-loader|@deepseek-ai/dsh-typert-loader|1
typert-gateway|@deepseek-ai/dsh-api-gateway|1
session-title|@deepseek-ai/dsh-session-title|1
session-title-llm|@deepseek-ai/dsh-session-title-first-prompt-llm|1
user-questions|@deepseek-ai/dsh-user-questions|1
agent|@deepseek-ai/dsh-agent|1
agent-default-model|@deepseek-ai/dsh-agent-default-model|1
jobs|@deepseek-ai/dsh-jobs-local|1
llm-retry|@deepseek-ai/dsh-llm-retry|1
settings|@deepseek-ai/dsh-settings-file|1
credentials|@deepseek-ai/dsh-credentials-local|1
llm-pi-ai|@deepseek-ai/dsh-llm-pi-ai|1
session-persistence-jsonl|@deepseek-ai/dsh-session-persistence-jsonl|1
attachment-local|@deepseek-ai/dsh-attachment-local|1
session-query-sqlite|@deepseek-ai/dsh-session-query-sqlite|1
session-projection|@deepseek-ai/dsh-session-projection|1
session-telemetry-otel|@deepseek-ai/dsh-session-telemetry-otel|1
subprocess|@deepseek-ai/dsh-subprocess-local|1
sandbox|@deepseek-ai/dsh-sandbox-local|1
sandbox-policy|@deepseek-ai/dsh-sandbox-policy|1
bash-sandbox|@deepseek-ai/dsh-bash-sandbox|1
pwsh-sandbox|@deepseek-ai/dsh-pwsh-sandbox|0
approval|@deepseek-ai/dsh-user-approval|1
permission|@deepseek-ai/dsh-permission-presets|1
shell-env|@deepseek-ai/dsh-shell-env|1
tool-bash|@deepseek-ai/dsh-tool-bash|0
tool-pwsh|@deepseek-ai/dsh-tool-pwsh|0
tool-jobs|@deepseek-ai/dsh-tool-jobs|0
fs-observation-policy|@deepseek-ai/dsh-fs-observation-policy|1
tool-fs|@deepseek-ai/dsh-tool-fs|0
tool-fs-search|@deepseek-ai/dsh-tool-fs-search|0
agent-instructions|@deepseek-ai/dsh-agent-instructions|0
skill|@deepseek-ai/dsh-skill|1
skill-filesystem|@deepseek-ai/dsh-skill-filesystem|0
skill-badge|@deepseek-ai/dsh-skill-badge|0
tool-skill|@deepseek-ai/dsh-tool-skill|0
commands|@deepseek-ai/dsh-commands|1
command-feedback|@deepseek-ai/dsh-command-feedback|1
goal|@deepseek-ai/dsh-goal|1
goal-round-driver|@deepseek-ai/dsh-goal-round-driver|1
command-goal|@deepseek-ai/dsh-command-goal|1
plan-mode|@deepseek-ai/dsh-plan-mode|0
token-meter|@deepseek-ai/dsh-token-meter|1
compaction-basic|@deepseek-ai/dsh-compaction-basic|0
command-compact|@deepseek-ai/dsh-command-compact|0
subagent|@deepseek-ai/dsh-subagent|1
subagent-spawn-in-process|@deepseek-ai/dsh-subagent-spawn-in-process|1
subagent-fork-in-process|@deepseek-ai/dsh-subagent-fork-in-process|1
tool-subagent-control|@deepseek-ai/dsh-tool-subagent-control|0
tool-subagent-list-agents|@deepseek-ai/dsh-tool-subagent-control/list-agents|0
tool-subagent|@deepseek-ai/dsh-tool-subagent|0
tool-subagent-fork|@deepseek-ai/dsh-tool-subagent|0
tool-subagent-report|@deepseek-ai/dsh-tool-subagent-report|1
workflow-worker-thread|@deepseek-ai/dsh-workflow-worker-thread|0
tool-workflow|@deepseek-ai/dsh-tool-workflow|0
timeout-policy|@deepseek-ai/dsh-tool-call-timeout-policy|1
spill-local|@deepseek-ai/dsh-spill-local|1
spill-policy|@deepseek-ai/dsh-spill-policy|1
session-checkpoint-policy|@deepseek-ai/dsh-session-checkpoint-policy|1
tool-result-pruner|@deepseek-ai/dsh-compaction-tool-result-pruner|0
tool-todo|@deepseek-ai/dsh-tool-todo|0
tool-goal|@deepseek-ai/dsh-tool-goal|0
tool-ralph|@deepseek-ai/dsh-tool-ralph|0
tool-str-replace-editor|@deepseek-ai/dsh-tool-str-replace-editor|0
repeat-tool-reminder|@deepseek-ai/dsh-repeat-tool-reminder|1
web|@deepseek-ai/dsh-web|1
web-search-deepseek|@deepseek-ai/dsh-web-search-deepseek|1
tool-web|@deepseek-ai/dsh-tool-web|0
tools|@deepseek-ai/dsh-tools|1
system-prompt|@deepseek-ai/dsh-system-prompt|1
agent-loop|@deepseek-ai/dsh-agent-loop|1
fs-sandbox|@deepseek-ai/dsh-fs-sandbox|1
llm-deepseek|@deepseek-ai/dsh-llm-deepseek|1
code-runtime|@deepseek-ai/dsh-code-runtime-worker-thread|1
storage|@deepseek-ai/dsh-storage|1
storage-json|@deepseek-ai/dsh-storage-json|1
storage-domain|@deepseek-ai/dsh-storage-domain|1
message-feedback|@deepseek-ai/dsh-message-feedback|1
session-log-download|@deepseek-ai/dsh-session-log-export|1
workspace|@deepseek-ai/dsh-workspace|1
session-projection-cache|@deepseek-ai/dsh-session-projection-cache|1
session-stats|@deepseek-ai/dsh-session-stats|1
directory-picker|@deepseek-ai/dsh-host-directory-picker-auto|1
plugin-inventory|@deepseek-ai/dsh-host-plugin-inventory|1
api-gateway|@deepseek-ai/dsh-host-apiproxy|1
cordis-host-runner|@deepseek-ai/dsh-cordis-host-runner|1
web-startup|@deepseek-ai/dsh-web-app/startup|1
webserver|@deepseek-ai/dsh-host-webserver|1
web-runtime|@deepseek-ai/dsh-web-app|1
client-hmr|@deepseek-ai/dsh-client-hmr|1
modules|@deepseek-ai/dsh-client-modules|1
connection|@deepseek-ai/dsh-client-connection|1
api-remotes|@deepseek-ai/dsh-api-remotes|1
client-runtime|@deepseek-ai/dsh-client-runtime|1
cordis-client-runner|@deepseek-ai/dsh-cordis-client-runner|1
ui-theme|@deepseek-ai/dsh-client-ui-theme|1
locale|@deepseek-ai/dsh-client-locale|1
ui-layout|@deepseek-ai/dsh-client-ui-layout|1
ui-sidebar|@deepseek-ai/dsh-client-ui-sidebar|1
ui-settings|@deepseek-ai/dsh-client-ui-settings|1
ui-settings-general|@deepseek-ai/dsh-client-ui-settings-general|1
ui-settings-models|@deepseek-ai/dsh-client-ui-settings-models|1
ui-settings-plugin-inventory|@deepseek-ai/dsh-client-ui-settings-plugin-inventory|1
ui-conversation|@deepseek-ai/dsh-client-ui-conversation|1
ui-tool|@deepseek-ai/dsh-client-ui-tool|1
ui-cordis|@deepseek-ai/dsh-client-ui-cordis|1
ui-workflow-run|@deepseek-ai/dsh-client-ui-workflow-run|1
ui-deliverables|@deepseek-ai/dsh-client-ui-deliverables|1
ui-workspace|@deepseek-ai/dsh-client-ui-workspace|1
ui-input-trigger|@deepseek-ai/dsh-client-ui-input-trigger|1
ui-commands|@deepseek-ai/dsh-client-ui-commands|1
ui-skill|@deepseek-ai/dsh-client-ui-skill|1
ui-subagent|@deepseek-ai/dsh-client-ui-subagent|1
ui-jobs|@deepseek-ai/dsh-client-ui-jobs|1
ui-goal|@deepseek-ai/dsh-client-ui-goal|1
ui-message-feedback|@deepseek-ai/dsh-client-ui-message-feedback|1
ui-model-selection|@deepseek-ai/dsh-client-ui-model-selection|1
ui-permission|@deepseek-ai/dsh-client-ui-permission-presets|1
ui-agent-preset|@deepseek-ai/dsh-client-ui-agent-preset|1
ui-settings-plugins|@deepseek-ai/dsh-client-ui-settings-plugins|1
ui-plan|@deepseek-ai/dsh-client-ui-plan|1
ui-user-questions|@deepseek-ai/dsh-client-ui-user-questions|1
ui-trajectory|@deepseek-ai/dsh-client-ui-trajectory|1
agent-presets|@deepseek-ai/dsh-agent-presets|1
`;

export const OFFICIAL_DSH_WEB_PLUGINS: readonly DshPluginInventoryEntry[] =
  OFFICIAL_WEB_PLUGIN_MANIFEST.trim()
    .split("\n")
    .map((line) => {
      const [entryId, moduleName, enabledValue] = line.split("|");
      const enabled = enabledValue === "1";
      return {
        entryId,
        moduleName,
        enabled,
        fiberPhase: enabled ? "active" : null,
        builtIn: true,
        version: "bundled",
      };
    });

export function mergeDshPluginInventory(
  loaded: readonly DshPluginInventoryEntry[],
): DshPluginInventoryEntry[] {
  const byId = new Map(OFFICIAL_DSH_WEB_PLUGINS.map((entry) => [entry.entryId, { ...entry }]));
  for (const entry of loaded) {
    byId.set(entry.entryId, { ...byId.get(entry.entryId), ...entry });
  }
  return [...byId.values()];
}
