import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Accessibility,
  Camera,
  Check,
  ExternalLink,
  FolderOpen,
  HardDrive,
  Keyboard,
  Mic,
  Monitor,
  RefreshCw,
  RotateCw,
  ShieldCheck,
  Terminal,
  Wifi,
  X,
  type LucideIcon,
} from "lucide-react";
import type {
  SystemPermission,
  SystemPermissionGrantAllResult,
  SystemPermissionReport,
  SystemPermissionStatus,
} from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";

const PERMISSION_ICONS: Record<string, LucideIcon> = {
  "screen-recording": Monitor,
  accessibility: Accessibility,
  "input-monitoring": Keyboard,
  automation: Terminal,
  "full-disk-access": HardDrive,
  microphone: Mic,
  camera: Camera,
  "local-network": Wifi,
  "folder-desktop": FolderOpen,
  "folder-documents": FolderOpen,
  "folder-downloads": FolderOpen,
};

const STATUS_COLOR: Record<SystemPermissionStatus, string> = {
  granted: "var(--status-success-fg, #1a7f37)",
  notGranted: "var(--status-error-fg, #c2410c)",
  unknown: "var(--text-hint)",
};

const BANNER_TONE: Record<"error" | "warning" | "info", string> = {
  error: "var(--status-error-fg, #c2410c)",
  warning: "var(--status-warning-fg, #b45309)",
  info: "var(--text-secondary)",
};

function Banner({
  tone,
  text,
  action,
}: {
  tone: "error" | "warning" | "info";
  text: string;
  action?: React.ReactNode;
}) {
  const color = BANNER_TONE[tone];
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 12,
        padding: "10px 12px",
        borderRadius: "var(--radius-md)",
        border: `1px solid color-mix(in srgb, ${color} 32%, transparent)`,
        background: `color-mix(in srgb, ${color} 8%, transparent)`,
        fontSize: 11.5,
        lineHeight: 1.55,
        color,
      }}
    >
      <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>{text}</span>
      {action}
    </div>
  );
}

/** 已授权的项目不需要再摆一排按钮,只留一个可见的"已获取"标记。 */
function StatusBadge({ status }: { status: SystemPermissionStatus }) {
  const { t } = useI18n();
  const label = t(`permissions.status.${status}`);
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
        flexShrink: 0,
        padding: "3px 8px",
        borderRadius: 999,
        fontSize: 11,
        fontWeight: 700,
        color: STATUS_COLOR[status],
        background: `color-mix(in srgb, ${STATUS_COLOR[status]} 12%, transparent)`,
      }}
    >
      {status === "granted" ? <Check size={11} strokeWidth={3} /> : null}
      {status === "notGranted" ? <X size={11} strokeWidth={3} /> : null}
      {label}
    </span>
  );
}

/**
 * 判断"刚授予、但要重启才生效"。基线取面板首次加载时的状态:进程启动后拿到的权限
 * 对当前进程无效(macOS 在启动时缓存 TCC 判定),所以只要有这类项目由未授权翻成
 * 已授权,就得提示重启。
 */
export function permissionsNeedingRestart(
  baseline: Record<string, SystemPermissionStatus>,
  permissions: readonly SystemPermission[],
): string[] {
  return permissions
    .filter(
      (permission) =>
        permission.needsRestart &&
        permission.status === "granted" &&
        baseline[permission.id] !== undefined &&
        baseline[permission.id] !== "granted",
    )
    .map((permission) => permission.id);
}

function statusMap(
  permissions: readonly SystemPermission[],
): Record<string, SystemPermissionStatus> {
  return Object.fromEntries(permissions.map((permission) => [permission.id, permission.status]));
}

