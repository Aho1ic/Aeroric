/**
 * Port of the DeepSeek Harness tool render-intent vocabulary
 * (`@deepseek-ai/dsh-tools/presentation`). The Harness computes one `view`
 * alongside every `tool/call` and `tool/result` it delivers — on the mux stream
 * as `session/event`'s `view` slot, and on a history page as `HistoryEntry.view`.
 * The view is a pagination-time derivation and is never persisted, so the same
 * event may arrive with a different view (or none) on a later delivery; keying
 * by `seq` and letting the newest delivery win mirrors that contract.
 *
 * Views are untrusted wire data, so every arm is narrowed structurally rather
 * than cast: an unrecognized or malformed view degrades to `undefined` and the
 * caller falls back to the raw event, which is exactly what the Harness
 * specifies for a UI without the matching capability.
 */

/** Category of a tool call, used to pick an icon or treatment. */
export type DshToolCallKind =
  | "read"
  | "edit"
  | "delete"
  | "move"
  | "search"
  | "execute"
  | "fetch"
  | "other";

const CALL_KINDS: readonly DshToolCallKind[] = [
  "read",
  "edit",
  "delete",
  "move",
  "search",
  "execute",
  "fetch",
  "other",
];

/** A file location a tool reads or modifies, so the UI can follow along. */
export interface DshFileLocation {
  path: string;
  /** 1-based line to focus, when the tool named one. */
  line?: number;
}

/** A single-file change; `oldText` is null for a create or an overwrite. */
export interface DshFileDiff {
  path: string;
  oldText: string | null;
  newText: string;
}

/** One numbered line of a read window, keeping the file's own numbering. */
export interface DshReadFileLine {
  number: number;
  text: string;
}

/** One citeable source in a completed web search. */
export interface DshWebSource {
  url: string;
  title?: string;
  snippet?: string;
  publishedAt?: string;
}

export interface DshGenericCallView {
  card: "generic";
  title: string;
  kind?: DshToolCallKind;
  rawInput?: unknown;
  locations?: DshFileLocation[];
}

export interface DshTerminalCallView {
  card: "terminal";
  title: string;
  description?: string;
  cwd?: string;
}

export interface DshDiffCallView {
  card: "diff";
  title: string;
  diffs: DshFileDiff[];
  locations?: DshFileLocation[];
}

/** Provider-neutral pending-call presentation. */
export type DshToolCallView = DshGenericCallView | DshTerminalCallView | DshDiffCallView;

export interface DshGenericResultView {
  card: "generic";
  title?: string;
}

export interface DshTerminalResultView {
  card: "terminal";
  title?: string;
  output?: string;
  /** Exit code when the run ended by exiting; mutually exclusive with `signal`. */
  exitCode?: number;
  signal?: string;
}

export interface DshDiffResultView {
  card: "diff";
  title?: string;
  diffs: DshFileDiff[];
}

export interface DshSearchLineMatch {
  lineNumber: number;
  line: string;
}

export interface DshSearchFileMatches {
  path: string;
  matches: DshSearchLineMatch[];
}

export interface DshSearchMatchesResultView {
  card: "search";
  shape: "matches";
  title?: string;
  files: DshSearchFileMatches[];
  truncated: boolean;
  total: number;
}

export interface DshSearchPathsResultView {
  card: "search";
  shape: "paths";
  title?: string;
  paths: string[];
  truncated: boolean;
  total: number;
}

export type DshSearchResultView = DshSearchMatchesResultView | DshSearchPathsResultView;

export interface DshReadResultView {
  card: "read";
  title?: string;
  path: string;
  offset: number;
  lines: DshReadFileLine[];
  totalLines: number;
  lang?: string;
}

export interface DshWebSearchResultView {
  card: "web";
  kind: "search";
  title?: string;
  sources: DshWebSource[];
  answer?: string;
  truncated: boolean;
}

export interface DshWebFetchResultView {
  card: "web";
  kind: "fetch";
  title?: string;
  url: string;
  statusCode: number;
  truncated: boolean;
}

export type DshWebResultView = DshWebSearchResultView | DshWebFetchResultView;

/** How a tool wants the completed call shown. */
export type DshToolResultView =
  | DshGenericResultView
  | DshTerminalResultView
  | DshDiffResultView
  | DshSearchResultView
  | DshReadResultView
  | DshWebResultView;

