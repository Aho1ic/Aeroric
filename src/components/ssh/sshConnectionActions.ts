import { invoke } from "../../lib/api/invoke";
import type { SshConnection } from "../../types";
import { writeClipboardText } from "../../lib/clipboard";

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

/**
 * 复制到剪贴板的 ssh 命令。
 *
 * **不夹带密码。** 原先把 `SSHPASS=... sshpass -e ssh ...` 整串写进剪贴板,等于把明文
 * 交给任意能读剪贴板的进程(浏览器扩展、下一次粘贴进聊天窗口)。判断有没有密码看
 * `hasPassword`,取明文走 `copySshConnectionPassword`。
 */
export function sshConnectionCommand(connection: SshConnection): string {
  const options = ["-p", shellQuote(String(connection.port))];
  if (connection.identityFile?.trim()) {
    options.unshift("-i", shellQuote(connection.identityFile.trim()));
  }
  const target = shellQuote(`${connection.username.trim()}@${formatSshHost(connection.host)}`);
  return `ssh ${options.join(" ")} ${target}`;
}

/** 按需取一条连接的明文密码并写入系统剪贴板。没有存密码时返回 false。 */
export async function copySshConnectionPassword(connection: SshConnection): Promise<boolean> {
  if (!connection.hasPassword) return false;
  const raw = await invoke<string | null>("get_ssh_connection_password", {
    connectionId: connection.id,
  });
  const password = raw?.trim() ?? "";
  if (!password) return false;
  await writeClipboardText(password);
  return true;
}
