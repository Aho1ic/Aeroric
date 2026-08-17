import { useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Bot,
  Check,
  ChevronDown,
  Code2,
  Copy,
  Feather,
  FileText,
  Globe2,
  Package,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  TerminalSquare,
  Trash2,
  Workflow,
} from "lucide-react";
import {
  mergeDshPluginInventory,
  OFFICIAL_DSH_WEB_PLUGINS,
  type DshFiberPhase,
  type DshPluginInventoryEntry,
} from "../../dshOfficialDefaults";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { AnimatedSelectionGroup } from "../ui/AnimatedSelection";
import { START_DSH_CREATOR_DRAFT_EVENT } from "./types";
import "./DshPluginsPanel.css";

type PrimaryView = "plugins" | "presets";
type PluginView = "configuration" | "inventory";
type ConfigSection = "shell" | "agent-loop" | "web-search-deepseek";

interface DshPluginWireEntry {
  name?: string;
  version?: string;
  enabled?: boolean;
  description?: string;
  entryId?: string;
  moduleName?: string;
  fiberPhase?: DshFiberPhase;
  builtIn?: boolean;
}

interface DshSettingsSnapshot {
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

interface DshAgentPreset {
  id: string;
  name?: string;
  description?: string;
}

interface DshPresetWire {
  id: string;
  trust: "system" | "user";
  isDefault: boolean;
  name?: string;
  description?: string;
  broken?: string;
}

function errorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return String(error || "Unknown error");
}

const DEFAULT_SETTINGS: DshSettingsSnapshot = {
  shell: { timeoutMs: 60_000, maxOutputBytes: 64_000 },
  agentLoop: { maxParallelToolCalls: 10 },
  webSearch: { baseUrl: "", maxUses: 5, apiKeyConfigured: false },
  defaultPreset: "standard",
  customPresets: [],
};

const BUILT_IN_PRESETS = [
  {
    id: "standard",
    icon: Bot,
    nameKey: "PresetStandardName",
    descriptionKey: "PresetStandardDescription",
  },
  { id: "code", icon: Code2, nameKey: "PresetCodeName", descriptionKey: "PresetCodeDescription" },
  {
    id: "minimal",
    icon: Feather,
    nameKey: "PresetMinimalName",
    descriptionKey: "PresetMinimalDescription",
  },
  {
    id: "cordis",
    icon: Sparkles,
    nameKey: "PresetCordisName",
    descriptionKey: "PresetCordisDescription",
  },
] as const;

const officialIds = new Set(OFFICIAL_DSH_WEB_PLUGINS.map((entry) => entry.entryId));

function moduleShortName(moduleName: string): string {
  const unscoped = moduleName.startsWith("@")
    ? moduleName.slice(moduleName.indexOf("/") + 1)
    : moduleName;
  return unscoped
    .replace(/^cordis:/, "")
    .replace(/^cordis-plugin-/, "")
    .replace(/^dsh-(?:host-|client-)?/, "");
}

function fallbackEntryId(moduleName: string): string {
  return moduleShortName(moduleName).replace(/^plugin-/, "");
}

function normalizeInventory(entries: readonly DshPluginWireEntry[]): DshPluginInventoryEntry[] {
  return entries.flatMap((entry) => {
    const moduleName = entry.moduleName ?? entry.name;
    if (!moduleName) return [];
    const entryId = entry.entryId ?? fallbackEntryId(moduleName);
    const enabled = entry.enabled ?? true;
    return [
      {
        entryId,
        moduleName,
        enabled,
        fiberPhase: entry.fiberPhase ?? (enabled ? "active" : null),
        builtIn: entry.builtIn ?? officialIds.has(entryId),
        version: entry.version,
      },
    ];
  });
}

