import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DshSessionHistory } from "../types";
import {
  mergeDshSessionEvents,
  projectDshSessionEvents,
  readDshHistoryPage,
  type DshSessionEvent,
} from "../dshSessionFeatures";
import { parseDshToolEventView, type DshToolEventView } from "../dshToolViews";

interface DshSessionEventFrame {
  type: "session/event";
  sessionId: string;
  event: DshSessionEvent;
  /** Host-computed render intent for a tool event; absent otherwise. */
  view?: unknown;
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
  const [views, setViews] = useState<Record<number, DshToolEventView>>({});
  const [projections, setProjections] = useState<Record<string, unknown>>({});
  const [loading, setLoading] = useState(true);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cursorRef = useRef<number | null>(null);

  const merge = useCallback((incoming: readonly DshSessionEvent[]) => {
    setEvents((current) => mergeDshSessionEvents(current, incoming));
  }, []);

  /**
   * A view is a per-delivery derivation the Harness never persists, so the
   * newest delivery for a seq wins and an absent view leaves the prior one in
   * place rather than clearing it.
   */
  const mergeViews = useCallback((incoming: Readonly<Record<number, DshToolEventView>>) => {
    if (Object.keys(incoming).length === 0) return;
    setViews((current) => ({ ...current, ...incoming }));
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    setEvents([]);
    setViews({});
    setProjections({});
    setLoading(true);
    setLoadingOlder(false);
    setHasMore(false);
    setError(null);
    cursorRef.current = null;

    void listen<DshSessionEventFrame>("dsh-session-event", (envelope) => {
      if (envelope.payload.sessionId !== sessionId) return;
      const event = envelope.payload.event;
      merge([event]);
      const view = parseDshToolEventView(envelope.payload.view);
      if (view !== undefined && typeof event.seq === "number") {
        mergeViews({ [event.seq]: view });
      }
    }).then((release) => {
      if (disposed) release();
      else unlisten = release;
    });

    void invoke<DshSessionHistory>("get_dsh_session_history", {
      sessionId,
      maxMessages: 200,
    })
      .then((history) => {
        if (disposed) return;
        const page = readDshHistoryPage(history.events);
        merge(page.events);
        mergeViews(page.views);
        cursorRef.current = firstSeq(page.events);
        setHasMore(history.hasMore && cursorRef.current !== null);
        setProjections(history.projections?.values ?? {});
      })
      .catch((caught) => {
        if (!disposed) setError(String(caught));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [merge, mergeViews, sessionId]);

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
      const page = readDshHistoryPage(history.events);
      merge(page.events);
      mergeViews(page.views);
      const next = firstSeq(page.events);
      cursorRef.current = next === null ? beforeSeq : Math.min(beforeSeq, next);
      setHasMore(history.hasMore && page.events.length > 0 && cursorRef.current > 0);
      if (history.projections?.values) setProjections(history.projections.values);
    } catch (caught) {
      setError(String(caught));
    } finally {
      setLoadingOlder(false);
    }
  }, [loadingOlder, merge, mergeViews, sessionId]);

  const features = useMemo(() => projectDshSessionEvents(events, views), [events, views]);
  return { features, projections, loading, loadingOlder, hasMore, error, loadOlder };
}
