import type { CommandMirror, InvokeTarget } from "../target";
import { invokeProjectFor } from "../invokeFacade";

/** Search 域镜像。当前 Rust 侧只有 local + remote（SSH），无 wsl 变体。 */
export const SEARCH_MIRRORS = {
  searchText: { local: "search_text", ssh: "remote_search_text" },
  searchStructured: { local: "search_structured", ssh: "remote_search_structured" },
  replaceTextPreview: { local: "replace_text_preview", ssh: "remote_replace_text_preview" },
  applyTextReplacements: {
    local: "apply_text_replacements",
    ssh: "remote_apply_text_replacements",
  },
} as const satisfies Record<string, CommandMirror>;

export async function searchText(
  target: InvokeTarget,
  extra: Record<string, unknown> = {},
): Promise<unknown> {
  return invokeProjectFor(target, SEARCH_MIRRORS.searchText, extra);
}

export async function searchStructured(
  target: InvokeTarget,
  extra: Record<string, unknown> = {},
): Promise<unknown> {
  return invokeProjectFor(target, SEARCH_MIRRORS.searchStructured, extra);
}
