import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PromptSkill } from "../types";
import { useI18n } from "../i18n";
import { DSH_SLASH_COMMANDS, type DshSlashCommand } from "../dshSlashCommands";

/**
 * Popover that lists the built-in dsh slash commands (`/compact`, `/feedback`,
 * `/goal`, `/plan`, `/permission`, `/export`, `/model`, `/skill`, `/subagent`)
 * for a dsh-family agent. Selecting a command inserts ``/<name> `` into the
 * linked PromptEditor caret (reusing the editor's existing slash-insertion
 * path) so the prompt dispatches server-side via `session.prompt`.
 *
 * The popupSelect-style commands (`/model`, `/skill`, `/subagent`,
 * `/permission`) open a secondary picker and then insert ``/command <arg>``.
 */
export function DshSlashPalette({
  editorInsert,
  onDismiss,
  sessionId,
}: {
  /** Insert ``/<name> `` at the editor caret; returns false if caret not on a slash. */
  editorInsert: (name: string) => boolean;
  onDismiss: () => void;
  sessionId?: string;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [picker, setPicker] = useState<DshSlashCommand | null>(null);
  const [remoteCommands, setRemoteCommands] = useState<DshSlashCommand[] | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!sessionId) return;
    let disposed = false;
    void invoke<
      | Array<{ name?: string; input?: { hint?: string } }>
      | { commands?: Array<{ name?: string; input?: { hint?: string } }> }
    >("list_dsh_commands", { sessionId })
      .then((value) => {
        if (disposed) return;
        const rows = Array.isArray(value) ? value : (value.commands ?? []);
        const commands = rows
          .filter(
            (row): row is { name: string; input?: { hint?: string } } =>
              typeof row.name === "string",
          )
          .map((row) => {
            const known = DSH_SLASH_COMMANDS.find((command) => command.name === row.name);
            return (
              known ?? {
                name: row.name,
                descriptionKey: "dsh.slash.title",
                hasArg: Boolean(row.input?.hint),
              }
            );
          });
        if (commands.length > 0) setRemoteCommands(commands);
      })
      .catch(() => {
        // The static catalog remains available while an older DSH build lacks
        // the generated commands Remote.
      });
    return () => {
      disposed = true;
    };
  }, [sessionId]);

  const availableCommands = remoteCommands ?? DSH_SLASH_COMMANDS;

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return availableCommands.filter((c) => !q || c.name.startsWith(q));
  }, [availableCommands, query]);

  const commit = useCallback(
    (cmd: DshSlashCommand) => {
      if (cmd.popup) {
        setPicker(cmd);
        return;
      }
      const ok = editorInsert(cmd.name);
      if (ok) onDismiss();
    },
    [editorInsert, onDismiss],
  );

  if (picker) {
    return (
      <DshSlashPicker
        command={picker}
        onPick={(arg) => {
          // The editor insertion expects a bare name; the arg is appended as
          // plain text after the slash token by the editor's skill inserter
          // path. For popup commands we insert the name then type the arg.
          const ok = editorInsert(`${picker.name} ${arg}`.trim());
          if (ok) onDismiss();
        }}
        onBack={() => setPicker(null)}
        onDismiss={onDismiss}
      />
    );
  }

  return (
    <div
      role="listbox"
      aria-label={t("dsh.slash.title")}
      style={{
        position: "absolute",
        bottom: "100%",
        left: 0,
        marginBottom: 6,
        width: 280,
        maxHeight: 260,
        overflowY: "auto",
        background: "var(--bg-card)",
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        boxShadow: "0 8px 24px rgba(0,0,0,0.18)",
        zIndex: 30,
        padding: 4,
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          onDismiss();
        } else if (e.key === "ArrowDown") {
          e.preventDefault();
          setActive((i) => Math.min(i + 1, filtered.length - 1));
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          setActive((i) => Math.max(i - 1, 0));
        } else if (e.key === "Enter") {
          e.preventDefault();
          const cmd = filtered[active];
          if (cmd) commit(cmd);
        }
      }}
    >
      <input
        ref={inputRef}
        autoFocus
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setActive(0);
        }}
        placeholder={t("dsh.slash.title")}
        style={{
          width: "100%",
          boxSizing: "border-box",
          padding: "5px 8px",
          marginBottom: 4,
          background: "var(--bg-input)",
          border: "1px solid var(--border-dim)",
          borderRadius: 5,
          color: "var(--text-primary)",
          fontSize: 12,
          outline: "none",
        }}
      />
      {filtered.map((cmd, idx) => (
        <button
          key={cmd.name}
          type="button"
          role="option"
          aria-selected={idx === active}
          onMouseEnter={() => setActive(idx)}
          onClick={() => commit(cmd)}
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 1,
            width: "100%",
            textAlign: "left",
            padding: "5px 8px",
            background: idx === active ? "var(--bg-hover)" : "transparent",
            border: "none",
            borderRadius: 5,
            cursor: "pointer",
            color: "var(--text-primary)",
          }}
        >
          <span style={{ fontSize: 12.5, fontWeight: 600 }}>/&nbsp;{cmd.name}</span>
          <span style={{ fontSize: 11, color: "var(--text-hint)" }}>{t(cmd.descriptionKey)}</span>
        </button>
      ))}
      {filtered.length === 0 && (
        <div style={{ padding: "8px", fontSize: 12, color: "var(--text-hint)" }}>—</div>
      )}
    </div>
  );
}

/**
 * Secondary picker that resolves the argument of a popupSelect-style command
 * (`/model`, `/skill`, `/permission`, `/subagent`) from the matching catalog.
 */
