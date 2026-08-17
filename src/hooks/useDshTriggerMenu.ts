import { useCallback, useEffect, useRef, useState } from "react";
import {
  DSH_MENU_CLOSED,
  detectDshTrigger,
  reduceDshMenu,
  seedDshMenuGroups,
  type DshMenuEvent,
  type DshMenuState,
  type DshTokenSpan,
  type DshTriggerCandidate,
  type DshTriggerChar,
  type DshTriggerGuard,
  type DshTriggerHit,
  type DshTriggerPosition,
} from "../dshInputTriggers";

/** Candidate request handed to a source; the signal is superseded on every new hit. */
export interface DshCandidateRequest {
  query: string;
  position: DshTriggerPosition;
  signal: AbortSignal;
}

/** Everything a source receives on pick: the candidate plus the span for CAS. */
export interface DshTriggerPick {
  candidate: DshTriggerCandidate;
  position: DshTriggerPosition;
  span: DshTokenSpan;
}

/**
 * What a pick produced. `{ text }` replaces the token span with literal text
 * (the Harness' plain-text reference path — the draft carries ordinary
 * characters and the prompt ships the same literal); `"handled"` means the
 * source dealt with the pick itself, e.g. by opening its own argument picker.
 */
export type DshPickOutcome = { text: string } | "handled" | undefined;

/** One trigger source: a menu group plus the behavior behind its rows. */
export interface DshTriggerSource {
  trigger: DshTriggerChar;
  /** Group id; unique per trigger char. */
  name: string;
  /** i18n key of the group's title row. */
  labelKey: string;
  /** Menu group order — lower is higher in the list (default 0). */
  order?: number;
  candidates(request: DshCandidateRequest): Promise<readonly DshTriggerCandidate[]>;
  onPick(pick: DshTriggerPick): DshPickOutcome;
  /**
   * Called when a menu opens for this trigger so the source can pull its
   * backing data. The Harness warms once at session-scope birth because its
   * sources read live client stores; here every catalog is a round trip, so the
   * menu opening is the prewarm moment.
   */
  warm?(): void;
}

/** Keys the menu intercepts while open (all behind the IME composition guard). */
export type DshArbitrateKey = "up" | "down" | "enter" | "escape";

/** consumed = handled; pick-highlighted = enter picked the highlight; pass = let the input see it. */
export type DshArbitrateOutcome = "consumed" | "pick-highlighted" | "pass";

export interface DshTriggerMenuController {
  /** Current menu snapshot, for rendering. */
  state: DshMenuState;
  /** Feed a draft/caret change through detection and drive the menu. */
  track(draft: string, caret: number, guard: DshTriggerGuard, draftRev: number): void;
  /** Execute the candidate at (source, index). */
  pick(source: string, index: number): void;
  /** Keyboard arbitration while the menu is open. */
  arbitrate(key: DshArbitrateKey, composing: boolean): DshArbitrateOutcome;
  /** External dismiss (pointer outside, blur, send). */
  dismiss(): void;
}

/** Registration order breaks ties; `Array.prototype.sort` is stable. */
function rosterFor(
  sources: readonly DshTriggerSource[],
  trigger: DshTriggerChar,
): readonly DshTriggerSource[] {
  return sources
    .filter((source) => source.trigger === trigger)
    .sort((left, right) => (left.order ?? 0) - (right.order ?? 0));
}

/**
 * The per-composer half of the trigger pipeline: owns the authoritative hit,
 * the menu snapshot, and the candidate-fetch lifecycle.
 *
 * The hit outlives the menu state deliberately — it is the single source of the
 * span a pick applies against, and the menu snapshot alone must never be used
 * for that. Candidate fetches are generation-gated *and* abort-superseded, so a
 * slow source settling after the next keystroke cannot repopulate a stale menu.
 *
 * @param sources - registered sources; memoize so the returned callbacks stay stable.
 * @param apply - splice literal text over the token span (the composer owns the draft).
 */
