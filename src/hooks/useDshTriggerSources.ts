import { useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import { DSH_SLASH_COMMANDS, type DshSlashCommand } from "../dshSlashCommands";
import { rankDshTriggerCandidates, type DshTriggerCandidate } from "../dshInputTriggers";
import type { DshTriggerSource } from "./useDshTriggerMenu";

/** One `list_dsh_commands` row (the Harness' `CommandDescriptor`). */
interface DshCommandRow {
  name?: unknown;
  description?: unknown;
  input?: { hint?: unknown } | null;
}

/** One `list_dsh_skills` row (`DshSkillEntry`). */
interface DshSkillRow {
  id?: unknown;
  name?: unknown;
  description?: unknown;
  whenToUse?: unknown;
  modelInvocable?: unknown;
}

/** One `list_dsh_subagents` row (`DshSubagentSummary`). */
interface DshSubagentRow {
  sessionId?: unknown;
  running?: unknown;
  mode?: unknown;
  label?: unknown;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value : undefined;
}

/** `command.list` answers either a bare array or `{ commands }`, depending on build. */
function commandRows(value: unknown): DshCommandRow[] {
  if (Array.isArray(value)) return value as DshCommandRow[];
  if (typeof value !== "object" || value === null) return [];
  const commands = (value as { commands?: unknown }).commands;
  return Array.isArray(commands) ? (commands as DshCommandRow[]) : [];
}

function rows<T>(value: unknown): T[] {
  return Array.isArray(value) ? (value as T[]) : [];
}

/**
 * The trigger sources of one DeepSeek Harness session: host commands and skills
 * under `/`, running subagents under `@` — the same roster the Harness' own
 * composer registers.
 *
 * Each catalog is fetched at most once per session and per menu opening; the
 * three filters deliberately differ, mirroring the sources they come from:
 * commands rank fuzzily (so `/cmp` still finds `compact`), skills match by name
 * prefix, and subagent labels match as a substring because they are free text.
 *
 * @param sessionId - the live session the catalogs belong to.
 * @param onPopupCommand - opens the argument picker of a popupSelect-style command.
 */
export function useDshTriggerSources(
  sessionId: string,
  onPopupCommand: (command: DshSlashCommand) => void,
): readonly DshTriggerSource[] {
  const { t } = useI18n();

  const sources = useMemo<readonly DshTriggerSource[]>(() => {
    // No session, no catalogs: an empty roster keeps the menu shut instead of
    // firing session-scoped commands with an empty id.
    if (!sessionId) return [];
    // The cache is closed over by this roster, so a session switch (or a
    // language switch, which rewrites every description) starts a fresh one.
    const cache = new Map<string, Promise<readonly DshTriggerCandidate[]>>();
    const load = (
      key: string,
      fetcher: () => Promise<readonly DshTriggerCandidate[]>,
    ): Promise<readonly DshTriggerCandidate[]> => {
      const cached = cache.get(key);
      if (cached) return cached;
      const pending = fetcher();
      // A failed catalog must not be cached, or the next menu opening could
      // never get past it.
      pending.catch(() => {
        if (cache.get(key) === pending) cache.delete(key);
      });
      cache.set(key, pending);
      return pending;
    };

    const staticCommands = (): DshTriggerCandidate[] =>
      DSH_SLASH_COMMANDS.map((command) => ({
        name: command.name,
        description: t(command.descriptionKey),
      }));

    const fetchCommands = async (): Promise<readonly DshTriggerCandidate[]> => {
      let value: unknown = null;
      try {
        value = await invoke<unknown>("list_dsh_commands", { sessionId });
      } catch {
        // An older Harness build without the generated commands Remote falls
        // through to the built-in catalog below rather than dropping the group.
      }
      const remote = commandRows(value).flatMap((row) => {
        const name = text(row.name);
        if (name === undefined) return [];
        const known = DSH_SLASH_COMMANDS.find((command) => command.name === name);
        const description = text(row.description) ?? (known ? t(known.descriptionKey) : undefined);
        const hint = text(row.input?.hint);
        return [{ name, ...(description ? { description } : {}), ...(hint ? { hint } : {}) }];
      });
      return remote.length > 0 ? remote : staticCommands();
    };

    const fetchSkills = async (): Promise<readonly DshTriggerCandidate[]> => {
      const value = await invoke<unknown>("list_dsh_skills", { sessionId });
      return rows<DshSkillRow>(value).flatMap((row) => {
        const name = text(row.name) ?? text(row.id);
        if (name === undefined) return [];
        const summary = text(row.description) ?? text(row.whenToUse);
        // The user-only marker rides the description, the menu's only secondary
        // text, rather than a badge of its own.
        const description =
          row.modelInvocable === false
            ? `${t("dsh.trigger.userOnly")}${summary ? ` · ${summary}` : ""}`
            : summary;
        return [{ name, ...(description ? { description } : {}) }];
      });
    };

    const fetchSubagents = async (): Promise<readonly DshTriggerCandidate[]> => {
      const value = await invoke<unknown>("list_dsh_subagents", { sessionId });
      return rows<DshSubagentRow>(value).flatMap((row) => {
        if (row.running !== true) return [];
        const name = text(row.label) ?? text(row.sessionId);
        if (name === undefined) return [];
        const mode = text(row.mode);
        return [{ name, ...(mode ? { description: mode } : {}) }];
      });
    };

    return [
      {
        trigger: "/",
        name: "command",
        labelKey: "dsh.trigger.group.command",
        order: 0,
        async candidates({ query }) {
          return rankDshTriggerCandidates(await load("commands", fetchCommands), query);
        },
        onPick({ candidate }) {
          const known = DSH_SLASH_COMMANDS.find((command) => command.name === candidate.name);
          // A popupSelect-style command takes a chosen argument: the token lands
          // now and the picker appends the argument at the caret.
          if (known?.popup) onPopupCommand(known);
          return { text: `/${candidate.name} ` };
        },
        warm() {
          void load("commands", fetchCommands);
        },
      },
      {
        trigger: "/",
        name: "skill",
        labelKey: "dsh.trigger.group.skill",
        order: 2,
        async candidates({ query }) {
          const skills = await load("skills", fetchSkills);
          return skills.filter((skill) => skill.name.startsWith(query));
        },
        onPick({ candidate }) {
          // Plain-text reference: the literal lands in the draft and ships to
          // the model verbatim; the Harness recognizes the leading `/name` and
          // injects the rendered skill body itself.
          return { text: `/${candidate.name} ` };
        },
        warm() {
          void load("skills", fetchSkills);
        },
      },
      {
        trigger: "@",
        name: "subagent",
        labelKey: "dsh.trigger.group.subagent",
        async candidates({ query }) {
          const children = await load("subagents", fetchSubagents);
          const needle = query.toLowerCase();
          return children.filter((child) => child.name.toLowerCase().includes(needle));
        },
        onPick({ candidate }) {
          return { text: `@${candidate.name} ` };
        },
        warm() {
          // Children spawn and exit mid-session, so the roster is re-pulled
          // every time the menu opens instead of cached for the session.
          cache.delete("subagents");
        },
      },
    ];
  }, [onPopupCommand, sessionId, t]);

  useEffect(() => {
    // Scope-birth prewarm: the stable catalogs are fetched before the first
    // keystroke so the first menu opening shows rows, not a loading row.
    for (const source of sources) source.warm?.();
  }, [sources]);

  return sources;
}
