import type { SshHostKey, SshHostKeyStatus } from "../../types";

export function createSshShellId(connectionId: string, now: number): string {
  return `ssh:${connectionId}:${now}`;
}

/**
 * 拿到 host key 判定后该做什么。
 *
 * 只有"未登记"才拦下来让用户确认指纹。"主机不通"照常连 —— 那不是 host key
 * 问题,应该让 ssh 报真实的连接错误,而不是在这里编一个。
 */
export function sshHostKeyGate(
  status: SshHostKeyStatus,
): { action: "prompt"; target: string; keys: SshHostKey[] } | { action: "connect" } {
  if (status.state === "unknown") {
    return { action: "prompt", target: status.target, keys: status.keys };
  }
  return { action: "connect" };
}

export function shouldAttemptSshAutoConnect({
  autoConnect,
  active,
  hasActiveSession,
  connectionId,
  lastStartedConnectionId,
}: {
  autoConnect: boolean;
  active: boolean;
  hasActiveSession: boolean;
  connectionId: string | null | undefined;
  lastStartedConnectionId: string | null;
}): boolean {
  return Boolean(
    autoConnect &&
    active &&
    !hasActiveSession &&
    connectionId &&
    lastStartedConnectionId !== connectionId,
  );
}
