import type { AgentType } from "./types";
import { agentFamily } from "./agents";

/**
 * Built-in dsh slash commands a user can type at the start of a prompt. dsh's
 * `session.prompt` recognizes a leading `/command` text and dispatches it to
 * the matching `command-*` plugin (`command-compact`, `command-feedback`,
 * `command-goal`, `plan-mode`, `permission-presets`, `session-log-export`),
 * returning a `command` slot in the response. The agent-side plugins do the
 * real work; Aeroric only needs to send the text and, for the popupSelect-style
 * commands, expand the chosen argument.
 *
 * These are visible only for dsh-family agents (the command-* plugins are
 * composition rows the dsh web profile mounts). For non-dsh agents the caller
 * should not offer them.
 */
export interface DshSlashCommand {
  /** Bare command name without the leading slash. */
  name: string;
  /** One-line description (i18n key under `dsh.slash.*`). */
  descriptionKey: string;
  /** Whether the command takes a free-text argument after the name. */
  hasArg: boolean;
  /** popupSelect-style: caller resolves the argument from a picker. */
  popup?: "model" | "skill" | "subagent" | "permission";
}

export const DSH_SLASH_COMMANDS: readonly DshSlashCommand[] = [
  { name: "compact", descriptionKey: "dsh.slash.compact", hasArg: false },
  { name: "feedback", descriptionKey: "dsh.slash.feedback", hasArg: true },
  { name: "goal", descriptionKey: "dsh.slash.goal", hasArg: true },
  { name: "plan", descriptionKey: "dsh.slash.plan", hasArg: true },
  { name: "permission", descriptionKey: "dsh.slash.permission", hasArg: true, popup: "permission" },
  { name: "export", descriptionKey: "dsh.slash.export", hasArg: false },
  { name: "model", descriptionKey: "dsh.slash.model", hasArg: true, popup: "model" },
  { name: "skill", descriptionKey: "dsh.slash.skill", hasArg: true, popup: "skill" },
  { name: "subagent", descriptionKey: "dsh.slash.subagent", hasArg: true, popup: "subagent" },
];

/**
 * Return the leading dsh slash command a prompt text resolves to, or null.
 * Mirrors dsh's own detection: only a prompt that STARTS with `/command`
 * (optional single arg) is treated as a command — mid-prompt slashes are not.
 */
export function detectDshSlashCommand(prompt: string): {
  command: DshSlashCommand;
  arg: string;
} | null {
  const trimmed = prompt.trimStart();
  if (!trimmed.startsWith("/")) return null;
  const rest = trimmed.slice(1);
  const spaceIdx = rest.search(/\s/);
  const name = spaceIdx === -1 ? rest : rest.slice(0, spaceIdx);
  const arg = spaceIdx === -1 ? "" : rest.slice(spaceIdx + 1).trim();
  const match = DSH_SLASH_COMMANDS.find((c) => c.name === name);
  if (!match) return null;
  return { command: match, arg };
}

/** Whether the slash-command palette should be offered for this agent. */
export function dshSlashCommandsAvailable(agent: AgentType | undefined): boolean {
  return agent ? agentFamily(agent) === "dsh" : false;
}
