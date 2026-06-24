import type {
  ReplacePreview,
  TextReplacement,
  TextSearchFileGroup,
  TextSearchMatch,
  TextSearchOptions,
} from "../../types";

export interface BuildTextSearchOptionsInput {
  caseSensitive: boolean;
  regex: boolean;
  wholeWord: boolean;
  includeGlob: string;
  excludeGlob: string;
  limit: number;
}

function trimmedGlob(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

export function buildTextSearchOptions(input: BuildTextSearchOptionsInput): TextSearchOptions {
  return {
    caseSensitive: input.caseSensitive,
    regex: input.regex,
    wholeWord: input.wholeWord,
    includeGlob: trimmedGlob(input.includeGlob),
    excludeGlob: trimmedGlob(input.excludeGlob),
    limit: input.limit,
  };
}

export function groupSearchMatches(matches: TextSearchMatch[]): TextSearchFileGroup[] {
  const groups: TextSearchFileGroup[] = [];
  const groupByPath = new Map<string, TextSearchFileGroup>();
  for (const match of matches) {
    let group = groupByPath.get(match.path);
    if (!group) {
      group = { path: match.path, name: match.name, matches: [] };
      groupByPath.set(match.path, group);
      groups.push(group);
    }
    group.matches.push(match);
  }
  return groups;
}

export function searchMatchPreview(match: TextSearchMatch, contextChars = 28): string {
  const start = Math.max(0, match.column - 1 - contextChars);
  const end = Math.min(
    match.lineText.length,
    Math.max(match.column - 1 + match.matchText.length + contextChars, contextChars),
  );
  const prefix = start > 0 ? "..." : "";
  const suffix = end < match.lineText.length ? "..." : "";
  return `${prefix}${match.lineText.slice(start, end)}${suffix}`;
}

export function flattenReplacePreview(preview: ReplacePreview): TextReplacement[] {
  return preview.files.flatMap((file) =>
    file.matches.map((match) => ({
      path: file.path,
      start: match.start,
      end: match.end,
      matchText: match.matchText,
      replacementText: match.replacementText,
    })),
  );
}

export function canApplyReplacementPreview(
  preview: ReplacePreview | null,
  query: string,
  replacement: string,
): boolean {
  return Boolean(
    preview &&
    preview.totalMatches > 0 &&
    preview.query === query.trim() &&
    preview.replacement === replacement,
  );
}
