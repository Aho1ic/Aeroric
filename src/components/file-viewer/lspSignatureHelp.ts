import { invoke } from "@tauri-apps/api/core";
import { StateEffect, StateField, showTooltip, ViewPlugin } from "@uiw/react-codemirror";
import type { Extension } from "@codemirror/state";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";
import { appendLspMarkdown, appendLspMarkdownInline } from "./lspMarkdown";

export type LspParameterInformation = {
  label: string;
  documentation?: string | null;
};

export type LspSignatureInformation = {
  label: string;
  documentation?: string | null;
  parameters: LspParameterInformation[];
};

export type LspSignatureHelp = {
  signatures: LspSignatureInformation[];
  activeSignature?: number | null;
  activeParameter?: number | null;
};

type LspSignatureHelpView = {
  state: {
    selection?: {
      main: {
        head: number;
      };
    };
    doc: {
      lineAt: (offset: number) => { number: number; from: number };
      sliceString?: (from: number, to: number) => string;
    };
  };
};

type LspSignatureTooltip = {
  pos: number;
  above: boolean;
  create: () => { dom: HTMLElement };
};

const setSignatureTooltip = StateEffect.define<LspSignatureTooltip | null>();

const signatureTooltipField = StateField.define<LspSignatureTooltip | null>({
  create: () => null,
  update: (tooltips, transaction) => {
    for (const effect of transaction.effects) {
      if (effect.is(setSignatureTooltip)) {
        return effect.value;
      }
    }
    return transaction.docChanged ? null : tooltips;
  },
  provide: (field) => showTooltip.from(field),
});

export type CreateLspSignatureHelpSourceOptions = {
  request: LspDocumentRequest;
  available: boolean;
  unavailableMessage?: string | null;
  remote?: LspRemoteContext;
  onError: (message: string) => void;
};

export type CreateLspSignatureHelpExtensionOptions = {
  request: LspDocumentRequest | null;
  available: boolean;
  unavailableMessage?: string | null;
  remote?: LspRemoteContext;
  onError: (message: string) => void;
};

function requestAtOffset(
  request: LspDocumentRequest,
  view: LspSignatureHelpView,
  offset: number,
): LspDocumentRequest {
  const line = view.state.doc.lineAt(offset);
  return {
    ...request,
    line: Math.max(0, line.number - 1),
    character: Math.max(0, offset - line.from),
  };
}

function isSignatureTrigger(view: LspSignatureHelpView, pos: number): boolean {
  const previous = view.state.doc.sliceString?.(Math.max(0, pos - 1), pos);
  return previous === "(" || previous === ",";
}

function activeSignature(help: LspSignatureHelp): LspSignatureInformation | null {
  const index = Math.min(
    Math.max(0, help.activeSignature ?? 0),
    Math.max(0, help.signatures.length - 1),
  );
  return help.signatures[index] ?? null;
}

function createSignatureTooltip(pos: number, help: LspSignatureHelp): LspSignatureTooltip | null {
  const signature = activeSignature(help);
  if (!signature) return null;
  const activeParameter = help.activeParameter ?? null;
  return {
    pos,
    above: true,
    create: () => {
      const dom = document.createElement("div");
      dom.className = "cm-lsp-signature-tooltip";

      const label = document.createElement("div");
      label.className = "cm-lsp-signature-label";
      label.textContent = signature.label;
      dom.appendChild(label);

      const parameter = activeParameter === null ? null : signature.parameters[activeParameter];
      if (parameter) {
        const parameterDom = document.createElement("div");
        parameterDom.className = "cm-lsp-signature-parameter";
        const parameterLabel = document.createElement("span");
        parameterLabel.textContent = parameter.documentation
          ? `${parameter.label}: `
          : parameter.label;
        parameterDom.appendChild(parameterLabel);
        if (parameter.documentation) {
          const parameterDocs = document.createElement("span");
          parameterDocs.className = "cm-lsp-signature-markdown";
          appendLspMarkdownInline(parameterDocs, parameter.documentation);
          parameterDom.appendChild(parameterDocs);
        }
        dom.appendChild(parameterDom);
      }

      if (signature.documentation) {
        const documentation = document.createElement("div");
        documentation.className = "cm-lsp-signature-docs";
        appendLspMarkdown(documentation, signature.documentation);
        dom.appendChild(documentation);
      }
      return { dom };
    },
  };
}

export function createLspSignatureHelpSource({
  request,
  available,
  unavailableMessage,
  remote,
  onError,
}: CreateLspSignatureHelpSourceOptions): (
  view: LspSignatureHelpView,
  pos: number,
  explicit?: boolean,
) => Promise<LspSignatureTooltip | null> {
  return async (view, pos, explicit = false) => {
    if (!explicit && !isSignatureTrigger(view, pos)) return null;
    if (!available) {
      onError(unavailableMessage ?? "Language server is unavailable.");
      return null;
    }

    try {
      const help = await invoke<LspSignatureHelp | null>(
        lspCommandName("lsp_signature_help", remote),
        lspInvokeArgs({ request: requestAtOffset(request, view, pos) }, remote),
      );
      if (!help?.signatures.length) return null;
      return createSignatureTooltip(pos, help);
    } catch (err) {
      onError(String(err));
      return null;
    }
  };
}

export function createLspSignatureHelpExtension({
  request,
  available,
  unavailableMessage,
  remote,
  onError,
}: CreateLspSignatureHelpExtensionOptions): Extension {
  if (!request) return [];
  const source = createLspSignatureHelpSource({
    request,
    available,
    unavailableMessage,
    remote,
    onError,
  });
  const plugin = ViewPlugin.fromClass(
    class {
      private requestId = 0;

      constructor(private readonly view: { dispatch: (spec: object) => void }) {}

      update(update: { docChanged: boolean; state: LspSignatureHelpView["state"] }) {
        if (!update.docChanged) return;
        const pos = update.state.selection?.main.head ?? 0;
        if (!isSignatureTrigger({ state: update.state }, pos)) return;
        const requestId = ++this.requestId;
        void source({ state: update.state }, pos, false).then((tooltip) => {
          if (requestId !== this.requestId) return;
          this.view.dispatch({ effects: setSignatureTooltip.of(tooltip) });
        });
      }
    },
  );
  return [signatureTooltipField, plugin];
}
