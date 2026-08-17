/**
 * Caret-driven `/` and `@` trigger pipeline, ported from the DeepSeek Harness'
 * `ui-input-trigger` pure core.
 *
 * The Harness detects a trigger token under the caret on every draft change
 * instead of only at the start of an empty prompt, fans the token's query out
 * to the sources registered for that trigger char, and reduces their answers
 * into one grouped menu. Everything here is pure — detection, menu reduction,
 * candidate ranking, and the span replacement a pick applies — so the rules can
 * be tested without a composer, a session, or a live catalog.
 */

/** Trigger character a source binds to. */
export type DshTriggerChar = "/" | "@";

/** Where the token sits: leading (the trimmed draft starts with it) or inline. */
export type DshTriggerPosition = "leading" | "inline";

/**
 * Trigger availability tier, derived from the composer phase.
 *
 * `plain` = both chars live; `claimed` = `/` suppressed and `@` live (a command
 * already owns the line); `frozen` = neither (the draft is not accepting input).
 */
export type DshTriggerTier = "plain" | "claimed" | "frozen";

export interface DshTriggerGuard {
  tier: DshTriggerTier;
}

/** Pick-moment snapshot of the token span; a stale `draftRev` voids the pick. */
export interface DshTokenSpan {
  start: number;
  end: number;
  draftRev: number;
}

/** A trigger token detected under the caret. */
export interface DshTriggerHit {
  trigger: DshTriggerChar;
  /** Text between the trigger char and the caret, live-filtered. */
  query: string;
  position: DshTriggerPosition;
  span: DshTokenSpan;
}

const WORD_CHAR = /[\p{L}\p{N}_]/u;
const WHITESPACE = /\s/u;

/**
 * Whether the char before a trigger allows it to open a menu: start of draft,
 * whitespace, or punctuation. `user@host` therefore never triggers, and `/`
 * stays dead as the second slash of `//` or right after a scheme separator
 * (`https:/`, `C:/`) so URLs and Windows paths type through untouched.
 */
function boundaryOk(draft: string, index: number, char: DshTriggerChar): boolean {
  if (index === 0) return true;
  const prev = draft.charAt(index - 1);
  if (WHITESPACE.test(prev)) return true;
  if (WORD_CHAR.test(prev)) return false;
  if (char === "/") {
    if (prev === "/") return false;
    if (prev === ":" && index >= 2 && !WHITESPACE.test(draft.charAt(index - 2))) return false;
  }
  return true;
}

/**
 * Detect the trigger token live at the caret, scanning left until whitespace.
 *
 * A guard-suppressed trigger char is scanned through like an ordinary
 * character rather than ending the scan, so `@name` still resolves while a
 * command owns the line.
 */
export function detectDshTrigger(
  draft: string,
  caret: number,
  guard: DshTriggerGuard,
): DshTriggerHit | null {
  if (guard.tier === "frozen") return null;
  for (let index = caret - 1; index >= 0; index -= 1) {
    const char = draft.charAt(index);
    if (WHITESPACE.test(char)) return null;
    if (char !== "/" && char !== "@") continue;
    if (guard.tier === "claimed" && char === "/") continue;
    if (!boundaryOk(draft, index, char)) continue;
    return {
      trigger: char,
      query: draft.slice(index + 1, caret),
      position: draft.search(/\S/) === index ? "leading" : "inline",
      span: { start: index, end: caret, draftRev: 0 },
    };
  }
  return null;
}

/** One menu candidate. Pure display data — no behavior declaration. */
export interface DshTriggerCandidate {
  name: string;
  description?: string;
  /** Ghost-text style hint shown after the name (e.g. an argument shape). */
  hint?: string;
}

/** One source's row group inside the menu. */
export interface DshMenuGroup {
  readonly source: string;
  readonly status: "pending" | "ready";
  readonly items: readonly DshTriggerCandidate[];
}

export interface DshMenuHighlight {
  readonly source: string;
  readonly index: number;
}

/** Menu state: one group per source; every group ready-and-empty auto-closes. */
export interface DshMenuState {
  readonly open: boolean;
  readonly hit: DshTriggerHit | null;
  /** Monotonic per-hit generation; a stale source settlement is dropped. */
  readonly generation: number;
  readonly groups: readonly DshMenuGroup[];
  readonly highlight: DshMenuHighlight | null;
}

