import { invoke } from "@tauri-apps/api/core";
import { Decoration, EditorView, WidgetType } from "@uiw/react-codemirror";
import type { Extension } from "@codemirror/state";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";
import type { LspInlayHint, LspPosition } from "../../types";

export const INLAY_HINT_LIMIT = 500;

type EditorDoc = {
  lines: number;
  line: (number: number) => { from: number; to: number };
  sliceString: (from: number, to: number) => string;
};

class InlayHintWidget extends WidgetType {
  constructor(private readonly hint: LspInlayHint) {
    super();
  }

  eq(other: WidgetType): boolean {
    return other instanceof InlayHintWidget && other.hint.label === this.hint.label;
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "cm-inlay-hint";
    span.textContent = this.hint.label;
    if (this.hint.tooltip) span.title = this.hint.tooltip;
    if (this.hint.paddingLeft) span.dataset.paddingLeft = "true";
    if (this.hint.paddingRight) span.dataset.paddingRight = "true";
    return span;
  }
}

export async function requestLspInlayHints(
  request: LspDocumentRequest,
  remote?: LspRemoteContext,
): Promise<LspInlayHint[]> {
  const hints = await invoke<LspInlayHint[]>(
    lspCommandName("lsp_inlay_hints", remote),
    lspInvokeArgs({ request }, remote),
  );
  return normalizeInlayHints(hints);
}

export function normalizeInlayHints(
  hints: LspInlayHint[],
  limit = INLAY_HINT_LIMIT,
): LspInlayHint[] {
  const seen = new Set<string>();
  const normalized: LspInlayHint[] = [];
  for (const hint of hints) {
    const label = hint.label.trim();
    if (!label) continue;
    const line = Number(hint.position.line);
    const character = Number(hint.position.character);
    if (!Number.isInteger(line) || line < 0) continue;
    if (!Number.isInteger(character) || character < 0) continue;
    const key = `${line}:${character}:${label}`;
    if (seen.has(key)) continue;
    seen.add(key);
    normalized.push({
      ...hint,
      label,
      position: { line, character },
      paddingLeft: Boolean(hint.paddingLeft),
      paddingRight: Boolean(hint.paddingRight),
    });
  }
  return normalized
    .sort(
      (a, b) =>
        a.position.line - b.position.line ||
        a.position.character - b.position.character ||
        a.label.localeCompare(b.label),
    )
    .slice(0, limit);
}

export function lspPositionToDocOffset(doc: EditorDoc, position: LspPosition): number | null {
  const lineNumber = position.line + 1;
  if (lineNumber < 1 || lineNumber > doc.lines) return null;
  const line = doc.line(lineNumber);
  const text = doc.sliceString(line.from, line.to);
  let units = 0;
  for (const [relative, char] of Array.from(text).entries()) {
    if (units === position.character) return line.from + utf16PrefixLength(text, relative);
    units += char.length;
    if (units > position.character) return null;
  }
  return units === position.character ? line.to : null;
}

export function createLspInlayHintsExtension(hints: LspInlayHint[]): Extension {
  if (hints.length === 0) return [];
  return EditorView.decorations.compute([], (state) => {
    const ranges = [];
    for (const hint of hints) {
      const offset = lspPositionToDocOffset(state.doc as unknown as EditorDoc, hint.position);
      if (offset === null) continue;
      ranges.push(
        Decoration.widget({
          widget: new InlayHintWidget(hint),
          side: 1,
        }).range(offset),
      );
    }
    return Decoration.set(ranges, true);
  });
}

function utf16PrefixLength(text: string, characterCount: number): number {
  return Array.from(text).slice(0, characterCount).join("").length;
}
