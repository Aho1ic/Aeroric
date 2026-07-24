import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  ChevronLeft,
  ChevronRight,
  Download,
  LayoutGrid,
  LayoutList,
  Plus,
  ShieldAlert,
  Upload,
} from "lucide-react";
import { useAgentOptions } from "../../hooks/useAgentOptions";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { APP_SETTINGS_CHANGED_EVENT, type AppSettings } from "./types";
import { AgentCardItem } from "./AgentCardItem";
import { AddAgentPanel } from "./AddAgentPanel";
import type { ThemeVariant } from "../../types";
import claudeLogo from "../../assets/claude.svg";
import chatgptLogo from "../../assets/chatgpt.svg";

type ProviderTab = "anthropic" | "openai";
type ViewMode = "card" | "bar";

const PAGE_SIZE = 8;
const VIEW_MODE_KEY = "aeroric-agent-view-mode";

function loadViewMode(): ViewMode {
  const stored = localStorage.getItem(VIEW_MODE_KEY);
  return stored === "bar" ? "bar" : "card";
}

export function AllAgentConfigsPanel({ themeVariant }: { themeVariant: ThemeVariant }) {
  const { t } = useI18n();
  const agentOptions = useAgentOptions();
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [tab, setTab] = useState<ProviderTab>("anthropic");
  const [viewMode, setViewMode] = useState<ViewMode>(loadViewMode);
  const [page, setPage] = useState(1);
  const [showAddAgent, setShowAddAgent] = useState(false);
  const [settings, setSettings] = useState<AppSettings | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      invoke<AppSettings>("load_app_settings")
        .then((s) => {
          if (!cancelled) setSettings(s);
        })
        .catch(() => {});
    };
    load();
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, load);
    return () => {
      cancelled = true;
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, load);
    };
  }, []);

  const filteredAgents = useMemo(
    () =>
      agentOptions.filter((o) =>
        tab === "anthropic" ? !o.codexLike : o.codexLike,
      ),
    [agentOptions, tab],
  );

  const totalPages = Math.max(1, Math.ceil(filteredAgents.length / PAGE_SIZE));
  const safePage = Math.min(page, totalPages);
  const pagedAgents = filteredAgents.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE);

  function getAgentMeta(agentKey: string) {
    if (!settings) return {};
    const creds = settings.builtin_agent_credentials?.[agentKey];
    if (creds) return { baseUrl: creds.base_url, apiKey: creds.api_key };
    const custom = settings.custom_agents?.find((a) => a.id === agentKey);
    if (custom) return { baseUrl: custom.base_url, apiKey: custom.api_key };
    return {};
  }

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
      const result = await invoke<{ imported_agent_ids: string[] }>(
        "import_all_agent_config_bundle",
        { inputPath },
      );
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
    setShowAddAgent(false);
    window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    const isCodexLike = agentOptions.find((o) => o.value === agentId)?.codexLike;
    if (isCodexLike === true) setTab("openai");
    else if (isCodexLike === false) setTab("anthropic");
  }

  function handleAgentDeleted() {
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
      {/* Bulk migration section */}
      <section
        style={{
          maxWidth: 720,
          padding: 20,
          border: "1px solid var(--border-dim)",
          borderRadius: 12,
          background: "var(--bg-subtle)",
        }}
      >
        <div style={{ display: "flex", alignItems: "flex-start", gap: 14 }}>
          <span
            style={{
              width: 38,
              height: 38,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
              borderRadius: 12,
              color: "var(--accent)",
              background: "color-mix(in srgb, var(--accent) 10%, transparent)",
            }}
          >
            <Archive size={19} />
          </span>
          <div style={{ minWidth: 0, flex: 1 }}>
            <div style={{ color: "var(--text-primary)", fontSize: 14, fontWeight: 700 }}>
              {t("appSettings.allAgentConfigsTitle")}
            </div>
            <div
              style={{
                marginTop: 5,
                color: "var(--text-muted)",
                fontSize: 12,
                lineHeight: 1.6,
              }}
            >
              {t("appSettings.allAgentConfigsHint", { count: agentOptions.length })}
            </div>
          </div>
        </div>

        <div
          style={{
            marginTop: 18,
            display: "flex",
            alignItems: "flex-start",
            gap: 8,
            padding: "10px 12px",
            border: "1px solid var(--border-dim)",
            borderRadius: 8,
            color: "var(--text-hint)",
            background: "var(--bg-input)",
            fontSize: 11.5,
            lineHeight: 1.5,
          }}
        >
          <ShieldAlert size={14} style={{ flexShrink: 0, marginTop: 1 }} />
          <span>{t("appSettings.allAgentConfigsSecurityHint")}</span>
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            gap: 8,
            marginTop: 18,
          }}
        >
          <Button
            variant="outline"
            size="sm"
            onClick={() => void handleImport()}
            disabled={importing || exporting}
          >
            <Upload size={13} />
            {importing
              ? t("appSettings.importingAllAgentConfigs")
              : t("appSettings.importAllAgentConfigs")}
          </Button>
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

        {message && (
          <div style={{ marginTop: 14, color: "var(--success)", fontSize: 12 }}>{message}</div>
        )}
        {error && (
          <div style={{ marginTop: 14, color: "var(--danger)", fontSize: 12 }}>{error}</div>
        )}
      </section>

      {/* Provider tabs + view toggle */}
      <div
        style={{
          maxWidth: 720,
          marginTop: 24,
          display: "flex",
          alignItems: "center",
          gap: 8,
        }}
      >
        <button
          type="button"
          onClick={() => {
            setTab("anthropic");
            setPage(1);
          }}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: "7px 14px",
            border: `1.5px solid ${tab === "anthropic" ? "var(--accent)" : "var(--border-medium)"}`,
            borderRadius: 8,
            background: tab === "anthropic" ? "var(--control-active-bg)" : "var(--bg-card)",
            color: tab === "anthropic" ? "var(--control-active-fg)" : "var(--text-secondary)",
            fontSize: 12.5,
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          <img
            src={claudeLogo}
            alt=""
            style={{ width: 16, height: 16, borderRadius: 3 }}
          />
          {t("appSettings.providerAnthropic")}
        </button>
        <button
          type="button"
          onClick={() => {
            setTab("openai");
            setPage(1);
          }}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: "7px 14px",
            border: `1.5px solid ${tab === "openai" ? "var(--accent)" : "var(--border-medium)"}`,
            borderRadius: 8,
            background: tab === "openai" ? "var(--control-active-bg)" : "var(--bg-card)",
            color: tab === "openai" ? "var(--control-active-fg)" : "var(--text-secondary)",
            fontSize: 12.5,
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          <img
            src={chatgptLogo}
            alt=""
            style={{
              width: 16,
              height: 16,
              borderRadius: 3,
              filter:
                themeVariant === "dark" ? "invert(1) brightness(1.35)" : undefined,
            }}
          />
          {t("appSettings.providerOpenAI")}
        </button>

        <div style={{ flex: 1 }} />

        <button
          type="button"
          title={t("appSettings.viewCards")}
          onClick={() => handleViewModeChange("card")}
          style={{
            width: 28,
            height: 28,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            border: "1px solid var(--border-medium)",
            borderRadius: 6,
            background: viewMode === "card" ? "var(--bg-hover)" : "transparent",
            color: viewMode === "card" ? "var(--text-primary)" : "var(--text-hint)",
            cursor: "pointer",
          }}
        >
          <LayoutGrid size={14} />
        </button>
        <button
          type="button"
          title={t("appSettings.viewBars")}
          onClick={() => handleViewModeChange("bar")}
          style={{
            width: 28,
            height: 28,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            border: "1px solid var(--border-medium)",
            borderRadius: 6,
            background: viewMode === "bar" ? "var(--bg-hover)" : "transparent",
            color: viewMode === "bar" ? "var(--text-primary)" : "var(--text-hint)",
            cursor: "pointer",
          }}
        >
          <LayoutList size={14} />
        </button>
      </div>

      {/* Agent list */}
      <div
        style={{
          maxWidth: 720,
          marginTop: 14,
          display: "flex",
          flexDirection: "column",
          gap: viewMode === "card" ? 10 : 6,
        }}
      >
        {pagedAgents.length === 0 && !showAddAgent && (
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

        {pagedAgents.map((option) => {
          const meta = getAgentMeta(option.value);
          return (
            <AgentCardItem
              key={option.value}
              option={option}
              viewMode={viewMode}
              themeVariant={themeVariant}
              logo={option.codexLike ? chatgptLogo : claudeLogo}
              baseUrl={meta.baseUrl}
              apiKey={meta.apiKey}
              onDeleted={handleAgentDeleted}
            />
          );
        })}
      </div>

      {/* Add Agent */}
      <div style={{ maxWidth: 720, marginTop: 14 }}>
        {showAddAgent ? (
          <div
            style={{
              border: "1px solid var(--border-dim)",
              borderRadius: 10,
              background: "var(--bg-subtle)",
              overflow: "hidden",
            }}
          >
            <AddAgentPanel onSaved={handleAgentSaved} />
          </div>
        ) : (
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowAddAgent(true)}
          >
            <Plus size={13} />
            {t("appSettings.addAgentInline")}
          </Button>
        )}
      </div>

      {/* Pagination */}
      {totalPages > 1 && (
        <div
          style={{
            maxWidth: 720,
            marginTop: 16,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 8,
          }}
        >
          <button
            type="button"
            disabled={safePage <= 1}
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            style={{
              width: 28,
              height: 28,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              border: "1px solid var(--border-medium)",
              borderRadius: 6,
              background: "transparent",
              color: safePage <= 1 ? "var(--text-disabled)" : "var(--text-secondary)",
              cursor: safePage <= 1 ? "default" : "pointer",
              opacity: safePage <= 1 ? 0.4 : 1,
            }}
          >
            <ChevronLeft size={14} />
          </button>
          <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>
            {safePage} / {totalPages}
          </span>
          <button
            type="button"
            disabled={safePage >= totalPages}
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            style={{
              width: 28,
              height: 28,
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              border: "1px solid var(--border-medium)",
              borderRadius: 6,
              background: "transparent",
              color: safePage >= totalPages ? "var(--text-disabled)" : "var(--text-secondary)",
              cursor: safePage >= totalPages ? "default" : "pointer",
              opacity: safePage >= totalPages ? 0.4 : 1,
            }}
          >
            <ChevronRight size={14} />
          </button>
        </div>
      )}
    </div>
  );
}
