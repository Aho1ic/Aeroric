import { invoke } from "../../lib/api/invoke";
import type { SshConnection } from "../../types";
import type { SftpConflictStrategy, SftpEndpoint, SftpEntry, SftpTauriEndpoint } from "./sftpTypes";
import { SFTP_COMMANDS } from "../../lib/api/sftpCommands";

export interface SftpDirectorySummary {
  fileCount: number;
  directoryCount: number;
  totalSize: number;
  modifiedAtMs: number | null;
}

export function toTauriSftpEndpoint(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
): SftpTauriEndpoint {
  if (endpoint.kind === "local") return { kind: "local", path: endpoint.path };
  if (endpoint.kind === "storage") {
    return { kind: "storage", connectionId: endpoint.connectionId, path: endpoint.path };
  }
  const connection = connections.find((item) => item.id === endpoint.connectionId);
  if (!connection) {
    throw new Error(`SSH connection not found: ${endpoint.connectionName}`);
  }
  return { kind: "ssh", connection, path: endpoint.path };
}

export function fileEndpoint(endpoint: SftpEndpoint, path: string): SftpEndpoint {
  return { ...endpoint, path };
}

export async function readSftpDir(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
): Promise<SftpEntry[]> {
  return invoke<SftpEntry[]>(SFTP_COMMANDS.readDir, {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
  });
}

export async function readSftpTextFile(endpoint: SftpEndpoint, connections: SshConnection[]) {
  return invoke<string>(SFTP_COMMANDS.readTextFile, {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
  });
}

export async function readSftpImagePreview(endpoint: SftpEndpoint, connections: SshConnection[]) {
  return invoke<{ dataUrl: string; mimeType: string; byteLength: number }>(
    SFTP_COMMANDS.readImagePreview,
    {
      endpoint: toTauriSftpEndpoint(endpoint, connections),
    },
  );
}

export async function readSftpDirectorySummary(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
) {
  return invoke<SftpDirectorySummary>(SFTP_COMMANDS.readDirectorySummary, {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
  });
}

export async function createSftpDirectory(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
  name: string,
) {
  return invoke(SFTP_COMMANDS.createDirectory, {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
    name,
  });
}

export async function deleteSftpPaths(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
  paths: string[],
) {
  return invoke(SFTP_COMMANDS.deletePaths, {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
    paths,
  });
}

export async function renameSftpPath(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
  path: string,
  newName: string,
) {
  return invoke(SFTP_COMMANDS.renamePath, {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
    path,
    newName,
  });
}

export async function transferSftpPaths(
  operation: "copy" | "move",
  source: SftpEndpoint,
  paths: string[],
  target: SftpEndpoint,
  connections: SshConnection[],
  conflictStrategy: SftpConflictStrategy = "fail",
) {
  return invoke(operation === "copy" ? SFTP_COMMANDS.copyPaths : SFTP_COMMANDS.movePaths, {
    source: toTauriSftpEndpoint(source, connections),
    paths,
    target: toTauriSftpEndpoint(target, connections),
    conflictStrategy,
  });
}
