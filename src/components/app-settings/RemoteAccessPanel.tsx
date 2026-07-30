import { useCallback, useEffect, useState } from "react";
import type React from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { QRCodeSVG } from "qrcode.react";
import { RefreshCw, Smartphone, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { Button } from "../ui/Button";
import { IconButton } from "../IconButton";

interface RemoteStatus {
  enabled: boolean;
  running: boolean;
  port: number;
  lanIp: string | null;
  onlineCount: number;
  relayUrl: string | null;
  relayToken: string | null;
  publicEndpoints: string[];
  /** off | connecting | online | error:<msg> */
  relayState: string;
}

interface RemoteDevice {
  id: string;
  name: string;
  createdAt: number;
  lastSeenAt: number;
  online: boolean;
}

interface RemoteInvite {
  pairingUrl: string;
  endpoint: string;
  expiresInSeconds: number;
}

const labelStyle: React.CSSProperties = {
  fontSize: 12,
  fontWeight: 600,
  color: "var(--text-secondary)",
  marginBottom: 5,
  display: "block",
};

const hintStyle: React.CSSProperties = {
  fontSize: 11,
  color: "var(--text-hint)",
  marginTop: 3,
};

const portInputStyle: React.CSSProperties = {
  width: 90,
  padding: "6px 9px",
  background: "var(--bg-input)",
  border: "1px solid var(--border-medium)",
  borderRadius: 7,
  color: "var(--text-primary)",
  fontSize: 12.5,
  fontFamily: "var(--font-mono)",
  outline: "none",
};

const wideInputStyle: React.CSSProperties = {
  ...portInputStyle,
  width: "100%",
  boxSizing: "border-box",
};

const endpointsTextareaStyle: React.CSSProperties = {
  ...wideInputStyle,
  minHeight: 56,
  resize: "vertical",
  lineHeight: 1.5,
};

function formatTimestamp(ts: number, locale: string): string {
  if (!ts) return "—";
  return new Date(ts).toLocaleString(locale, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function RemoteAccessPanel() {
  const { language, t } = useI18n();
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [devices, setDevices] = useState<RemoteDevice[]>([]);
  const [portDraft, setPortDraft] = useState("");
  const [invite, setInvite] = useState<RemoteInvite | null>(null);
  const [pairedNotice, setPairedNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [relayUrlDraft, setRelayUrlDraft] = useState<string | null>(null);
  const [relayTokenDraft, setRelayTokenDraft] = useState<string | null>(null);
  const [endpointsDraft, setEndpointsDraft] = useState<string | null>(null);
  const [configSaved, setConfigSaved] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextDevices] = await Promise.all([
        invoke<RemoteStatus>("remote_server_status"),
        invoke<RemoteDevice[]>("remote_list_devices"),
      ]);
      setStatus(nextStatus);
      setDevices(nextDevices);
      setPortDraft((prev) => (prev === "" ? String(nextStatus.port) : prev));
      // 公网配置草稿只在首次加载时填充,避免覆盖正在编辑的内容
      setRelayUrlDraft((prev) => prev ?? nextStatus.relayUrl ?? "");
      setRelayTokenDraft((prev) => prev ?? nextStatus.relayToken ?? "");
      setEndpointsDraft((prev) => prev ?? nextStatus.publicEndpoints.join("\n"));
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // 配对成功:二维码切换成完成态,并刷新设备列表。
  useEffect(() => {
    const unlisten = listen<{ deviceName: string }>("remote-device-paired", (event) => {
      setInvite(null);
      setPairedNotice(event.payload.deviceName);
      void refresh();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const handleToggle = async () => {
    if (!status || busy) return;
    setBusy(true);
    setError(null);
    setInvite(null);
    try {
      const command = status.enabled ? "remote_server_stop" : "remote_server_start";
      const port = Number.parseInt(portDraft, 10);
      const args = status.enabled || Number.isNaN(port) ? {} : { port };
      setStatus(await invoke<RemoteStatus>(command, args));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleCreateInvite = async () => {
    setBusy(true);
    setError(null);
    setPairedNotice(null);
    try {
      setInvite(await invoke<RemoteInvite>("remote_create_invite"));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleRevoke = async (deviceId: string) => {
    setError(null);
    try {
      await invoke("remote_revoke_device", { deviceId });
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  };

  const handleSaveConfig = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setConfigSaved(false);
    try {
      const publicEndpoints = (endpointsDraft ?? "")
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      const next = await invoke<RemoteStatus>("remote_update_config", {
        relayUrl: relayUrlDraft ?? "",
        relayToken: relayTokenDraft ?? "",
        publicEndpoints,
      });
      setStatus(next);
      setRelayUrlDraft(next.relayUrl ?? "");
      setRelayTokenDraft(next.relayToken ?? "");
      setEndpointsDraft(next.publicEndpoints.join("\n"));
      setConfigSaved(true);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const relayStateLabel = (state: string): string => {
    if (state === "off") return t("appSettings.remote.relayState.off");
    if (state === "connecting") return t("appSettings.remote.relayState.connecting");
    if (state === "online") return t("appSettings.remote.relayState.online");
    return t("appSettings.remote.relayState.error", {
      message: state.startsWith("error:") ? state.slice("error:".length) : state,
    });
  };

  const enabled = status?.enabled ?? false;

  return (
    <div style={{ maxWidth: 560 }}>
      <p style={{ fontSize: 12, color: "var(--text-secondary)", lineHeight: 1.6, marginTop: 0 }}>
        {t("appSettings.remote.description")}
      </p>

      {/* ── 服务开关 + 端口 ── */}
      <div style={{ display: "flex", flexDirection: "column", gap: 5, marginTop: 14 }}>
        <label style={labelStyle}>{t("appSettings.remote.serverToggle")}</label>
        <button
          type="button"
          role="switch"
          aria-checked={enabled}
          aria-label={t("appSettings.remote.serverToggle")}
          onClick={() => void handleToggle()}
          style={s.settingToggle}
        >
          <span style={s.settingToggleLabel}>
            {status?.running
              ? t("appSettings.remote.statusRunning", {
                  endpoint: `ws://${status.lanIp ?? "?"}:${status.port}`,
                })
              : t("appSettings.remote.statusStopped")}
          </span>
          <span
            style={{
              ...s.settingToggleTrack,
              background: enabled ? "var(--primary-action-bg)" : "var(--border-medium)",
            }}
          >
            <span
              style={{
                ...s.settingToggleKnob,
                transform: enabled ? "translateX(16px)" : "translateX(0)",
              }}
            />
          </span>
        </button>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 6 }}>
          <label style={{ ...labelStyle, marginBottom: 0 }}>{t("appSettings.remote.port")}</label>
          <input
            style={portInputStyle}
            value={portDraft}
            disabled={enabled}
            inputMode="numeric"
            onChange={(e) => setPortDraft(e.target.value.replace(/[^0-9]/g, ""))}
          />
          {status?.running && status.onlineCount > 0 ? (
            <span style={{ fontSize: 11.5, color: "var(--text-secondary)" }}>
              {t("appSettings.remote.onlineCount", { count: String(status.onlineCount) })}
            </span>
          ) : null}
        </div>
        <span style={hintStyle}>{t("appSettings.remote.portHint")}</span>
      </div>

      {/* ── 公网访问(relay / 自定义地址) ── */}
      <div style={{ marginTop: 22 }}>
        <label style={labelStyle}>{t("appSettings.remote.publicAccess")}</label>
        <div style={hintStyle}>{t("appSettings.remote.publicAccessHint")}</div>
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginTop: 10 }}>
          <div>
            <label style={labelStyle}>{t("appSettings.remote.relayUrl")}</label>
            <input
              style={wideInputStyle}
              value={relayUrlDraft ?? ""}
              placeholder="wss://relay.example.com"
              spellCheck={false}
              onChange={(e) => {
                setRelayUrlDraft(e.target.value);
                setConfigSaved(false);
              }}
            />
            <div style={hintStyle}>{t("appSettings.remote.relayUrlHint")}</div>
          </div>
          <div>
            <label style={labelStyle}>{t("appSettings.remote.relayToken")}</label>
            <input
              style={wideInputStyle}
              type="password"
              value={relayTokenDraft ?? ""}
              spellCheck={false}
              autoComplete="off"
              onChange={(e) => {
                setRelayTokenDraft(e.target.value);
                setConfigSaved(false);
              }}
            />
            <div style={hintStyle}>{t("appSettings.remote.relayTokenHint")}</div>
          </div>
          <div>
            <label style={labelStyle}>{t("appSettings.remote.publicEndpoints")}</label>
            <textarea
              style={endpointsTextareaStyle}
              value={endpointsDraft ?? ""}
              placeholder={"ws://100.64.0.5:6790\nwss://aeroric.example.com"}
              spellCheck={false}
              onChange={(e) => {
                setEndpointsDraft(e.target.value);
                setConfigSaved(false);
              }}
            />
            <div style={hintStyle}>{t("appSettings.remote.publicEndpointsHint")}</div>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <Button onClick={() => void handleSaveConfig()} disabled={busy}>
              {t("appSettings.remote.saveConfig")}
            </Button>
            {status?.running && (status.relayUrl ?? "").length > 0 ? (
              <span
                style={{
                  fontSize: 11.5,
                  color:
                    status.relayState === "online"
                      ? "var(--success)"
                      : status.relayState.startsWith("error")
                        ? "var(--danger)"
                        : "var(--text-secondary)",
                }}
              >
                {relayStateLabel(status.relayState)}
              </span>
            ) : null}
          </div>
          {configSaved ? (
            <div style={{ ...hintStyle, color: "var(--success)", marginTop: 0 }}>
              {t("appSettings.remote.configSaved")}
            </div>
          ) : null}
        </div>
      </div>

      {/* ── 配对二维码 ── */}
      <div style={{ marginTop: 22 }}>
        <label style={labelStyle}>{t("appSettings.remote.pairing")}</label>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Button
            onClick={() => void handleCreateInvite()}
            disabled={!status?.running || busy}
            style={{ display: "inline-flex", alignItems: "center", gap: 6 }}
          >
            <RefreshCw size={13} strokeWidth={2} />
            {invite
              ? t("appSettings.remote.regenerateInvite")
              : t("appSettings.remote.createInvite")}
          </Button>
        </div>
        {!status?.running ? (
          <div style={hintStyle}>{t("appSettings.remote.pairingNeedsServer")}</div>
        ) : null}
        {invite ? (
          <div
            style={{
              marginTop: 12,
              display: "inline-flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 8,
              padding: 14,
              background: "#fff",
              borderRadius: 10,
              border: "1px solid var(--border-medium)",
            }}
          >
            <QRCodeSVG value={invite.pairingUrl} size={190} marginSize={1} />
            <span style={{ fontSize: 11, color: "#555", fontFamily: "var(--font-mono)" }}>
              {invite.endpoint}
            </span>
          </div>
        ) : null}
        {invite ? <div style={hintStyle}>{t("appSettings.remote.inviteHint")}</div> : null}
        {pairedNotice ? (
          <div style={{ ...hintStyle, color: "var(--success)" }}>
            {t("appSettings.remote.pairedNotice", { name: pairedNotice })}
          </div>
        ) : null}
      </div>

      {/* ── 已配对设备 ── */}
      <div style={{ marginTop: 22 }}>
        <label style={labelStyle}>{t("appSettings.remote.devices")}</label>
        {devices.length === 0 ? (
          <div style={hintStyle}>{t("appSettings.remote.noDevices")}</div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {devices.map((device) => (
              <div
                key={device.id}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  padding: "8px 10px",
                  border: "1px solid var(--border-light)",
                  borderRadius: 8,
                  background: "var(--bg-secondary)",
                }}
              >
                <Smartphone size={15} strokeWidth={1.8} color="var(--text-secondary)" />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 12.5, color: "var(--text-primary)", fontWeight: 600 }}>
                    {device.name}
                    {device.online ? (
                      <span
                        style={{
                          display: "inline-block",
                          width: 7,
                          height: 7,
                          borderRadius: "50%",
                          background: "var(--success)",
                          marginLeft: 7,
                        }}
                        title={t("appSettings.remote.online")}
                      />
                    ) : null}
                  </div>
                  <div style={{ fontSize: 11, color: "var(--text-hint)" }}>
                    {t("appSettings.remote.lastSeen", {
                      time: formatTimestamp(
                        device.lastSeenAt,
                        language === "zh" ? "zh-CN" : "en-US",
                      ),
                    })}
                  </div>
                </div>
                <IconButton
                  icon={<Trash2 size={14} strokeWidth={1.8} />}
                  title={t("appSettings.remote.revoke")}
                  size={28}
                  onClick={() => void handleRevoke(device.id)}
                />
              </div>
            ))}
          </div>
        )}
      </div>

      {error ? (
        <div style={{ ...hintStyle, color: "var(--danger)", marginTop: 14 }}>{error}</div>
      ) : null}
    </div>
  );
}
