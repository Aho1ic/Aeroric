import { invoke } from "@tauri-apps/api/core";
import type { SshConnection } from "../../types";
import type { SftpConflictStrategy, SftpEndpoint, SftpEntry, SftpTauriEndpoint } from "./sftpTypes";

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
  return invoke<SftpEntry[]>("sftp_read_dir", {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
  });
}

export async function readSftpTextFile(endpoint: SftpEndpoint, connections: SshConnection[]) {
  return invoke<string>("sftp_read_text_file", {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
  });
}

export async function readSftpImagePreview(endpoint: SftpEndpoint, connections: SshConnection[]) {
  return invoke<{ dataUrl: string; mimeType: string; byteLength: number }>(
    "sftp_read_image_preview",
    {
      endpoint: toTauriSftpEndpoint(endpoint, connections),
    },
  );
}

export async function readSftpDirectorySummary(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
) {
  return invoke<SftpDirectorySummary>("sftp_read_directory_summary", {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
  });
}

export async function createSftpDirectory(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
  name: string,
) {
  return invoke("sftp_create_directory", {
    endpoint: toTauriSftpEndpoint(endpoint, connections),
    name,
  });
}

export async function deleteSftpPaths(
  endpoint: SftpEndpoint,
  connections: SshConnection[],
  paths: string[],
) {
  return invoke("sftp_delete_paths", {
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
  return invoke("sftp_rename_path", {
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
  return invoke(operation === "copy" ? "sftp_copy_paths" : "sftp_move_paths", {
    source: toTauriSftpEndpoint(source, connections),
    paths,
    target: toTauriSftpEndpoint(target, connections),
    conflictStrategy,
  });
}
