//! 采纳桥：意图成功 ⇒ 权威行替换该项目的内存行，仅此而已。
//!
//! 旧快照队列的过渡守卫（采纳后顺手刷新队列待发内容）随整表快照写退役一起删掉了：
//! 现在没有第二个写者，采纳后的内存行就是权威行。

import type { Task } from "../../types";
import { queueTaskIntent, replaceProjectRows, type TaskIntent } from "../../taskIntentQueue";

type SetTasks = React.Dispatch<React.SetStateAction<Task[]>>;

export function sendTaskIntent(opts: {
  projectId: string;
  intent: TaskIntent;
  setTasks: SetTasks;
  showToast?: (msg: string, kind?: "error" | "warning" | "success") => void;
  formatError?: (error: string, projectId: string) => string;
  operationKey?: string;
  onAdopted?: (rows: Task[]) => void;
  /** quiet 启动路径用：错误只进 console，不弹 toast（对齐旧 persistProjectTasksQuietly）。 */
  quiet?: boolean;
}): void {
  queueTaskIntent(opts.projectId, opts.intent, {
    operationKey: opts.operationKey,
    onAdopt: (rows) => {
      const taskRows = rows as Task[];
      opts.setTasks((prev) => replaceProjectRows(prev, opts.projectId, taskRows));
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
  });
}
