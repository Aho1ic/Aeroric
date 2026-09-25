//! 切换期间的采纳桥：意图成功 ⇒ 权威行替换该项目的内存行，
//! 并**顺手刷新旧快照队列里该项目的待发内容**。
//!
//! 刷新这一半是过渡守卫：#7/#15 还在发整表快照（它们随执行器接线同刀退役，
//! 见 `docs/native/reviews/tauri-adapter.md` §11）。没有这一步，一条比采纳更早
//! 排进快照队列的陈旧整表会在防抖窗口后把刚落的意图结果抹掉——正是 ADR-022
//! 要关的那个窗口在过渡期的新形状。快照写路径退役时，这两行一起删。

import type { Task } from "../../types";
import { persistProjectTasksQuietly } from "../../appProjectState";
import {
  queueTaskIntent,
  replaceProjectRows,
  type TaskIntent,
} from "../../taskIntentQueue";

type SetTasks = React.Dispatch<React.SetStateAction<Task[]>>;

export function sendTaskIntent(opts: {
  projectId: string;
  intent: TaskIntent;
  setTasks: SetTasks;
  showToast?: (msg: string, kind?: "error" | "warning" | "success") => void;
  formatError?: (error: string, projectId: string) => string;
  operationKey?: string;
  onAdopted?: (rows: Task[]) => void;
  /** quiet 启动路径用：错误只进 console，不弹 toast（对齐 persistProjectTasksQuietly）。 */
  quiet?: boolean;
}): void {
  queueTaskIntent(
    opts.projectId,
    opts.intent,
    {
      operationKey: opts.operationKey,
      onAdopt: (rows) => {
        const taskRows = rows as Task[];
        opts.setTasks((prev) => {
          const merged = replaceProjectRows(prev, opts.projectId, taskRows);
          persistProjectTasksQuietly(opts.projectId, merged);
          return merged;
        });
        opts.onAdopted?.(taskRows);
      },
      onError: (msg) => {
        if (opts.quiet) {
          console.error(msg);
          return;
        }
        opts.showToast?.(msg, "error");
      },
      formatError: opts.formatError,
    },
  );
}
