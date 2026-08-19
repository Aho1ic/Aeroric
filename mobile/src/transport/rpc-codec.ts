import {
  DEFAULT_RPC_CAPABILITIES,
  RPC_V2,
  RPC_V3,
  SUPPORTED_RPC_VERSIONS,
  encodeRpcV3Request,
  type RpcCapability,
  type RpcErrorShape,
  type RpcHostSnapshot,
  type RpcVersion,
} from "@aeroric/remote-contracts";

export { RPC_V2, RPC_V3, SUPPORTED_RPC_VERSIONS, type RpcVersion };
export type { RpcCapability, RpcErrorShape, RpcHostSnapshot } from "@aeroric/remote-contracts";

export type DecodedRpcEnvelope =
  | { kind: "response"; id: string; ok: true; result: unknown }
  | { kind: "response"; id: string; ok: false; error: string; errorShape?: RpcErrorShape }
  | { kind: "push"; event: string; seq?: number; data: unknown };

export function authenticationParams(params: Record<string, unknown>): Record<string, unknown> {
  return {
    ...params,
    supportedRpcVersions: [...SUPPORTED_RPC_VERSIONS],
    capabilities: [...DEFAULT_RPC_CAPABILITIES],
  };
}

export function negotiatedRpcVersion(value: unknown): RpcVersion {
  return value === RPC_V3 ? RPC_V3 : RPC_V2;
}

export function normalizeRpcCapabilities(value: unknown): RpcCapability[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is RpcCapability => typeof item === "string" && item.length > 0);
}

export function normalizeHostSnapshot(value: unknown): RpcHostSnapshot | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Record<string, unknown>;
  const name = typeof candidate.name === "string" ? candidate.name : "aeroric";
  const version = typeof candidate.version === "string" ? candidate.version : "";
  const platform = typeof candidate.platform === "string" ? candidate.platform : "";
  const rpcVersions = Array.isArray(candidate.rpcVersions)
    ? candidate.rpcVersions.filter((item): item is RpcVersion => item === RPC_V2 || item === RPC_V3)
    : [];
  return {
    name,
    version,
    platform,
    rpcVersions,
    capabilities: normalizeRpcCapabilities(candidate.capabilities),
  };
}

export function encodeAeroricRequest(
  version: RpcVersion,
  id: string,
  method: string,
  params: Record<string, unknown> = {},
): string {
  return JSON.stringify(
    version === RPC_V3
      ? encodeRpcV3Request(id, method, params)
      : { v: RPC_V2, id: Number(id), method, params },
  );
}

export function decodeAeroricEnvelope(value: unknown): DecodedRpcEnvelope | null {
  if (!value || typeof value !== "object") return null;
  const frame = value as Record<string, unknown>;
  if (frame.v === RPC_V3 && frame.type === "push" && typeof frame.event === "string") {
    return {
      kind: "push",
      event: frame.event,
      ...(typeof frame.seq === "number" ? { seq: frame.seq } : {}),
      data: frame.data,
    };
  }
  if (
    frame.v === RPC_V3 &&
    frame.type === "response" &&
    typeof frame.id === "string" &&
    typeof frame.ok === "boolean"
  ) {
    if (frame.ok) return { kind: "response", id: frame.id, ok: true, result: frame.result };
    const error = frame.error as { message?: unknown; code?: unknown } | undefined;
    const errorShape =
      error && typeof error === "object"
        ? {
            code: typeof error.code === "string" ? error.code : "remote_error",
            message:
              typeof error.message === "string"
                ? error.message
                : typeof error.code === "string"
                  ? error.code
                  : "request failed",
            retryable: Boolean((error as { retryable?: unknown }).retryable),
            ...((error as { details?: unknown }).details &&
            typeof (error as { details?: unknown }).details === "object"
              ? { details: (error as { details: Record<string, unknown> }).details }
              : {}),
          }
        : undefined;
    return {
      kind: "response",
      id: frame.id,
      ok: false,
      error: errorShape?.message || "request failed",
      ...(errorShape ? { errorShape } : {}),
    };
  }
  if (frame.v === RPC_V2 && typeof frame.push === "string") {
    return {
      kind: "push",
      event: frame.push,
      ...(typeof frame.seq === "number" ? { seq: frame.seq } : {}),
      data: frame.data,
    };
  }
  if (frame.v === RPC_V2 && typeof frame.id === "number" && typeof frame.ok === "boolean") {
    return frame.ok
      ? { kind: "response", id: String(frame.id), ok: true, result: frame.result }
      : {
          kind: "response",
          id: String(frame.id),
          ok: false,
          error: typeof frame.error === "string" ? frame.error : "request failed",
        };
  }
  return null;
}
