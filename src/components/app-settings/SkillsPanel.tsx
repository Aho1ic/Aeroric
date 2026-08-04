import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen, RotateCcw } from "lucide-react";
import { useI18n } from "../../i18n";
import type { Project, SkillHubConfig, SetSkillHubResult } from "../../types";
import { SKILL_HUB_CHANGED_EVENT } from "./types";
import { SkillHubView } from "../skill-hub/SkillHubView";
import s from "../../styles";

export function SkillsPanel() {
  const { t } = useI18n();
  const [config, setConfig] = useState<SkillHubConfig | null>(null);
  const [allProjects, setAllProjects] = useState<Project[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadData = useCallback(() => {
    Promise.all([
      invoke<SkillHubConfig>("get_skill_hub_config"),
      invoke<Project[]>("load_projects"),
    ])
      .then(([cfg, projects]) => {
        setConfig(cfg ?? null);
        setAllProjects(projects);
      })
      .catch(console.error);
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  useEffect(() => {
    const refresh = () => loadData();
    window.addEventListener(SKILL_HUB_CHANGED_EVENT, refresh);
    return () => window.removeEventListener(SKILL_HUB_CHANGED_EVENT, refresh);
  }, [loadData]);

  const handlePick = useCallback(async () => {
    setError(null);
    const selected = await openDialog({ directory: true, multiple: false });
    if (!selected) return;
    setBusy(true);
    try {
      const result = await invoke<SetSkillHubResult>("set_skill_hub_path", {
        path: selected as string,
      });
      setConfig(result.config);
      setAllProjects(result.projects as Project[]);
      window.dispatchEvent(
        new CustomEvent(SKILL_HUB_CHANGED_EVENT, {
          detail: { projects: result.projects },
        }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const handleClear = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("clear_skill_hub");
      setConfig(null);
      window.dispatchEvent(new CustomEvent(SKILL_HUB_CHANGED_EVENT));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const hubPath = config?.hubPath ?? "";

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      <div style={s.skillsPanelBody}>
        <div style={s.skillsPanelField}>
          <label style={s.skillsPanelLabel}>{t("skill.settings.hubPath")}</label>
          <div style={s.skillsPanelPathRow}>
            <div style={s.skillsPanelPathBox}>
              {hubPath ? (
                <span style={s.skillsPanelPathText}>{hubPath}</span>
              ) : (
                <span style={s.skillsPanelPathEmpty}>{t("skill.settings.notConfigured")}</span>
              )}
            </div>
            <button type="button" style={s.skillsPanelPickBtn} onClick={handlePick} disabled={busy}>
              <FolderOpen size={13} strokeWidth={2} />
              {t("skill.settings.choose")}
            </button>
            {hubPath ? (
              <button
                type="button"
                style={s.skillsPanelClearBtn}
                onClick={handleClear}
                disabled={busy}
                title={t("skill.settings.reset")}
              >
                <RotateCcw size={13} strokeWidth={2} />
              </button>
            ) : null}
          </div>
          <span style={s.skillsPanelHint}>{t("skill.settings.hubPathHint")}</span>
        </div>
        {error ? <div style={s.skillsPanelError}>{error}</div> : null}
      </div>

      <div style={s.skillsPanelContent}>
        <SkillHubView
          config={config}
          allProjects={allProjects}
          onOpenAppSettings={() => {}}
          embedded
        />
      </div>
    </div>
  );
}
