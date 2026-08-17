/**
 * DSH LLM Providers panel — view providers and their models, trigger discovery.
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, RefreshCw, Search } from "lucide-react";
import type { DshGlobalModels, DshModelGroup, DshProviderInfo } from "../../types";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import "./DshPluginsPanel.css";

function errorMessage(e: unknown): string {
  if (typeof e === "string" && e.trim()) return e;
  if (e instanceof Error) return e.message;
  return String(e || "Unknown error");
}

interface ProviderCardProps {
  provider: DshProviderInfo;
  modelGroup: DshModelGroup | undefined;
  t: (k: string, vars?: Record<string, string | number>) => string;
}

function ProviderCard({ provider, modelGroup, t }: ProviderCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [discovering, setDiscovering] = useState(false);
  const [discoverError, setDiscoverError] = useState<string | null>(null);

  async function handleDiscover() {
    setDiscovering(true);
    setDiscoverError(null);
    try {
      await invoke("discover_dsh_llm_models", {
        settingsNs: provider.settingsNs,
        provider: null,
        baseUrl: null,
        api: null,
        apiKey: null,
      });
    } catch (e) {
      setDiscoverError(t("appSettings.dshModelDiscoveryFailed", { error: errorMessage(e) }));
    } finally {
      setDiscovering(false);
    }
  }

  const models = modelGroup?.models ?? [];

  return (
    <div className={`dsh-config-card${expanded ? " dsh-config-card--open" : ""}`}>
      <button
        type="button"
        className="dsh-config-card__header"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
      >
        <div className="dsh-config-card__copy" style={{ flex: 1 }}>
          <strong>{provider.displayName ?? provider.settingsNs}</strong>
          <span>
            {t("appSettings.dshProviderSettingsNs")}: {provider.settingsNs}
          </span>
        </div>
        <span
          style={{
            padding: "2px 7px",
            borderRadius: "var(--radius-sm)",
            fontSize: 10,
            fontWeight: 600,
            background: provider.active
              ? "color-mix(in srgb, var(--success, #22c55e) 15%, transparent)"
              : "var(--bg-subtle)",
            color: provider.active ? "var(--success, #22c55e)" : "var(--text-hint)",
            flexShrink: 0,
          }}
        >
          {provider.active ? t("appSettings.dshProviderActive") : t("appSettings.dshProviderInactive")}
        </span>
        <ChevronDown size={14} className="dsh-chevron" />
      </button>

      {expanded && (
        <div className="dsh-config-card__body">
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 10 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-primary)" }}>
              {t("appSettings.dshModelsTitle")}
            </span>
            <Button
              variant="outline"
              size="xs"
              icon={Search}
              disabled={discovering}
              onClick={() => void handleDiscover()}
            >
              {discovering ? t("appSettings.dshDiscovering") : t("appSettings.dshDiscoverModels")}
            </Button>
          </div>

          {discoverError && (
            <p style={{ margin: "0 0 8px", fontSize: 10.5, color: "var(--danger)" }} role="alert">
              {discoverError}
            </p>
          )}

          {models.length === 0 ? (
            <div className="dsh-empty-state" style={{ padding: "12px 0" }}>
              {t("appSettings.dshNoModels")}
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
              {models.map((m) => (
                <div
                  key={m.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "7px 10px",
                    borderRadius: "var(--radius-sm)",
                    background: "var(--bg-subtle)",
                  }}
                >
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-primary)" }}>
                      {m.name ?? m.id}
                    </div>
                    {m.name && (
                      <div style={{ fontSize: 10.5, color: "var(--text-hint)", fontFamily: "var(--font-mono)" }}>
                        {m.id}
                      </div>
                    )}
                  </div>
                  {m.reasoning && (
                    <div style={{ fontSize: 10, color: "var(--text-secondary)" }}>
                      {m.reasoning.efforts.map((ef) => ef.name).join(" · ")}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ── Main panel ────────────────────────────────────────────────────────────────

export function DshLlmProvidersPanel() {
  const { t } = useI18n();
  const [providers, setProviders] = useState<DshProviderInfo[]>([]);
  const [globalModels, setGlobalModels] = useState<DshGlobalModels | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void load();
  }, []);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [prov, models] = await Promise.all([
        invoke<DshProviderInfo[]>("list_dsh_llm_providers"),
        invoke<DshGlobalModels>("list_dsh_llm_models"),
      ]);
      setProviders(prov);
      setGlobalModels(models);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }

  // Build a map from settingsNs → model group for quick lookup
  const groupByNs = new Map<string, DshModelGroup>();
  if (globalModels) {
    for (const g of globalModels.groups) {
      groupByNs.set(g.id, g);
    }
  }

  return (
    <div className="dsh-settings-panel">
      <div className="dsh-page">
        <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 12 }}>
          <header className="dsh-section-heading" style={{ flex: 1 }}>
            <h2>{t("appSettings.dshProvidersTitle")}</h2>
            <p>{t("appSettings.dshProvidersIntro")}</p>
          </header>
          <Button
            variant="outline"
            size="sm"
            icon={RefreshCw}
            disabled={loading}
            onClick={() => void load()}
            style={{ marginTop: 4, flexShrink: 0 }}
          >
            {t("appSettings.dshRefresh")}
          </Button>
        </div>

        {error && (
          <p className="dsh-toolbar-error" role="alert">
            {error}
          </p>
        )}

        {globalModels && globalModels.failures.length > 0 && (
          <div
            style={{
              margin: "12px 0",
              padding: "10px 12px",
              borderRadius: "var(--radius-sm)",
              background: "color-mix(in srgb, var(--danger) 10%, transparent)",
              border: "1px solid color-mix(in srgb, var(--danger) 22%, transparent)",
            }}
          >
            {globalModels.failures.map((f) => (
              <div key={f.id} style={{ fontSize: 11, color: "var(--danger)", lineHeight: 1.5 }}>
                <strong>{f.name}</strong>: {f.message}
              </div>
            ))}
          </div>
        )}

        <div style={{ marginTop: 18 }}>
          {loading ? (
            <div className="dsh-empty-state">{t("appSettings.dshLoading")}</div>
          ) : providers.length === 0 ? (
            <div className="dsh-empty-state">{t("appSettings.dshNoModels")}</div>
          ) : (
            <div className="dsh-config-list">
              {providers.map((p) => (
                <ProviderCard
                  key={p.settingsNs}
                  provider={p}
                  modelGroup={groupByNs.get(p.settingsNs)}
                  t={t}
                />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
