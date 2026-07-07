export type ProjectActionKind = "open" | "close" | "run";

export type ProjectActionStatus = "completed" | "failed";

export type ProjectActionTrace = {
  id: number;
  action: ProjectActionKind;
  target: string;
  startedAt: number;
};

export type ProjectActionResult = ProjectActionTrace & {
  status: ProjectActionStatus;
  message: string;
  finishedAt: number;
  durationMs: number;
  error?: string;
};

export type ActionFeedbackState = ProjectActionResult;

export const PROJECT_ACTION_LOG_LIMIT = 20;

export type ProjectActionLogSummary = {
  total: number;
  failed: number;
  averageDurationMs: number;
  byAction: Record<ProjectActionKind, number>;
  latest?: ProjectActionResult;
};

export function startProjectActionTrace({
  id,
  action,
  target,
  now = Date.now(),
}: {
  id: number;
  action: ProjectActionKind;
  target: string;
  now?: number;
}): ProjectActionTrace {
  return {
    id,
    action,
    target,
    startedAt: now,
  };
}

export function finishProjectActionTrace(
  trace: ProjectActionTrace,
  {
    message,
    status = "completed",
    error,
    now = Date.now(),
  }: {
    message: string;
    status?: ProjectActionStatus;
    error?: string;
    now?: number;
  },
): ProjectActionResult {
  return {
    ...trace,
    status,
    message,
    finishedAt: now,
    durationMs: Math.max(0, now - trace.startedAt),
    error,
  };
}

export function appendProjectActionLog(
  current: ProjectActionResult[],
  result: ProjectActionResult,
  limit = PROJECT_ACTION_LOG_LIMIT,
): ProjectActionResult[] {
  return [result, ...current].slice(0, Math.max(1, limit));
}

export function summarizeProjectActionLog(entries: ProjectActionResult[]): ProjectActionLogSummary {
  if (entries.length === 0) {
    return {
      total: 0,
      failed: 0,
      averageDurationMs: 0,
      byAction: { open: 0, close: 0, run: 0 },
    };
  }

  const durationTotal = entries.reduce((sum, entry) => sum + entry.durationMs, 0);
  const byAction = entries.reduce<Record<ProjectActionKind, number>>(
    (counts, entry) => ({
      ...counts,
      [entry.action]: counts[entry.action] + 1,
    }),
    { open: 0, close: 0, run: 0 },
  );
  return {
    total: entries.length,
    failed: entries.filter((entry) => entry.status === "failed").length,
    averageDurationMs: Math.round(durationTotal / entries.length),
    byAction,
    latest: entries[0],
  };
}

export function readProjectActionLog(storageKey: string): ProjectActionResult[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isProjectActionResult);
  } catch {
    return [];
  }
}

export function writeProjectActionLog(storageKey: string, entries: ProjectActionResult[]): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      storageKey,
      JSON.stringify(entries.slice(0, PROJECT_ACTION_LOG_LIMIT)),
    );
  } catch {
    // Best-effort telemetry for UI feedback; never block IDE actions on storage failures.
  }
}

function isProjectActionResult(value: unknown): value is ProjectActionResult {
  if (!value || typeof value !== "object") return false;
  const entry = value as Partial<ProjectActionResult>;
  return (
    typeof entry.id === "number" &&
    (entry.action === "open" || entry.action === "close" || entry.action === "run") &&
    typeof entry.target === "string" &&
    typeof entry.startedAt === "number" &&
    (entry.status === "completed" || entry.status === "failed") &&
    typeof entry.message === "string" &&
    typeof entry.finishedAt === "number" &&
    typeof entry.durationMs === "number"
  );
}
