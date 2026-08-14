import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Plus, Trash2, X, Package, RefreshCw } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button } from "../ui/Button";

const labelStyle: CSSProperties = {
  fontSize: 12,
  fontWeight: 600,
  color: "var(--text-secondary)",
  marginBottom: 5,
  display: "block",
};

const inputStyle: CSSProperties = {
  width: "100%",
  padding: "7px 10px",
  background: "var(--bg-input)",
  border: "1px solid var(--border-medium)",
  borderRadius: 7,
  color: "var(--text-primary)",
  fontSize: 12.5,
  fontFamily: "var(--font-mono)",
  outline: "none",
  boxSizing: "border-box",
};

const hintStyle: CSSProperties = {
  fontSize: 11,
  color: "var(--text-hint)",
  marginTop: 3,
};

interface DshPlugin {
  name: string;
  version: string;
  enabled: boolean;
  description?: string;
}

interface InstallDialogData {
  package: string;
  version: string;
}

export function DshPluginsPanel() {
  const { t } = useI18n();
  const [plugins, setPlugins] = useState<DshPlugin[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showInstallDialog, setShowInstallDialog] = useState(false);
  const [installData, setInstallData] = useState<InstallDialogData>({
    package: "",
    version: "latest",
  });
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  useEffect(() => {
    loadPlugins();
  }, []);

  async function loadPlugins() {
    setLoading(true);
    setError(null);
    try {
      const loaded = await invoke<DshPlugin[]>("list_dsh_plugins", { agent: "dsh" });
      setPlugins(loaded);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  function openInstallDialog() {
    setShowInstallDialog(true);
    setInstallData({ package: "", version: "latest" });
    setInstallError(null);
  }

  function closeInstallDialog() {
    setShowInstallDialog(false);
    setInstallError(null);
  }

  async function handleInstall() {
    const packageName = installData.package.trim();
    if (!packageName) {
      setInstallError(t("appSettings.dshPluginPackageRequired"));
      return;
    }

    setInstalling(true);
    setInstallError(null);
    try {
      await invoke("install_dsh_plugin", {
        agent: "dsh",
        package: packageName,
        version: installData.version === "latest" ? null : installData.version,
      });
      await loadPlugins();
      closeInstallDialog();
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setInstalling(false);
    }
  }

  async function handleUninstall(packageName: string) {
    try {
      await invoke("uninstall_dsh_plugin", { agent: "dsh", package: packageName });
      await loadPlugins();
      setDeleteConfirm(null);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleToggle(packageName: string, enabled: boolean) {
    try {
      await invoke("toggle_dsh_plugin", { agent: "dsh", package: packageName, enabled });
      setPlugins((prev) => prev.map((p) => (p.name === packageName ? { ...p, enabled } : p)));
    } catch (e) {
      setError(String(e));
    }
  }

  if (loading) {
    return (
      <div style={{ ...s.settingsBody, padding: 20 }}>
        <div style={{ color: "var(--text-hint)", fontSize: 13 }}>{t("common.loading")}</div>
      </div>
    );
  }

  return (
    <>
      <div style={{ ...s.settingsBody, padding: 20 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 16 }}>
          <Button onClick={openInstallDialog} style={{ fontSize: 12.5 }}>
            <Plus size={14} strokeWidth={2} />
            {t("appSettings.dshInstallPlugin")}
          </Button>
          <Button onClick={loadPlugins} style={{ fontSize: 12.5 }} disabled={loading}>
            <RefreshCw size={14} strokeWidth={2} />
            {t("common.refresh")}
          </Button>
        </div>

        {error && (
          <div
            style={{
              padding: 10,
              background: "var(--error-bg)",
              border: "1px solid var(--error-border)",
              borderRadius: 7,
              color: "var(--error-text)",
              fontSize: 12,
              marginBottom: 16,
            }}
          >
            {error}
          </div>
        )}

        {plugins.length === 0 ? (
          <div
            style={{
              padding: 40,
              textAlign: "center",
              color: "var(--text-hint)",
              fontSize: 13,
            }}
          >
            {t("appSettings.dshNoPlugins")}
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            {plugins.map((plugin) => (
              <div
                key={plugin.name}
                style={{
                  padding: 14,
                  background: "var(--surface-2)",
                  border: "1px solid var(--border-dim)",
                  borderRadius: 9,
                  display: "flex",
                  alignItems: "center",
                  gap: 12,
                }}
              >
                <Package size={18} color="var(--text-secondary)" />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                    {plugin.name}
                  </div>
                  <div style={{ fontSize: 11, color: "var(--text-hint)", marginTop: 2 }}>
                    {t("appSettings.dshPluginVersion", { version: plugin.version })}
                  </div>
                </div>
                <label
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    cursor: "pointer",
                    userSelect: "none",
                  }}
                >
                  <input
                    type="checkbox"
                    checked={plugin.enabled}
                    onChange={(e) => handleToggle(plugin.name, e.target.checked)}
                    style={{ display: "none" }}
                  />
                  <span style={{ fontSize: 11, color: "var(--text-secondary)" }}>
                    {t(plugin.enabled ? "common.enabled" : "common.disabled")}
                  </span>
                  <span
                    style={{
                      ...s.settingToggleTrack,
                      width: 36,
                      height: 20,
                      background: plugin.enabled
                        ? "var(--primary-action-bg)"
                        : "var(--border-medium)",
                    }}
                  >
                    <span
                      style={{
                        ...s.settingToggleKnob,
                        width: 16,
                        height: 16,
                        transform: plugin.enabled ? "translateX(16px)" : "translateX(0)",
                      }}
                    />
                  </span>
                </label>
                <button
                  type="button"
                  onClick={() => setDeleteConfirm(plugin.name)}
                  style={{
                    padding: "6px 10px",
                    background: "transparent",
                    border: "1px solid var(--border-medium)",
                    borderRadius: 6,
                    color: "var(--text-secondary)",
                    cursor: "pointer",
                    display: "flex",
                    alignItems: "center",
                    gap: 5,
                    fontSize: 11,
                  }}
                  title={t("common.delete")}
                >
                  <Trash2 size={13} strokeWidth={2} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {showInstallDialog && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0, 0, 0, 0.5)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1000,
          }}
          onClick={(e) => {
            if (e.target === e.currentTarget) closeInstallDialog();
          }}
        >
          <div
            style={{
              background: "var(--surface-1)",
              border: "1px solid var(--border-medium)",
              borderRadius: 12,
              width: "min(90%, 480px)",
              maxHeight: "80vh",
              overflow: "auto",
              boxShadow: "0 8px 32px rgba(0,0,0,0.3)",
            }}
          >
            <div
              style={{
                padding: "16px 20px",
                borderBottom: "1px solid var(--border-dim)",
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
              }}
            >
              <h3 style={{ margin: 0, fontSize: 14, fontWeight: 600 }}>
                {t("appSettings.dshInstallPlugin")}
              </h3>
              <button
                onClick={closeInstallDialog}
                style={{
                  background: "none",
                  border: "none",
                  cursor: "pointer",
                  padding: 4,
                  color: "var(--text-secondary)",
                }}
              >
                <X size={16} />
              </button>
            </div>

            <div style={{ padding: 20 }}>
              <div style={{ marginBottom: 16 }}>
                <label style={labelStyle}>{t("appSettings.dshPluginPackage")}</label>
                <input
                  type="text"
                  value={installData.package}
                  onChange={(e) => setInstallData({ ...installData, package: e.target.value })}
                  placeholder="@deepseek-ai/dsh-plugin-name"
                  style={inputStyle}
                  spellCheck={false}
                />
                <span style={hintStyle}>{t("appSettings.dshPluginPackageHint")}</span>
              </div>

              <div style={{ marginBottom: 16 }}>
                <label style={labelStyle}>{t("appSettings.dshPluginVersion")}</label>
                <input
                  type="text"
                  value={installData.version}
                  onChange={(e) => setInstallData({ ...installData, version: e.target.value })}
                  placeholder="latest"
                  style={inputStyle}
                  spellCheck={false}
                />
                <span style={hintStyle}>{t("appSettings.dshPluginVersionHint")}</span>
              </div>

              {installError && (
                <div
                  style={{
                    padding: 10,
                    background: "var(--error-bg)",
                    border: "1px solid var(--error-border)",
                    borderRadius: 7,
                    color: "var(--error-text)",
                    fontSize: 12,
                    marginBottom: 16,
                  }}
                >
                  {installError}
                </div>
              )}

              <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
                <Button onClick={closeInstallDialog} disabled={installing}>
                  {t("common.cancel")}
                </Button>
                <Button onClick={handleInstall} disabled={installing}>
                  {installing ? t("appSettings.dshInstalling") : t("common.install")}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}

      {deleteConfirm && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0, 0, 0, 0.5)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1000,
          }}
          onClick={(e) => {
            if (e.target === e.currentTarget) setDeleteConfirm(null);
          }}
        >
          <div
            style={{
              background: "var(--surface-1)",
              border: "1px solid var(--border-medium)",
              borderRadius: 12,
              width: "min(90%, 420px)",
              padding: 20,
              boxShadow: "0 8px 32px rgba(0,0,0,0.3)",
            }}
          >
            <h3 style={{ margin: "0 0 12px 0", fontSize: 14, fontWeight: 600 }}>
              {t("appSettings.dshUninstallPlugin")}
            </h3>
            <p style={{ margin: "0 0 20px 0", fontSize: 12.5, color: "var(--text-secondary)" }}>
              {t("appSettings.dshUninstallConfirm", { name: deleteConfirm })}
            </p>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <Button onClick={() => setDeleteConfirm(null)}>{t("common.cancel")}</Button>
              <Button
                onClick={() => handleUninstall(deleteConfirm)}
                style={{
                  background: "var(--error-bg)",
                  borderColor: "var(--error-border)",
                  color: "var(--error-text)",
                }}
              >
                {t("common.delete")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
