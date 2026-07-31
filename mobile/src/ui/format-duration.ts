/** 统计卡的时长格式化(紧凑,与 orca 一致的 d/h/m 三档)。 */
import { getLanguage } from "../i18n";

export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "—";
  const zh = getLanguage() === "zh";
  const totalMinutes = Math.floor(ms / 60_000);
  if (totalMinutes < 1) return zh ? "不到 1 分钟" : "<1m";
  const totalHours = Math.floor(totalMinutes / 60);
  if (totalHours < 1) return zh ? `${totalMinutes} 分钟` : `${totalMinutes}m`;
  const days = Math.floor(totalHours / 24);
  if (days >= 1) {
    const hours = totalHours % 24;
    return zh ? `${days} 天 ${hours} 小时` : `${days}d ${hours}h`;
  }
  const minutes = totalMinutes % 60;
  return zh ? `${totalHours} 小时 ${minutes} 分` : `${totalHours}h ${minutes}m`;
}

/** 统计数字的千分位展示;null/undefined → 占位符。 */
export function formatCount(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return value.toLocaleString("en-US");
}
