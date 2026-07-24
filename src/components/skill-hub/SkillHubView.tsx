import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Settings as SettingsIcon,
  Blocks,
  ExternalLink,
  AlertCircle,
  Trash2,
  Search,
  FolderInput,
  ShoppingBag,
} from "lucide-react";
import type {
  Project,
  Skill,
  SkillDeleteResult,
  SkillHubConfig,
  SkillInstallation,
} from "../../types";
import { useI18n } from "../../i18n";
import { SKILL_HUB_CHANGED_EVENT } from "../app-settings/types";
import { shortenPath } from "../../utils";
import { SkillManageDialog } from "./SkillManageDialog";
import s from "../../styles";

interface Props {
  config: SkillHubConfig | null;
  allProjects: Project[];
  onEnterSkillHub: () => void;
  onOpenAppSettings: () => void;
}

export function SkillHubView({ config, allProjects, onEnterSkillHub, onOpenAppSettings }: Props) {
  const { t } = useI18n();
  const [skills, setSkills] = useState<Skill[]>([]);
  const [installations, setInstallations] = useState<SkillInstallation[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [managedSkill, setManagedSkill] = useState<Skill | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [activeTab, setActiveTab] = useState<"installed" | "shop" | "local">("installed");
  const [skillSearch, setSkillSearch] = useState("");
  const [importMessage, setImportMessage] = useState<string | null>(null);

  const loadSkills = useCallback(() => {
    if (!config?.hubPath) {
      setSkills([]);
      return;
    }
    setLoading(true);
    setError(null);
    Promise.all([
      invoke<Skill[]>("list_skills"),
      invoke<SkillInstallation[]>("list_skill_installations", { skillName: null }),
    ])
      .then(([rows, installs]) => {
        setSkills(rows);
        setInstallations(installs);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [config?.hubPath]);

  useEffect(() => {
    loadSkills();
  }, [loadSkills, refreshKey]);

  useEffect(() => {
    const refresh = () => setRefreshKey((k) => k + 1);
    window.addEventListener(SKILL_HUB_CHANGED_EVENT, refresh);
    return () => window.removeEventListener(SKILL_HUB_CHANGED_EVENT, refresh);
  }, []);

  const installedProjectCounts = useMemo(() => {
    const grouped = new Map<string, Set<string>>();
    installations.forEach((ins) => {
      if (!grouped.has(ins.skillName)) grouped.set(ins.skillName, new Set());
      grouped.get(ins.skillName)!.add(ins.projectId);
    });
    const counts = new Map<string, number>();
    grouped.forEach((projectIds, skillName) => counts.set(skillName, projectIds.size));
    return counts;
  }, [installations]);

  const filteredSkills = useMemo(() => {
    if (!skillSearch.trim()) return skills;
    const q = skillSearch.trim().toLowerCase();
    return skills.filter(
      (sk) =>
        sk.name.toLowerCase().includes(q) ||
        (sk.displayName && sk.displayName.toLowerCase().includes(q)),
    );
  }, [skills, skillSearch]);

  const handleImportLocal = useCallback(async () => {
    const selected = await openDialog({
      title: t("skill.tab.importFromLocal"),
      multiple: false,
      directory: true,
    });
    if (!selected || Array.isArray(selected)) return;
    setError(null);
    setImportMessage(null);
    try {
      const name = await invoke<string>("import_local_skill", { sourcePath: selected });
      setImportMessage(t("skill.tab.importSuccess", { name }));
      setRefreshKey((k) => k + 1);
      window.dispatchEvent(new CustomEvent(SKILL_HUB_CHANGED_EVENT));
      setActiveTab("installed");
    } catch (e) {
      setError(String(e));
    }
  }, [t]);

  const handleDeleteSkill = useCallback(
    async (skill: Skill) => {
      const name = skill.displayName || skill.name;
      const ok = await confirm(t("skill.delete.prompt", { name }), {
        title: t("skill.delete.title", { name }),
        kind: "warning",
        okLabel: t("skill.delete.confirm"),
        cancelLabel: t("skill.delete.cancel"),
      });
      if (!ok) return;

      try {
        await invoke<SkillDeleteResult>("delete_skill", {
          skillName: skill.name,
          skillPath: skill.path,
        });
        setManagedSkill((current) => (current?.name === skill.name ? null : current));
        setRefreshKey((k) => k + 1);
        window.dispatchEvent(new CustomEvent(SKILL_HUB_CHANGED_EVENT));
      } catch (e) {
        setError(String(e));
      }
    },
    [t],
  );

  if (!config?.hubPath) {
    return (
      <div style={s.skillHubBody}>
        <div style={s.skillHubEmpty}>
          <Blocks size={36} strokeWidth={1.2} color="var(--text-hint)" />
          <div style={s.skillHubEmptyTitle}>{t("skill.empty.title")}</div>
          <div style={s.skillHubEmptyHint}>{t("skill.empty.hint")}</div>
          <button type="button" style={s.skillHubEmptyBtn} onClick={onOpenAppSettings}>
            <SettingsIcon size={13} strokeWidth={2} />
            {t("skill.empty.openSettings")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div style={s.skillHubBody}>
      <div style={s.skillHubHeader}>
        <div style={s.skillHubHeaderMain}>
          <div style={s.skillHubHeaderTitle}>{t("skill.header.title")}</div>
          <div style={s.skillHubHeaderPath} title={config.hubPath}>
            {shortenPath(config.hubPath)}
          </div>
        </div>
        {config.hubProjectId ? (
          <button
            type="button"
            style={s.skillHubHeaderBtn}
            onClick={onEnterSkillHub}
            title={t("skill.header.openInTaskView")}
          >
            <ExternalLink size={13} strokeWidth={2} />
            <span>{t("skill.header.openInTaskView")}</span>
          </button>
        ) : null}
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          marginTop: 2,
          marginBottom: 10,
        }}
      >
        {(["installed", "shop", "local"] as const).map((tab) => (
          <button
            key={tab}
            type="button"
            onClick={() => setActiveTab(tab)}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "7px 14px",
              border: `1.5px solid ${activeTab === tab ? "var(--accent)" : "var(--border-medium)"}`,
              borderRadius: 8,
              background: activeTab === tab ? "var(--control-active-bg)" : "var(--bg-card)",
              color: activeTab === tab ? "var(--control-active-fg)" : "var(--text-secondary)",
              fontSize: 12.5,
              fontWeight: 600,
              cursor: "pointer",
            }}
          >
            {t(`skill.tab.${tab === "local" ? "localImport" : tab}`)}
          </button>
        ))}

        <div style={{ flex: 1 }} />

        {activeTab === "installed" && (
          <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
            <Search
              size={13}
              style={{
                position: "absolute",
                left: 8,
                color: "var(--text-hint)",
                pointerEvents: "none",
              }}
            />
            <input
              type="text"
              value={skillSearch}
              onChange={(e) => setSkillSearch(e.target.value)}
              placeholder={t("skill.search")}
              style={{
                width: 170,
                height: 28,
                paddingLeft: 28,
                paddingRight: 8,
                border: "1px solid var(--border-medium)",
                borderRadius: 6,
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                fontSize: 12,
                outline: "none",
              }}
            />
          </div>
        )}
      </div>

      {error ? (
        <div style={s.skillHubError}>
          <AlertCircle size={14} strokeWidth={2} />
          <span>{error}</span>
        </div>
      ) : null}

      {importMessage ? (
        <div
          style={{
            padding: "8px 12px",
            marginBottom: 10,
            color: "var(--success)",
            fontSize: 12,
          }}
        >
          {importMessage}
        </div>
      ) : null}

      {activeTab === "installed" && (
        <>
          <div style={s.skillHubMeta}>
            {loading
              ? t("skill.list.loading")
              : t("skill.list.count", { count: filteredSkills.length })}
          </div>
          <div style={s.skillHubList}>
            {filteredSkills.length === 0 && !loading ? (
              <div style={s.skillHubEmptyList}>{t("skill.list.empty")}</div>
            ) : (
              filteredSkills.map((skill) => (
                <SkillRow
                  key={skill.path}
                  skill={skill}
                  installedProjectCount={installedProjectCounts.get(skill.name) ?? 0}
                  onManage={() => setManagedSkill(skill)}
                  onDelete={() => handleDeleteSkill(skill)}
                />
              ))
            )}
          </div>
        </>
      )}

      {activeTab === "shop" && (
        <div style={s.skillHubEmpty}>
          <ShoppingBag size={36} strokeWidth={1.2} color="var(--text-hint)" />
          <div style={s.skillHubEmptyTitle}>{t("skill.tab.shopComingSoon")}</div>
          <div style={s.skillHubEmptyHint}>{t("skill.tab.shopHint")}</div>
        </div>
      )}

      {activeTab === "local" && (
        <div style={s.skillHubEmpty}>
          <FolderInput size={36} strokeWidth={1.2} color="var(--text-hint)" />
          <div style={s.skillHubEmptyTitle}>{t("skill.tab.importHint")}</div>
          <button type="button" style={s.skillHubEmptyBtn} onClick={() => void handleImportLocal()}>
            <FolderInput size={13} strokeWidth={2} />
            {t("skill.tab.importFromLocal")}
          </button>
        </div>
      )}

      {managedSkill ? (
        <SkillManageDialog
          skill={managedSkill}
          allProjects={allProjects.filter((p) => p.id !== config.hubProjectId)}
          onClose={() => setManagedSkill(null)}
          onChanged={() => setRefreshKey((k) => k + 1)}
        />
      ) : null}
    </div>
  );
}

function SkillRow({
  skill,
  installedProjectCount,
  onManage,
  onDelete,
}: {
  skill: Skill;
  installedProjectCount: number;
  onManage: () => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  const title = skill.displayName || skill.name;

  return (
    <div style={s.skillRow}>
      <div style={s.skillRowMain}>
        <div style={s.skillRowTitle}>
          <span>{title}</span>
          {skill.displayName && skill.displayName !== skill.name ? (
            <span style={s.skillRowDirName}>{skill.name}</span>
          ) : null}
        </div>
        {skill.description ? (
          <div style={s.skillRowDesc}>{skill.description}</div>
        ) : (
          <div style={s.skillRowDescEmpty}>{t("skill.row.noDescription")}</div>
        )}
        <div style={s.skillRowMeta}>
          {t("skill.row.installedProjects", { count: installedProjectCount })}
        </div>
        {skill.hasError ? (
          <div style={s.skillRowError}>
            <AlertCircle size={11} strokeWidth={2} />
            <span>{skill.hasError}</span>
          </div>
        ) : null}
      </div>
      <div style={s.skillRowActions}>
        <button type="button" style={s.skillRowManageBtn} onClick={onManage}>
          {t("skill.row.manage")}
        </button>
        <button
          type="button"
          style={s.skillRowDeleteBtn}
          onClick={onDelete}
          title={t("skill.row.delete")}
          aria-label={t("skill.row.delete")}
        >
          <Trash2 size={13} strokeWidth={1.9} />
        </button>
      </div>
    </div>
  );
}
