import { invoke } from "@tauri-apps/api/core";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";
import type { OpenFileSelection } from "../../hooks/projectPanelsState";
import type { LspPosition, LspRange, LspSymbol } from "../../types";

export const DOCUMENT_SYMBOL_LIMIT = 250;

export type LspDocumentOutline = {
  symbols: LspSymbol[];
  truncated: boolean;
};

export type BreadcrumbPathSegment = {
  label: string;
  title: string;
};

export async function requestLspDocumentOutline(
  request: LspDocumentRequest,
  remote?: LspRemoteContext,
): Promise<LspDocumentOutline> {
  const symbols = await invoke<LspSymbol[]>(
    lspCommandName("lsp_document_symbols", remote),
    lspInvokeArgs({ request }, remote),
  );
  return normalizeDocumentSymbols(symbols);
}

export function normalizeDocumentSymbols(
  symbols: LspSymbol[],
  limit = DOCUMENT_SYMBOL_LIMIT,
): LspDocumentOutline {
  const validSymbols = symbols.filter((symbol) => symbol.name.trim() && symbol.path);
  return {
    symbols: validSymbols.slice(0, limit),
    truncated: validSymbols.length > limit,
  };
}

export function lspSymbolToSelection(symbol: LspSymbol): OpenFileSelection {
  return {
    line: symbol.selectionRange.start.line + 1,
    column: symbol.selectionRange.start.character + 1,
  };
}

export function activeSymbolBreadcrumbs(
  symbols: LspSymbol[],
  cursor: { line: number; column: number },
): LspSymbol[] {
  const position = {
    line: Math.max(0, cursor.line - 1),
    character: Math.max(0, cursor.column - 1),
  };
  return symbols
    .filter((symbol) => rangeContainsPosition(symbol.range, position))
    .sort(
      (a, b) =>
        rangeSize(b.range) - rangeSize(a.range) ||
        comparePositions(a.selectionRange.start, b.selectionRange.start),
    );
}

export function outlineSymbolDepth(symbol: LspSymbol, symbols: LspSymbol[]): number {
  let depth = 1;
  let current: LspSymbol | undefined = symbol;
  const visited = new Set<string>();
  while (current?.containerName) {
    const key = outlineSymbolKey(current);
    if (visited.has(key)) break;
    visited.add(key);
    const parent = symbols.find(
      (candidate) =>
        candidate.path === current?.path &&
        candidate.name === current.containerName &&
        rangeContainsPosition(candidate.range, current.selectionRange.start),
    );
    if (!parent) break;
    depth += 1;
    current = parent;
  }
  return depth;
}

export function outlineSymbolKey(symbol: LspSymbol): string {
  return [
    symbol.path,
    symbol.name,
    symbol.selectionRange.start.line,
    symbol.selectionRange.start.character,
  ].join(":");
}

export function fileBreadcrumbSegments(
  projectPath: string,
  filePath: string,
): BreadcrumbPathSegment[] {
  const relativePath = relativeFilePath(projectPath, filePath);
  const separator = relativePath.includes("\\") ? "\\" : "/";
  return relativePath
    .split(/[\\/]+/)
    .filter(Boolean)
    .map((label, index, parts) => ({
      label,
      title: parts.slice(0, index + 1).join(separator),
    }));
}

function relativeFilePath(projectPath: string, filePath: string): string {
  if (filePath === projectPath) return filePath.split(/[\\/]+/).pop() ?? filePath;
  if (filePath.startsWith(`${projectPath}/`) || filePath.startsWith(`${projectPath}\\`)) {
    return filePath.slice(projectPath.length + 1);
  }
  return filePath;
}

function rangeContainsPosition(range: LspRange, position: LspPosition): boolean {
  return comparePositions(position, range.start) >= 0 && comparePositions(position, range.end) <= 0;
}

function comparePositions(a: LspPosition, b: LspPosition): number {
  return a.line - b.line || a.character - b.character;
}

function rangeSize(range: LspRange): number {
  return (range.end.line - range.start.line) * 100_000 + range.end.character - range.start.character;
}