export function useDshTriggerMenu(
  sources: readonly DshTriggerSource[],
  apply: (replacement: string, span: DshTokenSpan) => void,
): DshTriggerMenuController {
  const [state, setState] = useState<DshMenuState>(DSH_MENU_CLOSED);
  const stateRef = useRef<DshMenuState>(DSH_MENU_CLOSED);
  const hitRef = useRef<DshTriggerHit | null>(null);
  const fetchRef = useRef<AbortController | null>(null);
  const sourcesRef = useRef(sources);
  const applyRef = useRef(apply);

  useEffect(() => {
    sourcesRef.current = sources;
    applyRef.current = apply;
  });

  const commit = useCallback((next: DshMenuState) => {
    if (next === stateRef.current) return;
    stateRef.current = next;
    setState(next);
  }, []);

  const dispatch = useCallback(
    (event: DshMenuEvent) => {
      commit(reduceDshMenu(stateRef.current, event));
    },
    [commit],
  );

  const stopFetch = useCallback(() => {
    fetchRef.current?.abort();
    fetchRef.current = null;
  }, []);

  useEffect(() => () => stopFetch(), [stopFetch]);

  const fetchCandidates = useCallback(
    (hit: DshTriggerHit, roster: readonly DshTriggerSource[]) => {
      stopFetch();
      const controller = new AbortController();
      fetchRef.current = controller;
      const generation = stateRef.current.generation;
      for (const source of roster) {
        void source
          .candidates({ query: hit.query, position: hit.position, signal: controller.signal })
          .then(
            (items) => {
              if (controller.signal.aborted) return;
              dispatch({ type: "source-settled", generation, source: source.name, items });
            },
            (error: unknown) => {
              if (controller.signal.aborted) return;
              // A failing source drops its group silently, as it does in the
              // Harness: one unreachable catalog must not take the menu down.
              console.error(`[dsh-trigger] source "${source.name}" candidates failed:`, error);
              dispatch({ type: "source-failed", generation, source: source.name });
            },
          );
      }
    },
    [dispatch, stopFetch],
  );

  const track = useCallback(
    (draft: string, caret: number, guard: DshTriggerGuard, draftRev: number) => {
      const raw = detectDshTrigger(draft, caret, guard);
      if (raw === null) {
        hitRef.current = null;
        stopFetch();
        dispatch({ type: "close" });
        return;
      }
      const hit: DshTriggerHit = { ...raw, span: { ...raw.span, draftRev } };
      const previous = stateRef.current;
      const unchanged =
        previous.open &&
        previous.hit !== null &&
        previous.hit.trigger === hit.trigger &&
        previous.hit.query === hit.query &&
        previous.hit.span.start === hit.span.start &&
        previous.hit.span.end === hit.span.end;
      // Always refresh the CAS material, even when the token itself is
      // unchanged: the draft revision behind it may have moved.
      hitRef.current = hit;
      if (unchanged) return;
      const roster = rosterFor(sourcesRef.current, hit.trigger);
      if (roster.length === 0) {
        stopFetch();
        dispatch({ type: "close" });
        return;
      }
      const opening =
        !previous.open || previous.hit === null || previous.hit.trigger !== hit.trigger;
      if (opening) {
        for (const source of roster) source.warm?.();
        commit(
          seedDshMenuGroups(
            stateRef.current,
            roster.map((source) => source.name),
          ),
        );
      }
      dispatch({ type: "hit", hit });
      fetchCandidates(hit, roster);
    },
    [commit, dispatch, fetchCandidates, stopFetch],
  );

  const pick = useCallback(
    (source: string, index: number) => {
      const current = stateRef.current;
      const hit = hitRef.current;
      if (!current.open || hit === null) return;
      const group = current.groups.find((item) => item.source === source);
      const candidate = group?.status === "ready" ? group.items[index] : undefined;
      if (candidate === undefined) return;
      const owner = rosterFor(sourcesRef.current, hit.trigger).find((item) => item.name === source);
      if (owner === undefined) return;
      const outcome = owner.onPick({ candidate, position: hit.position, span: hit.span });
      stopFetch();
      dispatch({ type: "close" });
      if (outcome !== undefined && outcome !== "handled") applyRef.current(outcome.text, hit.span);
    },
    [dispatch, stopFetch],
  );

  const dismiss = useCallback(() => {
    stopFetch();
    dispatch({ type: "close" });
  }, [dispatch, stopFetch]);

  const arbitrate = useCallback(
    (key: DshArbitrateKey, composing: boolean): DshArbitrateOutcome => {
      if (composing) return "pass";
      const current = stateRef.current;
      if (!current.open) return "pass";
      switch (key) {
        case "up":
          dispatch({ type: "move", dir: -1 });
          return "consumed";
        case "down":
          dispatch({ type: "move", dir: 1 });
          return "consumed";
        case "escape":
          dismiss();
          return "consumed";
        case "enter": {
          if (current.highlight === null) return "pass";
          pick(current.highlight.source, current.highlight.index);
          return "pick-highlighted";
        }
      }
    },
    [dismiss, dispatch, pick],
  );

  return { state, track, pick, arbitrate, dismiss };
}
