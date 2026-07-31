import { t, type MessageKey } from "../i18n";
import type { TaskStatus } from "../types";
import { theme } from "./theme";

/** 状态语义与桌面端 src/types.ts TaskStatus 一致;文案走 i18n。 */
const STATUS_COLOR: Record<TaskStatus, string> = {
  todo: theme.textHint,
  pending: theme.accent,
  running: theme.success,
  input_required: theme.warning,
  detached: theme.purple,
  interrupted: theme.warning,
  done: theme.info,
  failed: theme.danger,
  cancelled: theme.textHint,
};

export function taskStatusMeta(status: TaskStatus | string): { label: string; color: string } {
  const color = STATUS_COLOR[status as TaskStatus];
  if (!color) return { label: String(status), color: theme.textHint };
  return { label: t(`status.${status}` as MessageKey), color };
}

/** 活跃状态排前、其余按创建时间倒序,是列表的默认排序键。 */
const STATUS_RANK: Record<TaskStatus, number> = {
  input_required: 0,
  running: 1,
  pending: 2,
  detached: 3,
  interrupted: 4,
  todo: 5,
  done: 6,
  failed: 7,
  cancelled: 8,
};

export function taskStatusRank(status: TaskStatus | string): number {
  return STATUS_RANK[status as TaskStatus] ?? 9;
}
