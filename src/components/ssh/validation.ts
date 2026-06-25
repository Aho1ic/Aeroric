import type { SshConnection } from "../../types";

export interface SshConnectionDraft {
  name: string;
  group: string;
  host: string;
  port: string;
  username: string;
  identityFile: string;
  password: string;
  remotePath: string;
}

export type SshConnectionDraftErrors = Partial<Record<keyof SshConnectionDraft, string>>;

export function draftFromConnection(connection?: SshConnection | null): SshConnectionDraft {
  return {
    name: connection?.name ?? "",
    group: connection?.group ?? "",
    host: connection?.host ?? "",
    port: String(connection?.port ?? 22),
    username: connection?.username ?? "",
    identityFile: connection?.identityFile ?? "",
    password: connection?.password ?? "",
    remotePath: connection?.remotePath ?? "",
  };
}

export function normalizeSshPort(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return 22;
  if (!/^\d+$/.test(trimmed)) return null;
  const parsed = Number(trimmed);
  return parsed >= 1 && parsed <= 65535 ? parsed : null;
}

export function validateSshConnectionDraft(draft: SshConnectionDraft): SshConnectionDraftErrors {
  const errors: SshConnectionDraftErrors = {};
  if (!draft.name.trim()) errors.name = "Name is required.";
  if (!draft.host.trim()) errors.host = "Host is required.";
  if (!draft.username.trim()) errors.username = "Username is required.";
  if (normalizeSshPort(draft.port) === null) {
    errors.port = "Port must be between 1 and 65535.";
  }
  return errors;
}

export function normalizeSshConnectionDraft(
  draft: SshConnectionDraft,
  idSeed: number,
  now: number,
  existing?: SshConnection | null,
): SshConnection | null {
  const port = normalizeSshPort(draft.port);
  if (port === null) return null;
  const identityFile = draft.identityFile.trim();
  const password = draft.password.trim();
  const remotePath = draft.remotePath.trim();
  const group = draft.group.trim() || existing?.group?.trim() || "";
  return {
    id: existing?.id ?? String(idSeed),
    name: draft.name.trim(),
    ...(group ? { group } : {}),
    host: draft.host.trim(),
    port,
    username: draft.username.trim(),
    ...(identityFile ? { identityFile } : {}),
    ...(password ? { password } : {}),
    ...(remotePath ? { remotePath } : {}),
    createdAt: existing?.createdAt ?? now,
    ...(existing?.lastConnectedAt ? { lastConnectedAt: existing.lastConnectedAt } : {}),
  };
}
