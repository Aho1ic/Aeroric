import { invoke } from "@tauri-apps/api/core";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";
import type { OpenFileSelection } from "../../hooks/projectPanelsState";

export type LspReferenceLocation = {
  uri: string;
  path: string;
  range: {
    start: {
      line: number;
      character: number;
    };
    end: {
      line: number;
      character: number;
    };
  };
};

export type LspReferenceOpenTarget = {
  path: string;
  name: string;
  selection: OpenFileSelection;
};

export type LspReferencePreview = {
  line: number;
  column: number;
  text: string;
};

export async function findLspReferences(
  request: LspDocumentRequest,
  remote?: LspRemoteContext,
): Promise<LspReferenceLocation[]> {
  return invoke<LspReferenceLocation[]>(
    lspCommandName("lsp_references", remote),
    lspInvokeArgs({ request }, remote),
  );
}

export function lspReferenceToOpenTarget(location: LspReferenceLocation): LspReferenceOpenTarget {
  return {
    path: location.path,
    name: location.path.split(/[\\/]/).pop() ?? location.path,
    selection: {
      line: location.range.start.line + 1,
      column: location.range.start.character + 1,
    },
  };
}

export function lspReferenceKey(location: LspReferenceLocation, index = 0): string {
  return `${location.uri}:${location.range.start.line}:${location.range.start.character}:${index}`;
}

export function lspReferencePreviewLine(
  content: string,
  location: LspReferenceLocation,
  maxLength = 160,
): LspReferencePreview {
  const line = location.range.start.line + 1;
  const column = location.range.start.character + 1;
  const rawLine = content.split(/\r?\n/)[Math.max(0, line - 1)] ?? "";
  const normalized = rawLine.trim();
  const text =
    normalized.length > maxLength
      ? `${normalized.slice(0, Math.max(0, maxLength - 3))}...`
      : normalized;
  return {
    line,
    column,
    text,
  };
}
