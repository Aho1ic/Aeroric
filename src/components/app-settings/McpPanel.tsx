import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, Plus, Edit2, Trash2, TriangleAlert, X } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { zLayers } from "../../styles/zLayers";
import { Button } from "../ui/Button";
import { APP_SETTINGS_CHANGED_EVENT, type McpSettings, type McpServer } from "./types";

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

const textareaStyle: CSSProperties = {
  ...inputStyle,
  minHeight: 80,
  resize: "vertical",
  fontFamily: "var(--font-mono)",
};

const hintStyle: CSSProperties = {
  fontSize: 11,
  color: "var(--text-hint)",
  marginTop: 3,
};

type McpTestResult =
  | { status: "success"; message: string; serverName?: string; serverVersion?: string }
  | { status: "error"; message: string; stderr?: string }
  | { status: "timeout"; message: string };

interface ServerDialogData {
  name: string;
  command: string;
  args: string;
  env: string;
  enabled: boolean;
}

function parseEnv(text: string): Record<string, string> {
  const result: Record<string, string> = {};
  text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .forEach((line) => {
      const eq = line.indexOf("=");
      if (eq > 0) {
        result[line.slice(0, eq)] = line.slice(eq + 1);
      }
    });
  return result;
}

function formatEnv(env?: Record<string, string>): string {
  if (!env || Object.keys(env).length === 0) return "";
  return Object.entries(env)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
}

