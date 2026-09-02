/* 状态栏上的云盘同步一段。
 *
 * 三件事在这里做对:
 *
 * 1. **倒计时在本地推。** `nextRunInMs` 是快照,原样显示的话数字会卡住不动,看着像调度死了。
 *    这里一秒一跳,基准是拿到状态那一刻(`statusAt`)。
 * 2. **「有改动待同步」不是错误。** 它是正常的中间态(防抖还没到期),所以用暖色而不是红色 ——
 *    和保存状态那一段同一套口径。
 * 3. **等用户决定的冲突要能点进去。** 状态栏这一格只有 22px 高,放不下逐条决定;所以显示条数
 *    并把打开冲突面板挂在同一个按钮上。
 *
 * 没有云盘远端时整段不渲染:那不是「关着」,而是这个库压根没配同步 —— 显示一个「已关闭」会让
 * 用户去找一个不存在的开关。
 */

import { useEffect, useState, type CSSProperties } from "react";
import { AlertTriangle, Cloud, CloudOff, RefreshCw } from "lucide-react";

import {
  nextRunSeconds,
  pendingConflicts,
  syncFailures,
  syncVerdict,
  type SyncRemoteStatus,
  type SyncReport,
} from "./noteSync";

export type NoteSyncBadgeProps = {
  status: SyncRemoteStatus | null;
  /** 拿到 `status` 的时刻(`Date.now()`)。倒计时的基准。 */
  statusAt: number;
  report: SyncReport | null;
  /** 报告可能已经过期(守护线程之后又跑过一轮)。 */
  stale: boolean;
  running: boolean;
  onSyncNow: () => void;
  onOpenConflicts: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

/** 未同步的改动用暖色 —— 它是正常的中间态,不是故障。和保存状态那一段一致。 */
const DIRTY_COLOR = "var(--warning, #ff9500)";
const FAIL_COLOR = "var(--danger, #ff453a)";

const buttonStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 3,
  height: 16,
  padding: "0 4px",
  border: "none",
  borderRadius: 3,
  background: "transparent",
  color: "inherit",
  cursor: "pointer",
  font: "inherit",
};

/**
 * 一秒一跳的「现在」。只在真的有倒计时要显示时才装定时器 —— 自动同步关着的时候装一个
 * 每秒醒一次的 interval 纯属白烧电。
 */
function useTick(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}

export function NoteSyncBadge({
  status,
  statusAt,
  report,
  stale,
  running,
  onSyncNow,
  onOpenConflicts,
  t,
}: NoteSyncBadgeProps) {
  const counting = status !== null && status.nextRunInMs !== null;
  const now = useTick(counting);
  if (!status) return null;

  const seconds = nextRunSeconds(status, Math.max(0, now - statusAt));
  const conflicts = pendingConflicts(report);
  const failures = syncFailures(report);
  const verdict = syncVerdict(report);

  /* 主标签按「最该让用户知道的那一件」挑,不是把状态平铺:
     正在跑 > 有失败 > 等决定 > 有改动待同步 > 已是最新 / 已关闭。 */
  let label: string;
  let color = "var(--text-muted)";
  if (running) {
    label = t("notebook.sync.syncing");
    color = "var(--accent)";
  } else if (verdict === "failed") {
    label = t("notebook.sync.failed", { count: String(failures.length) });
    color = FAIL_COLOR;
  } else if (conflicts.length > 0) {
    label = t("notebook.sync.awaitingUser", { count: String(conflicts.length) });
    color = DIRTY_COLOR;
  } else if (status.dirty) {
    label = t("notebook.sync.dirty");
    color = DIRTY_COLOR;
  } else if (!status.autoSync) {
    label = t("notebook.sync.off");
  } else {
    label = t("notebook.sync.idle");
  }

  /* 倒计时。退避中(`failures > 0`)时说的是「重试」而不是「下一轮」—— 那两件事对用户
     的含义不同:一个是正常节奏,一个是出过错。 */
  const countdown =
    seconds === null
      ? null
      : status.failures > 0
        ? t("notebook.sync.retrying", {
            seconds: String(seconds),
            failures: String(status.failures),
          })
        : t("notebook.sync.nextRun", { seconds: String(seconds) });

  const Icon = status.autoSync ? Cloud : CloudOff;

  return (
    <span
      data-testid="note-sync-badge"
      style={{ display: "flex", alignItems: "center", gap: 4, flexShrink: 0, color }}
    >
      {/* role=status 让屏读在同步状态变化时播报,不用用户主动去查 —— 和保存状态一致。 */}
      <span role="status" style={{ display: "flex", alignItems: "center", gap: 3 }}>
        <Icon size={10} aria-hidden />
        {label}
      </span>

      {countdown ? (
        <span style={{ color: "var(--text-hint)", whiteSpace: "nowrap" }}>{countdown}</span>
      ) : null}

      <button
        type="button"
        aria-label={t("notebook.sync.syncNow")}
        title={t("notebook.sync.syncNow")}
        onClick={onSyncNow}
        disabled={running}
        style={{
          ...buttonStyle,
          color: "var(--text-hint)",
          cursor: running ? "progress" : "pointer",
          opacity: running ? 0.45 : 1,
        }}
      >
        <RefreshCw size={10} aria-hidden />
      </button>

      {/* 有冲突、或者报告过期了才给这个入口。两种情况都是「有东西要你看一眼」。 */}
      {conflicts.length > 0 || stale ? (
        <button
          type="button"
          aria-label={t("notebook.sync.conflicts")}
          title={stale ? t("notebook.sync.staleReport") : t("notebook.sync.conflicts")}
          onClick={onOpenConflicts}
          style={{ ...buttonStyle, color: conflicts.length > 0 ? DIRTY_COLOR : "var(--text-hint)" }}
        >
          <AlertTriangle size={10} aria-hidden />
          {conflicts.length > 0 ? conflicts.length : null}
        </button>
      ) : null}
    </span>
  );
}
