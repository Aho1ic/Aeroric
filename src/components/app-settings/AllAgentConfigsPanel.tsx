import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import * as Popover from "@radix-ui/react-popover";
import {
  Archive,
  ChevronDown,
  Download,
  LayoutGrid,
  LayoutList,
  Plus,
  Search,
  Upload,
} from "lucide-react";
import { useAgentOptions, useAgentSettings } from "../../hooks/useAgentOptions";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { APP_SETTINGS_CHANGED_EVENT } from "./types";
import { AgentCardItem } from "./AgentCardItem";
import { AddAgentModal } from "./AddAgentModal";
import { AgentDetailModal } from "./AgentDetailModal";
import type { AgentOption } from "../../agents";
import type { ThemeVariant } from "../../types";
import claudeLogo from "../../assets/claude.svg";
import chatgptLogo from "../../assets/chatgpt.svg";
import { AnimatedSelectionGroup } from "../ui/AnimatedSelection";
import { refreshLocalRouterRuntime } from "./shared";

type ProviderTab = "anthropic" | "openai";
type ViewMode = "card" | "bar";

const VIEW_MODE_KEY = "aeroric-agent-view-mode";

function loadViewMode(): ViewMode {
  const stored = localStorage.getItem(VIEW_MODE_KEY);
  return stored === "bar" ? "bar" : "card";
}

