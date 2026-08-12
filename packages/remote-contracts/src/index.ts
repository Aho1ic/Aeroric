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
      error: { code: string; message: string; retryable: boolean };
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

export const DEFAULT_RPC_CAPABILITIES = ["typed-envelope", "structured-error"] as const;

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
