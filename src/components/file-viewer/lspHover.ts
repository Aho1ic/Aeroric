import { invoke } from "@tauri-apps/api/core";
import { hoverTooltip } from "@uiw/react-codemirror";
import type { Extension } from "@codemirror/state";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";
import { appendLspMarkdown } from "./lspMarkdown";

export type LspHoverResult = {
  contents: string;
  range?: unknown;
};

type LspHoverView = {
  state: {
    doc: {
      lineAt: (offset: number) => { number: number; from: number };
    };
  };
};

type LspTooltip = {
  pos: number;
  above: boolean;
  create: () => { dom: HTMLElement };
};

export type CreateLspHoverExtensionOptions = {
  request: LspDocumentRequest | null;
  available: boolean;
  unavailableMessage?: string | null;
  remote?: LspRemoteContext;
  onError: (message: string) => void;
};

function requestAtOffset(
  request: LspDocumentRequest,
  view: LspHoverView,
  offset: number,
): LspDocumentRequest {
  const line = view.state.doc.lineAt(offset);
  return {
    ...request,
    line: Math.max(0, line.number - 1),
    character: Math.max(0, offset - line.from),
  };
}

function createHoverTooltip(pos: number, contents: string): LspTooltip {
  return {
    pos,
    above: true,
    create: () => {
      const dom = document.createElement("div");
      dom.className = "cm-lsp-hover-tooltip";
      appendLspMarkdown(dom, contents);
      return { dom };
    },
  };
}

export function createLspHoverExtension({
  request,
  available,
  unavailableMessage,
  remote,
  onError,
}: CreateLspHoverExtensionOptions): Extension {
  if (!request) return [];
  return hoverTooltip(async (view, pos) => {
    if (!available) {
      onError(unavailableMessage ?? "Language server is unavailable.");
      return null;
    }

    const nextRequest = requestAtOffset(request, view as unknown as LspHoverView, pos);
    try {
      const hover = await invoke<LspHoverResult | null>(
        lspCommandName("lsp_hover", remote),
        lspInvokeArgs({ request: nextRequest }, remote),
      );
      if (!hover?.contents.trim()) return null;
      return createHoverTooltip(pos, hover.contents);
    } catch (err) {
      onError(String(err));
      return null;
    }
  });
}
