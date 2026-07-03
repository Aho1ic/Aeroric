import { invoke } from "@tauri-apps/api/core";
import { EditorState, type Extension } from "@codemirror/state";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";

export type LspCompletionItem = {
  label: string;
  detail?: string | null;
  documentation?: string | null;
};

type LspCompletionContext = {
  pos: number;
  explicit: boolean;
  state: {
    doc: {
      lineAt: (offset: number) => { number: number; from: number; to?: number };
      sliceString?: (from: number, to: number) => string;
    };
  };
  matchBefore: (expr: RegExp) => { from: number; to: number; text: string } | null;
};

type LspCompletionOption = {
  label: string;
  detail?: string;
  info?: string;
  type?: string;
};

type LspCompletionResult = {
  from: number;
  options: LspCompletionOption[];
};

export type CreateLspCompletionSourceOptions = {
  request: LspDocumentRequest;
  available: boolean;
  unavailableMessage?: string | null;
  remote?: LspRemoteContext;
  onError: (message: string) => void;
};

export type CreateLspCompletionExtensionOptions = {
  request: LspDocumentRequest | null;
  available: boolean;
  unavailableMessage?: string | null;
  remote?: LspRemoteContext;
  onError: (message: string) => void;
};

function requestAtOffset(
  request: LspDocumentRequest,
  context: LspCompletionContext,
): LspDocumentRequest {
  const line = context.state.doc.lineAt(context.pos);
  return {
    ...request,
    line: Math.max(0, line.number - 1),
    character: Math.max(0, context.pos - line.from),
  };
}

function completionType(item: LspCompletionItem): string | undefined {
  const detail = item.detail?.toLowerCase() ?? "";
  if (detail.includes("function") || detail.includes("method") || item.label.endsWith("()")) {
    return "function";
  }
  if (detail.includes("class") || detail.includes("interface") || detail.includes("type ")) {
    return "class";
  }
  if (detail.includes("const") || detail.includes("let") || detail.includes("var")) {
    return "variable";
  }
  return undefined;
}

function shouldRequestCompletion(context: LspCompletionContext, wordFrom: number | null): boolean {
  if (context.explicit) return true;
  if (wordFrom !== null && wordFrom < context.pos) return true;
  const previous = context.state.doc.sliceString?.(Math.max(0, context.pos - 1), context.pos);
  return previous === ".";
}

export function createLspCompletionSource({
  request,
  available,
  unavailableMessage,
  remote,
  onError,
}: CreateLspCompletionSourceOptions): (
  context: LspCompletionContext,
) => Promise<LspCompletionResult | null> {
  return async (context) => {
    const word = context.matchBefore(/\w*/);
    if (!shouldRequestCompletion(context, word?.from ?? null)) return null;

    if (!available) {
      onError(unavailableMessage ?? "Language server is unavailable.");
      return null;
    }

    try {
      const items = await invoke<LspCompletionItem[]>(
        lspCommandName("lsp_completion", remote),
        lspInvokeArgs({ request: requestAtOffset(request, context) }, remote),
      );
      if (items.length === 0) return null;
      return {
        from: word?.from ?? context.pos,
        options: items.map((item) => ({
          label: item.label,
          detail: item.detail ?? undefined,
          info: item.documentation ?? undefined,
          type: completionType(item),
        })),
      };
    } catch (err) {
      onError(String(err));
      return null;
    }
  };
}

export function createLspCompletionExtension({
  request,
  available,
  unavailableMessage,
  remote,
  onError,
}: CreateLspCompletionExtensionOptions): Extension {
  if (!request) return [];
  const source = createLspCompletionSource({
    request,
    available,
    unavailableMessage,
    remote,
    onError,
  });
  return EditorState.languageData.of(() => [{ autocomplete: source }]);
}
