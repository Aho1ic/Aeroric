import { invoke } from "@tauri-apps/api/core";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";

export type LspTextEdit = {
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
  newText: string;
};

export type LspFileEdit = {
  uri: string;
  path: string;
  edits: LspTextEdit[];
};

export type LspWorkspaceEdit = {
  files: LspFileEdit[];
};

export type LspApplyWorkspaceEditSummary = {
  filesChanged: number;
  editsApplied: number;
  editsSkipped: number;
};

export function requestLspRename(
  request: LspDocumentRequest,
  newName: string,
  remote?: LspRemoteContext,
): Promise<LspWorkspaceEdit> {
  return invoke<LspWorkspaceEdit>(
    lspCommandName("lsp_rename", remote),
    lspInvokeArgs({ request, newName }, remote),
  );
}

export function applyLspWorkspaceEdit(
  projectPath: string,
  edit: LspWorkspaceEdit,
  remote?: LspRemoteContext,
): Promise<LspApplyWorkspaceEditSummary> {
  return invoke<LspApplyWorkspaceEditSummary>(
    lspCommandName("lsp_apply_workspace_edit", remote),
    lspInvokeArgs({ projectPath, edit }, remote),
  );
}