export function DshSlashPicker({
  command,
  onPick,
  onBack,
  onDismiss,
  projectPath,
  keyboardTargetRef,
}: {
  command: DshSlashCommand;
  onPick: (arg: string) => void;
  onBack: () => void;
  onDismiss: () => void;
  projectPath?: string;
  keyboardTargetRef?: RefObject<HTMLElement | null>;
}) {
  const { t } = useI18n();
  const [items, setItems] = useState<{ value: string; label: string }[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState(0);

  // Resolve picker candidates from the appropriate dsh RPC.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        let rows: { value: string; label: string }[] = [];
        if (command.popup === "model") {
          const data = await invoke<{ groups: { models: { id: string; name?: string }[] }[] }>(
            "list_dsh_llm_models",
          ).catch(() => null);
          if (data?.groups) {
            for (const g of data.groups) {
              for (const m of g.models) rows.push({ value: m.id, label: m.name ?? m.id });
            }
          }
        } else if (command.popup === "skill") {
          // list_project_skills is the Aeroric-side skill catalog (works for
          // any agent); dsh-side skills surface via session.prompt /skill.
          const skills = await invoke<PromptSkill[]>("list_project_skills", {
            ...(projectPath ? { projectPath } : {}),
            agent: "dsh",
          }).catch(() => [] as PromptSkill[]);
          rows = (skills ?? []).map((s) => ({ value: s.name, label: s.name }));
        } else if (command.popup === "permission") {
          rows = [
            { value: "read-only", label: "read-only" },
            { value: "workspace-write", label: "workspace-write" },
            { value: "danger-full-access", label: "danger-full-access" },
          ];
        } else if (command.popup === "subagent") {
          // Subagent ids are session-scoped; without a live session we offer a
          // bare /subagent stub the user completes.
          rows = [];
        }
        if (!cancelled) {
          setItems(rows);
          setLoading(false);
        }
      } catch (e) {
        if (!cancelled) {
          setError(String(e));
          setLoading(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [command.popup, projectPath]);

  useEffect(() => {
    const handleEditorKeyDown = (event: KeyboardEvent) => {
      const activeElement = document.activeElement;
      const keyboardTarget = keyboardTargetRef?.current;
      if (
        event.isComposing ||
        !(activeElement instanceof HTMLElement) ||
        (keyboardTarget
          ? activeElement !== keyboardTarget
          : !activeElement.classList.contains("prompt-editor"))
      ) {
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopImmediatePropagation();
        onDismiss();
        return;
      }
      if (event.key === "ArrowDown" && items && items.length > 0) {
        event.preventDefault();
        event.stopImmediatePropagation();
        setActive((index) => Math.min(index + 1, items.length - 1));
        return;
      }
      if (event.key === "ArrowUp" && items && items.length > 0) {
        event.preventDefault();
        event.stopImmediatePropagation();
        setActive((index) => Math.max(index - 1, 0));
        return;
      }
      if ((event.key === "Enter" || event.key === "Tab") && items?.[active]) {
        event.preventDefault();
        event.stopImmediatePropagation();
        onPick(items[active].value);
      }
    };
    document.addEventListener("keydown", handleEditorKeyDown, true);
    return () => document.removeEventListener("keydown", handleEditorKeyDown, true);
  }, [active, items, keyboardTargetRef, onDismiss, onPick]);

  return (
    <div
      role="listbox"
      style={{
        position: "absolute",
        bottom: "100%",
        left: 0,
        marginBottom: 6,
        width: 280,
        maxHeight: 260,
        overflowY: "auto",
        background: "var(--bg-card)",
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        boxShadow: "0 8px 24px rgba(0,0,0,0.18)",
        zIndex: 30,
        padding: 4,
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          onBack();
        } else if (e.key === "ArrowDown" && items) {
          e.preventDefault();
          setActive((i) => Math.min(i + 1, items.length - 1));
        } else if (e.key === "ArrowUp" && items) {
          e.preventDefault();
          setActive((i) => Math.max(i - 1, 0));
        } else if (e.key === "Enter" && items && items[active]) {
          e.preventDefault();
          onPick(items[active].value);
        }
      }}
    >
      <div style={{ fontSize: 11, color: "var(--text-hint)", padding: "2px 6px 6px" }}>
        /&nbsp;{command.name} — {t(command.descriptionKey)}
      </div>
      {loading ? (
        <div style={{ padding: 8, fontSize: 12, color: "var(--text-hint)" }}>…</div>
      ) : error ? (
        <div style={{ padding: 8, fontSize: 12, color: "var(--danger)" }}>{error}</div>
      ) : items && items.length > 0 ? (
        items.map((row, idx) => (
          <button
            key={row.value}
            type="button"
            role="option"
            aria-selected={idx === active}
            onMouseEnter={() => setActive(idx)}
            onMouseDown={(event) => {
              event.preventDefault();
              onPick(row.value);
            }}
            style={{
              width: "100%",
              textAlign: "left",
              padding: "5px 8px",
              background: idx === active ? "var(--bg-hover)" : "transparent",
              border: "none",
              borderRadius: 5,
              cursor: "pointer",
              color: "var(--text-primary)",
              fontSize: 12.5,
            }}
          >
            {row.label}
          </button>
        ))
      ) : (
        <div style={{ padding: 8, fontSize: 12, color: "var(--text-hint)" }}>—</div>
      )}
      <button
        type="button"
        onMouseDown={(event) => {
          event.preventDefault();
          onBack();
        }}
        style={{
          width: "100%",
          textAlign: "left",
          padding: "4px 8px",
          marginTop: 2,
          background: "transparent",
          border: "none",
          color: "var(--text-hint)",
          cursor: "pointer",
          fontSize: 11,
        }}
      >
        ‹ {t("dsh.slash.title")}
      </button>
    </div>
  );
}