/** The `view` slot riding one `tool/call` or `tool/result` delivery. */
export type DshToolEventView =
  | { for: "call"; view: DshToolCallView }
  | { for: "result"; view: DshToolResultView };

type Dict = Record<string, unknown>;

function dict(value: unknown): Dict | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Dict)
    : undefined;
}

function str(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

/** Required non-empty string; a blank title is treated as absent upstream. */
function requiredStr(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function num(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function bool(value: unknown): boolean {
  return value === true;
}

function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

/** Map an array through a narrowing parser, dropping entries that fail. */
function mapDefined<T>(value: unknown, parse: (item: Dict) => T | undefined): T[] {
  const result: T[] = [];
  for (const item of array(value)) {
    const record = dict(item);
    if (!record) continue;
    const parsed = parse(record);
    if (parsed !== undefined) result.push(parsed);
  }
  return result;
}

function parseLocation(item: Dict): DshFileLocation | undefined {
  const path = requiredStr(item.path);
  if (path === undefined) return undefined;
  const line = num(item.line);
  return line === undefined ? { path } : { path, line };
}

function parseDiff(item: Dict): DshFileDiff | undefined {
  const path = requiredStr(item.path);
  const newText = str(item.newText);
  if (path === undefined || newText === undefined) return undefined;
  // `oldText` is deliberately nullable: null means create/overwrite. Anything
  // that is neither a string nor null cannot be diffed against, so it degrades
  // to null rather than dropping the whole entry.
  const oldText = str(item.oldText) ?? null;
  return { path, oldText, newText };
}

function parseReadLine(item: Dict): DshReadFileLine | undefined {
  const number = num(item.number);
  const text = str(item.text);
  if (number === undefined || text === undefined) return undefined;
  return { number, text };
}

function parseLineMatch(item: Dict): DshSearchLineMatch | undefined {
  const lineNumber = num(item.lineNumber);
  const line = str(item.line);
  if (lineNumber === undefined || line === undefined) return undefined;
  return { lineNumber, line };
}

function parseFileMatches(item: Dict): DshSearchFileMatches | undefined {
  const path = requiredStr(item.path);
  if (path === undefined) return undefined;
  return { path, matches: mapDefined(item.matches, parseLineMatch) };
}

function parseWebSource(item: Dict): DshWebSource | undefined {
  const url = requiredStr(item.url);
  if (url === undefined) return undefined;
  const source: DshWebSource = { url };
  const title = requiredStr(item.title);
  if (title !== undefined) source.title = title;
  const snippet = requiredStr(item.snippet);
  if (snippet !== undefined) source.snippet = snippet;
  const publishedAt = requiredStr(item.publishedAt);
  if (publishedAt !== undefined) source.publishedAt = publishedAt;
  return source;
}

function parseCallKind(value: unknown): DshToolCallKind | undefined {
  return CALL_KINDS.find((kind) => kind === value);
}

/** Narrow one pending-call view; `undefined` when the arm is unusable. */
export function parseDshToolCallView(value: unknown): DshToolCallView | undefined {
  const record = dict(value);
  if (!record) return undefined;
  const title = requiredStr(record.title);
  if (title === undefined) return undefined;
  switch (record.card) {
    case "terminal": {
      const view: DshTerminalCallView = { card: "terminal", title };
      const description = requiredStr(record.description);
      if (description !== undefined) view.description = description;
      const cwd = requiredStr(record.cwd);
      if (cwd !== undefined) view.cwd = cwd;
      return view;
    }
    case "diff": {
      const diffs = mapDefined(record.diffs, parseDiff);
      // A diff card with no readable diff has nothing the generic card cannot
      // show, so it degrades instead of rendering an empty diff frame.
      if (diffs.length === 0) return undefined;
      const view: DshDiffCallView = { card: "diff", title, diffs };
      const locations = mapDefined(record.locations, parseLocation);
      if (locations.length > 0) view.locations = locations;
      return view;
    }
    case "generic": {
      const view: DshGenericCallView = { card: "generic", title };
      const kind = parseCallKind(record.kind);
      if (kind !== undefined) view.kind = kind;
      if (record.rawInput !== undefined) view.rawInput = record.rawInput;
      const locations = mapDefined(record.locations, parseLocation);
      if (locations.length > 0) view.locations = locations;
      return view;
    }
    default:
      return undefined;
  }
}

/** Narrow one completed-call view; `undefined` when the arm is unusable. */
export function parseDshToolResultView(value: unknown): DshToolResultView | undefined {
  const record = dict(value);
  if (!record) return undefined;
  const title = requiredStr(record.title);
  switch (record.card) {
    case "terminal": {
      const view: DshTerminalResultView = { card: "terminal" };
      if (title !== undefined) view.title = title;
      const output = str(record.output);
      if (output !== undefined) view.output = output;
      // exitCode and signal are mutually exclusive; a payload carrying both is
      // read as the signal, the stronger statement about how the run ended.
      const signal = requiredStr(record.signal);
      const exitCode = num(record.exitCode);
      if (signal !== undefined) view.signal = signal;
      else if (exitCode !== undefined) view.exitCode = exitCode;
      return view;
    }
    case "diff": {
      const diffs = mapDefined(record.diffs, parseDiff);
      if (diffs.length === 0) return undefined;
      const view: DshDiffResultView = { card: "diff", diffs };
      if (title !== undefined) view.title = title;
      return view;
    }
    case "search": {
      if (record.shape === "paths") {
        const paths = array(record.paths).filter(
          (item): item is string => typeof item === "string",
        );
        const view: DshSearchPathsResultView = {
          card: "search",
          shape: "paths",
          paths,
          truncated: bool(record.truncated),
          total: num(record.total) ?? paths.length,
        };
        if (title !== undefined) view.title = title;
        return view;
      }
      if (record.shape === "matches") {
        const files = mapDefined(record.files, parseFileMatches);
        const retained = files.reduce((sum, file) => sum + file.matches.length, 0);
        const view: DshSearchMatchesResultView = {
          card: "search",
          shape: "matches",
          files,
          truncated: bool(record.truncated),
          total: num(record.total) ?? retained,
        };
        if (title !== undefined) view.title = title;
        return view;
      }
      return undefined;
    }
    case "read": {
      const path = requiredStr(record.path);
      if (path === undefined) return undefined;
      const view: DshReadResultView = {
        card: "read",
        path,
        // An empty window still reports where it starts, so offset survives a
        // byte cap that selected no line.
        offset: num(record.offset) ?? 1,
        lines: mapDefined(record.lines, parseReadLine),
        totalLines: num(record.totalLines) ?? 0,
      };
      if (title !== undefined) view.title = title;
      const lang = requiredStr(record.lang);
      if (lang !== undefined) view.lang = lang;
      return view;
    }
    case "web": {
      if (record.kind === "fetch") {
        const url = requiredStr(record.url);
        const statusCode = num(record.statusCode);
        if (url === undefined || statusCode === undefined) return undefined;
        const view: DshWebFetchResultView = {
          card: "web",
          kind: "fetch",
          url,
          statusCode,
          truncated: bool(record.truncated),
        };
        if (title !== undefined) view.title = title;
        return view;
      }
      if (record.kind === "search") {
        const view: DshWebSearchResultView = {
          card: "web",
          kind: "search",
          sources: mapDefined(record.sources, parseWebSource),
          truncated: bool(record.truncated),
        };
        if (title !== undefined) view.title = title;
        const answer = requiredStr(record.answer);
        if (answer !== undefined) view.answer = answer;
        return view;
      }
      return undefined;
    }
    case "generic": {
      const view: DshGenericResultView = { card: "generic" };
      if (title !== undefined) view.title = title;
      return view;
    }
    default:
      return undefined;
  }
}

/**
 * Narrow one `view` slot from a mux frame or a history entry. `for` names which
 * vocabulary applies, so the arm is chosen without re-inspecting the event.
 */
export function parseDshToolEventView(value: unknown): DshToolEventView | undefined {
  const record = dict(value);
  if (!record) return undefined;
  if (record.for === "call") {
    const view = parseDshToolCallView(record.view);
    return view === undefined ? undefined : { for: "call", view };
  }
  if (record.for === "result") {
    const view = parseDshToolResultView(record.view);
    return view === undefined ? undefined : { for: "result", view };
  }
  return undefined;
}
