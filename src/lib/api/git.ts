import type { CommandMirror } from "../target";
import { invokeFileFor, invokeProjectFor, requireCommand } from "../invokeFacade";
import type { InvokeTarget } from "../target";

/** Git 域镜像表。没有 wsl 变体的命令由调用方决定是否回退 local。 */
export const GIT_MIRRORS = {
  status: { local: "git_status", ssh: "remote_git_status", wsl: "wsl_git_status" },
  // 本地没有 git_changes：工作区列表就是 git_status。SSH/WSL 用合并视图。
  changes: { local: "git_status", ssh: "remote_git_changes", wsl: "wsl_git_changes" },
  stage: { local: "git_stage", ssh: "remote_git_stage", wsl: "wsl_git_stage" },
  unstage: { local: "git_unstage", ssh: "remote_git_unstage", wsl: "wsl_git_unstage" },
  commit: { local: "git_commit", ssh: "remote_git_commit", wsl: "wsl_git_commit" },
  discardFile: {
    local: "git_discard_file",
    ssh: "remote_git_discard_file",
    wsl: "wsl_git_discard_file",
  },
  fileDiff: { local: "git_file_diff", ssh: "remote_git_file_diff", wsl: "wsl_git_file_diff" },
  log: { local: "git_log", ssh: "remote_git_log", wsl: "wsl_git_log" },
  listBranches: {
    local: "git_list_branches",
    ssh: "remote_git_list_branches",
    wsl: "wsl_git_list_branches",
  },
  checkoutBranch: {
    local: "git_checkout_branch",
    ssh: "remote_git_checkout_branch",
    wsl: "wsl_git_checkout_branch",
  },
  createBranch: {
    local: "git_create_branch",
    ssh: "remote_git_create_branch",
    wsl: "wsl_git_create_branch",
  },
  pull: { local: "git_pull", ssh: "remote_git_pull", wsl: "wsl_git_pull" },
  push: { local: "git_push", ssh: "remote_git_push", wsl: "wsl_git_push" },
  remoteCounts: {
    local: "git_remote_counts",
    ssh: "remote_git_remote_counts",
    wsl: "wsl_git_remote_counts",
  },
  branchGraph: {
    local: "git_branch_graph",
    ssh: "remote_git_branch_graph",
    wsl: "wsl_git_branch_graph",
  },
  stashList: {
    local: "git_stash_list",
    ssh: "remote_git_stash_list",
    wsl: "wsl_git_stash_list",
  },
  stashDiff: {
    local: "git_stash_diff",
    ssh: "remote_git_stash_diff",
    wsl: "wsl_git_stash_diff",
  },
  stashPush: {
    local: "git_stash_push",
    ssh: "remote_git_stash_push",
    wsl: "wsl_git_stash_push",
  },
  stashApply: {
    local: "git_stash_apply",
    ssh: "remote_git_stash_apply",
    wsl: "wsl_git_stash_apply",
  },
  conflictFiles: {
    local: "git_conflict_files",
    ssh: "remote_git_conflict_files",
    wsl: "wsl_git_conflict_files",
  },
  conflictPreview: {
    local: "git_conflict_preview",
    ssh: "remote_git_conflict_preview",
    wsl: "wsl_git_conflict_preview",
  },
  resolveConflict: {
    local: "git_resolve_conflict",
    ssh: "remote_git_resolve_conflict",
    wsl: "wsl_git_resolve_conflict",
  },
  showCommitDiff: {
    // 本地命令名是 git_show_diff（历史名），SSH/WSL 才叫 show_commit_diff。
    local: "git_show_diff",
    ssh: "remote_git_show_commit_diff",
    wsl: "wsl_git_show_commit_diff",
  },
  showFileDiff: {
    local: "git_show_file_diff",
    ssh: "remote_git_show_file_diff",
    wsl: "wsl_git_show_file_diff",
  },
} as const satisfies Record<string, CommandMirror>;

export type GitMirrorKey = keyof typeof GIT_MIRRORS;

export function gitCommand(target: InvokeTarget, key: GitMirrorKey): string {
  return requireCommand(GIT_MIRRORS[key], target);
}

/**
 * 既有组件的 `gitCommandName("git_stage")` 习惯：local 用裸名，
 * ssh/wsl 加 `remote_` / `wsl_` 前缀。与镜像表约定一致，供未建镜像键的命令使用。
 */
export function prefixGitCommand(logicalLocalCommand: string, target: InvokeTarget): string {
  if (target.kind === "ssh") return `remote_${logicalLocalCommand}`;
  if (target.kind === "wsl") return `wsl_${logicalLocalCommand}`;
  return logicalLocalCommand;
}

export async function gitStatus(target: InvokeTarget) {
  return invokeProjectFor(target, GIT_MIRRORS.status);
}

export async function gitChanges(target: InvokeTarget) {
  return invokeProjectFor(target, GIT_MIRRORS.changes);
}

export async function gitStage(target: InvokeTarget, files: string[]) {
  return invokeProjectFor(target, GIT_MIRRORS.stage, { files });
}

export async function gitCommit(target: InvokeTarget, message: string) {
  return invokeProjectFor(target, GIT_MIRRORS.commit, { message });
}

export async function gitFileDiff(target: InvokeTarget, path: string, staged?: boolean) {
  return invokeFileFor(target, GIT_MIRRORS.fileDiff, path, { staged });
}

export async function gitLog(target: InvokeTarget, limit?: number) {
  return invokeProjectFor(target, GIT_MIRRORS.log, { limit });
}

export async function gitListBranches(target: InvokeTarget) {
  return invokeProjectFor(target, GIT_MIRRORS.listBranches);
}
