/**
 * Session-log export helpers shared by the session header capsule and the
 * composer's `/export` line.
 *
 * Mirrors the Harness `session-log-download` plugin: the Web half hands
 * `GET /api/session.export` to the browser download manager, which owns the
 * destination. Aeroric has no browser, so the archive is streamed to a path
 * chosen in a native save dialog instead.
 */

/**
 * The archive filename the Harness itself proposes (`sessionLogZipFilename`),
 * mirrored so the save dialog defaults to the same name.
 */
export function dshSessionLogFilename(sessionId: string): string {
  return `dsh-session-${sessionId.replace(/[^A-Za-z0-9_-]/g, "_")}.zip`;
}

/** The command a composer line invokes, if the line is a slash-command line. */
export function dshCommandName(line: string): string | undefined {
  return /^\/([A-Za-z0-9_-]+)/.exec(line.trim())?.[1];
}

/** One `session.prompt` command outcome, as the Harness reports it. */
export interface DshCommandOutcome {
  kind: "success" | "error";
  text?: string;
}

/**
 * Read the `command` half of a `session.prompt` result. Absent when the line was
 * an ordinary prompt rather than a command, which is how the Harness signals
 * that no command ran.
 */
export function readDshCommandOutcome(value: unknown): DshCommandOutcome | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const command = (value as { command?: unknown }).command;
  if (typeof command !== "object" || command === null) return undefined;
  const { kind, text } = command as { kind?: unknown; text?: unknown };
  if (kind !== "success" && kind !== "error") return undefined;
  return typeof text === "string" ? { kind, text } : { kind };
}

/**
 * Whether a submitted line asked for the session-log export and the Harness
 * accepted it. The Harness command rejects an argument (`/export <path>`), so
 * only the success outcome starts a download — exactly the condition the Web
 * half observes on `command/executed`.
 */
export function requestsDshSessionLogExport(line: string, result: unknown): boolean {
  return dshCommandName(line) === "export" && readDshCommandOutcome(result)?.kind === "success";
}

/** Human-readable archive size for the saved-export report. */
export function formatDshExportSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "0 B";
  if (bytes < 1_024) return `${Math.round(bytes)} B`;
  const kib = bytes / 1_024;
  if (kib < 1_024) return `${kib.toFixed(kib < 10 ? 1 : 0)} KB`;
  const mib = kib / 1_024;
  if (mib < 1_024) return `${mib.toFixed(mib < 10 ? 1 : 0)} MB`;
  return `${(mib / 1_024).toFixed(1)} GB`;
}
