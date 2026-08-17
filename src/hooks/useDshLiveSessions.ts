import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DshJobView,
  DshJobsFrame,
  DshLiveSessionState,
  DshPlanProjection,
  DshProjectionFrame,
  DshQueueFrame,
  DshQueueItem,
  DshGoalProjection,
  DshSubscribedFrame,
  DshTodoItem,
} from "../types";

/**
 * Consumes the four `dsh-session-*` push events the Rust backend already
 * forwards verbatim from the dsh web `events.mux` SSE
 * (`dispatch_mux_frame` in src-tauri/src/dsh_webui.rs):
 *
 *  - `dsh-session-projection` — per-unit live view (title/goal/plan/todo/…)
 *  - `dsh-session-jobs`       — background job list for the session
 *  - `dsh-session-queue`      — pending-prompt queue snapshot
 *  - `dsh-session-subscribed` — subscription ack + lastSeq for replay
 *
 * State is keyed by session id so a single hook instance can host many running
 * sessions. Projection frames follow the dsh higher-seq-wins rule: a stale
 * frame (seq below the last applied for the same key) is dropped.
 */
export function useDshLiveSessions() {
  const [sessions, setSessions] = useState<Record<string, DshLiveSessionState>>({});
  // Per-session per-key watermark for the higher-seq-wins projection rule.
  const seqWatermarks = useRef<Record<string, Record<string, number>>>({});

  const applyProjection = useCallback((frame: DshProjectionFrame) => {
    // Higher-seq-wins: drop stale frames for the same (session,key).
    const wm = (seqWatermarks.current[frame.sessionId] ??= {});
    const lastSeq = wm[frame.key];
    if (typeof lastSeq === "number" && frame.seq < lastSeq) return;
    wm[frame.key] = frame.seq;

    setSessions((prev) => {
      const cur = prev[frame.sessionId] ?? {};
      const next: DshLiveSessionState = {
        ...cur,
        projections: { ...(cur.projections ?? {}), [frame.key]: frame.value },
      };
      switch (frame.key) {
        case "title":
          if (typeof frame.value === "string") next.title = frame.value;
          break;
        case "todo": {
          const value = frame.value as DshTodoItem[] | { items?: DshTodoItem[] } | null;
          const items = Array.isArray(value) ? value : value?.items;
          if (Array.isArray(items)) next.todo = items;
          break;
        }
        case "goal": {
          const goal = (frame.value as DshGoalProjection | null)?.goal ?? null;
          next.goal = goal ?? undefined;
          break;
        }
        case "plan": {
          const active = (frame.value as DshPlanProjection | null)?.active;
          next.planMode = Boolean(active);
          break;
        }
        case "permissions":
          next.permissions = frame.value;
          break;
        default:
          // Optional units (sessionStats/tokenUsage/contextPressure/...) stay
          // available through the generic projection map.
          break;
      }
      return { ...prev, [frame.sessionId]: next };
    });
  }, []);

  const applyJobs = useCallback((frame: DshJobsFrame) => {
    setSessions((prev) => ({
      ...prev,
      [frame.sessionId]: {
        ...(prev[frame.sessionId] ?? {}),
        jobs: Array.isArray(frame.jobs) ? (frame.jobs as DshJobView[]) : [],
      },
    }));
  }, []);

  const applyQueue = useCallback((frame: DshQueueFrame) => {
    setSessions((prev) => ({
      ...prev,
      [frame.sessionId]: {
        ...(prev[frame.sessionId] ?? {}),
        queue: Array.isArray(frame.items) ? (frame.items as DshQueueItem[]) : [],
      },
    }));
  }, []);

  const applySubscribed = useCallback((frame: DshSubscribedFrame) => {
    if (typeof frame.lastSeq !== "number") return;
    setSessions((prev) => ({
      ...prev,
      [frame.sessionId]: {
        ...(prev[frame.sessionId] ?? {}),
        lastSeq: frame.lastSeq,
      },
    }));
  }, []);

  useEffect(() => {
    const unlistenPromises: Promise<UnlistenFn>[] = [];
    unlistenPromises.push(
      listen<DshProjectionFrame>("dsh-session-projection", (e) => applyProjection(e.payload)),
    );
    unlistenPromises.push(listen<DshJobsFrame>("dsh-session-jobs", (e) => applyJobs(e.payload)));
    unlistenPromises.push(listen<DshQueueFrame>("dsh-session-queue", (e) => applyQueue(e.payload)));
    unlistenPromises.push(
      listen<DshSubscribedFrame>("dsh-session-subscribed", (e) => applySubscribed(e.payload)),
    );
    return () => {
      for (const p of unlistenPromises) p.then((fn) => fn());
    };
  }, [applyProjection, applyJobs, applyQueue, applySubscribed]);

  /** Drop state for a session once its task is closed. */
  const forgetSession = useCallback((sessionId: string) => {
    setSessions((prev) => {
      if (!prev[sessionId]) return prev;
      const next = { ...prev };
      delete next[sessionId];
      return next;
    });
    const wm = seqWatermarks.current;
    delete wm[sessionId];
  }, []);

  return { sessions, forgetSession };
}

export type DshLiveSessionsApi = ReturnType<typeof useDshLiveSessions>;
