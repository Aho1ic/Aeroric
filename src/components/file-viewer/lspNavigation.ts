import { invoke } from "@tauri-apps/api/core";
import { EditorView } from "@uiw/react-codemirror";
import type { Extension } from "@codemirror/state";
import {
  lspCommandName,
  lspInvokeArgs,
  type LspDocumentRequest,
  type LspRemoteContext,
} from "../../hooks/languageServerState";
import type { OpenFileSelection } from "../../hooks/projectPanelsState";

export type LspNavigationLocation = {
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

export type LspOpenTarget = {
  path: string;
  name: string;
  selection: OpenFileSelection;
};

export function lspLocationToOpenTarget(location: LspNavigationLocation): LspOpenTarget {
  return {
    path: location.path,
    name: location.path.split(/[\\/]/).pop() ?? location.path,
    selection: {
      line: location.range.start.line + 1,
      column: location.range.start.character + 1,
    },
  };
}

type LspNavigationView = {
  state: {
    doc: {
      lineAt: (offset: number) => { number: number; from: number };
    };
  };
  dom?: HTMLElement;
  posAtCoords: (coords: { x: number; y: number }) => number | null;
};

export type CreateLspNavigationExtensionOptions = {
  request: LspDocumentRequest | null;
  available: boolean;
  unavailableMessage?: string | null;
  remote?: LspRemoteContext;
  onOpenTarget: (target: LspOpenTarget) => void;
  onError: (message: string) => void;
};

function modifierLeftClick(event: MouseEvent): boolean {
  return event.button === 0 && (event.metaKey || event.ctrlKey);
}

function requestAtMousePosition(
  request: LspDocumentRequest,
  view: LspNavigationView,
  event: MouseEvent,
): LspDocumentRequest | null {
  const offset = view.posAtCoords({ x: event.clientX, y: event.clientY });
  if (offset === null) return null;
  const line = view.state.doc.lineAt(offset);
  return {
    ...request,
    line: Math.max(0, line.number - 1),
    character: Math.max(0, offset - line.from),
  };
}

export function createLspNavigationExtension({
  request,
  available,
  unavailableMessage,
  remote,
  onOpenTarget,
  onError,
}: CreateLspNavigationExtensionOptions): Extension {
  if (!request) return [];
  return EditorView.domEventHandlers({
    mousemove(event, view) {
      const navView = view as unknown as LspNavigationView;
      if (!navView.dom) return false;
      navView.dom.style.cursor = event.metaKey || event.ctrlKey ? "pointer" : "";
      return false;
    },
    mouseleave(_event, view) {
      const navView = view as unknown as LspNavigationView;
      if (navView.dom) navView.dom.style.cursor = "";
      return false;
    },
    mousedown(event, view) {
      if (!modifierLeftClick(event)) return false;
      event.preventDefault();
      event.stopPropagation();

      if (!available) {
        onError(unavailableMessage ?? "Language server is unavailable.");
        return true;
      }

      const nextRequest = requestAtMousePosition(
        request,
        view as unknown as LspNavigationView,
        event,
      );
      if (!nextRequest) {
        onError("No symbol position found.");
        return true;
      }

      void invoke<LspNavigationLocation[]>(
        lspCommandName("lsp_definition", remote),
        lspInvokeArgs({ request: nextRequest }, remote),
      )
        .then((locations) => {
          const location = locations[0];
          if (!location) {
            onError("No definition found.");
            return;
          }
          onOpenTarget(lspLocationToOpenTarget(location));
        })
        .catch((err) => {
          onError(String(err));
        });
      return true;
    },
  });
}
