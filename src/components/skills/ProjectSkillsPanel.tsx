import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, Sparkles } from "lucide-react";
import { useI18n } from "../../i18n";
import { SKILL_HUB_CHANGED_EVENT } from "../app-settings/types";
import type { PromptSkill } from "../../types";

interface ProjectSkillsPanelProps {
  projectPath: string;
  width: number;
}

export function ProjectSkillsPanel({ projectPath, width }: ProjectSkillsPanelProps) {
  const { t } = useI18n();
  const [skills, setSkills] = useState<PromptSkill[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadSkills = useCallback(async () => {
    if (!projectPath) {
      setSkills([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<PromptSkill[]>("list_project_skills", {
        projectPath,
        agent: "all",
        projectOnly: true,
      });
      setSkills(Array.isArray(result) ? result : []);
    } catch (reason) {
      setSkills([]);
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }, [projectPath]);

  useEffect(() => {
    void loadSkills();
    const refresh = () => void loadSkills();
    window.addEventListener(SKILL_HUB_CHANGED_EVENT, refresh);
    return () => window.removeEventListener(SKILL_HUB_CHANGED_EVENT, refresh);
  }, [loadSkills]);

  return (
    <section
      aria-label={t("skills.installedSkills")}
      aria-busy={loading}
      style={{
        width,
        height: "100%",
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        background: "var(--bg-panel)",
      }}
    >
      <header
        style={{
          minHeight: 40,
          padding: "0 12px",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 8,
          borderBottom: "1px solid var(--border-dim)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
          <Sparkles size={14} aria-hidden="true" />
          <strong style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {t("skills.installedSkills")}
          </strong>
        </div>
        <button
          type="button"
          onClick={() => void loadSkills()}
          disabled={loading}
          title={t("common.refresh")}
          aria-label={t("common.refresh")}
          style={{
            width: 28,
            height: 28,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            border: "1px solid var(--border-dim)",
            borderRadius: 6,
            background: "var(--bg-control)",
            color: "var(--text-secondary)",
            cursor: loading ? "default" : "pointer",
            opacity: loading ? 0.6 : 1,
          }}
        >
          <RefreshCw size={13} className={loading ? "aeroric-spin" : undefined} />
        </button>
      </header>

      <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 12 }}>
        {error && (
          <div style={{ color: "var(--danger)", fontSize: 12, marginBottom: 10 }}>{error}</div>
        )}
        {loading && skills.length === 0 ? (
          <div style={{ color: "var(--text-muted)", fontSize: 12 }}>{t("common.loading")}</div>
        ) : skills.length === 0 ? (
          <div style={{ color: "var(--text-muted)", fontSize: 12 }}>
            {t("skills.noSkillsInProject")}
          </div>
        ) : (
          <div style={{ display: "grid", gap: 8 }}>
            {skills.map((skill) => (
              <article
                key={`${skill.name}:${skill.path}`}
                style={{
                  padding: "9px 10px",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 8,
                  background: "var(--bg-sidebar)",
                }}
              >
                <div style={{ fontSize: 12, fontWeight: 700, color: "var(--text-primary)" }}>
                  /{skill.name}
                </div>
                {skill.description && (
                  <div
                    style={{
                      marginTop: 4,
                      color: "var(--text-secondary)",
                      fontSize: 11,
                      lineHeight: 1.45,
                      whiteSpace: "pre-wrap",
                    }}
                  >
                    {skill.description}
                  </div>
                )}
                <div
                  style={{
                    marginTop: 6,
                    color: "var(--text-hint)",
                    fontSize: 10,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={skill.path}
                >
                  {skill.path}
                </div>
              </article>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