export function AllAgentConfigsPanel({ themeVariant }: { themeVariant: ThemeVariant }) {
  const { t } = useI18n();
  const agentOptions = useAgentOptions();
  const settings = useAgentSettings();
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [tab, setTab] = useState<ProviderTab>("anthropic");
  const [viewMode, setViewMode] = useState<ViewMode>(loadViewMode);
  const [showAddAgentModal, setShowAddAgentModal] = useState(false);
  const [editingAgent, setEditingAgent] = useState<AgentOption | null>(null);
  const [showImportMenu, setShowImportMenu] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  const filteredAgents = useMemo(() => {
    const byTab = agentOptions.filter((o) => (tab === "anthropic" ? !o.codexLike : o.codexLike));
    if (!searchQuery.trim()) return byTab;
    const q = searchQuery.trim().toLowerCase();
    return byTab.filter((o) => o.label.toLowerCase().includes(q));
  }, [agentOptions, tab, searchQuery]);

  const agentMetaById = useMemo(() => {
    const result = new Map<string, { baseUrl?: string; apiKey?: string }>();
    for (const [agentId, credentials] of Object.entries(
      settings?.builtin_agent_credentials ?? {},
    )) {
      result.set(agentId, { baseUrl: credentials.base_url, apiKey: credentials.api_key });
    }
    for (const profile of settings?.custom_agents ?? []) {
      result.set(profile.id, { baseUrl: profile.base_url, apiKey: profile.api_key });
    }
    return result;
  }, [settings]);

  function handleViewModeChange(mode: ViewMode) {
    setViewMode(mode);
    localStorage.setItem(VIEW_MODE_KEY, mode);
  }

  async function handleExport() {
    if (exporting || importing) return;
    const outputPath = await saveDialog({
      title: t("appSettings.exportAllAgentConfigs"),
      defaultPath: "aeroric-all-agents.aeroric-agents.json",
      filters: [{ name: t("appSettings.allAgentConfigBundle"), extensions: ["json"] }],
    });
    if (!outputPath) return;
    setExporting(true);
    setMessage(null);
    setError(null);
    try {
      const result = await invoke<{ exported_agent_ids: string[] }>(
        "export_all_agent_config_bundle",
        { outputPath },
      );
      setMessage(
        t("appSettings.allAgentConfigsExported", {
          count: result.exported_agent_ids.length,
        }),
      );
    } catch (reason) {
      setError(String(reason));
    } finally {
      setExporting(false);
    }
  }

  async function handleImport() {
    if (exporting || importing) return;
    setShowImportMenu(false);
    const inputPath = await openDialog({
      title: t("appSettings.importAllAgentConfigs"),
      multiple: false,
      directory: false,
      filters: [{ name: t("appSettings.allAgentConfigBundle"), extensions: ["json"] }],
    });
    if (!inputPath || Array.isArray(inputPath)) return;
    setImporting(true);
    setMessage(null);
    setError(null);
    try {
      const importedAgentIds = inputPath.endsWith(".aeroric-agent.json")
        ? [
            (
              await invoke<{ agent_id: string }>("import_agent_config_bundle", {
                inputPath,
              })
            ).agent_id,
          ]
        : (
            await invoke<{ imported_agent_ids: string[] }>("import_all_agent_config_bundle", {
              inputPath,
            })
          ).imported_agent_ids;
      await refreshLocalRouterRuntime();
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setMessage(
        t("appSettings.allAgentConfigsImported", {
          count: importedAgentIds.length,
        }),
      );
    } catch (reason) {
      setError(String(reason));
    } finally {
      setImporting(false);
    }
  }

  async function handleImportCcSwitch() {
    if (exporting || importing) return;
    setShowImportMenu(false);
    const inputPath = await openDialog({
      title: t("appSettings.importFromCcSwitch"),
      multiple: false,
      directory: false,
      filters: [{ name: t("appSettings.ccSwitchConfigBundle"), extensions: ["sql"] }],
    });
    if (!inputPath || Array.isArray(inputPath)) return;
    setImporting(true);
    setMessage(null);
    setError(null);
    try {
      const result = await invoke<{ imported_agent_ids: string[] }>("import_cc_switch_config", {
        inputPath,
      });
      await refreshLocalRouterRuntime();
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setMessage(
        t("appSettings.allAgentConfigsImported", {
          count: result.imported_agent_ids.length,
        }),
      );
    } catch (reason) {
      setError(String(reason));
    } finally {
      setImporting(false);
    }
  }

  function handleAgentSaved(agentId: string) {
    setShowAddAgentModal(false);
    window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    const isCodexLike = agentOptions.find((o) => o.value === agentId)?.codexLike;
    if (isCodexLike === true) setTab("openai");
    else if (isCodexLike === false) setTab("anthropic");
  }

  function handleAgentDeleted() {
    setEditingAgent(null);
    window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
  }

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        overflow: "auto",
        padding: "22px 24px",
      }}
    >
      {/* Bulk migration section
          标题与导入/导出同一行,下拉菜单向上展开,避免盖住下方 Agent 搜索框。 */}
      <section
        style={{
          position: "relative",
          zIndex: 30,
          padding: "16px 18px",
          border: "1px solid var(--border-dim)",
          borderRadius: "var(--radius-lg)",
          background: "var(--bg-subtle)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            flexWrap: "wrap",
          }}
        >
          <span
            style={{
              width: 36,
              height: 36,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
              borderRadius: 11,
              color: "var(--accent)",
              background: "color-mix(in srgb, var(--accent) 10%, transparent)",
            }}
          >
            <Archive size={18} />
          </span>
          <div style={{ minWidth: 0, flex: "1 1 160px" }}>
            <div style={{ color: "var(--text-primary)", fontSize: 14, fontWeight: 700 }}>
              {t("appSettings.allAgentConfigsTitle")}
            </div>
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "flex-end",
              gap: 8,
              marginLeft: "auto",
              flex: "0 0 auto",
            }}
          >
            <Popover.Root open={showImportMenu} onOpenChange={setShowImportMenu}>
              <Popover.Trigger asChild>
                <Button variant="outline" size="sm" disabled={importing || exporting}>
                  <Upload size={13} />
                  {importing
                    ? t("appSettings.importingAllAgentConfigs")
                    : t("appSettings.importAllAgentConfigs")}
                  <ChevronDown size={11} />
                </Button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content
                  side="bottom"
                  align="end"
                  sideOffset={6}
                  collisionPadding={12}
                  avoidCollisions
                  sticky="always"
                  style={{
                    zIndex: 9999,
                    minWidth: 180,
                    maxWidth: "min(280px, calc(100vw - 24px))",
                    maxHeight: "min(320px, var(--radix-popover-content-available-height))",
                    overflowY: "auto",
                    padding: 4,
                    border: "1px solid var(--border-medium)",
                    borderRadius: "var(--radius-md)",
                    background: "var(--bg-card)",
                    boxShadow: "var(--shadow-popover)",
                  }}
                >
                  <button
                    type="button"
                    onClick={() => void handleImport()}
                    style={{
                      width: "100%",
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "7px 10px",
                      border: "none",
                      borderRadius: 6,
                      background: "transparent",
                      color: "var(--text-primary)",
                      fontSize: 12.5,
                      cursor: "pointer",
                      textAlign: "left",
                    }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background = "var(--bg-hover)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background = "transparent";
                    }}
                  >
                    {t("appSettings.importFromAeroric")}
                  </button>
                  <button
                    type="button"
                    onClick={() => void handleImportCcSwitch()}
                    style={{
                      width: "100%",
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "7px 10px",
                      border: "none",
                      borderRadius: 6,
                      background: "transparent",
                      color: "var(--text-primary)",
                      fontSize: 12.5,
                      cursor: "pointer",
                      textAlign: "left",
                    }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background = "var(--bg-hover)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background = "transparent";
                    }}
                  >
                    {t("appSettings.importFromCcSwitch")}
                  </button>
                </Popover.Content>
              </Popover.Portal>
            </Popover.Root>
            <Button
              variant="default"
              size="sm"
              onClick={() => void handleExport()}
              disabled={exporting || importing}
            >
              <Download size={13} />
              {exporting
                ? t("appSettings.exportingAllAgentConfigs")
                : t("appSettings.exportAllAgentConfigs")}
            </Button>
          </div>
        </div>

        {message && (
          <div style={{ marginTop: 12, color: "var(--success)", fontSize: 12 }}>{message}</div>
        )}
        {error && (
          <div style={{ marginTop: 12, color: "var(--danger)", fontSize: 12 }}>{error}</div>
        )}
      </section>

      {/* Provider tabs + Add Agent + view toggle */}
      <div
        style={{
          marginTop: 24,
          display: "flex",
          alignItems: "center",
          gap: 8,
        }}
      >
        <AnimatedSelectionGroup
          value={tab}
          onChange={setTab}
          ariaLabel={t("appSettings.provider")}
          options={[
            {
              value: "anthropic",
              label: (
                <>
                  <img src={claudeLogo} alt="" style={{ width: 16, height: 16, borderRadius: 3 }} />
                  {t("appSettings.providerAnthropic")}
                </>
              ),
            },
            {
              value: "openai",
              label: (
                <>
                  <img
                    src={chatgptLogo}
                    alt=""
                    style={{
                      width: 16,
                      height: 16,
                      borderRadius: 3,
                      filter: themeVariant === "dark" ? "invert(1) brightness(1.35)" : undefined,
                    }}
                  />
                  {t("appSettings.providerOpenAI")}
                </>
              ),
            },
          ]}
          itemStyle={{ minHeight: 30, padding: "6px 12px", fontSize: 12.5 }}
        />

        <Button variant="outline" size="sm" onClick={() => setShowAddAgentModal(true)}>
          <Plus size={13} />
          {t("appSettings.addAgentInline")}
        </Button>

        <div style={{ flex: 1 }} />

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
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t("appSettings.searchAgents")}
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

        <AnimatedSelectionGroup
          value={viewMode}
          onChange={handleViewModeChange}
          ariaLabel={t("appSettings.viewMode")}
          options={[
            {
              value: "card",
              label: <LayoutList size={14} />,
              ariaLabel: t("appSettings.viewCards"),
              title: t("appSettings.viewCards"),
            },
            {
              value: "bar",
              label: <LayoutGrid size={14} />,
              ariaLabel: t("appSettings.viewBars"),
              title: t("appSettings.viewBars"),
            },
          ]}
          itemStyle={{ width: 26, minHeight: 24, padding: 0 }}
        />
      </div>

      {/* Agent list */}
      <div
        style={
          viewMode === "bar"
            ? {
                marginTop: 14,
                display: "grid",
                gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))",
                gap: 10,
              }
            : {
                marginTop: 14,
                display: "flex",
                flexDirection: "column",
                gap: 10,
              }
        }
      >
        {filteredAgents.length === 0 && (
          <div
            style={{
              padding: "28px 0",
              textAlign: "center",
              color: "var(--text-hint)",
              fontSize: 12.5,
            }}
          >
            {t("appSettings.noAgentsInProvider")}
          </div>
        )}

        {filteredAgents.map((option) => {
          const meta = agentMetaById.get(option.value) ?? {};
          return (
            <AgentCardItem
              key={option.value}
              option={option}
              viewMode={viewMode}
              themeVariant={themeVariant}
              logo={option.codexLike ? chatgptLogo : claudeLogo}
              baseUrl={meta.baseUrl}
              apiKey={meta.apiKey}
              onClick={() => setEditingAgent(option)}
            />
          );
        })}
      </div>

      {/* Add Agent Modal */}
      {showAddAgentModal && (
        <AddAgentModal onClose={() => setShowAddAgentModal(false)} onSaved={handleAgentSaved} />
      )}

      {/* Agent Detail Modal */}
      {editingAgent && (
        <AgentDetailModal
          option={editingAgent}
          themeVariant={themeVariant}
          logo={editingAgent.codexLike ? chatgptLogo : claudeLogo}
          settings={settings}
          onClose={() => setEditingAgent(null)}
          onDeleted={handleAgentDeleted}
        />
      )}
    </div>
  );
}
