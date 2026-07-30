/** 简洁相对时间(en/zh 跟随系统语言)。 */
import { getLanguage } from "../i18n";

export function relativeTime(ts: number, now: number = Date.now()): string {
  if (!ts) return "";
  const zh = getLanguage() === "zh";
  const diff = now - ts;
  if (diff < 60_000) return zh ? "刚刚" : "just now";
  if (diff < 3_600_000) {
    const minutes = Math.floor(diff / 60_000);
    return zh ? `${minutes} 分钟前` : `${minutes}m ago`;
  }
  if (diff < 86_400_000) {
    const hours = Math.floor(diff / 3_600_000);
    return zh ? `${hours} 小时前` : `${hours}h ago`;
  }
  if (diff < 7 * 86_400_000) {
    const days = Math.floor(diff / 86_400_000);
    return zh ? `${days} 天前` : `${days}d ago`;
  }
  const d = new Date(ts);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}