/** Menu reduction events. A failed source is removed silently (logged only). */
export type DshMenuEvent =
  | { readonly type: "hit"; readonly hit: DshTriggerHit | null }
  | {
      readonly type: "source-settled";
      readonly generation: number;
      readonly source: string;
      readonly items?: readonly DshTriggerCandidate[];
    }
  | { readonly type: "source-failed"; readonly generation: number; readonly source: string }
  | { readonly type: "move"; readonly dir: 1 | -1 }
  | { readonly type: "close" };

/** Closed rest state at generation 0. */
export const DSH_MENU_CLOSED: DshMenuState = {
  open: false,
  hit: null,
  generation: 0,
  groups: [],
  highlight: null,
};

/**
 * Replace the roster with pending groups for `sources`, in menu order.
 *
 * The `hit` event carries no roster, so the reducer cannot invent groups: the
 * caller seeds them when a menu opens and then dispatches the hit.
 */
export function seedDshMenuGroups(state: DshMenuState, sources: readonly string[]): DshMenuState {
  return {
    ...state,
    groups: sources.map((source) => ({ source, status: "pending", items: [] })),
    highlight: null,
  };
}

/** Close, preserving the generation so in-flight settlements stay droppable. */
function closedMenu(state: DshMenuState): DshMenuState {
  if (!state.open && state.hit === null && state.groups.length === 0 && state.highlight === null) {
    return state;
  }
  return { open: false, hit: null, generation: state.generation, groups: [], highlight: null };
}

/** First item of the first non-empty ready group, or null. */
function firstHighlight(groups: readonly DshMenuGroup[]): DshMenuHighlight | null {
  for (const group of groups) {
    if (group.status === "ready" && group.items.length > 0)
      return { source: group.source, index: 0 };
  }
  return null;
}

/** The highlight itself while it still points at a ready item, else null. */
function validHighlight(
  highlight: DshMenuHighlight | null,
  groups: readonly DshMenuGroup[],
): DshMenuHighlight | null {
  if (!highlight) return null;
  const group = groups.find((item) => item.source === highlight.source);
  const live = group && group.status === "ready" && highlight.index < group.items.length;
  return live ? highlight : null;
}

/** Flatten ready items into (source, index) positions, in group order. */
function positions(groups: readonly DshMenuGroup[]): DshMenuHighlight[] {
  const out: DshMenuHighlight[] = [];
  for (const group of groups) {
    if (group.status !== "ready") continue;
    for (let index = 0; index < group.items.length; index += 1) {
      out.push({ source: group.source, index });
    }
  }
  return out;
}

/** True when every group is ready with zero items (the auto-close condition). */
function allReadyEmpty(groups: readonly DshMenuGroup[]): boolean {
  return groups.every((group) => group.status === "ready" && group.items.length === 0);
}

/**
 * Pure menu reducer.
 *
 * `hit` opens a new generation over the seeded roster (a null hit closes);
 * a settlement outside the current generation, the open menu, or the roster is
 * dropped; a settlement or failure that leaves every group ready-and-empty
 * auto-closes; `move` cycles the highlight across ready items. Stale or no-op
 * events return the same reference so subscribers can skip re-rendering.
 */
export function reduceDshMenu(state: DshMenuState, event: DshMenuEvent): DshMenuState {
  switch (event.type) {
    case "hit": {
      if (event.hit === null) return closedMenu(state);
      return {
        open: true,
        hit: event.hit,
        generation: state.generation + 1,
        groups: state.groups.map((group) => ({
          source: group.source,
          status: "pending",
          items: [],
        })),
        highlight: null,
      };
    }
    case "source-settled": {
      if (!state.open || event.generation !== state.generation) return state;
      const at = state.groups.findIndex((group) => group.source === event.source);
      if (at < 0) return state;
      const items = event.items ?? [];
      const groups = state.groups.map((group, index) =>
        index === at ? { source: group.source, status: "ready" as const, items } : group,
      );
      if (allReadyEmpty(groups)) return closedMenu(state);
      return {
        ...state,
        groups,
        highlight: validHighlight(state.highlight, groups) ?? firstHighlight(groups),
      };
    }
    case "source-failed": {
      if (!state.open || event.generation !== state.generation) return state;
      if (!state.groups.some((group) => group.source === event.source)) return state;
      const groups = state.groups.filter((group) => group.source !== event.source);
      if (groups.length === 0 || allReadyEmpty(groups)) return closedMenu(state);
      return {
        ...state,
        groups,
        highlight: validHighlight(state.highlight, groups) ?? firstHighlight(groups),
      };
    }
    case "move": {
      if (!state.open) return state;
      const pos = positions(state.groups);
      if (pos.length === 0) return state;
      const current = state.highlight;
      const at = current
        ? pos.findIndex((item) => item.source === current.source && item.index === current.index)
        : -1;
      const next =
        at < 0
          ? pos[event.dir === 1 ? 0 : pos.length - 1]
          : pos[(at + event.dir + pos.length) % pos.length];
      if (next === undefined) return state;
      if (current && next.source === current.source && next.index === current.index) return state;
      return { ...state, highlight: next };
    }
    case "close":
      return closedMenu(state);
  }
}