export function PermissionsPanel() {
  const { t } = useI18n();
  const [report, setReport] = useState<SystemPermissionReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [grantingAll, setGrantingAll] = useState(false);
  const [manualIds, setManualIds] = useState<string[]>([]);
  const [restartIds, setRestartIds] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const baseline = useRef<Record<string, SystemPermissionStatus> | null>(null);

  /** 合并一次结果:记录基线、累积待重启项目。 */
  const absorb = useCallback((next: SystemPermissionReport) => {
    if (!baseline.current) {
      baseline.current = statusMap(next.permissions);
    } else {
      const pending = permissionsNeedingRestart(baseline.current, next.permissions);
      if (pending.length > 0) {
        setRestartIds((current) => [...new Set([...current, ...pending])]);
      }
    }
    setReport(next);
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      absorb(await invoke<SystemPermissionReport>("list_system_permissions"));
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setLoading(false);
    }
  }, [absorb]);

  useEffect(() => {
    void load();
  }, [load]);

  /** 单项获取:替换该项快照,不动其他项目(目录类权限的探测结果会被整表刷新抹掉)。 */
  const requestOne = useCallback(async (id: string) => {
    setBusyId(id);
    setError(null);
    try {
      const updated = await invoke<SystemPermission>("request_system_permission", { id });
      setReport((current) => {
        if (!current) return current;
        const permissions = current.permissions.map((permission) =>
          permission.id === updated.id ? updated : permission,
        );
        if (baseline.current) {
          const pending = permissionsNeedingRestart(baseline.current, [updated]);
          if (pending.length > 0) {
            setRestartIds((ids) => [...new Set([...ids, ...pending])]);
          }
        }
        return { ...current, permissions };
      });
      setManualIds((ids) => ids.filter((manualId) => manualId !== id));
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setBusyId(null);
    }
  }, []);

  const grantAll = useCallback(async () => {
    setGrantingAll(true);
    setError(null);
    try {
      const result = await invoke<SystemPermissionGrantAllResult>("request_all_system_permissions");
      absorb(result.report);
      setManualIds(result.manual);
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setGrantingAll(false);
    }
  }, [absorb]);

  const openSettings = useCallback(async (id: string) => {
    setError(null);
    try {
      await invoke("open_system_permission_settings", { id });
    } catch (nextError) {
      setError(String(nextError));
    }
  }, []);

  const restart = useCallback(async () => {
    try {
      await invoke("restart_app_for_permissions");
    } catch (nextError) {
      setError(String(nextError));
    }
  }, []);

  const permissions = useMemo(() => report?.permissions ?? [], [report]);
  const grantedCount = useMemo(
    () => permissions.filter((permission) => permission.status === "granted").length,
    [permissions],
  );
  const busy = loading || grantingAll || busyId !== null;

  return (
    <div
      style={{
        ...s.settingsBody,
        display: "flex",
        flexDirection: "column",
        gap: 14,
        padding: 20,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 12,
        }}
      >
        <div style={{ minWidth: 0 }}>
          <div style={{ fontSize: 13, fontWeight: 700 }}>{t("permissions.title")}</div>
          <div style={{ marginTop: 4, fontSize: 11, color: "var(--text-muted)", lineHeight: 1.55 }}>
            {t("permissions.subtitle")}
          </div>
        </div>
        <div style={{ display: "flex", gap: 8, flexShrink: 0 }}>
          <button style={s.secondaryActionBtn} onClick={() => void load()} disabled={busy}>
            <RefreshCw size={14} className={loading ? "spin" : undefined} />
            {t("common.refresh")}
          </button>
          {report?.supported ? (
            <button style={s.primaryActionBtn} onClick={() => void grantAll()} disabled={busy}>
              <ShieldCheck size={14} />
              {grantingAll ? t("permissions.grantingAll") : t("permissions.grantAll")}
            </button>
          ) : null}
        </div>
      </div>

      {report?.supported ? (
        <div style={{ fontSize: 11, color: "var(--text-hint)" }}>
          {t("permissions.summary", { granted: grantedCount, total: permissions.length })}
        </div>
      ) : null}

      {error ? <Banner tone="error" text={error} /> : null}

      {restartIds.length > 0 ? (
        <Banner
          tone="warning"
          text={t("permissions.restartRequired")}
          action={
            <button style={s.primaryActionBtn} onClick={() => void restart()}>
              <RotateCw size={14} />
              {t("permissions.restartNow")}
            </button>
          }
        />
      ) : null}

      {manualIds.length > 0 ? (
        <Banner
          tone="info"
          text={t("permissions.manualRemaining", {
            names: manualIds.map((id) => t(`permissions.item.${id}.name`)).join("、"),
          })}
        />
      ) : null}

      {!loading && !report?.supported ? (
        <Banner tone="info" text={t("permissions.unsupportedPlatform")} />
      ) : null}

      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        {permissions.map((permission) => (
          <PermissionRow
            key={permission.id}
            permission={permission}
            busy={busy}
            working={busyId === permission.id}
            onRequest={() => void requestOne(permission.id)}
            onOpenSettings={() => void openSettings(permission.id)}
          />
        ))}
      </div>
    </div>
  );
}

