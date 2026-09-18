import { useEffect, useRef } from "react";
import { invoke } from "../../lib/api/invoke";
import { listen } from "@tauri-apps/api/event";
import type { TaskStatus } from "../../types";
import { PROJECT_PINNED_CHANGED_EVENT, dispatchAppSettingsChanged } from "../../appRemoteEvents";
import { APP_SETTINGS_CHANGED_EVENT } from "../../components/app-settings/types";
import {
  REMOTE_TERMINAL_RESIZED_EVENT,
  TASK_SESSION_EVENT,
  TASK_STATUS_EVENT,
} from "../../tauriEvents";
import { DBX_COMMANDS } from "../../lib/api/appCommands";
import { confirm } from "../../lib/appDialog";

export type CoreTauriListenersDeps = {
  updateTaskStatus: (
    taskId: string,
    status: TaskStatus,
    extra?: { attentionRequestedAt?: number },
    failureReason?: string,
  ) => void;
  scheduleForDoneTask: (taskId: string) => void;
  updateTaskSession: (
    taskId: string,
    sessionId: string,
    sessionPath: string,
    codexLike?: boolean,
    family?: string,
  ) => void;
  handleRemoteResize: (taskId: string, cols: number, rows: number) => void;
  applyPinnedChange: (payload: { projectId: string; pinned: boolean }) => void;
  /** [] deps 的 effect 用 ref 读最新文案。 */
  translateRef: { current: (key: string, params?: Record<string, string>) => string };
  isManuallyCompletedDsh: (taskId: string) => boolean;
};

/** 桌面核心 Tauri 事件：任务状态/会话/远程 resize/置顶/设置变更/生产库闸。 */
export function useAppCoreTauriListeners(deps: CoreTauriListenersDeps): void {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  useEffect(() => {
    const p1 = listen<{ task_id: string; status: TaskStatus; failure_reason?: string }>(
      TASK_STATUS_EVENT,
      (e) => {
        const { task_id, status, failure_reason } = e.payload;
        const d = depsRef.current;
        if (d.isManuallyCompletedDsh(task_id) && status !== "done") return;
        d.updateTaskStatus(task_id, status, undefined, failure_reason);
        if (status === "done") d.scheduleForDoneTask(task_id);
      },
    );
    const p2 = listen<{
      task_id: string;
      session_id: string;
      session_path: string;
      codex_like?: boolean;
      family?: string;
    }>(TASK_SESSION_EVENT, (e) => {
      const { task_id, session_id, session_path, codex_like, family } = e.payload;
      depsRef.current.updateTaskSession(task_id, session_id, session_path, codex_like, family);
    });
    const p3 = listen<{ task_id: string; cols: number; rows: number }>(
      REMOTE_TERMINAL_RESIZED_EVENT,
      (e) => {
        const { task_id, cols, rows } = e.payload;
        depsRef.current.handleRemoteResize(task_id, cols, rows);
      },
    );
    const p4 = listen<{ projectId: string; pinned: boolean }>(PROJECT_PINNED_CHANGED_EVENT, (e) => {
      depsRef.current.applyPinnedChange(e.payload);
    });
    const p5 = listen(APP_SETTINGS_CHANGED_EVENT, () => {
      dispatchAppSettingsChanged(window);
    });
    const p18 = listen<{
      requestId: string;
      connection: string;
      databases: string[];
      sql: string;
    }>("dbx-production-confirm-requested", (e) => {
      const { requestId, connection, databases, sql } = e.payload;
      void (async () => {
        const translate = depsRef.current.translateRef.current;
        let approved = false;
        try {
          const scope =
            databases.length > 0
              ? databases.join(", ")
              : translate("database.productionEntireConnection");
          approved = await confirm(
            translate("database.productionSqlWarning", { connection, databases: scope, sql }),
            {
              title: translate("database.productionWarningTitle"),
              kind: "warning",
              okLabel: translate("database.execute"),
              cancelLabel: translate("common.cancel"),
            },
          );
        } catch (error) {
          console.warn("production confirmation dialog failed", error);
        }
        try {
          await invoke(DBX_COMMANDS.respondProductionConfirmation, { requestId, approved });
        } catch (error) {
          console.warn("failed to answer production confirmation", error);
        }
      })();
    });
    return () => {
      p1.then((fn) => fn());
      p2.then((fn) => fn());
      p3.then((fn) => fn());
      p4.then((fn) => fn());
      p5.then((fn) => fn());
      p18.then((fn) => fn());
    };
  }, []);
}
