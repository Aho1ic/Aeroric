import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "../../lib/appDialog";
import { QRCodeSVG } from "qrcode.react";
import {
  AlertCircle,
  Check,
  ChevronDown,
  Copy,
  Globe2,
  RefreshCw,
  Server,
  Smartphone,
  Trash2,
} from "lucide-react";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { writeClipboardText } from "../file-explorer/clipboard";
import { Button } from "../ui/Button";

interface RemoteStatus {
  enabled: boolean;
  running: boolean;
  networkExposed: boolean;
  port: number;
  lanIp: string | null;
  lanAddresses: RemoteNetworkAddress[];
  onlineCount: number;
  relayUrl: string | null;
  relayToken: string | null;
  publicEndpoints: string[];
  /** off | connecting | online | error:<msg> */
  relayState: string;
}

interface RemoteNetworkAddress {
  interfaceName: string;
  ip: string;
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

interface RemotePairedEvent {
  deviceId?: string;
  deviceName: string;
}

interface NormalizedPublicConfig {
  relayUrl: string;
  relayToken: string;
  publicEndpoints: string[];
}

type CopyTarget = "address" | "pairing";
type ServiceAction = "starting" | "stopping";

const REMOTE_REFRESH_INTERVAL_MS = 5_000;
const FEEDBACK_DURATION_MS = 2_000;
const MIN_REMOTE_PORT = 1024;
const MAX_REMOTE_PORT = 65535;

function formatTimestamp(ts: number, locale: string): string {
  if (!ts) return "—";
  return new Date(ts).toLocaleString(locale, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatRemainingTime(seconds: number): string {
  const safeSeconds = Math.max(0, seconds);
  const minutes = Math.floor(safeSeconds / 60);
  const remainder = safeSeconds % 60;
  return `${minutes}:${String(remainder).padStart(2, "0")}`;
}

function normalizeWsAddress(value: string): string {
  return value.trim().replace(/\/+$/, "");
}

function normalizePublicConfig(
  relayUrl: string | null,
  relayToken: string | null,
  endpoints: string | null,
): NormalizedPublicConfig {
  const publicEndpoints = Array.from(
    new Set(
      (endpoints ?? "")
        .split("\n")
        .map(normalizeWsAddress)
        .filter((endpoint) => endpoint.length > 0),
    ),
  );
  return {
    relayUrl: normalizeWsAddress(relayUrl ?? ""),
    relayToken: (relayToken ?? "").trim(),
    publicEndpoints,
  };
}

function publicConfigMatchesStatus(config: NormalizedPublicConfig, status: RemoteStatus): boolean {
  return (
    config.relayUrl === (status.relayUrl ?? "") &&
    config.relayToken === (status.relayToken ?? "") &&
    config.publicEndpoints.length === status.publicEndpoints.length &&
    config.publicEndpoints.every((endpoint, index) => endpoint === status.publicEndpoints[index])
  );
}

export function RemoteAccessPanel() {
  const { language, t } = useI18n();
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [devices, setDevices] = useState<RemoteDevice[]>([]);
  const [portDraft, setPortDraft] = useState("");
  const [relayUrlDraft, setRelayUrlDraft] = useState<string | null>(null);
  const [relayTokenDraft, setRelayTokenDraft] = useState<string | null>(null);
  const [endpointsDraft, setEndpointsDraft] = useState<string | null>(null);
  const [invite, setInvite] = useState<RemoteInvite | null>(null);
  const [inviteExpiresAt, setInviteExpiresAt] = useState<number | null>(null);
  const [inviteRemainingSeconds, setInviteRemainingSeconds] = useState(0);
  const [inviteGeneratedNotice, setInviteGeneratedNotice] = useState(false);
  const [pairedNotice, setPairedNotice] = useState<string | null>(null);
  const [copiedTarget, setCopiedTarget] = useState<CopyTarget | null>(null);
  const [publicExpanded, setPublicExpanded] = useState(false);
  const [configSaved, setConfigSaved] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);
  const [serviceAction, setServiceAction] = useState<ServiceAction | null>(null);
  const [inviteBusy, setInviteBusy] = useState(false);
  const [publicSaving, setPublicSaving] = useState(false);
  const [addressSaving, setAddressSaving] = useState(false);
  const [revokingDeviceId, setRevokingDeviceId] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [serviceError, setServiceError] = useState<string | null>(null);
  const [pairingError, setPairingError] = useState<string | null>(null);
  const [publicError, setPublicError] = useState<string | null>(null);
  const [devicesError, setDevicesError] = useState<string | null>(null);

  const mountedRef = useRef(true);
  const activeRefreshRef = useRef<Promise<void> | null>(null);
  const stateEpochRef = useRef(0);
  const mutationInFlightRef = useRef(false);
  const draftsInitializedRef = useRef(false);
  const publicDraftDirtyRef = useRef(false);
  const portDraftDirtyRef = useRef(false);

  const refresh = useCallback(
    async ({
      showLoading = false,
      waitForCurrent = false,
    }: {
      showLoading?: boolean;
      waitForCurrent?: boolean;
    } = {}) => {
      while (activeRefreshRef.current) {
        if (!waitForCurrent) return;
        await activeRefreshRef.current;
      }
      if (!mountedRef.current) return;

      const requestEpoch = stateEpochRef.current;
      const request = (async () => {
        const [statusResult, devicesResult] = await Promise.allSettled([
          invoke<RemoteStatus>("remote_server_status"),
          invoke<RemoteDevice[]>("remote_list_devices"),
        ]);
        if (!mountedRef.current || requestEpoch !== stateEpochRef.current) return;

        if (statusResult.status === "fulfilled") {
          const nextStatus = statusResult.value;
          setStatus(nextStatus);
          setLoadError(null);
          if (!draftsInitializedRef.current) {
            draftsInitializedRef.current = true;
            setPortDraft(String(nextStatus.port));
            setRelayUrlDraft(nextStatus.relayUrl ?? "");
            setRelayTokenDraft(nextStatus.relayToken ?? "");
            setEndpointsDraft(nextStatus.publicEndpoints.join("\n"));
          } else {
            if (!portDraftDirtyRef.current) {
              setPortDraft(String(nextStatus.port));
            }
            if (!publicDraftDirtyRef.current) {
              setRelayUrlDraft(nextStatus.relayUrl ?? "");
              setRelayTokenDraft(nextStatus.relayToken ?? "");
              setEndpointsDraft(nextStatus.publicEndpoints.join("\n"));
            }
          }
        } else {
          setLoadError(String(statusResult.reason));
        }

        if (devicesResult.status === "fulfilled") {
          setDevices(devicesResult.value);
          setDevicesError(null);
        } else {
          setDevicesError(String(devicesResult.reason));
        }
      })();

      activeRefreshRef.current = request;
      if (showLoading) setInitialLoading(true);

      try {
        await request;
      } finally {
        if (activeRefreshRef.current === request) activeRefreshRef.current = null;
        if (mountedRef.current && showLoading) setInitialLoading(false);
      }
    },
    [],
  );

  useEffect(() => {
    mountedRef.current = true;
    void refresh({ showLoading: true });
    return () => {
      mountedRef.current = false;
    };
  }, [refresh]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      if (document.visibilityState !== "visible" || mutationInFlightRef.current) return;
      void refresh();
    }, REMOTE_REFRESH_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<RemotePairedEvent>("remote-device-paired", (event) => {
      if (disposed) return;
      stateEpochRef.current += 1;
      setInvite(null);
      setInviteExpiresAt(null);
      setInviteGeneratedNotice(false);
      setPairingError(null);
      setPairedNotice(event.payload.deviceName);
      void refresh({ waitForCurrent: true });
    })
      .then((release) => {
        if (disposed) release();
        else unlisten = release;
      })
      .catch((error) => {
        if (!disposed && mountedRef.current) {
          setPairingError(
            t("appSettings.remote.eventListenFailed", {
              message: String(error),
            }),
          );
        }
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh, t]);

  useEffect(() => {
    if (inviteExpiresAt === null) {
      setInviteRemainingSeconds(0);
      return;
    }

    let timer: number | undefined;
    const updateRemaining = () => {
      const remaining = Math.max(0, Math.ceil((inviteExpiresAt - Date.now()) / 1000));
      setInviteRemainingSeconds(remaining);
      if (remaining === 0 && timer !== undefined) {
        window.clearInterval(timer);
        timer = undefined;
      }
    };

    updateRemaining();
    if (inviteExpiresAt > Date.now()) {
      timer = window.setInterval(updateRemaining, 1_000);
    }
    return () => {
      if (timer !== undefined) window.clearInterval(timer);
    };
  }, [inviteExpiresAt]);

  useEffect(() => {
    if (!copiedTarget) return;
    const timer = window.setTimeout(() => setCopiedTarget(null), FEEDBACK_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [copiedTarget]);

  useEffect(() => {
    if (!configSaved) return;
    const timer = window.setTimeout(() => setConfigSaved(false), FEEDBACK_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [configSaved]);

  useEffect(() => {
    if (!inviteGeneratedNotice) return;
    const timer = window.setTimeout(() => setInviteGeneratedNotice(false), FEEDBACK_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [inviteGeneratedNotice]);

  useEffect(() => {
    if (!pairedNotice) return;
    const timer = window.setTimeout(() => setPairedNotice(null), FEEDBACK_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [pairedNotice]);

  const normalizedPublicConfig = useMemo(
    () => normalizePublicConfig(relayUrlDraft, relayTokenDraft, endpointsDraft),
    [endpointsDraft, relayTokenDraft, relayUrlDraft],
  );
  const publicDirty =
    status !== null &&
    relayUrlDraft !== null &&
    relayTokenDraft !== null &&
    endpointsDraft !== null &&
    !publicConfigMatchesStatus(normalizedPublicConfig, status);
  const portDirty = status !== null && portDraft !== String(status.port);
  const parsedPort = portDraft === "" ? null : Number(portDraft);
  const portInvalid =
    status !== null &&
    (parsedPort === null ||
      !Number.isInteger(parsedPort) ||
      parsedPort < MIN_REMOTE_PORT ||
      parsedPort > MAX_REMOTE_PORT);
  // A fresh server intentionally listens only on loopback until the user
  // generates a pairing QR. Do not offer a copied LAN URL before that explicit
  // exposure action, because it would be a dead endpoint.
  const address =
    status?.running && status.networkExposed && status.lanIp
      ? `ws://${status.lanIp}:${status.port}`
      : null;
  const addressUnavailable =
    status?.running && !status.networkExposed
      ? t("appSettings.remote.addressLocalOnly")
      : t("appSettings.remote.addressUnavailable");
  const inviteExpired = invite !== null && inviteRemainingSeconds <= 0;
  const serviceBusy = serviceAction !== null;
  const mutationBusy =
    serviceBusy || inviteBusy || publicSaving || addressSaving || revokingDeviceId !== null;
  const publicConfigured = Boolean(status?.relayUrl || status?.publicEndpoints.length);

  useEffect(() => {
    publicDraftDirtyRef.current = publicDirty;
  }, [publicDirty]);

  useEffect(() => {
    portDraftDirtyRef.current = portDirty;
  }, [portDirty]);

  const relayStateLabel = (state: string): string => {
    if (state === "off") return t("appSettings.remote.relayState.off");
    if (state === "connecting") return t("appSettings.remote.relayState.connecting");
    if (state === "online") return t("appSettings.remote.relayState.online");
    return t("appSettings.remote.relayState.error", {
      message: state.startsWith("error:") ? state.slice("error:".length) : state,
    });
  };

  const serviceTitle = serviceAction
    ? t(`appSettings.remote.${serviceAction}`)
    : initialLoading && !status
      ? t("appSettings.remote.statusLoading")
      : !status
        ? t("appSettings.remote.statusUnavailable")
        : status?.running
          ? t("appSettings.remote.statusActive")
          : status?.enabled
            ? t("appSettings.remote.statusStartFailed")
            : t("appSettings.remote.statusStopped");

  const serviceBadgeStyle = {
    ...s.remoteBadge,
    ...(!status && loadError
      ? s.remoteBadgeDanger
      : initialLoading && !status
        ? s.remoteBadgeMuted
        : status?.running
          ? s.remoteBadgeSuccess
          : status?.enabled
            ? s.remoteBadgeDanger
            : s.remoteBadgeMuted),
  };

  const relayBadgeStyle = {
    ...s.remoteBadge,
    ...(status?.relayUrl && status.relayState === "online"
      ? s.remoteBadgeSuccess
      : status?.relayUrl && status.relayState === "connecting"
        ? s.remoteBadgeWarning
        : status?.relayUrl && status.relayState.startsWith("error")
          ? s.remoteBadgeDanger
          : s.remoteBadgeMuted),
  };

  const clearInvite = () => {
    setInvite(null);
    setInviteExpiresAt(null);
    setInviteGeneratedNotice(false);
  };

  const handleToggle = async () => {
    if (!status || mutationInFlightRef.current || (!status.enabled && portInvalid)) return;
    mutationInFlightRef.current = true;
    stateEpochRef.current += 1;
    setServiceAction(status.enabled ? "stopping" : "starting");
    setServiceError(null);
    clearInvite();

    try {
      const command = status.enabled ? "remote_server_stop" : "remote_server_start";
      const args = status.enabled ? {} : { port: parsedPort! };
      const nextStatus = await invoke<RemoteStatus>(command, args);
      if (!mountedRef.current) return;
      setStatus(nextStatus);
      await refresh({ waitForCurrent: true });
    } catch (error) {
      if (mountedRef.current) setServiceError(String(error));
      await refresh({ waitForCurrent: true });
    } finally {
      mutationInFlightRef.current = false;
      if (mountedRef.current) setServiceAction(null);
    }
  };

  const handleCreateInvite = async () => {
    if (!status?.running || mutationInFlightRef.current) return;
    mutationInFlightRef.current = true;
    // A completed pairing invalidates the QR. If its event arrives while the
    // invite request is in flight, do not let the late response restore that
    // already-consumed invite.
    const inviteEpoch = stateEpochRef.current;
    setInviteBusy(true);
    setPairingError(null);
    setPairedNotice(null);
    setInviteGeneratedNotice(false);

    try {
      const nextInvite = await invoke<RemoteInvite>("remote_create_invite");
      if (!mountedRef.current || inviteEpoch !== stateEpochRef.current) return;
      setInvite(nextInvite);
      setInviteExpiresAt(Date.now() + nextInvite.expiresInSeconds * 1_000);
      setInviteRemainingSeconds(nextInvite.expiresInSeconds);
      setInviteGeneratedNotice(true);
      // The backend may have replaced a loopback listener with a LAN listener
      // as part of issuing this invite. Refresh before re-enabling UI actions
      // so the copied/displayed address reflects that verified new state.
      await refresh({ waitForCurrent: true });
    } catch (error) {
      if (mountedRef.current && inviteEpoch === stateEpochRef.current) {
        setPairingError(
          t("appSettings.remote.pairingFailed", {
            message: String(error),
          }),
        );
      }
    } finally {
      mutationInFlightRef.current = false;
      if (mountedRef.current) setInviteBusy(false);
    }
  };

  const handleSelectLanIp = async (lanIp: string) => {
    if (!status || lanIp === status.lanIp || mutationInFlightRef.current) return;
    mutationInFlightRef.current = true;
    stateEpochRef.current += 1;
    setAddressSaving(true);
    setServiceError(null);
    clearInvite();

    try {
      const nextStatus = await invoke<RemoteStatus>("remote_select_lan_ip", { lanIp });
      if (!mountedRef.current) return;
      setStatus(nextStatus);
      await refresh({ waitForCurrent: true });
    } catch (error) {
      if (mountedRef.current) {
        setServiceError(
          t("appSettings.remote.addressSelectFailed", {
            message: String(error),
          }),
        );
      }
      await refresh({ waitForCurrent: true });
    } finally {
      mutationInFlightRef.current = false;
      if (mountedRef.current) setAddressSaving(false);
    }
  };

  const handleCopy = async (value: string, target: CopyTarget) => {
    try {
      await writeClipboardText(value);
      if (!mountedRef.current) return;
      setCopiedTarget(target);
      if (target === "address") setServiceError(null);
      else setPairingError(null);
    } catch (error) {
      if (!mountedRef.current) return;
      const message = t("appSettings.remote.copyFailed", { message: String(error) });
      if (target === "address") setServiceError(message);
      else setPairingError(message);
    }
  };

  const handleRevoke = async (device: RemoteDevice) => {
    if (mutationInFlightRef.current) return;
    let accepted: boolean;
    try {
      accepted = await confirm(
        t("appSettings.remote.revokeConfirmMessage", { name: device.name }),
        {
          title: t("appSettings.remote.revokeConfirmTitle"),
          kind: "warning",
        },
      );
    } catch (error) {
      if (mountedRef.current) {
        setDevicesError(
          t("appSettings.remote.revokeFailed", {
            message: String(error),
          }),
        );
      }
      return;
    }
    if (!mountedRef.current || !accepted || mutationInFlightRef.current) return;

    mutationInFlightRef.current = true;
    stateEpochRef.current += 1;
    setRevokingDeviceId(device.id);
    setDevicesError(null);
    try {
      await invoke("remote_revoke_device", { deviceId: device.id });
      await refresh({ waitForCurrent: true });
    } catch (error) {
      if (mountedRef.current) {
        setDevicesError(
          t("appSettings.remote.revokeFailed", {
            message: String(error),
          }),
        );
      }
    } finally {
      mutationInFlightRef.current = false;
      if (mountedRef.current) setRevokingDeviceId(null);
    }
  };

  const handleSaveConfig = async () => {
    if (!publicDirty || mutationInFlightRef.current) return;
    mutationInFlightRef.current = true;
    stateEpochRef.current += 1;
    setPublicSaving(true);
    setPublicError(null);
    setConfigSaved(false);

    try {
      const nextStatus = await invoke<RemoteStatus>("remote_update_config", {
        relayUrl: normalizedPublicConfig.relayUrl,
        relayToken: normalizedPublicConfig.relayToken,
        publicEndpoints: normalizedPublicConfig.publicEndpoints,
      });
      if (!mountedRef.current) return;
      setStatus(nextStatus);
      publicDraftDirtyRef.current = false;
      setRelayUrlDraft(nextStatus.relayUrl ?? "");
      setRelayTokenDraft(nextStatus.relayToken ?? "");
      setEndpointsDraft(nextStatus.publicEndpoints.join("\n"));
      clearInvite();
      setConfigSaved(true);
      await refresh({ waitForCurrent: true });
    } catch (error) {
      if (mountedRef.current) {
        setPublicError(
          t("appSettings.remote.configSaveFailed", {
            message: String(error),
          }),
        );
      }
      await refresh({ waitForCurrent: true });
    } finally {
      mutationInFlightRef.current = false;
      if (mountedRef.current) setPublicSaving(false);
    }
  };

  return (
    <div style={s.remotePanel}>
      <div style={s.remotePanelInner}>
        <p style={s.remoteIntro}>{t("appSettings.remote.description")}</p>

        <section style={s.remoteStatusCard} aria-labelledby="remote-service-title">
          <div style={s.remoteStatusHeader}>
            <div style={s.remoteStatusIdentity}>
              <span style={s.remoteStatusIcon} aria-hidden="true">
                <Server size={19} strokeWidth={1.8} />
              </span>
              <div style={{ minWidth: 0 }}>
                <div style={s.remoteStatusLabel}>{t("appSettings.remote.serverToggle")}</div>
                <h2 id="remote-service-title" style={s.remoteStatusTitle}>
                  {serviceTitle}
                </h2>
                <div style={s.remoteStatusCopyRow}>
                  <span style={s.remoteStatusEndpoint}>{address ?? addressUnavailable}</span>
                  {address ? (
                    <button
                      type="button"
                      className="remote-access-focus"
                      style={s.remoteCopyButton}
                      aria-label={t("appSettings.remote.copyAddress")}
                      aria-live="polite"
                      onClick={() => void handleCopy(address, "address")}
                    >
                      {copiedTarget === "address" ? (
                        <Check size={12} strokeWidth={2} />
                      ) : (
                        <Copy size={12} strokeWidth={1.8} />
                      )}
                      {copiedTarget === "address"
                        ? t("appSettings.remote.addressCopied")
                        : t("appSettings.remote.copyAddress")}
                    </button>
                  ) : null}
                </div>
                <div style={s.remoteStatusMeta}>
                  <span style={serviceBadgeStyle}>{serviceTitle}</span>
                  {status?.running ? (
                    <span>
                      {t("appSettings.remote.onlineConnections", {
                        count: String(status.onlineCount),
                      })}
                    </span>
                  ) : null}
                </div>
              </div>
            </div>
          </div>

          <div style={s.remoteStatusActions}>
            <div style={s.remoteAddressField}>
              <label style={s.remoteLabel} htmlFor="remote-lan-ip">
                {t("appSettings.remote.localIp")}
              </label>
              <select
                id="remote-lan-ip"
                className="remote-access-field"
                style={s.remoteAddressSelect}
                value={status?.lanIp ?? ""}
                disabled={
                  !status || initialLoading || mutationBusy || status.lanAddresses.length === 0
                }
                onChange={(event) => void handleSelectLanIp(event.currentTarget.value)}
              >
                {status?.lanAddresses.length ? (
                  status.lanAddresses.map((candidate) => (
                    <option key={`${candidate.interfaceName}:${candidate.ip}`} value={candidate.ip}>
                      {candidate.ip} ({candidate.interfaceName})
                    </option>
                  ))
                ) : (
                  <option value="">{t("appSettings.remote.noLocalIp")}</option>
                )}
              </select>
            </div>
            <div style={s.remotePortField}>
              <label style={s.remoteLabel} htmlFor="remote-server-port">
                {t("appSettings.remote.port")}
              </label>
              <input
                id="remote-server-port"
                className="remote-access-field"
                style={s.remotePortInput}
                value={portDraft}
                disabled={!status || Boolean(status.running) || initialLoading || serviceBusy}
                inputMode="numeric"
                maxLength={5}
                aria-invalid={portInvalid || undefined}
                aria-describedby="remote-port-hint"
                onChange={(event) => {
                  portDraftDirtyRef.current = true;
                  setPortDraft(event.currentTarget.value.replace(/[^0-9]/g, ""));
                  setServiceError(null);
                }}
              />
            </div>
            <button
              type="button"
              className="remote-access-focus"
              role="switch"
              aria-checked={status?.enabled ?? false}
              aria-label={t("appSettings.remote.serverToggle")}
              disabled={
                !status || initialLoading || mutationBusy || (!status.enabled && portInvalid)
              }
              onClick={() => void handleToggle()}
              style={{
                ...s.settingToggle,
                width: 112,
                minHeight: 34,
                padding: "5px 8px 5px 10px",
                opacity:
                  !status || initialLoading || mutationBusy || (!status.enabled && portInvalid)
                    ? 0.55
                    : 1,
                cursor:
                  !status || initialLoading || mutationBusy || (!status.enabled && portInvalid)
                    ? "not-allowed"
                    : "pointer",
              }}
            >
              <span style={{ ...s.settingToggleLabel, fontSize: 11.5 }}>
                {status?.enabled
                  ? t("appSettings.remote.switchOn")
                  : t("appSettings.remote.switchOff")}
              </span>
              <span
                style={{
                  ...s.settingToggleTrack,
                  background: status?.enabled ? "var(--primary-action-bg)" : "var(--border-medium)",
                }}
              >
                <span
                  style={{
                    ...s.settingToggleKnob,
                    transform: status?.enabled ? "translateX(16px)" : "translateX(0)",
                  }}
                />
              </span>
            </button>
          </div>

          <div
            id="remote-port-hint"
            style={{
              ...s.remoteSectionHint,
              flexBasis: "100%",
              color: portInvalid ? "var(--danger)" : "var(--text-hint)",
            }}
          >
            {portInvalid
              ? t("appSettings.remote.portInvalid")
              : addressSaving
                ? t("appSettings.remote.savingLocalIp")
                : t("appSettings.remote.localIpHint")}
          </div>

          {loadError || serviceError ? (
            <div style={{ ...s.remoteError, flexBasis: "100%" }} role="alert">
              <AlertCircle size={14} strokeWidth={1.9} />
              <span style={{ flex: 1, minWidth: 0 }}>
                {serviceError ??
                  t("appSettings.remote.loadError", {
                    message: loadError ?? "",
                  })}
              </span>
              {loadError && !status ? (
                <Button
                  variant="outline"
                  size="xs"
                  onClick={() => void refresh({ showLoading: true, waitForCurrent: true })}
                  disabled={initialLoading}
                >
                  <RefreshCw
                    size={12}
                    className={initialLoading ? "spin" : undefined}
                    strokeWidth={1.9}
                  />
                  {t("appSettings.remote.retry")}
                </Button>
              ) : null}
            </div>
          ) : null}
        </section>

        <div style={s.remoteSectionGrid}>
          <section style={s.remoteSection} aria-labelledby="remote-pairing-title">
            <div style={s.remoteSectionHeader}>
              <div>
                <h2 id="remote-pairing-title" style={s.remoteSectionTitle}>
                  {t("appSettings.remote.pairing")}
                </h2>
                <p style={s.remoteSectionHint}>{t("appSettings.remote.pairingDescription")}</p>
              </div>
              {invite ? (
                <span
                  style={{
                    ...s.remoteBadge,
                    ...(inviteExpired ? s.remoteBadgeWarning : s.remoteBadgeSuccess),
                  }}
                >
                  {inviteExpired
                    ? t("appSettings.remote.inviteExpired")
                    : t("appSettings.remote.inviteExpiresIn", {
                        time: formatRemainingTime(inviteRemainingSeconds),
                      })}
                </span>
              ) : null}
            </div>

            <div style={s.remotePairingBody}>
              {invite && !inviteExpired ? (
                <div
                  style={s.remoteQrShell}
                  role="img"
                  aria-label={t("appSettings.remote.inviteGenerated")}
                >
                  <QRCodeSVG value={invite.pairingUrl} size={190} marginSize={1} />
                  <span style={s.remoteQrMeta}>{invite.endpoint}</span>
                </div>
              ) : (
                <div style={s.remotePairingEmpty}>
                  <Smartphone size={28} strokeWidth={1.6} aria-hidden="true" />
                  <span>
                    {inviteExpired
                      ? t("appSettings.remote.inviteExpired")
                      : status?.running
                        ? t("appSettings.remote.inviteHint")
                        : t("appSettings.remote.pairingNeedsServer")}
                  </span>
                </div>
              )}

              <div style={s.remoteActionRow}>
                <Button
                  size="sm"
                  onClick={() => void handleCreateInvite()}
                  disabled={!status?.running || mutationBusy}
                >
                  <RefreshCw
                    size={13}
                    className={inviteBusy ? "spin" : undefined}
                    strokeWidth={2}
                  />
                  {inviteBusy
                    ? t("appSettings.remote.generatingInvite")
                    : invite
                      ? t("appSettings.remote.regenerateInvite")
                      : t("appSettings.remote.createInvite")}
                </Button>
                {invite && !inviteExpired ? (
                  <Button
                    variant="outline"
                    size="sm"
                    aria-live="polite"
                    onClick={() => void handleCopy(invite.pairingUrl, "pairing")}
                  >
                    {copiedTarget === "pairing" ? (
                      <Check size={13} strokeWidth={2} />
                    ) : (
                      <Copy size={13} strokeWidth={1.8} />
                    )}
                    {copiedTarget === "pairing"
                      ? t("appSettings.remote.pairingLinkCopied")
                      : t("appSettings.remote.copyPairingLink")}
                  </Button>
                ) : null}
              </div>

              {inviteGeneratedNotice ? (
                <div style={s.remoteFeedbackSuccess} role="status" aria-live="polite">
                  <Check size={13} strokeWidth={2} />
                  {t("appSettings.remote.inviteGenerated")}
                </div>
              ) : null}
              {pairedNotice ? (
                <div style={s.remotePairingNotice} role="status" aria-live="polite">
                  <Check size={13} strokeWidth={2} />
                  {t("appSettings.remote.pairedNotice", { name: pairedNotice })}
                </div>
              ) : null}
              {pairingError ? (
                <div style={s.remoteError} role="alert">
                  <AlertCircle size={14} strokeWidth={1.9} />
                  {pairingError}
                </div>
              ) : null}
            </div>
          </section>

          <section style={s.remoteSection} aria-labelledby="remote-devices-title">
            <div style={s.remoteSectionHeader}>
              <div>
                <h2 id="remote-devices-title" style={s.remoteSectionTitle}>
                  {t("appSettings.remote.devices")}
                </h2>
                <p style={s.remoteSectionHint}>
                  {status?.running
                    ? t("appSettings.remote.onlineConnections", {
                        count: String(status.onlineCount),
                      })
                    : t("appSettings.remote.statusStopped")}
                </p>
              </div>
              <span style={{ ...s.remoteBadge, ...s.remoteBadgeMuted }}>{devices.length}</span>
            </div>

            {initialLoading && devices.length === 0 ? (
              <div style={s.remoteEmptyState}>{t("appSettings.remote.loading")}</div>
            ) : devices.length === 0 ? (
              <div style={s.remoteEmptyState}>{t("appSettings.remote.noDevices")}</div>
            ) : (
              <div style={s.remoteDeviceList}>
                {devices.map((device) => {
                  const revoking = revokingDeviceId === device.id;
                  return (
                    <div key={device.id} style={s.remoteDeviceRow}>
                      <span style={s.remoteDeviceIcon} aria-hidden="true">
                        <Smartphone size={15} strokeWidth={1.8} />
                      </span>
                      <div style={s.remoteDeviceInfo}>
                        <div style={s.remoteDeviceNameRow}>
                          <span style={s.remoteDeviceName} title={device.name}>
                            {device.name}
                          </span>
                          <span
                            style={{
                              ...s.remoteStatusDot,
                              background: device.online ? "var(--success)" : "var(--text-hint)",
                              boxShadow: device.online
                                ? s.remoteStatusDot.boxShadow
                                : "0 0 0 3px color-mix(in srgb, var(--text-hint) 12%, transparent)",
                            }}
                            aria-hidden="true"
                          />
                          <span
                            style={{
                              ...s.remoteDeviceState,
                              color: device.online ? "var(--success)" : "var(--text-hint)",
                            }}
                          >
                            {device.online
                              ? t("appSettings.remote.online")
                              : t("appSettings.remote.offline")}
                          </span>
                        </div>
                        <div style={s.remoteDeviceMeta}>
                          {t("appSettings.remote.lastSeen", {
                            time: formatTimestamp(
                              device.lastSeenAt,
                              language === "zh" ? "zh-CN" : "en-US",
                            ),
                          })}
                        </div>
                      </div>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`${t("appSettings.remote.revoke")} ${device.name}`}
                        title={t("appSettings.remote.revoke")}
                        style={{ color: "var(--danger)" }}
                        disabled={mutationBusy}
                        onClick={() => void handleRevoke(device)}
                      >
                        {revoking ? (
                          <RefreshCw size={14} className="spin" strokeWidth={1.8} />
                        ) : (
                          <Trash2 size={14} strokeWidth={1.8} />
                        )}
                      </Button>
                    </div>
                  );
                })}
              </div>
            )}

            {devicesError ? (
              <div style={s.remoteError} role="alert">
                <AlertCircle size={14} strokeWidth={1.9} />
                <span style={{ flex: 1, minWidth: 0 }}>{devicesError}</span>
                <Button
                  variant="outline"
                  size="xs"
                  onClick={() => void refresh({ waitForCurrent: true })}
                >
                  {t("appSettings.remote.retry")}
                </Button>
              </div>
            ) : null}
          </section>
        </div>

        <section style={s.remoteDisclosure}>
          <button
            type="button"
            className="remote-access-focus"
            style={s.remoteDisclosureHeader}
            aria-expanded={publicExpanded}
            aria-controls="remote-public-access-content"
            aria-label={
              publicExpanded
                ? t("appSettings.remote.collapseAdvanced")
                : t("appSettings.remote.expandAdvanced")
            }
            onClick={() => setPublicExpanded((expanded) => !expanded)}
          >
            <span style={s.remoteDisclosureTitleRow}>
              <Globe2 size={16} strokeWidth={1.8} color="var(--text-secondary)" />
              <span style={{ minWidth: 0, flex: 1 }}>
                <span style={{ ...s.remoteSectionTitle, display: "block" }}>
                  {t("appSettings.remote.publicAccessAdvanced")}
                </span>
                <span style={{ ...s.remoteSectionHint, display: "block" }}>
                  {t("appSettings.remote.publicAccessSummary")}
                </span>
              </span>
              <span style={relayBadgeStyle}>
                {!status
                  ? t(
                      initialLoading
                        ? "appSettings.remote.statusLoading"
                        : "appSettings.remote.statusUnavailable",
                    )
                  : status.relayUrl
                    ? relayStateLabel(status.relayState)
                    : publicConfigured
                      ? t("appSettings.remote.publicAccessConfigured")
                      : t("appSettings.remote.publicAccessNotConfigured")}
              </span>
            </span>
            <ChevronDown
              size={16}
              strokeWidth={1.9}
              aria-hidden="true"
              style={{
                ...s.remoteDisclosureChevron,
                transform: publicExpanded ? "rotate(180deg)" : "rotate(0deg)",
              }}
            />
          </button>

          {publicExpanded ? (
            <div id="remote-public-access-content" style={s.remoteDisclosureContent}>
              <p style={{ ...s.remoteSectionHint, margin: 0 }}>
                {t("appSettings.remote.publicAccessHint")}
              </p>
              {status?.enabled ? (
                <div style={s.remoteWarning}>
                  <AlertCircle size={14} strokeWidth={1.9} />
                  {t("appSettings.remote.restartWarning")}
                </div>
              ) : null}

              <div style={s.remoteFormGrid}>
                <div style={s.remoteField}>
                  <label style={s.remoteLabel} htmlFor="remote-relay-url">
                    {t("appSettings.remote.relayUrl")}
                  </label>
                  <input
                    id="remote-relay-url"
                    className="remote-access-field"
                    style={s.remoteInput}
                    value={relayUrlDraft ?? ""}
                    placeholder="wss://relay.example.com"
                    spellCheck={false}
                    disabled={!status || initialLoading || publicSaving}
                    onChange={(event) => {
                      publicDraftDirtyRef.current = true;
                      setRelayUrlDraft(event.currentTarget.value);
                      setConfigSaved(false);
                      setPublicError(null);
                    }}
                  />
                  <span style={s.remoteSectionHint}>{t("appSettings.remote.relayUrlHint")}</span>
                </div>

                <div style={s.remoteField}>
                  <label style={s.remoteLabel} htmlFor="remote-relay-token">
                    {t("appSettings.remote.relayToken")}
                  </label>
                  <input
                    id="remote-relay-token"
                    className="remote-access-field"
                    style={s.remoteInput}
                    type="password"
                    value={relayTokenDraft ?? ""}
                    spellCheck={false}
                    autoComplete="off"
                    disabled={!status || initialLoading || publicSaving}
                    onChange={(event) => {
                      publicDraftDirtyRef.current = true;
                      setRelayTokenDraft(event.currentTarget.value);
                      setConfigSaved(false);
                      setPublicError(null);
                    }}
                  />
                  <span style={s.remoteSectionHint}>{t("appSettings.remote.relayTokenHint")}</span>
                </div>

                <div style={{ ...s.remoteField, ...s.remoteFormFullWidth }}>
                  <label style={s.remoteLabel} htmlFor="remote-public-endpoints">
                    {t("appSettings.remote.publicEndpoints")}
                  </label>
                  <textarea
                    id="remote-public-endpoints"
                    className="remote-access-field"
                    style={s.remoteTextarea}
                    value={endpointsDraft ?? ""}
                    placeholder={"ws://100.64.0.5:6790\nwss://aeroric.example.com"}
                    spellCheck={false}
                    disabled={!status || initialLoading || publicSaving}
                    onChange={(event) => {
                      publicDraftDirtyRef.current = true;
                      setEndpointsDraft(event.currentTarget.value);
                      setConfigSaved(false);
                      setPublicError(null);
                    }}
                  />
                  <span style={s.remoteSectionHint}>
                    {t("appSettings.remote.publicEndpointsHint")}
                  </span>
                </div>
              </div>

              {publicError ? (
                <div style={s.remoteError} role="alert">
                  <AlertCircle size={14} strokeWidth={1.9} />
                  {publicError}
                </div>
              ) : null}

              <div
                style={{
                  ...s.remoteActionRow,
                  justifyContent: "space-between",
                }}
              >
                <div aria-live="polite">
                  {configSaved ? (
                    <span style={s.remoteFeedbackSuccess}>
                      <Check size={13} strokeWidth={2} />
                      {t("appSettings.remote.configSaved")}
                    </span>
                  ) : publicDirty ? (
                    <span style={s.remoteFeedback}>{t("appSettings.remote.unsavedChanges")}</span>
                  ) : null}
                </div>
                <Button
                  size="sm"
                  onClick={() => void handleSaveConfig()}
                  disabled={!publicDirty || mutationBusy}
                >
                  {publicSaving ? <RefreshCw size={13} className="spin" strokeWidth={1.9} /> : null}
                  {publicSaving
                    ? t("appSettings.remote.savingConfig")
                    : t("appSettings.remote.saveConfig")}
                </Button>
              </div>
            </div>
          ) : null}
        </section>
      </div>
    </div>
  );
}
