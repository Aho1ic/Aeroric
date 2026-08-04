import type { SshConnection } from "../../types";

function formatSshHost(host: string): string {
  const trimmed = host.trim();
  if (trimmed.includes(":") && !trimmed.startsWith("[")) return `[${trimmed}]`;
  return trimmed;
}

function shellQuote(value: string): string {
  if (/^[A-Za-z0-9_./:@%+=,-]+$/.test(value)) return value;
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export function sshConnectionUrl(connection: SshConnection): string {
  const username = encodeURIComponent(connection.username.trim());
  const host = formatSshHost(connection.host);
  const remotePath = connection.remotePath?.trim();
  const path = remotePath
    ? encodeURI(remotePath.startsWith("/") ? remotePath : `/${remotePath}`)
    : "";
  return `ssh://${username}@${host}:${connection.port}${path}`;
}

export function sshConnectionCommand(connection: SshConnection): string {
  const options = ["-p", shellQuote(String(connection.port))];
  if (connection.identityFile?.trim()) {
    options.unshift("-i", shellQuote(connection.identityFile.trim()));
  }
  return `ssh ${options.join(" ")} ${shellQuote(`${connection.username.trim()}@${formatSshHost(connection.host)}`)}`;
}
