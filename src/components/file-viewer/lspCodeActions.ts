import { invoke } from "@tauri-apps/api/core";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";
import type { DiagnosticItem } from "../../types";
import type { LspWorkspaceEdit } from "./lspRename";

export type LspCommand = {
  title?: string | null;
  command: string;
  arguments?: unknown[];
};

export type LspCodeAction = {
  title: string;
  kind?: string | null;
  edit?: LspWorkspaceEdit | null;
  command?: LspCommand | null;
};

export type LspCodeActionDiagnostic = {
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
  severity: 1 | 2 | 3 | 4;
  source: string;
  message: string;
  code?: string;
};

function lspDiagnosticSeverity(severity: DiagnosticItem["severity"]): 1 | 2 | 3 | 4 {
  if (severity === "error") return 1;
  if (severity === "warning") return 2;
  return 3;
}

export function diagnosticToLspCodeActionDiagnostic(
  diagnostic: DiagnosticItem,
): LspCodeActionDiagnostic {
  const line = Math.max(0, diagnostic.line - 1);
  const character = Math.max(0, diagnostic.column - 1);
  return {
    range: {
      start: { line, character },
      end: { line, character },
    },
    severity: lspDiagnosticSeverity(diagnostic.severity),
    source: diagnostic.source.startsWith("lsp:")
      ? diagnostic.source.slice("lsp:".length)
      : diagnostic.source,
    message: diagnostic.message,
    ...(diagnostic.code ? { code: diagnostic.code } : {}),
  };
}

export function diagnosticsForLspCodeAction(
  request: LspDocumentRequest,
  diagnostics: DiagnosticItem[],
): LspCodeActionDiagnostic[] {
  const line = request.line + 1;
  return diagnostics
    .filter((diagnostic) => diagnostic.file === request.filePath && diagnostic.line === line)
    .map(diagnosticToLspCodeActionDiagnostic);
}

export function requestLspCodeActions(
  request: LspDocumentRequest,
  diagnostics: LspCodeActionDiagnostic[] = [],
  remote?: LspRemoteContext,
): Promise<LspCodeAction[]> {
  return invoke<LspCodeAction[]>(
    lspCommandName("lsp_code_actions", remote),
    lspInvokeArgs({ request, diagnostics }, remote),
  );
}

export function executeLspCommand(
  request: LspDocumentRequest,
  command: LspCommand,
  remote?: LspRemoteContext,
): Promise<void> {
  return invoke<void>(
    lspCommandName("lsp_execute_command", remote),
    lspInvokeArgs({ request, command }, remote),
  );
}