function positiveInteger(value: string, fallback: number): number {
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function SectionHeading({ title, intro }: { title: string; intro: string }) {
  return (
    <header className="dsh-section-heading">
      <h2>{title}</h2>
      <p>{intro}</p>
    </header>
  );
}

export function DshPluginsPanel() {
  const { t } = useI18n();
  const [primaryView, setPrimaryView] = useState<PrimaryView>("plugins");
  const [settings, setSettings] = useState<DshSettingsSnapshot>(DEFAULT_SETTINGS);
  const [openingConfig, setOpeningConfig] = useState(false);
  const [configOpenError, setConfigOpenError] = useState(false);
  const [presetRuntimeError, setPresetRuntimeError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    void invoke<DshSettingsSnapshot>("get_dsh_settings_snapshot", { agent: "dsh" }).then(
      (snapshot) => {
        if (current) setSettings({ ...DEFAULT_SETTINGS, ...snapshot });
      },
      () => {
        // The official roster remains useful before DSH is installed or in browser preview mode.
      },
    );
    void invoke<DshPresetWire[]>("list_dsh_agent_presets").then((presets) => {
      if (!current) return;
      setPresetRuntimeError(null);
      if (!presets.length) return;
      const defaultPreset = presets.find((preset) => preset.isDefault)?.id;
      setSettings((previous) => ({
        ...previous,
        ...(defaultPreset ? { defaultPreset } : {}),
        customPresets: presets
          .filter((preset) => preset.trust === "user")
          .map((preset) => ({ id: preset.id, name: preset.name, description: preset.description })),
      }));
    }).catch((error: unknown) => {
      if (current) setPresetRuntimeError(errorMessage(error));
      // The local YAML snapshot remains available before a Web profile can boot.
    });
    return () => {
      current = false;
    };
  }, []);

  async function openConfigFile() {
    if (openingConfig) return;
    setOpeningConfig(true);
    setConfigOpenError(false);
    try {
      await invoke("open_dsh_config_file", { agent: "dsh" });
    } catch {
      setConfigOpenError(true);
    } finally {
      setOpeningConfig(false);
    }
  }

  return (
    <div className="dsh-settings-panel">
      <div className="dsh-settings-toolbar">
        <span />
        <Button
          variant="outline"
          size="sm"
          icon={FileText}
          disabled={openingConfig}
          onClick={() => void openConfigFile()}
        >
          {t(openingConfig ? "appSettings.dshOpeningConfigFile" : "appSettings.dshOpenConfigFile")}
        </Button>
      </div>
      {configOpenError ? (
        <p className="dsh-toolbar-error" role="status">
          {t("appSettings.dshOpenConfigFileFailed")}
        </p>
      ) : null}
      <AnimatedSelectionGroup
        value={primaryView}
        onChange={setPrimaryView}
        ariaLabel={t("appSettings.dshPrimaryViews")}
        role="tablist"
        equalWidth
        className="dsh-primary-tabs"
        options={[
          {
            value: "plugins",
            label: (
              <>
                <Package size={14} />
                {t("appSettings.dshPluginsTab")}
              </>
            ),
          },
          {
            value: "presets",
            label: (
              <>
                <Bot size={14} />
                {t("appSettings.dshAgentPresetsTab")}
              </>
            ),
          },
        ]}
      />

      {primaryView === "plugins" ? (
        <PluginsView settings={settings} onSettingsChange={setSettings} />
      ) : (
        <AgentPresetsView
          settings={settings}
          onSettingsChange={setSettings}
          runtimeError={presetRuntimeError}
          onRuntimeError={setPresetRuntimeError}
        />
      )}
    </div>
  );
}

function PluginsView({
  settings,
  onSettingsChange,
}: {
  settings: DshSettingsSnapshot;
  onSettingsChange: (settings: DshSettingsSnapshot) => void;
}) {
  const { t } = useI18n();
  const [view, setView] = useState<PluginView>("configuration");

  return (
    <section className="dsh-page" aria-label={t("appSettings.dshPluginsTitle")}>
      <SectionHeading
        title={t("appSettings.dshPluginsTitle")}
        intro={t("appSettings.dshPluginsIntro")}
      />
      <AnimatedSelectionGroup
        value={view}
        onChange={setView}
        ariaLabel={t("appSettings.dshPluginViews")}
        role="tablist"
        variant="underline"
        className="dsh-secondary-tabs"
        options={[
          { value: "configuration", label: t("appSettings.dshPluginConfiguration") },
          { value: "inventory", label: t("appSettings.dshPluginList") },
        ]}
      />
      <div className="dsh-tab-panel" role="tabpanel">
        {view === "configuration" ? (
          <PluginConfiguration settings={settings} onSettingsChange={onSettingsChange} />
        ) : (
          <PluginInventory />
        )}
      </div>
    </section>
  );
}

interface PluginConfigCardProps {
  section: ConfigSection;
  icon: typeof TerminalSquare;
  title: string;
  description: string;
  dirty: boolean;
  saving: boolean;
  failed: boolean;
  onSave: () => void;
  onDiscard: () => void;
  children: ReactNode;
}

