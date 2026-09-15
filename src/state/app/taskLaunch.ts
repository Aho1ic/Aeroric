import { invoke } from "@tauri-apps/api/core";
import type { SshConnection, Task } from "../../types";
import { taskCommandByKind } from "../../lib/api/session";
import { DSH_TASK_COMMANDS } from "../../lib/api/worktree";

export type TaskLaunchDeps = {
  createOutputChannel: (taskId: string) => unknown;
  writeErrorToTerminal: (taskId: string, text: string) => void;
  terminalSize: { cols: number; rows: number };
  onFailed: (taskId: string, message: string) => void;
};

function reportLaunchFailure(deps: TaskLaunchDeps, taskId: string, err: unknown) {
  const msg = err instanceof Error ? err.message : String(err);
  deps.writeErrorToTerminal(taskId, `\r\nError: ${msg}\r\n`);
  deps.onFailed(taskId, msg);
}

export type LocalRunInput = {
  task: Task;
  projectPath: string;
  images: string[];
  texts?: string[];
  injectPromptIntoTerminal?: boolean;
  promptOverride?: string;
  isDsh: boolean;
};

export function launchLocalTask(deps: TaskLaunchDeps, input: LocalRunInput) {
  const { task, projectPath, images, texts = [], injectPromptIntoTerminal = false, promptOverride, isDsh } =
    input;
  if (isDsh) {
    invoke(DSH_TASK_COMMANDS.run, {
      taskId: task.id,
      agent: task.agent,
      projectPath,
      prompt: promptOverride ?? task.prompt,
      sessionId: task.dshSessionId,
      agentPreset: task.dshAgentPreset,
      selectedModel: task.selectedModel,
      reasoningEffort: task.reasoningEffort,
      permissionMode: task.permissionMode,
      images,
      clientTimeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      onOutput: deps.createOutputChannel(task.id),
    }).catch((err: unknown) => reportLaunchFailure(deps, task.id, err));
    return;
  }
  invoke(taskCommandByKind("local", "run"), {
    taskId: task.id,
    projectPath,
    prompt: promptOverride ?? task.prompt,
    createdAt: task.createdAt,
    agent: task.agent,
    selectedModel: task.selectedModel,
    reasoningEffort: task.reasoningEffort,
    speed: task.speed,
    permissionMode: task.permissionMode,
    images,
    texts,
    forcePromptInjection: injectPromptIntoTerminal,
    cols: deps.terminalSize.cols,
    rows: deps.terminalSize.rows,
    onOutput: deps.createOutputChannel(task.id),
  }).catch((err: unknown) => reportLaunchFailure(deps, task.id, err));
}

export function launchSshTask(
  deps: TaskLaunchDeps,
  input: {
    task: Task;
    connection: SshConnection;
    remoteProjectPath: string;
    injectPromptIntoTerminal?: boolean;
    promptOverride?: string;
  },
) {
  const { task, connection, remoteProjectPath, injectPromptIntoTerminal = false, promptOverride } =
    input;
  invoke(taskCommandByKind("ssh", "run"), {
    taskId: task.id,
    connection,
    remoteProjectPath,
    prompt: promptOverride ?? task.prompt,
    agent: task.agent,
    selectedModel: task.selectedModel,
    reasoningEffort: task.reasoningEffort,
    speed: task.speed,
    permissionMode: task.permissionMode,
    forcePromptInjection: injectPromptIntoTerminal,
    cols: deps.terminalSize.cols,
    rows: deps.terminalSize.rows,
    onOutput: deps.createOutputChannel(task.id),
  }).catch((err: unknown) => reportLaunchFailure(deps, task.id, err));
}

export function launchWslTask(
  deps: TaskLaunchDeps,
  input: {
    task: Task;
    distribution: string;
    linuxProjectPath: string;
    injectPromptIntoTerminal?: boolean;
    promptOverride?: string;
  },
) {
  const { task, distribution, linuxProjectPath, injectPromptIntoTerminal = false, promptOverride } =
    input;
  invoke(taskCommandByKind("wsl", "run"), {
    taskId: task.id,
    distribution,
    linuxProjectPath,
    prompt: promptOverride ?? task.prompt,
    agent: task.agent,
    selectedModel: task.selectedModel,
    reasoningEffort: task.reasoningEffort,
    speed: task.speed,
    permissionMode: task.permissionMode,
    forcePromptInjection: injectPromptIntoTerminal,
    cols: deps.terminalSize.cols,
    rows: deps.terminalSize.rows,
    onOutput: deps.createOutputChannel(task.id),
  }).catch((err: unknown) => reportLaunchFailure(deps, task.id, err));
}