/** Extra weight for name starts and separator boundaries. */
function boundaryBonus(name: string, index: number): number {
  const prev = name.charAt(index - 1);
  return index === 0 || prev === "-" || prev === "_" ? 8 : 0;
}

/**
 * Score the strongest ordered-subsequence alignment in O(name × query).
 * Boundary and adjacent matches earn weight; skipped and leading characters
 * cost weight. `undefined` = the query is not a subsequence of the name.
 */
function fuzzyScore(name: string, query: string): number | undefined {
  if (query === "") return 0;
  if (query.length > name.length) return undefined;
  const noMatch = Number.NEGATIVE_INFINITY;
  let previous = Array<number>(name.length).fill(noMatch);
  for (let index = 0; index < name.length; index += 1) {
    if (name.charAt(index) === query.charAt(0)) {
      previous[index] = 1 + boundaryBonus(name, index) - index;
    }
  }
  for (let queryIndex = 1; queryIndex < query.length; queryIndex += 1) {
    const current = Array<number>(name.length).fill(noMatch);
    let bestGapped = noMatch;
    for (let index = 0; index < name.length; index += 1) {
      const gappedIndex = index - 2;
      if (gappedIndex >= 0) {
        const prior = previous[gappedIndex] ?? noMatch;
        if (prior !== noMatch) bestGapped = Math.max(bestGapped, prior + gappedIndex);
      }
      if (name.charAt(index) !== query.charAt(queryIndex)) continue;
      const bonus = 1 + boundaryBonus(name, index);
      const adjacent = index > 0 ? (previous[index - 1] ?? noMatch) : noMatch;
      if (adjacent !== noMatch) current[index] = adjacent + bonus + 4;
      if (bestGapped !== noMatch) {
        current[index] = Math.max(current[index] ?? noMatch, bestGapped + bonus + 1 - index);
      }
    }
    previous = current;
  }
  let best = noMatch;
  for (const score of previous) best = Math.max(best, score);
  return best === noMatch ? undefined : best;
}

/**
 * Case-insensitive fuzzy filter with prefix matches first and stable ordering
 * for equal scores — the Harness' command-menu ranking, so `/cmp` still finds
 * `compact` without listing every command that merely contains those letters.
 */
export function rankDshTriggerCandidates(
  candidates: readonly DshTriggerCandidate[],
  rawQuery: string,
): readonly DshTriggerCandidate[] {
  const query = rawQuery.toLowerCase();
  if (query === "") return candidates;
  const ranked: {
    candidate: DshTriggerCandidate;
    index: number;
    prefix: boolean;
    score: number;
  }[] = [];
  candidates.forEach((candidate, index) => {
    const name = candidate.name.toLowerCase();
    const score = fuzzyScore(name, query);
    if (score !== undefined) {
      ranked.push({ candidate, index, prefix: name.startsWith(query), score });
    }
  });
  ranked.sort(
    (left, right) =>
      Number(right.prefix) - Number(left.prefix) ||
      right.score - left.score ||
      left.index - right.index,
  );
  return ranked.map((match) => match.candidate);
}

/** Draft rewrite a pick produces: the new text plus where the caret lands. */
export interface DshTokenReplacement {
  text: string;
  caret: number;
}

/**
 * Replace one trigger token with literal text.
 *
 * The span is a pick-moment snapshot, so it carries the draft revision it was
 * taken against: a draft that moved on in between voids the whole action
 * instead of splicing text over whatever now occupies those offsets.
 */
export function replaceDshTriggerToken(
  draft: string,
  span: DshTokenSpan,
  replacement: string,
  draftRev: number,
): DshTokenReplacement | null {
  if (span.draftRev !== draftRev) return null;
  if (span.start < 0 || span.start > span.end || span.end > draft.length) return null;
  return {
    text: `${draft.slice(0, span.start)}${replacement}${draft.slice(span.end)}`,
    caret: span.start + replacement.length,
  };
}
