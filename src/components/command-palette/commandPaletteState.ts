import type { CommandPaletteItem, CommandPaletteMode, CommandPaletteParsedInput } from "./types";

export function commandPaletteModeForInput(input: string): CommandPaletteParsedInput {
  const trimmed = input.trimStart();
  if (trimmed.startsWith(">")) {
    return { mode: "command", query: trimmed.slice(1).trimStart() };
  }
  return { mode: "file", query: trimmed };
}

function itemSearchText(item: CommandPaletteItem): string {
  return [item.title, item.subtitle, ...(item.keywords ?? [])]
    .filter(Boolean)
    .join(" ")
    .toLocaleLowerCase();
}

function scoreItem(item: CommandPaletteItem, query: string): number | null {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return 10;
  const title = item.title.toLocaleLowerCase();
  const subtitle = item.subtitle?.toLocaleLowerCase() ?? "";
  const keywords = (item.keywords ?? []).map((keyword) => keyword.toLocaleLowerCase());
  if (title === normalizedQuery) return 0;
  if (title.startsWith(normalizedQuery)) return 1;
  if (keywords.some((keyword) => keyword === normalizedQuery)) return 2;
  if (title.includes(normalizedQuery)) return 3;
  if (keywords.some((keyword) => keyword.includes(normalizedQuery))) return 4;
  if (subtitle.includes(normalizedQuery)) return 5;
  if (itemSearchText(item).includes(normalizedQuery)) return 6;
  return null;
}

export function rankCommandPaletteItems(
  items: CommandPaletteItem[],
  query: string,
  mode: CommandPaletteMode,
): CommandPaletteItem[] {
  return items
    .filter((item) => item.kind === mode)
    .map((item) => ({ item, score: scoreItem(item, query) }))
    .filter((entry): entry is { item: CommandPaletteItem; score: number } => entry.score !== null)
    .sort(
      (a, b) =>
        a.score.toString().localeCompare(b.score.toString(), undefined, { numeric: true }) ||
        a.item.title.localeCompare(b.item.title),
    )
    .map((entry) => entry.item);
}

export function moveCommandPaletteSelection(
  currentIndex: number,
  delta: number,
  resultCount: number,
): number {
  if (resultCount <= 0) return 0;
  return (currentIndex + delta + resultCount) % resultCount;
}
