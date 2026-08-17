import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DshSessionHistory } from "../types";
import {
  mergeDshSessionEvents,
  projectDshSessionEvents,
  type DshSessionEvent,
} from "../dshSessionFeatures";

interface DshSessionEventFrame {
  type: "session/event";
  sessionId: string;
  event: DshSessionEvent;
}

function historyEvents(history: DshSessionHistory): DshSessionEvent[] {
  return history.events.filter((event): event is DshSessionEvent => (
    typeof event === "object" && event !== null && typeof (event as { type?: unknown }).type === "string"
  ));
}

function firstSeq(events: readonly DshSessionEvent[]): number | null {
  let first: number | null = null;
  for (const event of events) {
    if (typeof event.seq !== "number") continue;
    first = first === null ? event.seq : Math.min(first, event.seq);
  }
  return first;
}

/** History-paged plus live event feed used by the advanced DSH session UI. */
export function useDshSessionFeatures(sessionId: string) {
  const [events, setEvents] = useState<DshSessionEvent[]>([]);
  const [projections, setProjections] = useState<Record<string, unknown>>({});
  const [loading, setLoading] = useState(true);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cursorRef = useRef<number | null>(null);

  const merge = useCallback((incoming: readonly DshSessionEvent[]) => {
    setEvents((current) => mergeDshSessionEvents(current, incoming));
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    setEvents([]);
    setProjections({});
    setLoading(true);
    setLoadingOlder(false);
    setHasMore(false);
    setError(null);
    cursorRef.current = null;

    void listen<DshSessionEventFrame>("dsh-session-event", (envelope) => {
      if (envelope.payload.sessionId !== sessionId) return;
      merge([envelope.payload.event]);
    }).then((release) => {
      if (disposed) release();
      else unlisten = release;
    });

    void invoke<DshSessionHistory>("get_dsh_session_history", {
      sessionId,
      maxMessages: 200,
    }).then((history) => {
      if (disposed) return;
      const page = historyEvents(history);
      merge(page);
      cursorRef.current = firstSeq(page);
      setHasMore(history.hasMore && cursorRef.current !== null);
      setProjections(history.projections?.values ?? {});
    }).catch((caught) => {
      if (!disposed) setError(String(caught));
    }).finally(() => {
      if (!disposed) setLoading(false);
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [merge, sessionId]);

  const loadOlder = useCallback(async () => {
    const beforeSeq = cursorRef.current;
    if (beforeSeq === null || loadingOlder) return;
    setLoadingOlder(true);
    setError(null);
    try {
      const history = await invoke<DshSessionHistory>("get_dsh_session_history", {
        sessionId,
        beforeSeq,
        maxMessages: 200,
      });
      const page = historyEvents(history);
      merge(page);
      const next = firstSeq(page);
      cursorRef.current = next === null ? beforeSeq : Math.min(beforeSeq, next);
      setHasMore(history.hasMore && page.length > 0 && cursorRef.current > 0);
      if (history.projections?.values) setProjections(history.projections.values);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setLoadingOlder(false);
    }
  }, [loadingOlder, merge, sessionId]);

  const features = useMemo(() => projectDshSessionEvents(events), [events]);
  return { features, projections, loading, loadingOlder, hasMore, error, loadOlder };
}
