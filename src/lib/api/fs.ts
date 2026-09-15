import type { CommandMirror, InvokeTarget } from "../target";
import { invokeFileFor } from "../invokeFacade";

/** 与 `lib.rs` 注册名对齐。没有的键不要发明 —— 缺镜像时 requireCommand 会拦下。 */
export const FS_MIRRORS = {
  readDir: {
    local: "read_dir_entries",
    ssh: "remote_read_dir_entries",
    wsl: "wsl_read_dir_entries",
  },
  readContent: {
    local: "read_file_content",
    ssh: "remote_read_file_content",
    wsl: "wsl_read_file_content",
  },
  writeContent: {
    local: "write_file_content",
    ssh: "remote_write_file_content",
    wsl: "wsl_write_file_content",
  },
  readImage: {
    local: "read_image_preview",
    ssh: "remote_read_image_preview",
    wsl: "wsl_read_image_preview",
  },
  createFile: {
    local: "create_file",
    ssh: "remote_create_file",
    wsl: "wsl_create_file",
  },
  createDirectory: {
    local: "create_directory",
    ssh: "remote_create_directory",
    wsl: "wsl_create_directory",
  },
  renamePath: {
    local: "rename_path",
    ssh: "remote_rename_path",
    wsl: "wsl_rename_path",
  },
  deletePath: {
    local: "delete_path",
    ssh: "remote_delete_path",
    wsl: "wsl_delete_path",
  },
} as const satisfies Record<string, CommandMirror>;

/** 项目配置读写。WSL 读命令名历史为 `read_wsl_project_config`（无 `wsl_` 前缀）。 */
export const PROJECT_CONFIG_MIRRORS = {
  read: {
    local: "read_project_config",
    ssh: "remote_read_project_config",
    wsl: "read_wsl_project_config",
  },
  init: {
    local: "init_project_config",
    // 远程/WSL 目前只在本地项目初始化配置；缺镜像时 requireCommand 会拦下。
  },
} as const satisfies Record<string, CommandMirror>;

export type FsMirrorKey = keyof typeof FS_MIRRORS;

export async function readDirEntries<T = unknown>(target: InvokeTarget, path: string): Promise<T> {
  return invokeFileFor<T>(target, FS_MIRRORS.readDir, path);
}

export async function readFileContent(target: InvokeTarget, path: string): Promise<string> {
  return invokeFileFor<string>(target, FS_MIRRORS.readContent, path);
}

export async function writeFileContent(
  target: InvokeTarget,
  path: string,
  content: string,
): Promise<unknown> {
  return invokeFileFor(target, FS_MIRRORS.writeContent, path, { content });
}

export async function readImagePreview<T = unknown>(
  target: InvokeTarget,
  path: string,
): Promise<T> {
  return invokeFileFor<T>(target, FS_MIRRORS.readImage, path);
}

export async function createFile(target: InvokeTarget, path: string): Promise<unknown> {
  return invokeFileFor(target, FS_MIRRORS.createFile, path);
}

export async function createDirectory(target: InvokeTarget, path: string): Promise<unknown> {
  return invokeFileFor(target, FS_MIRRORS.createDirectory, path);
}

export async function renamePath(
  target: InvokeTarget,
  path: string,
  newName: string,
): Promise<unknown> {
  return invokeFileFor(target, FS_MIRRORS.renamePath, path, { newName });
}

export async function deletePath(target: InvokeTarget, path: string): Promise<unknown> {
  return invokeFileFor(target, FS_MIRRORS.deletePath, path);
}
