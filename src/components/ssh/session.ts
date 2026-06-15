export function createSshShellId(connectionId: string, now: number): string {
  return `ssh:${connectionId}:${now}`;
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
