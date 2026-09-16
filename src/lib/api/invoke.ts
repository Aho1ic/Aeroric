/**
 * 组件层唯一的 Tauri IPC 入口。
 *
 * `src/components/**` 不得再直接 `import { invoke } from "@tauri-apps/api/core"`：
 * 命令名必须来自 `lib/api/*` 的镜像表或 domain 常量，避免再散落字符串协议。
 * hooks / lib 内部实现仍可按需使用底层 API。
 */
export { invoke, isTauri, Channel } from "@tauri-apps/api/core";
export type { InvokeArgs, InvokeOptions } from "@tauri-apps/api/core";