function PluginConfigCard({
  icon: Icon,
  title,
  description,
  dirty,
  saving,
  failed,
  onSave,
  onDiscard,
  children,
}: PluginConfigCardProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <li className={`dsh-config-card${open ? " dsh-config-card--open" : ""}`}>
      <button
        type="button"
        className="dsh-config-card__header"
        aria-expanded={open}
        aria-label={`${t(open ? "appSettings.dshCollapse" : "appSettings.dshExpand")}: ${title}`}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="dsh-config-card__icon">
          <Icon size={17} />
        </span>
        <span className="dsh-config-card__copy">
          <strong>{title}</strong>
          <span>{description}</span>
        </span>
        {dirty ? <span className="dsh-unsaved">{t("appSettings.dshUnsaved")}</span> : null}
        <ChevronDown className="dsh-chevron" size={15} aria-hidden />
      </button>
      {open ? (
        <div className="dsh-config-card__body">
          <div className="dsh-fields">{children}</div>
          <footer className="dsh-config-card__footer">
            {failed ? <span role="status">{t("appSettings.dshSaveFailed")}</span> : <span />}
            <Button variant="ghost" size="sm" disabled={!dirty || saving} onClick={onDiscard}>
              {t("appSettings.dshDiscard")}
            </Button>
            <Button size="sm" disabled={!dirty || saving} onClick={onSave}>
              {t(saving ? "appSettings.dshSaving" : "common.save")}
            </Button>
          </footer>
        </div>
      ) : null}
    </li>
  );
}

function ConfigField({
  id,
  label,
  hint,
  value,
  type = "text",
  placeholder,
  status,
  onChange,
}: {
  id: string;
  label: string;
  hint: string;
  value: string;
  type?: "text" | "number" | "password";
  placeholder?: string;
  status?: string;
  onChange: (value: string) => void;
}) {
  const descriptionIds = [status ? `${id}-status` : null, `${id}-hint`].filter(Boolean).join(" ");
  return (
    <div className="dsh-field">
      <label className="dsh-field__label" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        type={type}
        min={type === "number" ? 1 : undefined}
        value={value}
        placeholder={placeholder}
        autoComplete={type === "password" ? "off" : undefined}
        aria-describedby={descriptionIds}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
      {status ? (
        <span id={`${id}-status`} className="dsh-field__status">
          {status}
        </span>
      ) : null}
      <span id={`${id}-hint`} className="dsh-field__hint">
        {hint}
      </span>
    </div>
  );
}

