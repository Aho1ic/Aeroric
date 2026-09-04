/** Fields shared by the mobile Agent configuration editor and RPC payload. */
export type AgentConfigKind = "codex" | "claude_code" | "dsh";

/**
 * The Chat Completions bridge only belongs to Codex profiles.
 *
 * Keep the field out of non-Codex requests entirely: the desktop RPC treats an
 * explicit `true` as invalid, and older clients that sent an unconditional
 * `false` could therefore not save Claude/DSH profiles.
 */
export function chatCompletionsProxyPayload(
  kind: AgentConfigKind,
  enabled: boolean,
): { enableChatCompletionsProxy?: boolean } {
  return kind === "codex" ? { enableChatCompletionsProxy: enabled } : {};
}
