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
 * 判断"已授权、但本进程还没拿到"。
 *
 * 首要依据是后端的 `restartRequired`:它由新进程探测得出(系统已授权 + 本进程未拿到),
 * 不依赖面板看到过什么。基线只作为后备——新进程探测不可用时,仍能靠"由未授权翻成
 * 已授权"这一迹象补上提示。
 */
export function permissionsNeedingRestart(
  baseline: Record<string, SystemPermissionStatus>,
  permissions: readonly SystemPermission[],
): string[] {
  return permissions
    .filter(
      (permission) =>
        permission.restartRequired ||
        (permission.needsRestart &&
          permission.status === "granted" &&
          baseline[permission.id] !== undefined &&
          baseline[permission.id] !== "granted"),
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
    // 基线只用于「探测不可用」时的后备判断,但 restartRequired 首次加载就要生效:
    // 用户很可能是先在系统设置里开了开关、再打开这个面板的。
    const pending = permissionsNeedingRestart(baseline.current ?? {}, next.permissions);
    baseline.current ??= statusMap(next.permissions);
    if (pending.length > 0) {
      setRestartIds((current) => [...new Set([...current, ...pending])]);
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

  /**
   * 清除该项授权记录后立刻重新请求。
   *
   * 这是 ad-hoc 签名升级后唯一的修法:旧记录绑的是上一版的 cdhash,系统设置里再点
   * 开关也不会让它重新对上,必须清掉重新授权。
   */
  const resetOne = useCallback(async (id: string) => {
    setBusyId(id);
    setError(null);
    try {
      await invoke<SystemPermission>("reset_system_permission", { id });
      const updated = await invoke<SystemPermission>("request_system_permission", { id });
      setReport((current) =>
        current
          ? {
              ...current,
              permissions: current.permissions.map((permission) =>
                permission.id === updated.id ? updated : permission,
              ),
            }
          : current,
      );
      // 记录已经清掉,旧的"待重启"结论不再成立。
      setRestartIds((ids) => ids.filter((restartId) => restartId !== id));
      baseline.current = { ...(baseline.current ?? {}), [id]: "notGranted" };
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setBusyId(null);
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
  // Linux 全是只报告项,一个"一键获取"按钮点下去什么都不会发生,不如不摆。
  const canGrantAny = useMemo(
    () => permissions.some((permission) => permission.canRequestInApp),
    [permissions],
  );
  const busy = loading || grantingAll || busyId !== null;
  const identityWarning = report?.identity.warning;

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
          {report?.supported && canGrantAny ? (
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

      {/* 系统按什么身份记授权。签名不稳定时"设置里开着、应用报未获取"是必然结果,
          这条横幅是那个假阴性唯一的解释入口。 */}
      {identityWarning ? (
        <Banner
          tone="warning"
          text={t(`permissions.identity.${identityWarning}`, {
            subject: report?.identity.subject ?? "",
          })}
        />
      ) : null}

      {/* 探测失败时 systemStatus 退化为进程内的旧答案,可能又变回那个假阴性,说清楚。 */}
      {report?.supported && !report.freshProbe && report.freshProbeError ? (
        <Banner
          tone="info"
          text={t("permissions.freshProbeFailed", { reason: report.freshProbeError })}
        />
      ) : null}

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
            onReset={() => void resetOne(permission.id)}
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
  onReset,
}: {
  permission: SystemPermission;
  busy: boolean;
  working: boolean;
  onRequest: () => void;
  onOpenSettings: () => void;
  onReset: () => void;
}) {
  const { t } = useI18n();
  const Icon = PERMISSION_ICONS[permission.id] ?? ShieldCheck;
  const name = t(`permissions.item.${permission.id}.name`);
  const granted = permission.status === "granted";
  // 目录类权限的检测会弹系统询问,所以按钮文案要说清点下去会发生什么。
  const requestLabel = permission.probePrompts
    ? t("permissions.action.checkAndGrant")
    : t("permissions.action.grant");
  // 系统记着未授权、而签名身份又不稳定时,「重新授权」比反复点「获取」有用:
  // 旧记录绑的是上一版 cdhash,不清掉就永远对不上。
  const canReset = permission.canReset && !granted;

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
          {permission.restartRequired ? (
            <span style={{ fontSize: 10.5, fontWeight: 700, color: BANNER_TONE.warning }}>
              {t("permissions.pendingRestartTag")}
            </span>
          ) : permission.needsRestart ? (
            <span style={{ fontSize: 10.5, color: "var(--text-hint)" }}>
              {t("permissions.needsRestartTag")}
            </span>
          ) : null}
        </div>
        <div style={{ marginTop: 4, fontSize: 11, color: "var(--text-muted)", lineHeight: 1.55 }}>
          {t(`permissions.item.${permission.id}.description`)}
        </div>
        {/* 三种需要补一句的情形,按"用户此刻最想知道什么"排序。 */}
        {permission.restartRequired ? (
          <div
            style={{ marginTop: 4, fontSize: 10.5, color: BANNER_TONE.warning, lineHeight: 1.5 }}
          >
            {t("permissions.grantedPendingRestart")}
          </div>
        ) : permission.status === "unknown" ? (
          <div style={{ marginTop: 4, fontSize: 10.5, color: "var(--text-hint)", lineHeight: 1.5 }}>
            {permission.probePrompts
              ? t("permissions.unknownBecauseProbePrompts")
              : t("permissions.unknownReason", {
                  reason: permission.detail ?? t("permissions.noQueryApi"),
                })}
          </div>
        ) : permission.detail ? (
          // Linux 的会话 / 用户组成因就在这里:状态确定,但"为什么"才是可行动的信息。
          <div style={{ marginTop: 4, fontSize: 10.5, color: "var(--text-hint)", lineHeight: 1.5 }}>
            {permission.detail}
          </div>
        ) : null}
        {permission.reportOnly ? (
          <div style={{ marginTop: 4, fontSize: 10.5, color: "var(--text-hint)", lineHeight: 1.5 }}>
            {t("permissions.reportOnly")}
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
        {canReset ? (
          <button
            style={{ ...s.secondaryActionBtn, height: 30, fontSize: 12 }}
            onClick={onReset}
            disabled={busy}
            title={t("permissions.action.resetHint")}
          >
            <RotateCw size={13} />
            {t("permissions.action.reset")}
          </button>
        ) : null}
      </div>
    </div>
  );
}