function PluginConfiguration({
  settings,
  onSettingsChange,
}: {
  settings: DshSettingsSnapshot;
  onSettingsChange: (settings: DshSettingsSnapshot) => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(settings);
  const [saving, setSaving] = useState<ConfigSection | null>(null);
  const [failed, setFailed] = useState<ConfigSection | null>(null);
  const [apiKey, setApiKey] = useState("");

  useEffect(() => setDraft(settings), [settings]);

  const shellDirty =
    draft.shell.timeoutMs !== settings.shell.timeoutMs ||
    draft.shell.maxOutputBytes !== settings.shell.maxOutputBytes;
  const loopDirty =
    draft.agentLoop.maxParallelToolCalls !== settings.agentLoop.maxParallelToolCalls;
  const searchDirty =
    draft.webSearch.baseUrl !== settings.webSearch.baseUrl ||
    draft.webSearch.maxUses !== settings.webSearch.maxUses ||
    apiKey.trim().length > 0;

  async function save(section: ConfigSection) {
    setSaving(section);
    setFailed(null);
    const values =
      section === "shell"
        ? draft.shell
        : section === "agent-loop"
          ? draft.agentLoop
          : { ...draft.webSearch, apiKey: apiKey.trim() || undefined };
    try {
      const snapshot = await invoke<DshSettingsSnapshot>("save_dsh_plugin_settings", {
        agent: "dsh",
        section,
        values,
      });
      const next = { ...DEFAULT_SETTINGS, ...snapshot };
      setDraft(next);
      onSettingsChange(next);
      setApiKey("");
    } catch {
      setFailed(section);
    } finally {
      setSaving(null);
    }
  }

  return (
    <ul className="dsh-config-list">
      <PluginConfigCard
        section="shell"
        icon={TerminalSquare}
        title={t("appSettings.dshBashTitle")}
        description={t("appSettings.dshBashDescription")}
        dirty={shellDirty}
        saving={saving === "shell"}
        failed={failed === "shell"}
        onSave={() => void save("shell")}
        onDiscard={() => setDraft((current) => ({ ...current, shell: settings.shell }))}
      >
        <ConfigField
          id="dsh-shell-timeout"
          type="number"
          label={t("appSettings.dshBashTimeoutMs")}
          hint={t("appSettings.dshBashTimeoutMsHint")}
          value={String(draft.shell.timeoutMs)}
          onChange={(value) =>
            setDraft((current) => ({
              ...current,
              shell: { ...current.shell, timeoutMs: positiveInteger(value, 0) },
            }))
          }
        />
        <ConfigField
          id="dsh-shell-output"
          type="number"
          label={t("appSettings.dshBashMaxOutputBytes")}
          hint={t("appSettings.dshBashMaxOutputBytesHint")}
          value={String(draft.shell.maxOutputBytes)}
          onChange={(value) =>
            setDraft((current) => ({
              ...current,
              shell: { ...current.shell, maxOutputBytes: positiveInteger(value, 0) },
            }))
          }
        />
      </PluginConfigCard>

      <PluginConfigCard
        section="agent-loop"
        icon={Workflow}
        title={t("appSettings.dshAgentLoopTitle")}
        description={t("appSettings.dshAgentLoopDescription")}
        dirty={loopDirty}
        saving={saving === "agent-loop"}
        failed={failed === "agent-loop"}
        onSave={() => void save("agent-loop")}
        onDiscard={() => setDraft((current) => ({ ...current, agentLoop: settings.agentLoop }))}
      >
        <ConfigField
          id="dsh-agent-loop-parallel"
          type="number"
          label={t("appSettings.dshAgentLoopMaxParallel")}
          hint={t("appSettings.dshAgentLoopMaxParallelHint")}
          value={String(draft.agentLoop.maxParallelToolCalls)}
          onChange={(value) =>
            setDraft((current) => ({
              ...current,
              agentLoop: { maxParallelToolCalls: positiveInteger(value, 0) },
            }))
          }
        />
      </PluginConfigCard>

      <PluginConfigCard
        section="web-search-deepseek"
        icon={Globe2}
        title={t("appSettings.dshWebSearchTitle")}
        description={t("appSettings.dshWebSearchDescription")}
        dirty={searchDirty}
        saving={saving === "web-search-deepseek"}
        failed={failed === "web-search-deepseek"}
        onSave={() => void save("web-search-deepseek")}
        onDiscard={() => {
          setDraft((current) => ({ ...current, webSearch: settings.webSearch }));
          setApiKey("");
        }}
      >
        <ConfigField
          id="dsh-web-search-key"
          type="password"
          label={t("appSettings.dshWebSearchApiKey")}
          hint={t("appSettings.dshWebSearchApiKeyHint")}
          status={t(
            settings.webSearch.apiKeyConfigured
              ? "appSettings.dshWebSearchApiKeySet"
              : "appSettings.dshWebSearchApiKeyUnset",
          )}
          value={apiKey}
          onChange={setApiKey}
        />
        <ConfigField
          id="dsh-web-search-endpoint"
          label={t("appSettings.dshWebSearchBaseUrl")}
          hint={t("appSettings.dshWebSearchBaseUrlHint")}
          placeholder="https://api.deepseek.com/anthropic/v1"
          value={draft.webSearch.baseUrl}
          onChange={(value) =>
            setDraft((current) => ({
              ...current,
              webSearch: { ...current.webSearch, baseUrl: value },
            }))
          }
        />
        <ConfigField
          id="dsh-web-search-max-uses"
          type="number"
          label={t("appSettings.dshWebSearchMaxUses")}
          hint={t("appSettings.dshWebSearchMaxUsesHint")}
          value={String(draft.webSearch.maxUses)}
          onChange={(value) =>
            setDraft((current) => ({
              ...current,
              webSearch: { ...current.webSearch, maxUses: positiveInteger(value, 0) },
            }))
          }
        />
      </PluginConfigCard>
    </ul>
  );
}

function phaseLabel(phase: DshFiberPhase, t: (key: string) => string): string {
  const keys: Record<Exclude<DshFiberPhase, null>, string> = {
    pending: "appSettings.dshCordisPending",
    loading: "appSettings.dshCordisLoading",
    active: "appSettings.dshCordisActive",
    failed: "appSettings.dshCordisFailed",
    unloading: "appSettings.dshCordisUnloading",
  };
  return t(phase === null ? "appSettings.dshCordisUnobserved" : keys[phase]);
}

function PluginInventory() {
  const { t } = useI18n();
  const [plugins, setPlugins] = useState(() => mergeDshPluginInventory([]));
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [usingFallback, setUsingFallback] = useState(false);

  async function loadPlugins() {
    setRefreshing(true);
    try {
      const loaded = await invoke<DshPluginWireEntry[]>("list_dsh_plugins", { agent: "dsh" });
      setPlugins(mergeDshPluginInventory(normalizeInventory(loaded)));
      setUsingFallback(loaded.length === 0);
    } catch {
      setPlugins(mergeDshPluginInventory([]));
      setUsingFallback(true);
    } finally {
      setRefreshing(false);
    }
  }

  useEffect(() => {
    void loadPlugins();
  }, []);

  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filtered = useMemo(
    () =>
      plugins.filter(
        (entry) =>
          !normalizedQuery ||
          [entry.entryId, entry.moduleName].some((value) =>
            value.toLocaleLowerCase().includes(normalizedQuery),
          ),
      ),
    [normalizedQuery, plugins],
  );

  return (
    <div className="dsh-inventory" aria-busy={refreshing}>
      <div className="dsh-inventory__toolbar">
        <label className="dsh-plugin-search">
          <Search size={15} aria-hidden />
          <span className="dsh-visually-hidden">{t("appSettings.dshSearchPlugins")}</span>
          <input
            type="search"
            value={query}
            placeholder={t("appSettings.dshSearchPlugins")}
            aria-label={t("appSettings.dshSearchPlugins")}
            onChange={(event) => setQuery(event.currentTarget.value)}
          />
        </label>
        <Button
          variant="ghost"
          size="icon-sm"
          icon={RefreshCw}
          disabled={refreshing}
          aria-label={t("common.refresh")}
          title={t("common.refresh")}
          className={refreshing ? "dsh-refresh--spinning" : undefined}
          onClick={() => void loadPlugins()}
        />
      </div>
      <div className="dsh-inventory__heading">
        <div>
          <h3>{t("appSettings.dshPluginList")}</h3>
          {usingFallback ? <span>{t("appSettings.dshOfficialInventory")}</span> : null}
        </div>
        <strong data-plugin-count={filtered.length}>{filtered.length}</strong>
      </div>
      {filtered.length === 0 ? (
        <p className="dsh-empty-state">{t("appSettings.dshNoMatchingPlugins")}</p>
      ) : (
        <ul className="dsh-plugin-grid">
          {filtered.map((entry) => {
            const open = expanded === entry.entryId;
            const configuration = t(
              entry.enabled ? "appSettings.dshEnabledTag" : "appSettings.dshDisabledTag",
            );
            const cordis = phaseLabel(entry.fiberPhase, t);
            return (
              <li key={entry.entryId} className="dsh-plugin-card" data-open={open || undefined}>
                <button
                  type="button"
                  className="dsh-plugin-card__main"
                  aria-expanded={open}
                  aria-label={`${moduleShortName(entry.moduleName)}, ${configuration}${entry.enabled ? `, ${cordis}` : ""}`}
                  onClick={() => setExpanded(open ? null : entry.entryId)}
                >
                  <strong title={entry.moduleName}>{moduleShortName(entry.moduleName)}</strong>
                  <span className="dsh-plugin-card__trailing">
                    {entry.enabled ? (
                      <span
                        className="dsh-phase-dot"
                        data-phase={entry.fiberPhase ?? "unobserved"}
                        role="img"
                        aria-label={cordis}
                        title={cordis}
                      />
                    ) : null}
                    <span className="dsh-config-tag" data-enabled={entry.enabled}>
                      {configuration}
                    </span>
                    <ChevronDown className="dsh-chevron" size={13} aria-hidden />
                  </span>
                </button>
                {open ? (
                  <div className="dsh-plugin-card__details">
                    <code>{entry.entryId}</code>
                    <dl>
                      <div>
                        <dt>{t("appSettings.dshConfigurationStatus")}</dt>
                        <dd>{configuration}</dd>
                      </div>
                      {entry.enabled ? (
                        <div>
                          <dt>{t("appSettings.dshCordisStatus")}</dt>
                          <dd>{cordis}</dd>
                        </div>
                      ) : null}
                    </dl>
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function AgentPresetsView({
  settings,
  onSettingsChange,
  runtimeError,
  onRuntimeError,
}: {
  settings: DshSettingsSnapshot;
  onSettingsChange: (settings: DshSettingsSnapshot) => void;
  runtimeError: string | null;
  onRuntimeError: (message: string | null) => void;
}) {
  const { t } = useI18n();
  const [savingPreset, setSavingPreset] = useState<string | null>(null);
  const [failedPreset, setFailedPreset] = useState<string | null>(null);

  async function makeDefault(id: string): Promise<boolean> {
    if (id === settings.defaultPreset) return true;
    if (savingPreset) return false;
    setSavingPreset(id);
    setFailedPreset(null);
    onRuntimeError(null);
    try {
      await invoke("set_dsh_web_default_preset", { preset: id });
      // The Web API is authoritative. The local snapshot is only refreshed
      // for display and may lag while DSH hot-reloads its settings document.
      const snapshot = await invoke<DshSettingsSnapshot>("get_dsh_settings_snapshot", { agent: "dsh" });
      onSettingsChange({ ...DEFAULT_SETTINGS, ...snapshot, defaultPreset: id });
      return true;
    } catch (error: unknown) {
      onRuntimeError(errorMessage(error));
      setFailedPreset(id);
      return false;
    } finally {
      setSavingPreset(null);
    }
  }

  async function startCreatorDraft() {
    window.dispatchEvent(new CustomEvent(START_DSH_CREATOR_DRAFT_EVENT));
  }

  async function openPresetDocument(preset: string) {
    onRuntimeError(null);
    try {
      await invoke("open_dsh_agent_preset_document", { preset });
    } catch (error: unknown) {
      onRuntimeError(errorMessage(error));
    }
  }

  async function copyPreset(from: string, targetPreset: string) {
    if (!targetPreset.trim()) return;
    onRuntimeError(null);
    setSavingPreset(from);
    try {
      await invoke("copy_dsh_agent_preset", {
        from,
        targetPreset,
        name: targetPreset.trim(),
      });
      const snapshot = await invoke<DshSettingsSnapshot>("get_dsh_settings_snapshot", {
        agent: "dsh",
      });
      onSettingsChange({ ...DEFAULT_SETTINGS, ...snapshot });
    } catch (error: unknown) {
      onRuntimeError(errorMessage(error));
    } finally {
      setSavingPreset(null);
    }
  }

  async function removePreset(preset: string) {
    onRuntimeError(null);
    setSavingPreset(preset);
    try {
      await invoke("remove_dsh_agent_preset", { preset });
      const snapshot = await invoke<DshSettingsSnapshot>("get_dsh_settings_snapshot", {
        agent: "dsh",
      });
      onSettingsChange({ ...DEFAULT_SETTINGS, ...snapshot });
    } catch (error: unknown) {
      onRuntimeError(errorMessage(error));
    } finally {
      setSavingPreset(null);
    }
  }

  return (
    <section className="dsh-page" aria-label={t("appSettings.dshAgentPresetsTitle")}>
      <SectionHeading
        title={t("appSettings.dshAgentPresetsTitle")}
        intro={t("appSettings.dshAgentPresetsIntro")}
      />
      {runtimeError ? (
        <p className="dsh-preset-runtime-error" role="status">
          {t("appSettings.dshPresetSaveFailedDetail", { message: runtimeError })}
        </p>
      ) : null}
      <PresetGroup title={t("appSettings.dshBuiltInGroup")}>
        {BUILT_IN_PRESETS.map(({ id, icon: Icon, nameKey, descriptionKey }) => (
          <PresetCard
            key={id}
            id={id}
            icon={<Icon size={18} />}
            name={t(`appSettings.dsh${nameKey}`)}
            description={t(`appSettings.dsh${descriptionKey}`)}
            builtIn
            active={settings.defaultPreset === id}
            saving={savingPreset === id}
            failed={failedPreset === id}
            onSelect={() => void makeDefault(id)}
          />
        ))}
      </PresetGroup>
      <PresetGroup title={t("appSettings.dshCustomGroup")}>
        {settings.customPresets.length ? (
          settings.customPresets.map((preset) => (
            <PresetCard
              key={preset.id}
              id={preset.id}
              icon={<Bot size={18} />}
              name={preset.name ?? preset.id}
              description={preset.description ?? t("appSettings.dshNoPresetDescription")}
              active={settings.defaultPreset === preset.id}
              saving={savingPreset === preset.id}
              failed={failedPreset === preset.id}
              onSelect={() => void makeDefault(preset.id)}
              onRead={() => void openPresetDocument(preset.id)}
              onCopy={(target) => void copyPreset(preset.id, target)}
              onRemove={() => void removePreset(preset.id)}
            />
          ))
        ) : null}
        <Button
          variant="outline"
          size="sm"
          icon={Plus}
          className="dsh-creator-button"
          disabled={savingPreset !== null}
          onClick={() => void startCreatorDraft()}
        >
          {t("appSettings.dshCreatorDraft")}
        </Button>
      </PresetGroup>
    </section>
  );
}

function PresetGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="dsh-preset-group">
      <h3>{title}</h3>
      <div className="dsh-preset-grid">{children}</div>
    </section>
  );
}

function PresetCard({
  id,
  icon,
  name,
  description,
  builtIn = false,
  active,
  saving,
  failed,
  onSelect,
  onRead,
  onCopy,
  onRemove,
}: {
  id: string;
  icon: ReactNode;
  name: string;
  description: string;
  builtIn?: boolean;
  active: boolean;
  saving: boolean;
  failed: boolean;
  onSelect: () => void;
  onRead?: () => void;
  onCopy?: (target: string) => void;
  onRemove?: () => void;
}) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="dsh-preset-card"
      data-active={active || undefined}
      aria-pressed={active}
      disabled={active || saving}
      title={active ? t("appSettings.dshPresetInUse") : t("appSettings.dshSetDefaultPreset")}
      onClick={onSelect}
    >
      <span className="dsh-preset-card__head">
        <span className="dsh-preset-card__icon">{icon}</span>
        <strong>{name}</strong>
        <span className="dsh-preset-badge">
          {t(builtIn ? "appSettings.dshBuiltIn" : "appSettings.dshCustom")}
        </span>
        {active ? (
          <span className="dsh-preset-active">
            <Check size={12} />
            {t("appSettings.dshPresetInUse")}
          </span>
        ) : null}
      </span>
      <span className="dsh-preset-card__description">{description}</span>
      <code>{id}</code>
      {failed ? (
        <span className="dsh-preset-error">{t("appSettings.dshPresetSaveFailed")}</span>
      ) : null}
      {onRead && (
        <span
          className="dsh-preset-card__actions"
          // One inline row of custom-preset actions: open the preset file,
          // duplicate into a new preset (typed below the card), and delete.
          style={{ display: "inline-flex", gap: 6, marginTop: 6 }}
          onClick={(e) => e.stopPropagation()}
        >
          <button
            type="button"
            className="dsh-preset-card__action"
            onClick={onRead}
            title={t("appSettings.dshOpenPresetFile")}
            style={presetActionBtn}
          >
            <FileText size={12} />
          </button>
          <button
            type="button"
            className="dsh-preset-card__action"
            onClick={() => setCopied((v) => !v)}
            title={t("appSettings.dshCopyPreset")}
            style={presetActionBtn}
          >
            <Copy size={12} />
          </button>
          {copied && onCopy && (
            <input
              autoFocus
              type="text"
              placeholder={t("appSettings.dshCopyPresetTarget")}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  const v = (e.target as HTMLInputElement).value.trim();
                  if (v) onCopy(v);
                  setCopied(false);
                } else if (e.key === "Escape") {
                  setCopied(false);
                }
              }}
              style={{
                height: 22,
                width: 110,
                padding: "0 6px",
                border: "1px solid var(--border-medium)",
                borderRadius: 4,
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                fontSize: 11,
                outline: "none",
              }}
            />
          )}
          {onRemove && (
            <button
              type="button"
              className="dsh-preset-card__action"
              onClick={onRemove}
              title={t("appSettings.dshRemovePreset")}
              style={{ ...presetActionBtn, color: "var(--danger)" }}
            >
              <Trash2 size={12} />
            </button>
          )}
        </span>
      )}
    </button>
  );
}

const presetActionBtn: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  width: 24,
  height: 24,
  padding: 0,
  background: "var(--bg-hover)",
  border: "1px solid var(--border-dim)",
  borderRadius: 4,
  color: "var(--text-secondary)",
  cursor: "pointer",
};
