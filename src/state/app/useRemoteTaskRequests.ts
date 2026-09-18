import { useEffect, useRef } from "react";
import { invoke } from "../../lib/api/invoke";
import { listen } from "@tauri-apps/api/event";
import type { AgentType, PermissionMode, Project, SshConnection, Task } from "../../types";
import { resolveProjectLocation } from "../../types";
import { REMOTE_TASK_REQUEST_EVENT } from "../../tauriEvents";
import { REMOTE_TASK_COMMANDS } from "../../lib/api/appCommands";

export type RemoteTaskRequestPayload = {
  requestId: string;
  kind: "create" | "resume";
  projectId?: string;
  taskId?: string;
  prompt?: string;
  agent?: string;
  permissionMode?: string;
  selectedModel?: string;
  reasoningEffort?: string;
  speed?: string;
  dshAgentPreset?: string;
};

export type RemoteTaskSubmitOptions = {
  prompt: string;
  agent: AgentType;
  permissionMode: PermissionMode;
  images: string[];
  texts: string[];
  immediate: boolean;
  launchMode: string;
  baseBranch: string;
  selectedModel?: string;
  reasoningEffort?: string | null;
  speed?: string;
  dshAgentPreset?: string;
};

type SubmitTask = (
  project: Project,
  options: RemoteTaskSubmitOptions,
  opts?: { persistBeforeLaunch?: boolean },
) => Promise<Task | null>;

export type RemoteTaskRequestDeps = {
  projectsRef: { current: Project[] };
  tasksRef: { current: Task[] };
  sshConnectionsRef: { current: SshConnection[] };
  startupReadyRef: { current: Promise<void> };
  remoteTaskMutationQueuesRef: { current: Map<string, Promise<void>> };
  submit: SubmitTask;
  resume: (taskId: string, opts?: { persistBeforeLaunch?: boolean }) => Promise<boolean>;
  runTodo: (task: Task, opts?: { persistBeforeLaunch?: boolean }) => Promise<boolean>;
  showToast: (msg: string, kind?: "error" | "warning" | "success") => void;
  translate: (key: string, params?: Record<string, string>) => string;
};

function taskHasResumableSession(task: Task): boolean {
  return Boolean(
    task.claudeSessionId ||
    task.codexSessionId ||
    task.claudeSessionPath ||
    task.codexSessionPath ||
    task.dshSessionId ||
    task.dshSessionPath ||
    task.ompSessionId ||
    task.ompSessionPath,
  );
}

/** 手机远程 create/resume：复用桌面 submit/resume 流程并按 project 串行。 */
export function useRemoteTaskRequests(deps: RemoteTaskRequestDeps): void {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  useEffect(() => {
    const p = listen<RemoteTaskRequestPayload>(REMOTE_TASK_REQUEST_EVENT, async (e) => {
      const {
        requestId,
        kind,
        projectId,
        taskId,
        prompt,
        agent,
        permissionMode,
        selectedModel,
        reasoningEffort,
        speed,
        dshAgentPreset,
      } = e.payload;
      if (!requestId) return;
      const latest = depsRef.current;
      const complete = async (
        accepted: boolean,
        resultTaskId?: string,
        error?: string,
        resultTask?: Task,
      ) => {
        try {
          await invoke(REMOTE_TASK_COMMANDS.completeTaskRequest, {
            requestId,
            accepted,
            taskId: resultTaskId,
            error,
            task: resultTask,
          });
        } catch (err) {
          console.error("remote_complete_task_request failed", err);
        }
      };
      try {
        await latest.startupReadyRef.current;
      } catch (error) {
        await complete(false, undefined, `Desktop initialization failed: ${String(error)}`);
        return;
      }
      const queueProjectId =
        projectId ??
        (taskId
          ? latest.tasksRef.current.find((item) => item.id === taskId)?.projectId
          : undefined);
      const runRequest = async () => {
        if (kind === "resume") {
          if (!taskId) {
            await complete(false, undefined, "Resume request is missing taskId");
            return;
          }
          const task = latest.tasksRef.current.find((item) => item.id === taskId);
          if (!task) {
            await complete(false, undefined, `Task not found: ${taskId}`);
            return;
          }
          if (projectId && task.projectId !== projectId) {
            await complete(false, undefined, "Task does not belong to the requested project");
            return;
          }
          const project = latest.projectsRef.current.find((item) => item.id === task.projectId);
          if (!project) {
            await complete(false, undefined, "Task project is missing on the desktop");
            return;
          }
          const location = resolveProjectLocation(project);
          if (
            location.kind === "ssh" &&
            !latest.sshConnectionsRef.current.some(
              (connection) => connection.id === location.connectionId,
            )
          ) {
            await complete(false, undefined, "SSH connection is not configured on the desktop");
            return;
          }
          if (location.kind === "ssh" && !taskHasResumableSession(task)) {
            await complete(false, undefined, "SSH task has no resumable session");
            return;
          }
          try {
            const accepted =
              task.status === "todo"
                ? await latest.runTodo(task, { persistBeforeLaunch: true })
                : await latest.resume(taskId, { persistBeforeLaunch: true });
            const pendingTask = accepted
              ? latest.tasksRef.current.find((item) => item.id === taskId)
              : undefined;
            await complete(
              accepted,
              accepted ? taskId : undefined,
              accepted ? undefined : "Task cannot be resumed on this desktop",
              pendingTask,
            );
          } catch (error) {
            await complete(false, undefined, `Failed to save task before resume: ${String(error)}`);
          }
          return;
        }
        if (kind !== "create" || !prompt) {
          await complete(false, undefined, "Invalid task creation request");
          return;
        }
        const project = latest.projectsRef.current.find((item) => item.id === projectId);
        if (!project) {
          latest.showToast(latest.translate("remote.taskRequest.projectMissing"), "error");
          await complete(false, undefined, "Project not found on the desktop");
          return;
        }
        const location = resolveProjectLocation(project);
        if (
          location.kind === "ssh" &&
          !latest.sshConnectionsRef.current.some(
            (connection) => connection.id === location.connectionId,
          )
        ) {
          await complete(false, undefined, "SSH connection is not configured on the desktop");
          return;
        }
        try {
          const createdTask = await latest.submit(
            project,
            {
              prompt,
              agent: (agent ?? "claude") as AgentType,
              permissionMode: (permissionMode ?? "ask") as PermissionMode,
              selectedModel,
              reasoningEffort,
              speed,
              dshAgentPreset,
              images: [],
              texts: [],
              immediate: true,
              launchMode: "local",
              baseBranch: "",
            },
            { persistBeforeLaunch: true },
          );
          await complete(
            !!createdTask,
            createdTask?.id,
            createdTask ? undefined : "Desktop rejected the task creation request",
            createdTask ?? undefined,
          );
        } catch (error) {
          await complete(false, undefined, `Failed to save task before launch: ${String(error)}`);
        }
      };

      if (!queueProjectId) {
        await runRequest();
        return;
      }
      const queues = latest.remoteTaskMutationQueuesRef.current;
      const previous = queues.get(queueProjectId) ?? Promise.resolve();
      const queued = previous.catch(() => {}).then(runRequest);
      queues.set(queueProjectId, queued);
      try {
        await queued;
      } finally {
        if (queues.get(queueProjectId) === queued) {
          queues.delete(queueProjectId);
        }
      }
    });
    return () => {
      p.then((fn) => fn());
    };
  }, []);
}
