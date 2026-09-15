import type { CommandMirror, InvokeTarget } from "../target";
import { invokeFor, prefixCommand, projectArgs } from "../invokeFacade";

/** LSP 目前只有 local + SSH；WSL 无镜像，requireCommand 会拦下。 */
export const LSP_MIRRORS = {
  serverStatus: { local: "lsp_server_status", ssh: "remote_lsp_server_status" },
  openDocument: { local: "lsp_open_document", ssh: "remote_lsp_open_document" },
  changeDocument: { local: "lsp_change_document", ssh: "remote_lsp_change_document" },
  closeDocument: { local: "lsp_close_document", ssh: "remote_lsp_close_document" },
} as const satisfies Record<string, CommandMirror>;

export type LspMirrorKey = keyof typeof LSP_MIRRORS;

export function lspCommand(target: InvokeTarget, key: LspMirrorKey): string {
  return prefixCommand(LSP_MIRRORS[key].local, target);
}

export async function invokeLspFor<T>(
  target: InvokeTarget,
  key: LspMirrorKey,
  extra: Record<string, unknown> = {},
): Promise<T> {
  return invokeFor<T>(target, LSP_MIRRORS[key], { ...projectArgs(target), ...extra });
}
