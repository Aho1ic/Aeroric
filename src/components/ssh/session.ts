export function createSshShellId(connectionId: string, now: number): string {
  return `ssh:${connectionId}:${now}`;
}
