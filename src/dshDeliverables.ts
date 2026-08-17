/**
 * Clickable produced-file references for a DeepSeek Harness closing message.
 *
 * The Harness' deliverables plugin asks the model to write the files it created
 * or changed as Markdown inline code, then links every such token its
 * produced-file vocabulary recognizes. The vocabulary is the mutation tools'
 * own follow-along locations, never the prose: a token becomes an opener only
 * when a successful mutation really recorded that path, so a reference can
 * neither open the wrong file nor point at nothing.
 */

import type { DshProducedFile } from "./dshSessionFeatures";

/** One run of prose: literal text, or a resolved produced-file reference. */
export type DshProseSegment =
  | { kind: "text"; text: string }
  | { kind: "mention"; token: string; path: string };

/**
 * Trailing path segment, the part that identifies the file at a glance.
 *
 * @param path - Slash- or backslash-separated path.
 * @returns The final segment, or the whole string when separator-free.
 */
export function dshBasename(path: string): string {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return at === -1 ? path : path.slice(at + 1);
}

/**
 * Produced paths a message at `seq` may reference, in first-seen order.
 *
 * The cut is by sequence rather than by turn: Aeroric's fold already keeps one
 * entry per path for the whole session, so scoping to the closing turn would
 * leave a file written early and touched again later unlinkable. A path that
 * appears here was produced by this session before the message named it, which
 * is what the reference promises; the wider set only ever makes the ambiguity
 * check below stricter.
 *
 * @param files - The session's produced files, ascending by seq.
 * @param seq - The referencing message's seq; later mutations are excluded.
 * @returns Distinct paths produced up to and including `seq`.
 */
export function dshMentionVocabulary(
  files: readonly DshProducedFile[],
  seq = Number.POSITIVE_INFINITY,
): readonly string[] {
  const paths: string[] = [];
  const seen = new Set<string>();
  for (const file of files) {
    if (file.seq > seq || seen.has(file.path)) continue;
    seen.add(file.path);
    paths.push(file.path);
  }
  return paths;
}

/**
 * Resolve one inline-code token against the produced vocabulary.
 *
 * A token resolves by exact path, or by being exactly the basename of exactly
 * one produced path. A basename two paths share stays inert rather than
 * guessing, as does a suffix that is neither — the renderer never infers a file
 * from something that merely looks like a path.
 *
 * @param paths - The vocabulary from {@link dshMentionVocabulary}.
 * @param token - The authored token, exactly as written.
 * @returns The produced path, or undefined when the token names no known file.
 */
export function resolveDshProducedMention(
  paths: readonly string[],
  token: string,
): string | undefined {
  if (paths.includes(token)) return token;
  const matches = paths.filter((path) => dshBasename(path) === token);
  return matches.length === 1 ? matches[0] : undefined;
}

/** An inline-code span, or the unmatched backtick run that ends at `end`. */
interface DshCodeSpan {
  /** Index just past the span, or just past an unmatched opening run. */
  end: number;
  /** The span's content, or null when the opening run found no closer. */
  token: string | null;
}

/** CommonMark drops one space from each end when both are present. */
function unpad(content: string): string {
  return content.startsWith(" ") && content.endsWith(" ") && content.trim() !== ""
    ? content.slice(1, -1)
    : content;
}

/**
 * Read the inline-code span opening at `start`.
 *
 * The opening run closes on a run of the same length, as in Markdown, but never
 * across a line break: a file reference sits on one line, and stopping there is
 * what keeps a fenced code block from being mistaken for one long span.
 */
function codeSpanAt(prose: string, start: number): DshCodeSpan {
  let open = start;
  while (prose[open] === "`") open += 1;
  const fence = open - start;
  let cursor = open;
  while (cursor < prose.length) {
    const char = prose[cursor];
    if (char === "\n") break;
    if (char !== "`") {
      cursor += 1;
      continue;
    }
    let run = cursor;
    while (prose[run] === "`") run += 1;
    if (run - cursor === fence) return { end: run, token: unpad(prose.slice(open, cursor)) };
    cursor = run;
  }
  return { end: open, token: null };
}

/**
 * Split prose into literal runs and the produced-file references it names.
 *
 * Only a recognized reference changes: an unmatched backtick run and a token no
 * produced path answers stay in the text exactly as written, since this surface
 * renders prose rather than Markdown and must not grow code chrome of its own.
 *
 * @param prose - The message text.
 * @param paths - The vocabulary from {@link dshMentionVocabulary}.
 * @returns Segments in document order; a single text run when nothing resolves.
 */
export function segmentDshProse(prose: string, paths: readonly string[]): DshProseSegment[] {
  const segments: DshProseSegment[] = [];
  let buffer = "";
  let index = 0;
  const flush = () => {
    if (buffer) segments.push({ kind: "text", text: buffer });
    buffer = "";
  };
  while (index < prose.length) {
    if (prose[index] !== "`") {
      buffer += prose[index];
      index += 1;
      continue;
    }
    const span = codeSpanAt(prose, index);
    const token = span.token;
    const path = token === null ? undefined : resolveDshProducedMention(paths, token);
    if (token === null || path === undefined) {
      buffer += prose.slice(index, span.end);
      index = span.end;
      continue;
    }
    flush();
    segments.push({ kind: "mention", token, path });
    index = span.end;
  }
  flush();
  return segments;
}

/** Attribute the transcript's delegated opener reads the resolved path from. */
export const DSH_MENTION_ATTRIBUTE = "data-dsh-file";

/**
 * Turn the produced-file references of already-sanitized prose HTML into
 * openers.
 *
 * The transcript renders Markdown before this runs, so a reference is a `<code>`
 * element rather than a token: each one whose text names a produced file gets an
 * opener button, and every other stays the inert code it was. A `<code>` inside
 * a fenced block or an anchor is skipped — a listing is not a reference, and a
 * button cannot nest in a link. The button is built through the DOM, never by
 * splicing strings, so a path can add no markup of its own.
 *
 * @param html - Sanitized prose HTML from the Markdown pipeline.
 * @param paths - The vocabulary from {@link dshMentionVocabulary}.
 * @param label - Localizes the accessible open-label for a resolved path.
 * @returns The HTML with resolved references wrapped in openers.
 */
export function linkDshProducedMentions(
  html: string,
  paths: readonly string[],
  label: (path: string) => string,
): string {
  if (paths.length === 0) return html;
  const template = document.createElement("template");
  template.innerHTML = html;
  let linked = false;
  for (const code of template.content.querySelectorAll("code")) {
    if (code.closest("pre") !== null || code.closest("a") !== null) continue;
    const token = code.textContent ?? "";
    const path = resolveDshProducedMention(paths, token);
    if (path === undefined) continue;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "dsh-file-mention";
    button.title = path;
    button.setAttribute(DSH_MENTION_ATTRIBUTE, path);
    button.setAttribute("aria-label", label(path));
    button.textContent = token;
    code.replaceChildren(button);
    linked = true;
  }
  return linked ? template.innerHTML : html;
}
