import { useCallback, useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useProjectsStore, useTasksStore } from "./index";
import type { Project, SshConnection, Task } from "../../types";
import { APP_SHELL_COMMANDS, SSH_CONNECTION_COMMANDS } from "../../lib/api/appCommands";
import { APP_PLATFORM } from "../../platform";
import { isHideWindowShortcut } from "../../shortcuts";
import { flushPendingSavesBeforeExit } from "../../taskFlush";
import { SELECTED_CONDA_ENV_KEY } from "../../appProjectState";
import {
  APP_EXIT_REQUESTED_EVENT,
  APP_RESTART_REQUESTED_EVENT,
} from "../../tauriEvents";

/** App 宿主权威列表 → zustand 只读镜像。 */
export function useHostStateMirrors(
  projects: Project[],
  tasks: Task[],
  activeProjectId: string | null,
): void {
  useEffect(() => {
    useProjectsStore.getState().syncFromHost(projects);
  }, [projects]);
  useEffect(() => {
    useTasksStore.getState().syncFromHost(tasks);
  }, [tasks]);
  useEffect(() => {
    useProjectsStore.getState().setSelectedProjectId(activeProjectId);
  }, [activeProjectId]);
}

/** Cmd+W 隐藏窗口（仅 macOS）。 */
export function useHideWindowShortcut(): void {
  useEffect(() => {
    if (APP_PLATFORM !== "macos") return;
    function handleHideWindow(event: KeyboardEvent) {
      if (!isHideWindowShortcut(event, APP_PLATFORM)) return;
      event.preventDefault();
      invoke(APP_SHELL_COMMANDS.hideWindow).catch(console.error);
    }
    window.addEventListener("keydown", handleHideWindow, true);
    return () => window.removeEventListener("keydown", handleHideWindow, true);
  }, []);
}

/**
 * 退出/重启：flush 落盘 → 后端 exit/restart。
 * 与原 App 内 effect 逐字对齐（macOS 不拦 onCloseRequested）。
 */
export function useAppLifecycle(
  showToast: (msg: string, kind?: "error" | "warning" | "success") => void,
  formatExitSaveFailed: (error: string) => string,
): void {
  const showToastRef = useRef(showToast);
  const formatRef = useRef(formatExitSaveFailed);
  useEffect(() => {
    showToastRef.current = showToast;
    formatRef.current = formatExitSaveFailed;
  }, [showToast, formatExitSaveFailed]);

  useEffect(() => {
    if (!isTauri()) return;

    let lifecyclePromise: Promise<void> | null = null;
    const completeLifecycleAction = (action: "exit" | "restart") => {
      if (lifecyclePromise) return lifecyclePromise;
      lifecyclePromise = (async () => {
        try {
          await flushPendingSavesBeforeExit();
          await invoke(
            action === "restart" ? "restart_app_after_task_flush" : "exit_app_after_task_flush",
          );
        } catch (error) {
          console.error("Failed to save tasks before exit", error);
          showToastRef.current(formatRef.current(String(error)), "error");
        } finally {
          lifecyclePromise = null;
        }
      })();
      return lifecyclePromise;
    };
    const closeListener =
      APP_PLATFORM === "macos"
        ? Promise.resolve(() => {})
        : getCurrentWindow().onCloseRequested((event) => {
            event.preventDefault();
            void completeLifecycleAction("exit");
          });
    const exitListener = listen(APP_EXIT_REQUESTED_EVENT, () => {
      void completeLifecycleAction("exit");
    });
    const restartListener = listen(APP_RESTART_REQUESTED_EVENT, () => {
      void completeLifecycleAction("restart");
    });
    let disposed = false;
    void Promise.all([closeListener, exitListener, restartListener])
      .then(() => {
        if (disposed) return;
        return invoke(APP_SHELL_COMMANDS.exitListenerReady);
      })
      .catch((error) => {
        console.error("Failed to register app lifecycle listener", error);
      });

    return () => {
      disposed = true;
      closeListener.then((unlisten) => unlisten());
      exitListener.then((unlisten) => unlisten());
      restartListener.then((unlisten) => unlisten());
    };
  }, []);
}

export function useSshConnectionPersistence(
  setSshConnections: (connections: SshConnection[]) => void,
  showToast: (msg: string, kind?: "error" | "warning" | "success") => void,
  formatSaveFailed: (error: string) => string,
  formatDeleteFailed: (error: string) => string,
): {
  handleSshConnectionsChange: (connections: SshConnection[]) => void;
  handleDeleteSshConnection: (connectionId: string) => Promise<void>;
} {
  const handleSshConnectionsChange = useCallback(
    (connections: SshConnection[]) => {
      setSshConnections(connections);
      invoke(SSH_CONNECTION_COMMANDS.save, { connections }).catch((e: unknown) => {
        console.error(e);
        showToast(formatSaveFailed(String(e)), "error");
      });
    },
    [setSshConnections, showToast, formatSaveFailed],
  );

  const handleDeleteSshConnection = useCallback(
    async (connectionId: string) => {
      try {
        const connections = await invoke<SshConnection[]>("delete_ssh_connection", {
          connectionId,
        });
        setSshConnections(connections);
      } catch (e: unknown) {
        console.error(e);
        showToast(formatDeleteFailed(String(e)), "error");
      }
    },
    [setSshConnections, showToast, formatDeleteFailed],
  );

  return { handleSshConnectionsChange, handleDeleteSshConnection };
}

export function persistSelectedCondaEnv(path: string | null): void {
  if (path) {
    localStorage.setItem(SELECTED_CONDA_ENV_KEY, path);
  } else {
    localStorage.removeItem(SELECTED_CONDA_ENV_KEY);
  }
}

/** 输入控件禁用自动补全/拼写检查（全局 focusin 一次）。 */
export function useDisableTextInputAutoFeatures(
  disable: (target: EventTarget | null) => void,
): void {
  useEffect(() => {
    const handleFocusIn = (event: FocusEvent) => disable(event.target);
    document.addEventListener("focusin", handleFocusIn, true);
    document.querySelectorAll("input, textarea").forEach((el) => disable(el));
    return () => document.removeEventListener("focusin", handleFocusIn, true);
  }, [disable]);
}
