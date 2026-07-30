import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { Eye, EyeOff, RefreshCw, RotateCcw, Save } from "lucide-react";
import type {
  WslDistribution,
  WslDistributionProbe,
  WslEnvironment,
  WslSettings,
  WslStatus,
} from "../../types";
import { useI18n } from "../../i18n";
import { AnimatedSelectionTrack } from "../ui/AnimatedSelection";
import s from "../../styles";

const SENSITIVE_ENV = /(TOKEN|SECRET|PASSWORD|COOKIE|AUTH|KEY)/i;
const inputStyle: React.CSSProperties = {
  width: "100%",
  boxSizing: "border-box",
  padding: "8px 10px",
  border: "1px solid var(--border-medium)",
  borderRadius: 7,
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  fontFamily: "var(--font-ui)",
  outline: "none",
};

function emptyDistributionSettings() {
  return { agentPaths: {}, agentConfigPaths: {} };
}

export function maskWslEnvironmentValue(
  name: string,
  value: string,
  reveal: boolean,
  sensitiveNames?: readonly string[],
): string {
  if (reveal || !value) return value;
  const sensitive = SENSITIVE_ENV.test(name) || Boolean(sensitiveNames?.includes(name));
  return sensitive ? "••••••••" : value;
}

export function WslPanel() {
  const { t } = useI18n();
  const [status, setStatus] = useState<WslStatus | null>(null);
  const [distributions, setDistributions] = useState<WslDistribution[]>([]);
  const [settings, setSettings] = useState<WslSettings>({ distributions: {} });
  const [selected, setSelected] = useState("");
  const [probe, setProbe] = useState<WslDistributionProbe | null>(null);
  const [environment, setEnvironment] = useState<WslEnvironment | null>(null);
  const [globalConfig, setGlobalConfig] = useState("");
  const [wslConf, setWslConf] = useState("");
  const [agentConfigs, setAgentConfigs] = useState<Record<string, string>>({});
  const [revealSensitive, setRevealSensitive] = useState(false);
  const [dirtyRestart, setDirtyRestart] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedSettings = useMemo(
    () => settings.distributions[selected] ?? emptyDistributionSettings(),
    [selected, settings.distributions],
  );

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const [nextStatus, nextDistributions, nextSettings, nextGlobalConfig] = await Promise.all([
        invoke<WslStatus>("get_wsl_status"),
        invoke<WslDistribution[]>("list_wsl_distributions"),
        invoke<WslSettings>("load_wsl_settings"),
        invoke<string | null>("read_wsl_config_file", { kind: "global", distribution: null }),
      ]);
      setStatus(nextStatus);
      setDistributions(nextDistributions);
      setSettings(nextSettings);
      setGlobalConfig(nextGlobalConfig ?? "");
      const preferred =
        nextSettings.defaultDistribution ??
        nextDistributions.find((item) => item.isDefault)?.name ??
        nextDistributions[0]?.name ??
        "";
      setSelected((current) =>
        current && nextDistributions.some((item) => item.name === current) ? current : preferred,
      );
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (!selected) {
      setProbe(null);
      setEnvironment(null);
      return;
    }
    let cancelled = false;
    Promise.all([
      invoke<WslDistributionProbe>("probe_wsl_distribution", { distribution: selected }),
      invoke<WslEnvironment>("read_wsl_environment", { distribution: selected }),
      invoke<string | null>("read_wsl_config_file", {
        distribution: selected,
        kind: "wslConf",
      }),
      ...["claude", "codex"].map((agent) =>
        invoke<string | null>("read_wsl_agent_config", { distribution: selected, agent }).then(
          (content) => [agent, content ?? ""] as const,
        ),
      ),
    ])
      .then(([nextProbe, nextEnvironment, nextWslConf, ...configs]) => {
        if (cancelled) return;
        setProbe(nextProbe);
        setEnvironment(nextEnvironment);
        setWslConf(nextWslConf ?? "");
        setAgentConfigs(Object.fromEntries(configs));
      })
      .catch((nextError) => {
        if (!cancelled) setError(String(nextError));
      });
    return () => {
      cancelled = true;
    };
  }, [selected]);

  const patchSelectedSettings = (patch: Partial<typeof selectedSettings>) => {
    setSettings((current) => ({
      ...current,
      distributions: {
        ...current.distributions,
        [selected]: { ...selectedSettings, ...patch },
      },
    }));
  };

  const saveAll = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("save_wsl_settings", { settings });
      await invoke("write_wsl_config_file", {
        kind: "global",
        distribution: null,
        content: globalConfig,
      });
      if (selected) {
        await invoke("write_wsl_config_file", {
          kind: "wslConf",
          distribution: selected,
          content: wslConf,
        });
        await Promise.all(
          Object.entries(agentConfigs).map(([agent, content]) =>
            invoke("write_wsl_agent_config", { distribution: selected, agent, content }),
          ),
        );
      }
      setDirtyRestart(true);
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setBusy(false);
    }
  };

  const restart = async () => {
    const accepted = await confirm(t("wsl.restartConfirm"), {
      title: t("wsl.restart"),
      kind: "warning",
    });
    if (!accepted) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("restart_wsl");
      setDirtyRestart(false);
      await load();
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      style={{ padding: 20, overflowY: "auto", display: "flex", flexDirection: "column", gap: 18 }}
    >
      <div
        style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12 }}
      >
        <div>
          <div style={{ fontSize: 13, fontWeight: 700 }}>{t("wsl.title")}</div>
          <div style={{ marginTop: 4, fontSize: 11, color: "var(--text-muted)" }}>
            {status?.installed
              ? t("wsl.installedSummary", { count: status.distributionCount })
              : status?.error || t("wsl.notInstalled")}
          </div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button style={s.secondaryActionBtn} onClick={() => void load()} disabled={busy}>
            <RefreshCw size={14} />
            {t("common.refresh")}
          </button>
          <button style={s.primaryActionBtn} onClick={() => void saveAll()} disabled={busy}>
            <Save size={14} />
            {t("common.save")}
          </button>
        </div>
      </div>

      {distributions.length > 0 && (
        <AnimatedSelectionTrack
          value={selected}
          ariaLabel={t("wsl.distribution")}
          style={{ display: "flex", flexWrap: "wrap", gap: 6, padding: 4 }}
        >
          {distributions.map((distribution) => (
            <button
              key={distribution.name}
              data-animated-selection-item
              data-selection-value={distribution.name}
              aria-pressed={selected === distribution.name}
              onClick={() => setSelected(distribution.name)}
              style={{
                position: "relative",
                zIndex: 1,
                border: "none",
                borderRadius: 7,
                padding: "7px 10px",
                background: "transparent",
                color:
                  selected === distribution.name
                    ? "var(--control-active-fg)"
                    : "var(--text-secondary)",
                cursor: "pointer",
              }}
            >
              {distribution.name} · WSL{distribution.version ?? "?"}
            </button>
          ))}
        </AnimatedSelectionTrack>
      )}

      {selected && (
        <>
          <label style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <span style={{ fontSize: 12, fontWeight: 650 }}>{t("wsl.defaultDistribution")}</span>
            <select
              value={settings.defaultDistribution ?? ""}
              onChange={(event) =>
                setSettings((current) => ({
                  ...current,
                  defaultDistribution: event.target.value || undefined,
                }))
              }
              style={inputStyle}
            >
              <option value="">{t("wsl.useSystemDefault")}</option>
              {distributions.map((distribution) => (
                <option key={distribution.name} value={distribution.name}>
                  {distribution.name}
                </option>
              ))}
            </select>
          </label>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            {[
              [t("wsl.home"), probe?.home ?? ""],
              [t("wsl.loginShell"), environment?.shell ?? probe?.shell ?? ""],
              ["PATH", environment?.path ?? ""],
              [t("wsl.user"), probe?.user ?? ""],
            ].map(([label, value]) => (
              <div key={label} style={{ minWidth: 0 }}>
                <div style={{ fontSize: 11, color: "var(--text-muted)" }}>{label}</div>
                <div
                  style={{
                    marginTop: 4,
                    fontFamily: "var(--font-mono)",
                    fontSize: 11,
                    overflowWrap: "anywhere",
                  }}
                >
                  {value}
                </div>
              </div>
            ))}
          </div>
          <label style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <span style={{ fontSize: 12, fontWeight: 650 }}>{t("wsl.shellOverride")}</span>
            <input
              value={selectedSettings.shellOverride ?? ""}
              onChange={(event) => patchSelectedSettings({ shellOverride: event.target.value })}
              placeholder={probe?.shell}
              style={inputStyle}
            />
          </label>
          {["claude", "codex"].map((agent) => (
            <div key={agent} style={{ display: "flex", flexDirection: "column", gap: 7 }}>
              <div style={{ fontSize: 12, fontWeight: 700 }}>
                {agent === "claude" ? "Claude" : "Codex"}
              </div>
              <input
                aria-label={`${agent} executable`}
                value={selectedSettings.agentPaths[agent] ?? ""}
                onChange={(event) =>
                  patchSelectedSettings({
                    agentPaths: { ...selectedSettings.agentPaths, [agent]: event.target.value },
                  })
                }
                placeholder={
                  agent === "claude" ? probe?.claudePath || "claude" : probe?.codexPath || "codex"
                }
                style={inputStyle}
              />
              <input
                aria-label={`${agent} config path`}
                value={selectedSettings.agentConfigPaths[agent] ?? ""}
                onChange={(event) =>
                  patchSelectedSettings({
                    agentConfigPaths: {
                      ...selectedSettings.agentConfigPaths,
                      [agent]: event.target.value,
                    },
                  })
                }
                placeholder={
                  agent === "claude"
                    ? `${probe?.home ?? "$HOME"}/.claude/settings.json`
                    : `${probe?.home ?? "$HOME"}/.codex/config.toml`
                }
                style={inputStyle}
              />
              <textarea
                aria-label={`${agent} config`}
                value={agentConfigs[agent] ?? ""}
                onChange={(event) =>
                  setAgentConfigs((current) => ({ ...current, [agent]: event.target.value }))
                }
                style={{
                  ...inputStyle,
                  minHeight: 110,
                  fontFamily: "var(--font-mono)",
                  resize: "vertical",
                }}
              />
            </div>
          ))}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <label style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span style={{ fontSize: 12, fontWeight: 650 }}>.wslconfig</span>
              <textarea
                value={globalConfig}
                onChange={(event) => setGlobalConfig(event.target.value)}
                style={{
                  ...inputStyle,
                  minHeight: 150,
                  fontFamily: "var(--font-mono)",
                  resize: "vertical",
                }}
              />
            </label>
            <label style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span style={{ fontSize: 12, fontWeight: 650 }}>/etc/wsl.conf</span>
              <textarea
                value={wslConf}
                onChange={(event) => setWslConf(event.target.value)}
                style={{
                  ...inputStyle,
                  minHeight: 150,
                  fontFamily: "var(--font-mono)",
                  resize: "vertical",
                }}
              />
            </label>
          </div>
          <div>
            <button
              style={s.secondaryActionBtn}
              onClick={() => setRevealSensitive((value) => !value)}
            >
              {revealSensitive ? <EyeOff size={14} /> : <Eye size={14} />}
              {revealSensitive ? t("wsl.hideSensitive") : t("wsl.revealSensitive")}
            </button>
            <div
              style={{
                marginTop: 8,
                maxHeight: 220,
                overflow: "auto",
                fontFamily: "var(--font-mono)",
                fontSize: 11,
              }}
            >
              {Object.entries(environment?.variables ?? {}).map(([name, value]) => (
                <div
                  key={name}
                  style={{
                    display: "grid",
                    gridTemplateColumns: "180px 1fr",
                    gap: 8,
                    padding: "3px 0",
                  }}
                >
                  <span style={{ color: "var(--text-muted)" }}>{name}</span>
                  <span style={{ overflowWrap: "anywhere" }}>
                    {maskWslEnvironmentValue(
                      name,
                      value,
                      revealSensitive,
                      environment?.sensitiveNames,
                    )}
                  </span>
                </div>
              ))}
            </div>
          </div>
          {dirtyRestart && (
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span style={{ color: "var(--warning)", fontSize: 12 }}>
                {t("wsl.restartRequired")}
              </span>
              <button style={s.secondaryActionBtn} onClick={() => void restart()} disabled={busy}>
                <RotateCcw size={14} />
                {t("wsl.restart")}
              </button>
            </div>
          )}
        </>
      )}
      {error && <div style={{ color: "var(--danger)", fontSize: 12 }}>{error}</div>}
    </div>
  );
}