export type CancelTarget =
  | { kind: "ssh" }
  | { kind: "wsl" }
  | { kind: "dsh" }
  | { kind: "local"; projectPath: string };

export function cancelTaskInvoke(taskId: string, target: CancelTarget): Promise<unknown> {
  if (target.kind === "ssh") {
    return invoke(taskCommandByKind("ssh", "cancel"), { taskId });
  }
  if (target.kind === "wsl") {
    return invoke(taskCommandByKind("wsl", "cancel"), { taskId });
  }
  if (target.kind === "dsh") {
    return invoke(DSH_TASK_COMMANDS.cancel, { taskId });
  }
  return invoke(taskCommandByKind("local", "cancel"), {
    taskId,
    projectPath: target.projectPath,
  });
}

export function resumeDshTask(
  deps: TaskLaunchDeps,
  input: {
    task: Task;
    projectPath: string;
    sessionId: string;
  },
) {
  const { task, projectPath, sessionId } = input;
  invoke(DSH_TASK_COMMANDS.run, {
    taskId: task.id,
    agent: task.agent,
    projectPath,
    // Reconnect the persistent DSH session without replaying the original
    // user message; subsequent input goes through the DSH composer.
    prompt: "",
    sessionId,
    agentPreset: task.dshAgentPreset,
    selectedModel: task.selectedModel,
    reasoningEffort: task.reasoningEffort,
    permissionMode: task.permissionMode,
    images: [],
    clientTimeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    onOutput: deps.createOutputChannel(task.id),
  }).catch((err: unknown) => reportLaunchFailure(deps, task.id, err));
}

export function resumeSshTask(
  deps: TaskLaunchDeps,
  input: {
    task: Task;
    connection: SshConnection;
    remoteProjectPath: string;
    sessionId: string;
  },
) {
  const { task, connection, remoteProjectPath, sessionId } = input;
  invoke(taskCommandByKind("ssh", "resume"), {
    taskId: task.id,
    connection,
    remoteProjectPath,
    agent: task.agent,
    sessionId,
    permissionMode: task.permissionMode,
    selectedModel: task.selectedModel,
    reasoningEffort: task.reasoningEffort,
    speed: task.speed,
    cols: deps.terminalSize.cols,
    rows: deps.terminalSize.rows,
    onOutput: deps.createOutputChannel(task.id),
  }).catch((err: unknown) => reportLaunchFailure(deps, task.id, err));
}

export function resumeWslTask(
  deps: TaskLaunchDeps,
  input: {
    task: Task;
    distribution: string;
    linuxProjectPath: string;
    sessionId: string;
  },
) {
  const { task, distribution, linuxProjectPath, sessionId } = input;
  invoke(taskCommandByKind("wsl", "resume"), {
    taskId: task.id,
    distribution,
    linuxProjectPath,
    agent: task.agent,
    sessionId,
    permissionMode: task.permissionMode,
    selectedModel: task.selectedModel,
    reasoningEffort: task.reasoningEffort,
    speed: task.speed,
    cols: deps.terminalSize.cols,
    rows: deps.terminalSize.rows,
    onOutput: deps.createOutputChannel(task.id),
  }).catch((err: unknown) => reportLaunchFailure(deps, task.id, err));
}

export function resumeLocalTask(
  deps: TaskLaunchDeps,
  input: {
    task: Task;
    projectPath: string;
    sessionId: string;
  },
) {
  const { task, projectPath, sessionId } = input;
  invoke(taskCommandByKind("local", "resume"), {
    taskId: task.id,
    projectPath,
    agent: task.agent,
    sessionId,
    prompt: task.prompt,
    permissionMode: task.permissionMode,
    selectedModel: task.selectedModel,
    reasoningEffort: task.reasoningEffort,
    speed: task.speed,
    cols: deps.terminalSize.cols,
    rows: deps.terminalSize.rows,
    onOutput: deps.createOutputChannel(task.id),
  }).catch((err: unknown) => reportLaunchFailure(deps, task.id, err));
}