function PermissionRow({
  permission,
  busy,
  working,
  onRequest,
  onOpenSettings,
}: {
  permission: SystemPermission;
  busy: boolean;
  working: boolean;
  onRequest: () => void;
  onOpenSettings: () => void;
}) {
  const { t } = useI18n();
  const Icon = PERMISSION_ICONS[permission.id] ?? ShieldCheck;
  const name = t(`permissions.item.${permission.id}.name`);
  const granted = permission.status === "granted";
  // 目录类权限的检测会弹系统询问,所以按钮文案要说清点下去会发生什么。
  const requestLabel = permission.probePrompts
    ? t("permissions.action.checkAndGrant")
    : t("permissions.action.grant");

  return (
    <div
      role="group"
      aria-label={name}
      data-permission-id={permission.id}
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 12,
        padding: "12px 14px",
        borderRadius: "var(--radius-md)",
        border: "1px solid var(--border-dim)",
        background: "color-mix(in srgb, var(--bg-subtle) 72%, transparent)",
      }}
    >
      <Icon
        size={16}
        strokeWidth={1.8}
        color={granted ? STATUS_COLOR.granted : "var(--text-secondary)"}
        style={{ marginTop: 1, flexShrink: 0 }}
      />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
          <span style={{ fontSize: 12.5, fontWeight: 650 }}>{name}</span>
          <StatusBadge status={permission.status} />
          {permission.needsRestart ? (
            <span style={{ fontSize: 10.5, color: "var(--text-hint)" }}>
              {t("permissions.needsRestartTag")}
            </span>
          ) : null}
        </div>
        <div style={{ marginTop: 4, fontSize: 11, color: "var(--text-muted)", lineHeight: 1.55 }}>
          {t(`permissions.item.${permission.id}.description`)}
        </div>
        {permission.status === "unknown" ? (
          <div style={{ marginTop: 4, fontSize: 10.5, color: "var(--text-hint)", lineHeight: 1.5 }}>
            {permission.probePrompts
              ? t("permissions.unknownBecauseProbePrompts")
              : t("permissions.unknownReason", {
                  reason: permission.detail ?? t("permissions.noQueryApi"),
                })}
          </div>
        ) : null}
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 6, flexShrink: 0 }}>
        {!granted && permission.canRequestInApp ? (
          <button
            style={{ ...s.secondaryActionBtn, height: 30, fontSize: 12 }}
            onClick={onRequest}
            disabled={busy}
          >
            {working ? <RefreshCw size={13} className="spin" /> : <ShieldCheck size={13} />}
            {requestLabel}
          </button>
        ) : null}
        {permission.canOpenSettings ? (
          <button
            style={{ ...s.secondaryActionBtn, height: 30, fontSize: 12 }}
            onClick={onOpenSettings}
            title={t("permissions.action.openSettings")}
          >
            <ExternalLink size={13} />
            {t("permissions.action.openSettings")}
          </button>
        ) : null}
      </div>
    </div>
  );
}