function parseArgs(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

function formatArgs(args?: string[]): string {
  return args && args.length > 0 ? args.join("\n") : "";
}

export function McpPanel() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<McpSettings>({ servers: {}, enabled: false });
  const [originalSettings, setOriginalSettings] = useState<McpSettings>({
    servers: {},
    enabled: false,
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dialogMode, setDialogMode] = useState<"add" | "edit" | null>(null);
  const [editingName, setEditingName] = useState<string | null>(null);
  const [dialogData, setDialogData] = useState<ServerDialogData>({
    name: "",
    command: "",
    args: "",
    env: "",
    enabled: true,
  });
  const [dialogError, setDialogError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<McpTestResult | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  useEffect(() => {
    loadSettings();
  }, []);

  async function loadSettings() {
    setLoading(true);
    setError(null);
    try {
      const loaded = await invoke<McpSettings>("get_mcp_settings");
      setSettings(loaded);
      setOriginalSettings(JSON.parse(JSON.stringify(loaded)));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleSave() {
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      const updated = await invoke<McpSettings>("set_mcp_settings", { settings });
      setSettings(updated);
      setOriginalSettings(JSON.parse(JSON.stringify(updated)));
      window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  function openAddDialog() {
    setDialogMode("add");
    setEditingName(null);
    setDialogData({
      name: "",
      command: "",
      args: "",
      env: "",
      enabled: true,
    });
    setDialogError(null);
    setTestResult(null);
  }

  function openEditDialog(name: string, server: McpServer) {
    setDialogMode("edit");
    setEditingName(name);
    setDialogData({
      name,
      command: server.command,
      args: formatArgs(server.args),
      env: formatEnv(server.env),
      enabled: server.enabled ?? true,
    });
    setDialogError(null);
    setTestResult(null);
  }

  function closeDialog() {
    setDialogMode(null);
    setEditingName(null);
    setTestResult(null);
  }

  function handleDialogSave() {
    setDialogError(null);
    const trimmedName = dialogData.name.trim();
    const trimmedCommand = dialogData.command.trim();

    if (!trimmedName) {
      setDialogError(t("appSettings.mcpServerNameRequired"));
      return;
    }
    if (!trimmedCommand) {
      setDialogError(t("appSettings.mcpServerCommandRequired"));
      return;
    }

    if (dialogMode === "add" && settings.servers[trimmedName]) {
      setDialogError(t("appSettings.mcpServerNameRequired"));
      return;
    }

    if (dialogMode === "edit" && editingName && trimmedName !== editingName) {
      if (settings.servers[trimmedName]) {
        setDialogError(t("appSettings.mcpServerNameRequired"));
        return;
      }
    }

    const newServer: McpServer = {
      name: trimmedName,
      command: trimmedCommand,
      args: parseArgs(dialogData.args),
      env: parseEnv(dialogData.env),
      enabled: dialogData.enabled,
    };

    const newServers = { ...settings.servers };
    if (dialogMode === "edit" && editingName && trimmedName !== editingName) {
      delete newServers[editingName];
    }
    newServers[trimmedName] = newServer;

    setSettings({ ...settings, servers: newServers });
    closeDialog();
  }

  async function handleTestConnection() {
    setTesting(true);
    setTestResult(null);
    setDialogError(null);
    try {
      const config: McpServer = {
        name: dialogData.name.trim(),
        command: dialogData.command.trim(),
        args: parseArgs(dialogData.args),
        env: parseEnv(dialogData.env),
        enabled: dialogData.enabled,
      };
      const result = await invoke<McpTestResult>("test_mcp_server", { config });
      setTestResult(result);
    } catch (e) {
      setTestResult({ status: "error", message: String(e) });
    } finally {
      setTesting(false);
    }
  }

  function handleDelete(name: string) {
    const newServers = { ...settings.servers };
    delete newServers[name];
    setSettings({ ...settings, servers: newServers });
    setDeleteConfirm(null);
  }

  const isDirty = JSON.stringify(settings) !== JSON.stringify(originalSettings);
  const servers = Object.entries(settings.servers);

  return (
    <>
      <div
        style={{
          ...s.settingsBody,
          display: "flex",
          flexDirection: "column",
          gap: 14,
          padding: "18px 20px 14px",
        }}
      >
        {error && <div style={{ color: "var(--danger)", fontSize: 12.5 }}>{error}</div>}
        {loading && (
          <div style={{ color: "var(--text-hint)", fontSize: 13 }}>{t("common.loading")}</div>
        )}

        <div>
          <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
            <input
              type="checkbox"
              checked={settings.enabled}
              onChange={(e) => setSettings({ ...settings, enabled: e.target.checked })}
              disabled={loading}
              style={{ cursor: "pointer" }}
            />
            <span style={{ fontSize: 12.5, fontWeight: 600, color: "var(--text-primary)" }}>
              {t("appSettings.mcpEnabled")}
            </span>
          </label>
          <div style={hintStyle}>{t("appSettings.mcpEnabledHint")}</div>
        </div>

        <div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              marginBottom: 8,
            }}
          >
            <label style={labelStyle}>{t("appSettings.mcpServers")}</label>
            <Button
              variant="outline"
              size="xs"
              onClick={openAddDialog}
              disabled={loading}
              style={{ marginBottom: 5 }}
            >
              <Plus size={13} /> {t("appSettings.mcpAddServer")}
            </Button>
          </div>

          {servers.length === 0 ? (
            <div
              style={{
                padding: "20px 16px",
                textAlign: "center",
                color: "var(--text-hint)",
                fontSize: 12.5,
                background: "var(--bg-card)",
                border: "1px solid var(--border-dim)",
                borderRadius: 8,
              }}
            >
              {t("appSettings.mcpNoServers")}
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {servers.map(([name, server]) => (
                <div
                  key={name}
                  style={{
                    padding: "12px 14px",
                    background: "var(--bg-card)",
                    border: "1px solid var(--border-medium)",
                    borderRadius: 8,
                    display: "flex",
                    alignItems: "flex-start",
                    gap: 10,
                  }}
                >
                  <input
                    type="checkbox"
                    checked={server.enabled ?? true}
                    onChange={(e) => {
                      const newServers = { ...settings.servers };
                      newServers[name] = { ...server, enabled: e.target.checked };
                      setSettings({ ...settings, servers: newServers });
                    }}
                    disabled={loading}
                    style={{ marginTop: 2, cursor: "pointer", flexShrink: 0 }}
                  />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div
                      style={{
                        fontSize: 13,
                        fontWeight: 600,
                        color: "var(--text-primary)",
                        marginBottom: 4,
                      }}
                    >
                      {name}
                    </div>
                    <div
                      style={{
                        fontSize: 11.5,
                        fontFamily: "var(--font-mono)",
                        color: "var(--text-secondary)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {server.command}
                      {server.args && server.args.length > 0 && ` ${server.args.join(" ")}`}
                    </div>
                  </div>
                  <div style={{ display: "flex", gap: 4, flexShrink: 0 }}>
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      onClick={() => openEditDialog(name, server)}
                      disabled={loading}
                      title={t("appSettings.mcpEditServer")}
                    >
                      <Edit2 size={13} />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      onClick={() => setDeleteConfirm(name)}
                      disabled={loading}
                      title={t("appSettings.mcpDeleteServer")}
                    >
                      <Trash2 size={13} />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      <div style={s.settingsFooter}>
        {saved && (
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              fontSize: 12,
              color: "var(--success)",
            }}
          >
            <Check size={12} /> {t("common.saved")}
          </span>
        )}
        <Button variant="default" size="sm" onClick={handleSave} disabled={saving || !isDirty}>
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>

      {dialogMode && (
        <div
          role="presentation"
          style={{
            position: "fixed",
            inset: 0,
            // AppSettingsDialog(overlay)内部弹出,需高于其遮罩。
            zIndex: zLayers.overlayNested,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "rgba(0,0,0,0.36)",
          }}
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) closeDialog();
          }}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label={
              dialogMode === "add" ? t("appSettings.mcpAddServer") : t("appSettings.mcpEditServer")
            }
            style={{
              width: "min(560px, calc(100vw - 48px))",
              maxHeight: "min(680px, calc(100vh - 80px))",
              display: "flex",
              flexDirection: "column",
              border: "1px solid color-mix(in srgb, var(--border-medium) 72%, #ffffff 28%)",
              borderRadius: 28,
              background: "var(--bg-card)",
              boxShadow: "var(--shadow-popover)",
              overflow: "hidden",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "14px 18px",
                borderBottom: "1px solid var(--border-dim)",
                flexShrink: 0,
              }}
            >
              <span style={{ fontSize: 14, fontWeight: 700, color: "var(--text-primary)" }}>
                {dialogMode === "add"
                  ? t("appSettings.mcpAddServer")
                  : t("appSettings.mcpEditServer")}
              </span>
              <button
                type="button"
                onClick={closeDialog}
                style={{
                  width: 26,
                  height: 26,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  border: "none",
                  borderRadius: 6,
                  background: "transparent",
                  color: "var(--text-secondary)",
                  cursor: "pointer",
                }}
                title={t("common.close")}
              >
                <X size={15} />
              </button>
            </div>

            <div
              style={{
                flex: 1,
                minHeight: 0,
                overflow: "auto",
                padding: "18px 20px",
                display: "flex",
                flexDirection: "column",
                gap: 14,
              }}
            >
              {dialogError && (
                <div style={{ color: "var(--danger)", fontSize: 12.5 }}>{dialogError}</div>
              )}

              <div>
                <label style={labelStyle} htmlFor="mcp-server-name">
                  {t("appSettings.mcpServerName")}
                </label>
                <input
                  id="mcp-server-name"
                  style={inputStyle}
                  value={dialogData.name}
                  onChange={(e) => setDialogData({ ...dialogData, name: e.target.value })}
                  placeholder="filesystem"
                  disabled={dialogMode === "edit"}
                  spellCheck={false}
                />
              </div>

              <div>
                <label style={labelStyle} htmlFor="mcp-server-command">
                  {t("appSettings.mcpServerCommand")}
                </label>
                <input
                  id="mcp-server-command"
                  style={inputStyle}
                  value={dialogData.command}
                  onChange={(e) => setDialogData({ ...dialogData, command: e.target.value })}
                  placeholder="npx"
                  spellCheck={false}
                />
              </div>

              <div>
                <label style={labelStyle} htmlFor="mcp-server-args">
                  {t("appSettings.mcpServerArgs")}
                </label>
                <textarea
                  id="mcp-server-args"
                  style={textareaStyle}
                  value={dialogData.args}
                  onChange={(e) => setDialogData({ ...dialogData, args: e.target.value })}
                  placeholder="-y&#10;@modelcontextprotocol/server-filesystem&#10;/path/to/directory"
                  spellCheck={false}
                />
                <div style={hintStyle}>{t("appSettings.mcpServerArgsHint")}</div>
              </div>

              <div>
                <label style={labelStyle} htmlFor="mcp-server-env">
                  {t("appSettings.mcpServerEnv")}
                </label>
                <textarea
                  id="mcp-server-env"
                  style={textareaStyle}
                  value={dialogData.env}
                  onChange={(e) => setDialogData({ ...dialogData, env: e.target.value })}
                  placeholder="API_KEY=your_key_here&#10;DEBUG=true"
                  spellCheck={false}
                />
                <div style={hintStyle}>{t("appSettings.mcpServerEnvHint")}</div>
              </div>

              <div>
                <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                  <input
                    type="checkbox"
                    checked={dialogData.enabled}
                    onChange={(e) => setDialogData({ ...dialogData, enabled: e.target.checked })}
                    style={{ cursor: "pointer" }}
                  />
                  <span style={{ fontSize: 12.5, fontWeight: 600, color: "var(--text-primary)" }}>
                    {t("appSettings.mcpServerEnabled")}
                  </span>
                </label>
              </div>

              {testResult && (
                <div
                  role="status"
                  style={{
                    padding: "10px 12px",
                    borderRadius: 7,
                    fontSize: 12,
                    border: `1px solid ${testResult.status === "success" ? "var(--success)" : "var(--danger)"}`,
                    background:
                      testResult.status === "success"
                        ? "color-mix(in srgb, var(--success) 8%, transparent)"
                        : "color-mix(in srgb, var(--danger) 8%, transparent)",
                    color: testResult.status === "success" ? "var(--success)" : "var(--danger)",
                  }}
                >
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    {testResult.status === "success" ? (
                      <Check size={13} style={{ flexShrink: 0 }} />
                    ) : (
                      <TriangleAlert size={13} style={{ flexShrink: 0 }} />
                    )}
                    <div style={{ flex: 1 }}>
                      <div>{testResult.message}</div>
                      {testResult.status === "success" && testResult.serverName && (
                        <div style={{ marginTop: 4, fontSize: 11, opacity: 0.9 }}>
                          {testResult.serverName}
                          {testResult.serverVersion && ` v${testResult.serverVersion}`}
                        </div>
                      )}
                      {testResult.status === "error" && testResult.stderr && (
                        <pre
                          style={{
                            marginTop: 6,
                            fontSize: 10.5,
                            fontFamily: "var(--font-mono)",
                            whiteSpace: "pre-wrap",
                            maxHeight: 100,
                            overflow: "auto",
                            opacity: 0.85,
                          }}
                        >
                          {testResult.stderr}
                        </pre>
                      )}
                    </div>
                  </div>
                </div>
              )}
            </div>

            <div
              style={{
                padding: "12px 18px",
                borderTop: "1px solid var(--border-dim)",
                display: "flex",
                alignItems: "center",
                justifyContent: "flex-end",
                gap: 8,
                flexShrink: 0,
              }}
            >
              <Button
                variant="outline"
                size="sm"
                onClick={handleTestConnection}
                disabled={testing || !dialogData.command.trim()}
              >
                {testing ? t("appSettings.mcpTestingServer") : t("appSettings.mcpTestServer")}
              </Button>
              <Button variant="outline" size="sm" onClick={closeDialog}>
                {t("common.cancel")}
              </Button>
              <Button variant="default" size="sm" onClick={handleDialogSave}>
                {t("common.save")}
              </Button>
            </div>
          </div>
        </div>
      )}

      {deleteConfirm && (
        <div
          role="presentation"
          style={{
            position: "fixed",
            inset: 0,
            // 可能叠在编辑弹窗(overlayNested)之上。
            zIndex: zLayers.overlayNestedDeep,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "rgba(0,0,0,0.36)",
          }}
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) setDeleteConfirm(null);
          }}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-label={t("appSettings.mcpDeleteServer")}
            style={{
              width: "min(400px, calc(100vw - 48px))",
              border: "1px solid color-mix(in srgb, var(--border-medium) 72%, #ffffff 28%)",
              borderRadius: 20,
              background: "var(--bg-card)",
              boxShadow: "var(--shadow-popover)",
              padding: "20px 22px",
              display: "flex",
              flexDirection: "column",
              gap: 16,
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text-primary)" }}>
              {t("appSettings.mcpConfirmDelete", { name: deleteConfirm })}
            </div>
            <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
              <Button variant="outline" size="sm" onClick={() => setDeleteConfirm(null)}>
                {t("common.cancel")}
              </Button>
              <Button variant="destructive" size="sm" onClick={() => handleDelete(deleteConfirm)}>
                {t("common.delete")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
