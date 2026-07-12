import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, RefreshCw, TriangleAlert } from "lucide-react";
import { useAgentOptions } from "../../hooks/useAgentOptions";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { APP_SETTINGS_CHANGED_EVENT, type AgentUpgradeResult } from "./types";

export function AgentUpdatesPanel() {
  const { t } = useI18n();
  const agentOptions = useAgentOptions();
  const agentIds = useMemo(
    () => agentOptions.map((option) => String(option.value)),
    [agentOptions],
  );
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [versions, setVersions] = useState<Record<string, string>>({});
  const [results, setResults] = useState<Record<string, AgentUpgradeResult>>({});
  const [refreshing, setRefreshing] = useState(false);
  const [upgrading, setUpgrading] = useState<Set<string>>(() => new Set());
  const [error, setError] = useState<string | null>(null);

  const refreshVersions = useCallback(async () => {
    setRefreshing(true);
    setError(null);
    try {
      const detected = await Promise.all(
        agentIds.map(async (agent) => [
          agent,
          await invoke<string>("detect_agent_version", { agent }),
        ]),
      );
      setVersions(Object.fromEntries(detected));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRefreshing(false);
    }
  }, [agentIds]);

  useEffect(() => {
    setSelected((current) => new Set([...current].filter((agent) => agentIds.includes(agent))));
    void refreshVersions();
  }, [agentIds, refreshVersions]);

  async function upgradeAgents(agents: string[]) {
    if (agents.length === 0) return;
    setUpgrading(new Set(agents));
    setError(null);
    try {
      const nextResults = await invoke<AgentUpgradeResult[]>("upgrade_agent_versions", { agents });
      setResults((current) => ({
        ...current,
        ...Object.fromEntries(nextResults.map((result) => [result.agent, result])),
      }));
      setVersions((current) => ({
        ...current,
        ...Object.fromEntries(
          nextResults.map((result) => [
            result.agent,
            result.current_version || result.previous_version,
          ]),
        ),
      }));
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setUpgrading(new Set());
    }
  }

  const allSelected = agentIds.length > 0 && agentIds.every((agent) => selected.has(agent));

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        overflow: "auto",
        padding: "18px 20px",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 16,
          marginBottom: 14,
        }}
      >
        <div style={{ minWidth: 0 }}>
          <div style={{ fontSize: 13, fontWeight: 650, color: "var(--text-primary)" }}>
            {t("appSettings.agentUpdatesTitle")}
          </div>
          <div
            style={{
              marginTop: 4,
              maxWidth: 620,
              fontSize: 11.5,
              lineHeight: 1.5,
              color: "var(--text-hint)",
            }}
          >
            {t("appSettings.agentUpdatesHint")}
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void refreshVersions()}
          disabled={refreshing || upgrading.size > 0}
        >
          <RefreshCw size={12} className={refreshing ? "spin" : undefined} />
          {refreshing ? t("appSettings.refreshing") : t("appSettings.refreshVersions")}
        </Button>
      </div>

      {error && (
        <div style={{ marginBottom: 12, fontSize: 12, color: "var(--danger)" }}>{error}</div>
      )}

      <div
        style={{
          border: "1px solid var(--border-dim)",
          borderRadius: 8,
          overflow: "hidden",
          background: "var(--bg-input)",
        }}
      >
        <div
          style={{
            minHeight: 38,
            padding: "0 12px",
            display: "grid",
            gridTemplateColumns: "28px minmax(160px, 1fr) minmax(100px, 150px) 112px",
            alignItems: "center",
            borderBottom: "1px solid var(--border-dim)",
            background: "var(--bg-subtle)",
            color: "var(--text-hint)",
            fontSize: 11.5,
            fontWeight: 600,
          }}
        >
          <input
            type="checkbox"
            aria-label={t("appSettings.selectAllAgents")}
            checked={allSelected}
            onChange={(event) => setSelected(event.target.checked ? new Set(agentIds) : new Set())}
          />
          <span>{t("appSettings.agentConfiguration")}</span>
          <span>{t("appSettings.installedVersions")}</span>
          <span />
        </div>

        {agentOptions.map((option, index) => {
          const agent = String(option.value);
          const result = results[agent];
          const rowUpgrading = upgrading.has(agent);
          return (
            <div
              key={agent}
              style={{
                minHeight: 52,
                padding: "6px 12px",
                display: "grid",
                gridTemplateColumns: "28px minmax(160px, 1fr) minmax(100px, 150px) 112px",
                alignItems: "center",
                gap: 0,
                borderBottom:
                  index === agentOptions.length - 1 ? "none" : "1px solid var(--border-dim)",
              }}
            >
              <input
                type="checkbox"
                aria-label={option.label}
                checked={selected.has(agent)}
                onChange={(event) =>
                  setSelected((current) => {
                    const next = new Set(current);
                    if (event.target.checked) next.add(agent);
                    else next.delete(agent);
                    return next;
                  })
                }
                disabled={upgrading.size > 0}
              />
              <div style={{ minWidth: 0 }}>
                <div
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    color: "var(--text-primary)",
                    fontSize: 12.5,
                    fontWeight: 600,
                  }}
                  title={option.label}
                >
                  {option.label}
                </div>
                <div style={{ marginTop: 2, fontSize: 10.5, color: "var(--text-hint)" }}>
                  {option.codexLike ? "Codex CLI" : "Claude Code CLI"}
                </div>
              </div>
              <div style={{ minWidth: 0 }}>
                <div
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontFamily: "var(--font-mono)",
                    fontSize: 11.5,
                    color: "var(--text-secondary)",
                  }}
                >
                  {versions[agent] || t("common.notDetected")}
                </div>
                {result && (
                  <div
                    title={result.message}
                    style={{
                      marginTop: 2,
                      display: "flex",
                      alignItems: "center",
                      gap: 4,
                      fontSize: 10.5,
                      color: result.success ? "var(--success)" : "var(--danger)",
                    }}
                  >
                    {result.success ? <Check size={11} /> : <TriangleAlert size={11} />}
                    {result.success
                      ? t("appSettings.upgradeComplete")
                      : t("appSettings.upgradeFailed")}
                  </div>
                )}
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => void upgradeAgents([agent])}
                disabled={upgrading.size > 0}
              >
                <RefreshCw size={12} className={rowUpgrading ? "spin" : undefined} />
                {rowUpgrading ? t("appSettings.upgrading") : t("appSettings.upgradeToLatest")}
              </Button>
            </div>
          );
        })}
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
          marginTop: 14,
        }}
      >
        <span style={{ fontSize: 11.5, color: "var(--text-hint)" }}>
          {t("appSettings.selectedAgentsCount", { count: selected.size })}
        </span>
        <Button
          variant="default"
          size="sm"
          onClick={() => void upgradeAgents([...selected])}
          disabled={selected.size === 0 || upgrading.size > 0}
        >
          <RefreshCw size={12} className={upgrading.size > 0 ? "spin" : undefined} />
          {upgrading.size > 0 ? t("appSettings.upgrading") : t("appSettings.upgradeSelectedAgents")}
        </Button>
      </div>
    </div>
  );
}
