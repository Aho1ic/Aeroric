import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { createPortal } from "react-dom";
import {
  Bell,
  X,
  ExternalLink,
  Check,
  CheckCheck,
  Info,
  AlertTriangle,
  AlertCircle,
  Download,
  RotateCcw,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import type { NotificationItem, ReleaseInstallResult, ReleaseUpdatePrepareResult } from "../types";
import { useNotifications } from "../hooks/useNotifications";
import { useI18n } from "../i18n";
import s from "../styles";

const notificationBodyStyle: CSSProperties = {
  fontSize: 11.5,
  color: "var(--text-muted)",
  lineHeight: 1.5,
  whiteSpace: "pre-line",
  overflow: "hidden",
  display: "-webkit-box",
  WebkitLineClamp: 3,
  WebkitBoxOrient: "vertical",
};

const notificationBodyZhStyle: CSSProperties = {
  ...notificationBodyStyle,
  marginTop: 4,
};

function LevelIcon({ level }: { level: string }) {
  switch (level) {
    case "warning":
      return <AlertTriangle size={14} strokeWidth={2} color="var(--color-warning)" />;
    case "error":
      return <AlertCircle size={14} strokeWidth={2} color="var(--danger)" />;
    default:
      return <Info size={14} strokeWidth={2} color="var(--accent)" />;
  }
}

function NotificationEntry({
  item,
  onMarkRead,
}: {
  item: NotificationItem;
  onMarkRead: (id: string) => void;
}) {
  const { t } = useI18n();
  const [hov, setHov] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [prepareResult, setPrepareResult] = useState<ReleaseUpdatePrepareResult | null>(null);
  const [installResult, setInstallResult] = useState<ReleaseInstallResult | null>(null);
  const releaseTag = item.releaseTag ?? null;
  const canInstallUpdate = Boolean(item.updateInstallSupported && releaseTag && !installResult);
  const helperRunning = prepareResult?.helperStatus === "running";

  useEffect(() => {
    let cancelled = false;
    if (!item.updateInstallSupported || !releaseTag) return;
    invoke<ReleaseUpdatePrepareResult | null>("get_pending_release_update", {
      tagName: releaseTag,
    })
      .then((result) => {
        if (cancelled) return;
        if (result) {
          setPrepareResult(result);
          setInstallError(result.error);
        } else {
          setPrepareResult(null);
          setInstallError(null);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [item.updateInstallSupported, releaseTag]);

  const handleClick = async () => {
    if (!item.isRead) onMarkRead(item.id);
    if (item.url) {
      await openUrl(item.url);
    }
  };

  const handleInstallClick = async (event: React.MouseEvent<HTMLButtonElement>) => {
    event.preventDefault();
    event.stopPropagation();
    if (!releaseTag || preparing || restarting || helperRunning) return;
    setInstallError(null);

    if (!prepareResult) {
      setPreparing(true);
      try {
        const result = await invoke<ReleaseUpdatePrepareResult>("prepare_release_update", {
          tagName: releaseTag,
        });
        setPrepareResult(result);
      } catch (error) {
        setInstallError(String(error));
      } finally {
        setPreparing(false);
      }
      return;
    }

    setRestarting(true);
    try {
      const result = await invoke<ReleaseInstallResult>("restart_and_install_release_update", {
        tagName: releaseTag,
      });
      setInstallResult(result);
    } catch (error) {
      setInstallError(String(error));
      setRestarting(false);
    }
  };

  return (
    <div
      onClick={handleClick}
      onMouseEnter={() => setHov(true)}
      onMouseLeave={() => setHov(false)}
      style={{
        padding: "10px 12px",
        borderBottom: "1px solid var(--border-dim)",
        cursor: item.url ? "pointer" : "default",
        background: hov ? "var(--bg-hover)" : item.isRead ? "transparent" : "var(--accent-subtle)",
        transition: "background 0.12s",
        display: "grid",
        gridTemplateColumns: "auto minmax(0, 1fr) auto",
        gap: 10,
        alignItems: "flex-start",
      }}
    >
      <div style={{ flexShrink: 0, marginTop: 2 }}>
        <LevelIcon level={item.level} />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            marginBottom: 3,
          }}
        >
          <span
            style={{
              fontSize: 12.5,
              fontWeight: item.isRead ? 500 : 600,
              color: "var(--text-primary)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              flex: 1,
            }}
          >
            {item.title}
          </span>
          {item.url && (
            <ExternalLink
              size={11}
              strokeWidth={2}
              color="var(--text-hint)"
              style={{ flexShrink: 0 }}
            />
          )}
        </div>
        <div style={notificationBodyStyle}>{item.body}</div>
        {item.bodyZh && <div style={notificationBodyZhStyle}>{item.bodyZh}</div>}
        {canInstallUpdate && (
          <button
            type="button"
            disabled={preparing || restarting || helperRunning || Boolean(installResult)}
            onClick={handleInstallClick}
            aria-label={
              prepareResult
                ? t("notification.restartUpdateAria", { tag: releaseTag ?? "" })
                : t("notification.downloadUpdateAria", { tag: releaseTag ?? "" })
            }
            style={{
              marginTop: 8,
              height: 24,
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "0 8px",
              border: "1px solid var(--border-dim)",
              borderRadius: 6,
              background: "var(--control-active-bg)",
              color: "var(--control-active-fg)",
              fontSize: 11,
              fontWeight: 650,
              cursor: preparing || restarting || helperRunning ? "default" : "pointer",
              opacity: preparing || restarting || helperRunning ? 0.72 : 1,
            }}
          >
            {prepareResult ? (
              <RotateCcw size={12} strokeWidth={2.4} />
            ) : (
              <Download size={12} strokeWidth={2.4} />
            )}
            {restarting || helperRunning
              ? t("notification.restartingUpdate")
              : preparing
                ? t("notification.downloadingUpdate")
                : prepareResult
                  ? t("notification.restartUpdate", { tag: releaseTag ?? "" })
                  : t("notification.downloadUpdate", { tag: releaseTag ?? "" })}
          </button>
        )}
        {prepareResult && !installResult && (
          <div style={{ ...notificationBodyStyle, marginTop: 6, WebkitLineClamp: 2 }}>
            {t("notification.updateReadyRestart")}
          </div>
        )}
        {installResult && (
          <div style={{ ...notificationBodyStyle, marginTop: 6, WebkitLineClamp: 2 }}>
            {installResult.restarted
              ? t("notification.installCompleteRestarting", {
                  path: installResult.installedAppPath,
                })
              : t("notification.installerOpened", { path: installResult.installedAppPath })}
          </div>
        )}
        {installError && (
          <div
            style={{
              ...notificationBodyStyle,
              marginTop: 6,
              color: "var(--danger)",
              WebkitLineClamp: 2,
            }}
          >
            {t("notification.installFailed", { error: installError })}
          </div>
        )}
        <div
          style={{
            fontSize: 10.5,
            color: "var(--text-hint)",
            marginTop: 4,
          }}
        >
          {item.createdAt}
        </div>
      </div>
      {!item.isRead && (
        <button
          title={t("notification.markAsRead")}
          onClick={(e) => {
            e.stopPropagation();
            onMarkRead(item.id);
          }}
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            padding: 2,
            borderRadius: 4,
            display: "flex",
            alignItems: "center",
            color: "var(--text-hint)",
            flexShrink: 0,
            marginTop: 1,
          }}
        >
          <Check size={12} strokeWidth={2.5} />
        </button>
      )}
    </div>
  );
}

export function NotificationBell({
  buttonStyle,
  iconSize = 14,
  renderAsContent = false,
}: {
  buttonStyle?: CSSProperties;
  iconSize?: number;
  renderAsContent?: boolean;
} = {}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const { result, loading, error, markRead, markAllRead } = useNotifications();

  const unreadCount = result?.unreadCount ?? 0;
  const isActive = unreadCount > 0 || loading || Boolean(error);
  const bellColor = error
    ? "var(--danger)"
    : unreadCount > 0
      ? "var(--accent)"
      : "var(--text-hint)";

  function handleOverlayClick(e: React.MouseEvent<HTMLDivElement>) {
    if (e.target === e.currentTarget) {
      setOpen(false);
    }
  }

  const notificationContent = (
    <div
      style={{
        width: renderAsContent ? "100%" : 420,
        maxWidth: renderAsContent ? "100%" : "calc(100vw - 32px)",
        maxHeight: renderAsContent ? "100%" : "72vh",
        background: "var(--bg-card)",
        border: renderAsContent ? "none" : "1px solid var(--border-medium)",
        borderRadius: renderAsContent ? 0 : 14,
        boxShadow: renderAsContent ? "none" : "var(--shadow-popover)",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      {!renderAsContent && (
        <div
          style={{
            padding: "14px 16px",
            borderBottom: "1px solid var(--border-dim)",
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          <span
            style={{
              fontSize: 14,
              fontWeight: 700,
              color: "var(--text-primary)",
              flex: 1,
            }}
          >
            {t("notification.title")}
            {unreadCount > 0 && (
              <span
                style={{
                  marginLeft: 6,
                  fontSize: 11,
                  fontWeight: 500,
                  color: "var(--text-muted)",
                }}
              >
                ({unreadCount} {t("notification.unread")})
              </span>
            )}
          </span>
          {unreadCount > 0 && (
            <button
              title={t("notification.markAllAsRead")}
              onClick={markAllRead}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: 3,
                borderRadius: 4,
                display: "flex",
                alignItems: "center",
                color: "var(--text-muted)",
              }}
            >
              <CheckCheck size={14} strokeWidth={2} />
            </button>
          )}
          <button title={t("common.close")} onClick={() => setOpen(false)} style={s.modalCloseBtn}>
            <X size={16} strokeWidth={2} />
          </button>
        </div>
      )}

      <div
        style={{
          flex: 1,
          overflowY: "auto",
        }}
      >
        {loading && !result ? (
          <div
            style={{
              padding: 24,
              textAlign: "center",
              fontSize: 12,
              color: "var(--text-hint)",
            }}
          >
            {t("common.loading")}
          </div>
        ) : error && !result ? (
          <div
            style={{
              padding: 24,
              textAlign: "center",
              fontSize: 12,
              color: "var(--danger)",
              lineHeight: 1.5,
            }}
          >
            {error}
          </div>
        ) : !result || result.notifications.length === 0 ? (
          <div
            style={{
              padding: 24,
              textAlign: "center",
              fontSize: 12,
              color: "var(--text-hint)",
            }}
          >
            {t("notification.noNotifications")}
          </div>
        ) : (
          result.notifications.map((item) => (
            <NotificationEntry key={item.id} item={item} onMarkRead={markRead} />
          ))
        )}
      </div>
    </div>
  );

  if (renderAsContent) {
    return notificationContent;
  }

  return (
    <>
      <button
        style={{
          ...s.sidebarIconBtn,
          opacity: isActive ? 1 : 0.5,
          ...buttonStyle,
        }}
        title={t("notification.title")}
        onClick={() => setOpen((v) => !v)}
      >
        <Bell size={iconSize} strokeWidth={1.6} color={bellColor} />
        {unreadCount > 0 && (
          <span
            style={{
              position: "absolute",
              top: -1,
              right: -1,
              minWidth: 12,
              height: 12,
              borderRadius: 6,
              background: "var(--danger)",
              color: "var(--fg-on-accent)",
              fontSize: 8,
              fontWeight: 700,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              padding: "0 2px",
              lineHeight: 1,
            }}
          >
            {unreadCount > 99 ? "99+" : unreadCount}
          </span>
        )}
      </button>

      {open &&
        createPortal(
          <div
            role="dialog"
            aria-modal="true"
            aria-label={t("notification.title")}
            style={s.modalOverlay}
            onClick={handleOverlayClick}
          >
            {notificationContent}
          </div>,
          document.body,
        )}
    </>
  );
}

export function UpdateBanner() {
  const { t } = useI18n();
  const { latestUpdate } = useNotifications();

  if (!latestUpdate || !latestUpdate.releaseTag) return null;

  return (
    <div
      data-testid="update-banner"
      role="status"
      aria-label={t("notification.updateAvailable", { tag: latestUpdate.releaseTag })}
      style={{
        position: "absolute",
        right: 2,
        bottom: "calc(100% + 7px)",
        width: 9,
        height: 9,
        borderRadius: "50%",
        background: "var(--danger)",
        border: "2px solid var(--bg-sidebar)",
        boxShadow: "0 0 0 1px color-mix(in srgb, var(--danger) 42%, transparent)",
        pointerEvents: "auto",
      }}
    />
  );
}
