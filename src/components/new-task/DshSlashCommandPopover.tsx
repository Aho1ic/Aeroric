import { SlashSquare } from "lucide-react";
import type { DshSlashCommand } from "../../dshSlashCommands";
import { useI18n } from "../../i18n";
import s from "../../styles";

export function DshSlashCommandPopover({
  commands,
  commandIndex,
  query,
  onSelectCommand,
  onSetCommandIndex,
}: {
  commands: readonly DshSlashCommand[];
  commandIndex: number;
  query: string;
  onSelectCommand: (command: DshSlashCommand) => void;
  onSetCommandIndex: (index: number) => void;
}) {
  const { t } = useI18n();

  return (
    <div style={s.mentionDropdown} role="listbox" aria-label={t("dsh.slash.title")}>
      <div style={s.mentionSeparator}>
        <SlashSquare size={11} />
        <span>{t("dsh.slash.title")}</span>
      </div>
      {commands.length === 0 ? (
        <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-hint)" }}>
          {query ? t("skillPrompt.noResults", { query }) : t("skillPrompt.empty")}
        </div>
      ) : (
        commands.map((command, index) => (
          <button
            key={command.name}
            type="button"
            role="option"
            aria-selected={index === commandIndex}
            style={{
              ...s.mentionOption,
              width: "100%",
              border: "none",
              textAlign: "left",
              color: "inherit",
              background: index === commandIndex ? "var(--accent-subtle)" : "transparent",
            }}
            onMouseDown={(event) => {
              event.preventDefault();
              onSelectCommand(command);
            }}
            onMouseEnter={() => onSetCommandIndex(index)}
          >
            <span style={{ color: "var(--usage-dsh)", flexShrink: 0, display: "flex" }}>
              <SlashSquare size={12} />
            </span>
            <span style={{ ...s.mentionOptionName, fontFamily: "var(--font-mono)" }}>
              /{command.name}
            </span>
            <span style={s.mentionOptionDir}>{t(command.descriptionKey)}</span>
          </button>
        ))
      )}
    </div>
  );
}
