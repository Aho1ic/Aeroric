export const RPC_V2 = 2 as const;
export const RPC_V3 = 3 as const;
export const SUPPORTED_RPC_VERSIONS = [RPC_V3, RPC_V2] as const;

export type RpcVersion = (typeof SUPPORTED_RPC_VERSIONS)[number];
export type RpcId = string;

export interface RpcV3Request {
  v: 3;
  type: "request";
  id: RpcId;
  method: string;
  params: Record<string, unknown>;
}

export type RpcV3Response =
  | { v: 3; type: "response"; id: RpcId; ok: true; result: unknown }
  | {
      v: 3;
      type: "response";
      id: RpcId;
      ok: false;
      error: RpcErrorShape;
    };

export interface RpcV3Push {
  v: 3;
  type: "push";
  event: string;
  seq?: number;
  data: unknown;
}

export type RpcV3Envelope = RpcV3Request | RpcV3Response | RpcV3Push;

export interface RpcAuthCapabilities {
  supportedRpcVersions?: RpcVersion[];
  capabilities?: string[];
}

/**
 * Capabilities describe remotely safe operations, not desktop UI panels. Keep
 * this list additive so an older phone can hide a newer operation cleanly.
 */
export const DEFAULT_RPC_CAPABILITIES = [
  "typed-envelope",
  "structured-error",
  "events-replay",
  "projects.grouping",
  "projects.pinning",
  "tasks.lifecycle",
  "tasks.models",
  "tasks.approvals",
  "session.structured",
  "session.dsh",
  "terminal.stream",
  "files.read",
  "files.write",
  "git.read",
  "agent-config.status",
  "agent-config.write",
  "stats.summary",
] as const;

export type RpcCapability = (typeof DEFAULT_RPC_CAPABILITIES)[number] | (string & {});

export interface RpcHostSnapshot {
  name: string;
  version: string;
  platform: string;
  rpcVersions: RpcVersion[];
  capabilities: RpcCapability[];
}

export interface RpcErrorShape {
  code: string;
  message: string;
  retryable: boolean;
  details?: Record<string, unknown>;
}

export type RpcProjectLocation =
  | { kind: "local"; path?: string }
  | { kind: "ssh"; connectionId?: string; remotePath?: string }
  | { kind: "wsl"; distribution?: string; linuxPath?: string };

export interface RpcProjectProjection {
  id: string;
  name: string;
  path: string;
  location?: RpcProjectLocation;
  branch?: string;
  group?: string;
  lastOpenedAt: number;
  orderIndex?: number;
  hiddenFromRail?: boolean;
  pinned?: boolean;
}

export interface RpcTaskProjection {
  id: string;
  projectId: string;
  name?: string;
  prompt: string;
  agent: string;
  family?: "claude" | "codex" | "dsh";
  selectedModel?: string;
  dshAgentPreset?: string;
  reasoningEffort?: string;
  speed?: string;
  permissionMode?: "ask" | "auto_edit" | "full_access";
  status: string;
  createdAt: number;
  attentionRequestedAt?: number;
  starred?: boolean;
  failureReason?: string;
  sessionFamily?: "claude" | "codex" | "dsh";
  worktreePath?: string;
  worktreeBranch?: string;
  baseBranch?: string;
  worktreeDiscarded?: boolean;
  additions?: number;
  deletions?: number;
  approval?: Record<string, unknown>;
  codexSessionId?: string;
  codexSessionPath?: string;
  claudeSessionId?: string;
  claudeSessionPath?: string;
  dshSessionId?: string;
  dshSessionPath?: string;
  dshWorkspaceId?: string;
  dshPromptMode?: string;
  sessionAgent?: string;
  sessionCodexLike?: boolean;
}

export type RpcSessionContent =
  | { type: "text"; text: string }
  | { type: "thinking"; thinking: string }
  | { type: "tool_use"; id: string; name: string; input: string }
  | { type: "tool_result"; id: string; output: string }
  | { type: "attachment"; name: string; mediaType: string; source: string }
  | { type: "opaque"; name: string; value: unknown };

export interface RpcSessionMessage {
  role: "user" | "assistant" | "system";
  content: RpcSessionContent[];
  messageId?: string;
  id?: string;
  timestamp?: number;
}

export function selectRpcVersion(supported?: readonly number[]): RpcVersion {
  return supported?.includes(RPC_V3) ? RPC_V3 : RPC_V2;
}

export function encodeRpcV3Request(
  id: RpcId,
  method: string,
  params: Record<string, unknown> = {},
): RpcV3Request {
  return { v: RPC_V3, type: "request", id, method, params };
}

export function isRpcV3Envelope(value: unknown): value is RpcV3Envelope {
  if (!value || typeof value !== "object") return false;
  const candidate = value as { v?: unknown; type?: unknown };
  return (
    candidate.v === RPC_V3 &&
    (candidate.type === "request" || candidate.type === "response" || candidate.type === "push")
  );
}
