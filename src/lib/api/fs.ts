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
} as const satisfies Record<string, CommandMirror>;

export type FsMirrorKey = keyof typeof FS_MIRRORS;

export async function readDirEntries(target: InvokeTarget, path: string) {
  return invokeFileFor(target, FS_MIRRORS.readDir, path);
}

export async function readFileContent(target: InvokeTarget, path: string) {
  return invokeFileFor(target, FS_MIRRORS.readContent, path);
}

export async function writeFileContent(target: InvokeTarget, path: string, content: string) {
  return invokeFileFor(target, FS_MIRRORS.writeContent, path, { content });
}

export async function readImagePreview(target: InvokeTarget, path: string) {
  return invokeFileFor(target, FS_MIRRORS.readImage, path);
}
