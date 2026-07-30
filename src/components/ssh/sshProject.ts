import type { SshConnection } from "../../types";

export interface SshProjectInput {
  connectionId: string;
  remotePath: string;
  name: string;
}

export function deriveRemoteProjectName(remotePath: string, fallback: string): string {
  const trimmed = remotePath.trim().replace(/\/+$/, "");
  if (!trimmed) return fallback.trim() || "remote";
  const parts = trimmed.split("/");
  return parts[parts.length - 1] || fallback.trim() || "remote";
}

export function sshProjectInputForConnection(connection: SshConnection): SshProjectInput | null {
  const remotePath = connection.remotePath?.trim() ?? "";
  if (!remotePath) return null;
  return {
    connectionId: connection.id,
    remotePath,
    name: connection.name.trim() || deriveRemoteProjectName(remotePath, connection.name),
  };
}
