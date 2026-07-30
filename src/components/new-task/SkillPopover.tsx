import { Sparkles } from "lucide-react";
import type { PromptSkill } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";

export function SkillPopover({
  skillSearch,
  skills,
  skillIndex,
  loading,
  commandPrefix,
  onSelectSkill,
  onSetSkillIndex,
}: {
  skillSearch: string;
  skills: PromptSkill[];
  skillIndex: number;
  loading: boolean;
  commandPrefix: "/" | "$";
  onSelectSkill: (skill: PromptSkill) => void;
  onSetSkillIndex: (index: number) => void;
}) {
  const { t } = useI18n();

  return (
    <div style={s.mentionDropdown} role="listbox" aria-label={t("skillPrompt.title")}>
      <div style={s.mentionSeparator}>
        <Sparkles size={11} />
        <span>{t("skillPrompt.title")}</span>
      </div>
      {loading ? (
        <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-hint)" }}>
          {t("skillPrompt.loading")}
        </div>
      ) : skills.length === 0 ? (
        <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-hint)" }}>
          {skillSearch
            ? t("skillPrompt.noResults", { query: skillSearch })
            : t("skillPrompt.empty")}
        </div>
      ) : (
        skills.map((skill, index) => (
          <button
            key={`${skill.name}:${skill.path}`}
            type="button"
            role="option"
            aria-selected={index === skillIndex}
            style={{
              ...s.mentionOption,
              width: "100%",
              border: "none",
              textAlign: "left",
              color: "inherit",
              background: index === skillIndex ? "var(--accent-subtle)" : "transparent",
            }}
            onMouseDown={(event) => {
              event.preventDefault();
              onSelectSkill(skill);
            }}
            onMouseEnter={() => onSetSkillIndex(index)}
          >
            <span style={{ color: "var(--accent)", flexShrink: 0, display: "flex" }}>
              <Sparkles size={12} />
            </span>
            <span style={{ ...s.mentionOptionName, fontFamily: "var(--font-mono)" }}>
              {commandPrefix}
              {skill.name}
            </span>
            {skill.description && (
              <span style={s.mentionOptionDir}>{skill.description.split("\n")[0]}</span>
            )}
          </button>
        ))
      )}
    </div>
  );
}
